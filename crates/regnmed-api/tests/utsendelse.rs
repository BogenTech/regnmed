//! E-postutsendelse end to end: an issued invoice's PDF goes onto the
//! shared mail rail as a real JetStream message (regnid's wire format —
//! attachment base64, reply-to the company), the insert-only
//! utsendelseslogg records it, and an unconfigured rail answers with a
//! clear message. Needs DATABASE_URL and a `nats-server` binary on
//! PATH (spawned with JetStream on an ephemeral port); skips otherwise.

mod common;

use std::process::{Child, Command, Stdio};
use std::time::Duration;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use base64::Engine as _;
use common::{TestIdp, test_state, unique_orgnr};
use futures_util::StreamExt as _;
use tower::ServiceExt;
use uuid::Uuid;

use regnmed_api::{AppState, mailq, router};

struct NatsServer {
    child: Child,
    url: String,
    dir: std::path::PathBuf,
}

impl Drop for NatsServer {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

async fn start_nats() -> Option<NatsServer> {
    let dir = std::env::temp_dir().join(format!("regnmed-nats-{}", Uuid::new_v4().simple()));
    std::fs::create_dir_all(&dir).ok()?;
    let port = free_port();
    let child = match Command::new("nats-server")
        .args(["-js", "-p", &port.to_string(), "-sd"])
        .arg(&dir)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    {
        Ok(child) => child,
        Err(_) => {
            eprintln!("skipping: nats-server not found on PATH");
            return None;
        }
    };
    let url = format!("nats://127.0.0.1:{port}");
    let server = NatsServer { child, url, dir };
    for _ in 0..50 {
        if mailq::connect(&server.url, None, None).await.is_ok() {
            return Some(server);
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    panic!("nats-server did not come up");
}

async fn request(
    state: &AppState,
    method: &str,
    uri: &str,
    bearer: &str,
    body: Option<String>,
) -> (StatusCode, serde_json::Value) {
    let mut builder = Request::builder()
        .method(method)
        .uri(uri)
        .header("authorization", format!("Bearer {bearer}"));
    if body.is_some() {
        builder = builder.header("content-type", "application/json");
    }
    let response = router(state.clone())
        .oneshot(builder.body(Body::from(body.unwrap_or_default())).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), 1024 * 1024)
        .await
        .unwrap();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null),
    )
}

#[tokio::test]
async fn invoice_mail_rides_the_shared_rail() {
    let idp = TestIdp::new();
    let Some(base_state) = test_state(&idp).await else {
        return;
    };
    let Some(nats) = start_nats().await else {
        return;
    };
    let js = mailq::connect(&nats.url, None, None).await.unwrap();
    let state = AppState {
        mailq: Some(js.clone()),
        ..base_state.clone()
    };

    let sub = format!("test|{}", Uuid::new_v4());
    let person = regnmed_db::ensure_person(&state.pool, &sub, Some("Kari Bokfører"), None)
        .await
        .unwrap();
    let company = regnmed_db::create_company(&state.pool, &unique_orgnr(), "Utsendelse AS")
        .await
        .unwrap();
    regnmed_db::ensure_company_member(&state.pool, company, person, "admin")
        .await
        .unwrap();
    regnmed_db::ensure_journal(&state.pool, company, "GL", "Hovedbok")
        .await
        .unwrap();
    for (number, name) in [
        ("1500", "Kundefordringer"),
        ("3000", "Salgsinntekt"),
        ("2700", "Utgående mva"),
    ] {
        regnmed_db::ensure_account(&state.pool, company, number, name)
            .await
            .unwrap();
    }
    regnmed_db::set_account_reskontro(&state.pool, company, "1500", Some("kunde"))
        .await
        .unwrap();
    let (party_id, party_no) =
        regnmed_db::create_party(&state.pool, company, "kunde", "Kunde & Co AS", None, None)
            .await
            .unwrap();
    let token = idp.token(&sub, "Kari Bokfører");

    // Company reply-to + customer e-mail.
    let (status, _) = request(
        &state,
        "PUT",
        &format!("/companies/{company}/settings"),
        &token,
        Some(serde_json::json!({ "email": "post@utsendelse.example" }).to_string()),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    regnmed_db::update_party_contact(
        &state.pool,
        company,
        party_id,
        None,
        Some("kunde@example.test"),
    )
    .await
    .unwrap();

    let (status, issued) = request(
        &state,
        "POST",
        &format!("/companies/{company}/invoices"),
        &token,
        Some(
            serde_json::json!({
                "party_no": party_no,
                "invoice_date": "2026-07-24",
                "due_date": "2026-08-07",
                "lines": [{ "description": "Konsulentbistand", "unit_price_ore": 10_000_00, "vat_code": "3" }],
            })
            .to_string(),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body: {issued}");
    let invoice_id = issued["invoice_id"].as_str().unwrap().to_string();

    // Send — recipient defaults to the party's stored e-mail.
    let (status, sent) = request(
        &state,
        "POST",
        &format!("/companies/{company}/invoices/{invoice_id}/send"),
        &token,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body: {sent}");
    assert_eq!(sent["to"], "kunde@example.test");

    // The message is really on the stream, in regnid's wire format.
    let stream = js.get_stream(mailq::STREAM).await.unwrap();
    let consumer: async_nats::jetstream::consumer::PullConsumer = stream
        .get_or_create_consumer(
            "test-reader",
            async_nats::jetstream::consumer::pull::Config {
                durable_name: Some("test-reader".into()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    let mut messages = consumer.messages().await.unwrap();
    let message = tokio::time::timeout(Duration::from_secs(5), messages.next())
        .await
        .expect("a mail on the stream")
        .unwrap()
        .unwrap();
    let mail: serde_json::Value = serde_json::from_slice(&message.payload).unwrap();
    message.ack().await.unwrap();
    assert_eq!(mail["to"], "kunde@example.test");
    assert_eq!(mail["reply_to"], "post@utsendelse.example");
    assert!(
        mail["subject"].as_str().unwrap().contains("Faktura 1"),
        "{mail}"
    );
    assert!(mail["text"].as_str().unwrap().contains("KID"));
    let attachment = &mail["attachments"][0];
    assert_eq!(attachment["filename"], "faktura-1.pdf");
    assert_eq!(attachment["content_type"], "application/pdf");
    let pdf = base64::engine::general_purpose::STANDARD
        .decode(attachment["content_base64"].as_str().unwrap())
        .unwrap();
    assert!(
        pdf.starts_with(b"%PDF-1.4"),
        "the stored salgsdokument rides along"
    );

    // The insert-only log recorded the send.
    let (_, log) = request(
        &state,
        "GET",
        &format!("/companies/{company}/invoices/{invoice_id}/utsendelser"),
        &token,
        None,
    )
    .await;
    let rows = log["utsendelser"].as_array().unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["to"], "kunde@example.test");
    assert_eq!(rows[0]["sent_by"], "Kari Bokfører");

    // Without the rail configured, the endpoint says so instead of
    // pretending.
    let (status, err) = request(
        &base_state,
        "POST",
        &format!("/companies/{company}/invoices/{invoice_id}/send"),
        &token,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(err.to_string().contains("NATS_URL"), "body: {err}");
}
