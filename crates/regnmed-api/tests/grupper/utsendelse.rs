//! E-postutsendelse end to end: an issued invoice's PDF goes onto the
//! shared mail rail as a real JetStream message (regnid's wire format —
//! attachment base64, reply-to the company), the insert-only
//! utsendelseslogg records it, and an unconfigured rail answers with a
//! clear message. Needs DATABASE_URL and a `nats-server` binary on
//! PATH (spawned with JetStream on an ephemeral port); skips otherwise.

use std::process::{Child, Command, Stdio};
use std::time::Duration;

use crate::common::{TestIdp, test_state, unique_orgnr};
use axum::body::Body;
use axum::http::{Request, StatusCode};
use base64::Engine as _;
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
        None,
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

/// The invitation mail actually goes out (#66).
///
/// What matters beyond "a message appeared": the mail carries **no
/// token**, because redemption is still e-mail match at login. A test
/// that only checked the subject would not notice a future change that
/// started putting a secret in the link — so the body is asserted to
/// contain the portal address and nothing that looks like a credential.
///
/// The second half is the reason the send cannot be wired into the
/// invitation transaction: with the rail down the invitation must still
/// be created, and the response must say the mail did not go.
#[tokio::test]
async fn the_invitation_mail_goes_out_and_carries_no_token() {
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
        portal_base: Some("https://regnmed.example/".into()),
        ..base_state.clone()
    };

    let sub = format!("admin|{}", Uuid::new_v4());
    let admin = regnmed_db::ensure_person(&state.pool, &sub, Some("Kari Admin"), None)
        .await
        .unwrap();
    let company = regnmed_db::create_company(&state.pool, &unique_orgnr(), "Invitasjon AS")
        .await
        .unwrap();
    regnmed_db::ensure_company_member(&state.pool, company, admin, "admin")
        .await
        .unwrap();
    let token = idp.token(&sub, "Kari Admin");

    let (status, invited) = request(
        &state,
        "POST",
        &format!("/companies/{company}/invitations"),
        &token,
        Some(r#"{"epost":"nyansatt@example.test","rolle":"bokforing"}"#.into()),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body: {invited}");
    assert_eq!(invited["epost_sendt"], true, "body: {invited}");

    let stream = js.get_stream(mailq::STREAM).await.unwrap();
    let consumer: async_nats::jetstream::consumer::PullConsumer = stream
        .get_or_create_consumer(
            "invitasjon-reader",
            async_nats::jetstream::consumer::pull::Config {
                durable_name: Some("invitasjon-reader".into()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    let mut messages = consumer.messages().await.unwrap();
    let message = tokio::time::timeout(Duration::from_secs(5), messages.next())
        .await
        .expect("an invitation mail on the stream")
        .unwrap()
        .unwrap();
    let mail: serde_json::Value = serde_json::from_slice(&message.payload).unwrap();

    assert_eq!(mail["to"], "nyansatt@example.test");
    let text = mail["text"].as_str().unwrap();
    assert!(text.contains("Invitasjon AS"), "{text}");
    assert!(text.contains("bokforing"), "{text}");
    assert!(
        text.contains("Kari Admin"),
        "the inviter should be named: {text}"
    );
    assert!(
        text.contains("https://regnmed.example"),
        "the portal link should be there: {text}"
    );
    // No attachment, and no token: the link is the front page, and the
    // access hangs on the address rather than on this message.
    assert!(
        mail["attachments"].is_null(),
        "an invitation carries no document: {mail}"
    );
    assert!(
        !text.contains("token") && !text.contains("invitation_id") && !text.contains('?'),
        "the mail must carry no credential: {text}"
    );

    // The send is logged like every other, against the invitation.
    let invitation_id = invited["invitasjon_id"].as_str().unwrap();
    let logged: i64 = sqlx::query_scalar(
        "select count(*) from utsendelse where invitation_id = $1::uuid and company_id = $2",
    )
    .bind(invitation_id)
    .bind(company)
    .fetch_one(&state.pool)
    .await
    .unwrap();
    assert_eq!(logged, 1);

    // And the listing shows when it last went, so an admin knows whether
    // resending is warranted.
    let (_, open) = request(
        &state,
        "GET",
        &format!("/companies/{company}/invitations"),
        &token,
        None,
    )
    .await;
    assert!(
        open["invitasjoner"][0]["sist_sendt"].is_string(),
        "body: {open}"
    );

    // Resending sends again — the invitation itself is untouched.
    let (status, again) = request(
        &state,
        "POST",
        &format!("/companies/{company}/invitations/{invitation_id}/resend"),
        &token,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body: {again}");
    let logged: i64 =
        sqlx::query_scalar("select count(*) from utsendelse where invitation_id = $1::uuid")
            .bind(invitation_id)
            .fetch_one(&state.pool)
            .await
            .unwrap();
    assert_eq!(
        logged, 2,
        "a resend is a second utsendelse, not a replacement"
    );

    // Without the rail: the invitation is still created — an outage must
    // not take membership administration with it — and the response says
    // plainly that no mail went.
    let (status, invited) = request(
        &base_state,
        "POST",
        &format!("/companies/{company}/invitations"),
        &token,
        Some(r#"{"epost":"nummer.to@example.test","rolle":"les"}"#.into()),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body: {invited}");
    assert!(invited["invitasjon_id"].is_string(), "body: {invited}");
    assert_eq!(invited["epost_sendt"], false);
    assert!(
        invited["epost_grunn"]
            .as_str()
            .unwrap_or_default()
            .contains("NATS_URL"),
        "body: {invited}"
    );
}
