//! Publisher side of the shared outbound-mail rail (docs/faktura.md,
//! #32): regnmed enqueues on the SAME JetStream stream regnid's mail
//! workers consume — one rail for all outbound mail, one place SMTP is
//! configured. The stream name, subject and message shape are the wire
//! contract with regnid (its `mailq`/`mail` modules); the fields here
//! mirror regnid's `OutboundMail` exactly.
//!
//! The utsendelse id doubles as `Nats-Msg-Id`, so a retried publish is
//! deduplicated by the stream and the insert-only utsendelseslogg row
//! is the same event as the queue message.

use anyhow::{Context as _, Result};
use async_nats::jetstream;
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use uuid::Uuid;

/// Wire contract with regnid — do not rename.
pub const STREAM: &str = "REGNID_MAIL";
pub const SUBJECT: &str = "regnid.mail.send";

#[derive(Debug, serde::Serialize)]
pub struct OutboundMail {
    pub id: Uuid,
    pub to: String,
    pub subject: String,
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reply_to: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub attachments: Vec<MailAttachment>,
}

#[derive(Debug, serde::Serialize)]
pub struct MailAttachment {
    pub filename: String,
    pub content_type: String,
    pub content_base64: String,
}

impl OutboundMail {
    pub fn from_payload(id: Uuid, payload: &regnmed_db::EmailPayload) -> Self {
        OutboundMail {
            id,
            to: payload.to.clone(),
            subject: payload.subject.clone(),
            text: payload.text.clone(),
            reply_to: payload.reply_to.clone(),
            attachments: vec![MailAttachment {
                filename: payload.filename.clone(),
                content_type: "application/pdf".into(),
                content_base64: BASE64.encode(&payload.pdf),
            }],
        }
    }
}

/// Connects and makes sure the stream exists (same idempotent config as
/// regnid's side), so a misconfigured queue fails at startup — not at
/// the first send.
pub async fn connect(
    url: &str,
    user: Option<String>,
    password: Option<String>,
) -> Result<jetstream::Context> {
    let mut opts = async_nats::ConnectOptions::new().name("regnmed");
    if let (Some(user), Some(password)) = (user, password) {
        opts = opts.user_and_password(user, password);
    }
    let client = opts
        .connect(url)
        .await
        .with_context(|| format!("connecting to NATS at {url}"))?;
    let js = jetstream::new(client);
    js.get_or_create_stream(jetstream::stream::Config {
        name: STREAM.into(),
        subjects: vec![SUBJECT.into()],
        retention: jetstream::stream::RetentionPolicy::WorkQueue,
        duplicate_window: std::time::Duration::from_secs(120),
        ..Default::default()
    })
    .await
    .context("creating/getting the mail stream")?;
    Ok(js)
}

pub async fn connect_from_env() -> Result<Option<jetstream::Context>> {
    let Ok(url) = std::env::var("NATS_URL") else {
        return Ok(None);
    };
    let js = connect(
        &url,
        std::env::var("NATS_USER").ok(),
        std::env::var("NATS_PASSWORD").ok(),
    )
    .await?;
    Ok(Some(js))
}

/// Publishes one mail and waits for the stream's ack.
pub async fn publish(js: &jetstream::Context, mail: &OutboundMail) -> Result<()> {
    let mut headers = async_nats::HeaderMap::new();
    headers.insert("Nats-Msg-Id", mail.id.to_string());
    js.publish_with_headers(SUBJECT, headers, serde_json::to_vec(mail)?.into())
        .await?
        .await
        .context("waiting for JetStream publish ack")?;
    Ok(())
}
