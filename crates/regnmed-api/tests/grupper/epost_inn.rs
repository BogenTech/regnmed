//! Inbound e-mail (#35): a message on the mail rail becomes innboks
//! documents by the ordinary immutable route, an unknown sender lands in
//! quarantine until an admin decides, and nothing disappears silently.
//! Runs against a real nats-server on PATH (skipped without), and
//! requires DATABASE_URL.

use std::process::{Child, Command, Stdio};
use std::time::Duration;

use crate::common::{TestIdp, test_state, unique_orgnr};
use axum::body::Body;
use axum::http::{Request, StatusCode};
use base64::Engine as _;
use tower::ServiceExt;
use uuid::Uuid;

use regnmed_api::{AppState, mailq, mailq_in, router};

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
    let dir = std::env::temp_dir().join(format!("regnmed-nats-in-{}", Uuid::new_v4().simple()));
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
    let bytes = axum::body::to_bytes(response.into_body(), 4 * 1024 * 1024)
        .await
        .unwrap();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null),
    )
}

fn mail(to: &str, from: &str, message_id: &str, filnavn: &str) -> mailq_in::InboundMail {
    mailq_in::InboundMail {
        to: to.into(),
        from: from.into(),
        subject: Some("Faktura juli".into()),
        text: Some("Hei, her kommer fakturaen. Mvh Grossisten".into()),
        message_id: message_id.into(),
        attachments: vec![mailq_in::InboundAttachment {
            filename: filnavn.into(),
            content_type: "application/pdf".into(),
            content_base64: base64::engine::general_purpose::STANDARD
                .encode(format!("innholdet i {filnavn}").as_bytes()),
        }],
    }
}

#[tokio::test]
async fn email_becomes_inbox_documents_and_an_unknown_sender_is_quarantined() {
    let idp = TestIdp::new();
    let Some(state) = test_state(&idp).await else {
        return;
    };
    let Some(nats) = start_nats().await else {
        return;
    };

    let sub = format!("test|{}", Uuid::new_v4());
    let person = regnmed_db::ensure_person(&state.pool, &sub, Some("Ingrid Innboks"), None)
        .await
        .unwrap();
    let company = regnmed_db::create_company(&state.pool, &unique_orgnr(), "Mottak AS")
        .await
        .unwrap();
    regnmed_db::ensure_company_member(&state.pool, company, person, "admin")
        .await
        .unwrap();
    let token = idp.token(&sub, "Ingrid Innboks");
    let base = format!("/companies/{company}");

    // ---- The address does not exist until somebody asks for it ----
    let (status, settings) = request(
        &state,
        "GET",
        &format!("{base}/inbox/settings"),
        &token,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{settings}");
    assert!(settings["local_part"].is_null());

    let (status, created) = request(
        &state,
        "POST",
        &format!("{base}/inbox/settings/address"),
        &token,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{created}");
    let local = created["local_part"].as_str().unwrap().to_string();
    assert!(local.starts_with("bilag-mottak-as-"), "{local}");
    assert!(
        local.len() > "bilag-mottak-as-".len() + 8,
        "adressen har en uforutsigbar hale: {local}"
    );

    // Grossisten is on the list; nobody else is.
    let (status, _) = request(
        &state,
        "POST",
        &format!("{base}/inbox/settings/senders"),
        &token,
        Some(
            serde_json::json!({"sender": "@grossisten.no", "note": "fast leverandør"}).to_string(),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // ---- Kjør konsumenten mot en ekte JetStream-strøm ----
    let js = mailq::connect(&nats.url, None, None).await.unwrap();
    mailq_in::ensure_stream(&js).await.unwrap();
    let worker = tokio::spawn(mailq_in::run(js.clone(), state.pool.clone()));

    let publish = |mail: mailq_in::InboundMail| {
        let js = js.clone();
        async move {
            js.publish(mailq_in::SUBJECT, serde_json::to_vec(&mail).unwrap().into())
                .await
                .unwrap()
                .await
                .unwrap();
        }
    };

    // Known sender → a document in the innboks, with no decision taken.
    publish(mail(
        &format!("{local}@mottak.regnmed.no"),
        "Ola <POST@Grossisten.no>",
        "<msg-1@grossisten.no>",
        "faktura-1001.pdf",
    ))
    .await;
    // Ukjent avsender → karantene.
    publish(mail(
        &format!("{local}@mottak.regnmed.no"),
        "ukjent@annetsted.no",
        "<msg-2@annetsted.no>",
        "kvittering.pdf",
    ))
    .await;
    // No attachment → rejected, but logged.
    publish(mailq_in::InboundMail {
        to: format!("{local}@mottak.regnmed.no"),
        from: "post@grossisten.no".into(),
        subject: Some("Bare en hilsen".into()),
        text: Some("ingen vedlegg her".into()),
        message_id: "<msg-3@grossisten.no>".into(),
        attachments: vec![],
    })
    .await;
    // The same message again (queues repeat) → no duplicate.
    publish(mail(
        &format!("{local}@mottak.regnmed.no"),
        "post@grossisten.no",
        "<msg-1@grossisten.no>",
        "faktura-1001.pdf",
    ))
    .await;

    // Wait until the log holds all three messages.
    let mut mail_rows = serde_json::Value::Null;
    for _ in 0..50 {
        let (_, body) = request(&state, "GET", &format!("{base}/inbox/mail"), &token, None).await;
        if body["mail"].as_array().is_some_and(|m| m.len() >= 3) {
            mail_rows = body;
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let rows = mail_rows["mail"].as_array().expect("loggen fylles");
    assert_eq!(
        rows.len(),
        3,
        "dubletten ble ikke logget på nytt: {mail_rows}"
    );
    let by_id = |id: &str| {
        rows.iter()
            .find(|m| m["message_id"] == id)
            .unwrap_or_else(|| panic!("mangler {id}"))
            .clone()
    };
    let mottatt = by_id("<msg-1@grossisten.no>");
    assert_eq!(mottatt["status"], "mottatt");
    assert_eq!(
        mottatt["fra"], "post@grossisten.no",
        "visningsnavn strippet"
    );
    assert_eq!(
        mottatt["tekst"], "Hei, her kommer fakturaen. Mvh Grossisten",
        "brødteksten er lagret som dokumentasjon av opprinnelse"
    );
    let karantene = by_id("<msg-2@annetsted.no>");
    assert_eq!(karantene["status"], "karantene");
    assert!(
        karantene["note"]
            .as_str()
            .unwrap()
            .contains("avsenderliste"),
        "{karantene}"
    );
    let uten_vedlegg = by_id("<msg-3@grossisten.no>");
    assert_eq!(uten_vedlegg["status"], "avvist");
    assert!(
        uten_vedlegg["note"]
            .as_str()
            .unwrap()
            .contains("ingen vedlegg")
    );

    // ---- Only the approved sender's attachment is a document ----
    let (_, listing) = request(
        &state,
        "GET",
        &format!("{base}/inbox?status=ny"),
        &token,
        None,
    )
    .await;
    let docs = listing["documents"].as_array().unwrap();
    assert_eq!(docs.len(), 1, "karantene lager ingen dokumenter: {listing}");
    assert_eq!(docs[0]["filename"], "faktura-1001.pdf");
    assert_eq!(
        docs[0]["uploaded_by"], "post@grossisten.no",
        "avsenderadressen er den som leverte bilaget"
    );
    assert_eq!(docs[0]["status"], "ny", "e-post bokfører ingenting");

    // ---- Admin slipper karantenen gjennom ----
    let mail_id = karantene["mail_id"].as_str().unwrap();
    let (status, released) = request(
        &state,
        "POST",
        &format!("{base}/inbox/mail/{mail_id}/release"),
        &token,
        Some(serde_json::json!({"tillat_avsender": true}).to_string()),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{released}");
    assert_eq!(released["dokumenter"], 1);
    let (_, listing) = request(
        &state,
        "GET",
        &format!("{base}/inbox?status=ny"),
        &token,
        None,
    )
    .await;
    assert_eq!(listing["documents"].as_array().unwrap().len(), 2);
    // …and the sender is now on the list, so next time goes straight in.
    let (_, settings) = request(
        &state,
        "GET",
        &format!("{base}/inbox/settings"),
        &token,
        None,
    )
    .await;
    assert!(
        settings["avsendere"]
            .as_array()
            .unwrap()
            .iter()
            .any(|s| s["sender"] == "ukjent@annetsted.no"),
        "{settings}"
    );
    // A decided e-mail cannot be decided again.
    let (status, _) = request(
        &state,
        "POST",
        &format!("{base}/inbox/mail/{mail_id}/release"),
        &token,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    // ---- Rotation: the old address stops working ----
    let (_, rotated) = request(
        &state,
        "POST",
        &format!("{base}/inbox/settings/address"),
        &token,
        None,
    )
    .await;
    let ny = rotated["local_part"].as_str().unwrap().to_string();
    assert_ne!(ny, local);
    publish(mail(
        &format!("{local}@mottak.regnmed.no"),
        "post@grossisten.no",
        "<msg-4@grossisten.no>",
        "for-sent.pdf",
    ))
    .await;
    tokio::time::sleep(Duration::from_millis(500)).await;
    let (_, body) = request(&state, "GET", &format!("{base}/inbox/mail"), &token, None).await;
    assert!(
        !body["mail"]
            .as_array()
            .unwrap()
            .iter()
            .any(|m| m["message_id"] == "<msg-4@grossisten.no>"),
        "en tilbakekalt adresse tar ikke imot noe: {body}"
    );

    worker.abort();
}
