//! Consumer side of the shared mail rail (docs/epost-inn.md, #35).
//!
//! Utgående post går ut på `regnid.mail.send` (src/mailq.rs);
//! innkommende post kommer inn på `regnid.mail.received`, publisert av
//! den samme mail-infrastrukturen. Det er med vilje: plattformen har ÉN
//! mail-rail, og MX-en/mottaket bor i regnid — som aldri vendores inn
//! her. Feltene under er wire-kontrakten, akkurat som for utgående.
//!
//! Uten `NATS_URL` finnes ingen konsument, og e-post-inn er rett og
//! slett av. Adressen vises da som «ikke konfigurert» i portalen i
//! stedet for å love noe som ikke virker.

use anyhow::{Context as _, Result};
use async_nats::jetstream;
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use futures_util::StreamExt as _;

use sqlx::PgPool;

/// Wire contract with regnid — do not rename.
pub const STREAM: &str = "REGNID_MAIL_IN";
pub const SUBJECT: &str = "regnid.mail.received";
/// The durable consumer regnmed pulls from; a restart resumes where it
/// left off instead of replaying the world.
pub const CONSUMER: &str = "regnmed-innboks";

#[derive(Debug, serde::Deserialize, serde::Serialize)]
pub struct InboundMail {
    /// The address the message was delivered to — this is what routes
    /// it to a company.
    pub to: String,
    pub from: String,
    #[serde(default)]
    pub subject: Option<String>,
    #[serde(default)]
    pub text: Option<String>,
    pub message_id: String,
    #[serde(default)]
    pub attachments: Vec<InboundAttachment>,
}

#[derive(Debug, serde::Deserialize, serde::Serialize)]
pub struct InboundAttachment {
    pub filename: String,
    #[serde(default = "octet_stream")]
    pub content_type: String,
    pub content_base64: String,
}

fn octet_stream() -> String {
    "application/octet-stream".into()
}

/// Refuses an attachment larger than this; a mail rail is not a file
/// share, and the innboks stores what it accepts forever.
const MAX_VEDLEGG_BYTES: usize = 20 * 1024 * 1024;

impl InboundMail {
    /// Wire shape → what the db layer stores. Undecodable or oversized
    /// attachments are dropped with a reason rather than failing the
    /// whole message: the rest of the mail is still evidence.
    pub fn to_db(&self, raw: Vec<u8>) -> (regnmed_db::MottattEpost, Vec<String>) {
        let mut advarsler = Vec::new();
        let mut attachments = Vec::new();
        for a in &self.attachments {
            match BASE64.decode(a.content_base64.as_bytes()) {
                Ok(bytes) if bytes.len() > MAX_VEDLEGG_BYTES => advarsler.push(format!(
                    "{} er større enn {} MB og ble ikke lagret",
                    a.filename,
                    MAX_VEDLEGG_BYTES / (1024 * 1024)
                )),
                Ok(bytes) if bytes.is_empty() => {}
                Ok(bytes) => attachments.push(regnmed_db::MottattVedlegg {
                    filename: a.filename.clone(),
                    content_type: a.content_type.clone(),
                    content: bytes,
                }),
                Err(_) => advarsler.push(format!("{} kunne ikke dekodes", a.filename)),
            }
        }
        (
            regnmed_db::MottattEpost {
                to: self.to.clone(),
                from: self.from.clone(),
                subject: self.subject.clone(),
                text: self.text.clone(),
                message_id: self.message_id.clone(),
                raw,
                attachments,
            },
            advarsler,
        )
    }
}

/// Makes sure the inbound stream exists (idempotent, same shape regnid
/// would create), so a misconfigured rail fails at startup.
pub async fn ensure_stream(js: &jetstream::Context) -> Result<()> {
    js.get_or_create_stream(jetstream::stream::Config {
        name: STREAM.into(),
        subjects: vec![SUBJECT.into()],
        ..Default::default()
    })
    .await
    .context("creating/getting the inbound mail stream")?;
    Ok(())
}

/// Runs until the process stops: pulls received mail and turns it into
/// innboks documents (or quarantine). Each message is acked only after
/// it is stored, and reception is idempotent per Message-Id, so a retry
/// after a crash cannot duplicate a document.
pub async fn run(js: jetstream::Context, pool: PgPool) -> Result<()> {
    ensure_stream(&js).await?;
    let stream = js.get_stream(STREAM).await?;
    let consumer = stream
        .get_or_create_consumer(
            CONSUMER,
            jetstream::consumer::pull::Config {
                durable_name: Some(CONSUMER.into()),
                ack_policy: jetstream::consumer::AckPolicy::Explicit,
                ..Default::default()
            },
        )
        .await
        .context("creating/getting the innboks mail consumer")?;

    let mut messages = consumer.messages().await?;
    while let Some(message) = messages.next().await {
        let message = match message {
            Ok(m) => m,
            Err(e) => {
                eprintln!("e-post-inn: kunne ikke lese melding: {e}");
                continue;
            }
        };
        let raw = message.payload.to_vec();
        match serde_json::from_slice::<InboundMail>(&raw) {
            Ok(mail) => {
                let (db_mail, advarsler) = mail.to_db(raw);
                for advarsel in &advarsler {
                    eprintln!("e-post-inn ({}): {advarsel}", mail.message_id);
                }
                match regnmed_db::receive_mail(&pool, &db_mail).await {
                    Ok(utfall) => println!(
                        "e-post-inn: {} fra {} → {utfall:?}",
                        mail.message_id, mail.from
                    ),
                    Err(e) => {
                        // Unroutable mail (unknown address) is not worth
                        // redelivering; log it and move on rather than
                        // blocking the queue forever.
                        eprintln!("e-post-inn: {} avvist: {e:#}", mail.message_id);
                    }
                }
            }
            Err(e) => eprintln!("e-post-inn: meldingen er ikke gyldig JSON: {e}"),
        }
        if let Err(e) = message.ack().await {
            eprintln!("e-post-inn: ack feilet: {e}");
        }
    }
    Ok(())
}
