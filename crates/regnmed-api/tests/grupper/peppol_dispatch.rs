//! Peppol dispatch persistence (docs/efaktura.md §3.3): an attempt that may
//! have reached the provider can never be followed by another POST.
//! Needs DATABASE_URL; skips otherwise.

use crate::common::{
    TestIdp, give_parties_address, make_invoiceable, test_state, unique_orgnumber,
};
use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::post;
use axum::{Json, Router};
use chrono::NaiveDate;
use regnmed_core::peppol::{DirectoryAnswer, EventKind, own_event_key, provider_event_key};
use regnmed_db::job::{self, Kind, Runtime};
use regnmed_db::peppol::{self, BeginAttempt, ResendPolicy};
use regnmed_db::peppol_worker::{self, EventAction, handle_provider_event};
use regnmed_gov::recommand::Recommand;
use serde_json::json;
use sqlx::PgPool;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use tower::ServiceExt;
use uuid::Uuid;

async fn issued_invoice(pool: &PgPool) -> (Uuid, Uuid, Uuid) {
    let company = regnmed_db::create_company(pool, &unique_orgnumber(), "Peppol Selger AS")
        .await
        .unwrap();
    make_invoiceable(pool, company).await;
    regnmed_db::ensure_journal(pool, company, "GL", "Hovedbok")
        .await
        .unwrap();
    for (number, name) in [
        ("1500", "Kundefordringer"),
        ("3000", "Salgsinntekt"),
        ("2700", "Utgående mva"),
    ] {
        regnmed_db::ensure_account(pool, company, number, name)
            .await
            .unwrap();
    }
    regnmed_db::set_account_subledger(pool, company, "1500", Some("kunde"))
        .await
        .unwrap();
    let buyer = unique_orgnumber();
    let (_, party_no) = regnmed_db::create_party(
        pool,
        company,
        "kunde",
        "Peppol Kjøper AS",
        Some(&buyer),
        None,
    )
    .await
    .unwrap();
    give_parties_address(pool, company).await;
    let day = NaiveDate::from_ymd_opt(2026, 11, 21).unwrap();
    let invoice = regnmed_db::create_invoice(
        pool,
        company,
        &regnmed_db::InvoiceDraft {
            cash_payment_method: None,
            party_no,
            buyer_reference: Some("TEST".into()),
            invoice_date: day,
            due_date: NaiveDate::from_ymd_opt(2026, 12, 5).unwrap(),
            delivery_date: day,
            delivery_place: None,
            journal_code: "GL".into(),
            receivable_account: "1500".into(),
            vat_account: "2700".into(),
            currency: None,
            exchange_rate_micro: None,
            lines: vec![regnmed_db::InvoiceLineDraft {
                description: "Rådgivning".into(),
                account_number: "3000".into(),
                quantity_milli: 1000,
                unit_price_ore: 100_000,
                vat_code: Some("3".into()),
                department: None,
                project: None,
                product_id: None,
            }],
        },
        "test",
        None,
    )
    .await
    .unwrap()
    .invoice_id;
    let xml = regnmed_db::settings::issued_attachment_id(pool, company, invoice, "application/xml")
        .await
        .unwrap()
        .expect("the issued invoice stores its EHF");
    (company, invoice, xml)
}

async fn dispatch(pool: &PgPool, company: Uuid, invoice: Uuid, xml: Uuid) -> Uuid {
    peppol::create_dispatch(
        pool,
        company,
        invoice,
        "0192:123456785",
        xml,
        "Faktura 1",
        "test",
    )
    .await
    .unwrap()
}

async fn outcome(pool: &PgPool, id: Uuid, attempt: i32, kind: EventKind) {
    assert!(
        peppol::record_event(
            pool,
            id,
            attempt,
            kind,
            &own_event_key(id, attempt, kind),
            ""
        )
        .await
        .unwrap()
    );
}

#[tokio::test]
async fn an_attempt_that_may_have_gone_out_blocks_the_next_post() {
    let idp = TestIdp::new();
    let Some(state) = test_state(&idp).await else {
        return;
    };
    let pool = &state.pool;
    let (company, invoice, xml) = issued_invoice(pool).await;
    let id = dispatch(pool, company, invoice, xml).await;

    assert_eq!(
        peppol::begin_attempt(pool, id, ResendPolicy::SafeOnly)
            .await
            .unwrap(),
        BeginAttempt::Started(1)
    );
    // A successor that finds an attempt with no outcome must not POST.
    assert_eq!(
        peppol::begin_attempt(pool, id, ResendPolicy::AllowAgedUnresolved)
            .await
            .unwrap(),
        BeginAttempt::Blocked {
            attempt: 1,
            status: EventKind::SendAttempt
        }
    );

    outcome(pool, id, 1, EventKind::NotSent).await;
    assert_eq!(
        peppol::begin_attempt(pool, id, ResendPolicy::SafeOnly)
            .await
            .unwrap(),
        BeginAttempt::Started(2)
    );

    outcome(pool, id, 2, EventKind::Unresolved).await;
    for policy in [ResendPolicy::SafeOnly, ResendPolicy::AllowAgedUnresolved] {
        assert_eq!(
            peppol::begin_attempt(pool, id, policy).await.unwrap(),
            BeginAttempt::Blocked {
                attempt: 2,
                status: EventKind::Unresolved
            },
            "an unresolved attempt younger than the delay stays blocked"
        );
    }

    let rows = regnmed_db::list_dispatches(pool, company, invoice)
        .await
        .unwrap();
    assert_eq!(rows[0].channel, "peppol");
    assert_eq!(rows[0].to, "0192:123456785");
}

#[tokio::test]
async fn parallel_workers_start_one_attempt() {
    let idp = TestIdp::new();
    let Some(state) = test_state(&idp).await else {
        return;
    };
    let (company, invoice, xml) = issued_invoice(&state.pool).await;
    let id = dispatch(&state.pool, company, invoice, xml).await;

    let workers: Vec<_> = (0..16)
        .map(|_| {
            let pool = state.pool.clone();
            tokio::spawn(async move {
                peppol::begin_attempt(&pool, id, ResendPolicy::SafeOnly)
                    .await
                    .unwrap()
            })
        })
        .collect();
    let mut started = 0;
    for worker in workers {
        if matches!(worker.await.unwrap(), BeginAttempt::Started(_)) {
            started += 1;
        }
    }
    assert_eq!(started, 1);
}

#[tokio::test]
async fn events_and_provider_links_are_recorded_once() {
    let idp = TestIdp::new();
    let Some(state) = test_state(&idp).await else {
        return;
    };
    let pool = &state.pool;
    let (company, invoice, xml) = issued_invoice(pool).await;
    let id = dispatch(pool, company, invoice, xml).await;
    assert_eq!(
        peppol::begin_attempt(pool, id, ResendPolicy::SafeOnly)
            .await
            .unwrap(),
        BeginAttempt::Started(1)
    );

    let document = format!("doc_{}", Uuid::new_v4().simple());
    let key = provider_event_key(&document, EventKind::Delivered);
    assert!(
        peppol::record_event(pool, id, 1, EventKind::Delivered, &key, "")
            .await
            .unwrap()
    );
    assert!(
        !peppol::record_event(pool, id, 1, EventKind::Delivered, &key, "")
            .await
            .unwrap()
    );
    // A late event of lower rank does not move the status back.
    let accepted = provider_event_key(&document, EventKind::Accepted);
    peppol::record_event(pool, id, 1, EventKind::Accepted, &accepted, "")
        .await
        .unwrap();
    assert_eq!(
        peppol::latest_status(pool, id).await.unwrap(),
        Some((1, EventKind::Delivered))
    );

    peppol::link_provider_document(pool, &document, id, 1)
        .await
        .unwrap();
    peppol::link_provider_document(pool, &document, id, 1)
        .await
        .unwrap();
    let other = format!("doc_{}", Uuid::new_v4().simple());
    assert!(
        peppol::link_provider_document(pool, &other, id, 1)
            .await
            .is_err(),
        "one attempt cannot silently own two provider documents"
    );
}

#[derive(Clone)]
struct MockProvider {
    supports: bool,
    send_status: u16,
    posts: Arc<AtomicUsize>,
}

async fn mock_provider(supports: bool, send_status: u16) -> (Arc<Recommand>, Arc<AtomicUsize>) {
    let posts = Arc::new(AtomicUsize::new(0));
    let mock = MockProvider {
        supports,
        send_status,
        posts: posts.clone(),
    };
    let app = Router::new()
        .route(
            "/api/v1/verify-document-support",
            post(|State(m): State<MockProvider>| async move {
                Json(json!({ "success": true, "isValid": m.supports }))
            }),
        )
        .route(
            "/api/v1/{company}/send",
            post(|State(m): State<MockProvider>| async move {
                m.posts.fetch_add(1, Ordering::SeqCst);
                (
                    StatusCode::from_u16(m.send_status).unwrap(),
                    Json(json!({
                        "success": m.send_status == 200,
                        "id": format!("doc_{}", Uuid::new_v4().simple()),
                        "deliveryStatus": "delivered",
                    })),
                )
            }),
        )
        .with_state(mock);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (
        Arc::new(Recommand::new("k", "s", Some(&base)).unwrap()),
        posts,
    )
}

async fn run_dispatch(
    pool: &PgPool,
    company: Uuid,
    invoice: Uuid,
    runtime: &Runtime,
) -> job::JobStatus {
    let id = job::create_deduped(
        pool,
        company,
        Kind::InvoiceDispatch,
        &invoice.to_string(),
        &json!({ "invoice_id": invoice }),
        "test",
        None,
    )
    .await
    .unwrap();
    let claimed = job::claim(pool, id, "test-worker").await.unwrap().unwrap();
    job::process(pool, &claimed, "test-worker", runtime)
        .await
        .unwrap();
    job::get(pool, company, id).await.unwrap().unwrap()
}

async fn verified_sender(pool: &PgPool, company: Uuid) {
    peppol::insert_sender(
        pool,
        company,
        &format!("c_{}", Uuid::new_v4().simple()),
        true,
    )
    .await
    .unwrap();
}

#[tokio::test]
async fn the_worker_sends_once_and_records_delivery() {
    let idp = TestIdp::new();
    let Some(state) = test_state(&idp).await else {
        return;
    };
    let pool = &state.pool;
    let (company, invoice, _) = issued_invoice(pool).await;
    verified_sender(pool, company).await;
    let (provider, posts) = mock_provider(true, 200).await;
    let runtime = Runtime {
        peppol: Some(provider),
        ..Default::default()
    };

    let first = run_dispatch(pool, company, invoice, &runtime).await;
    assert_eq!(first.status, "done", "{:?}", first.error);
    assert_eq!(first.result.as_ref().unwrap()["status"], "levert");
    let second = run_dispatch(pool, company, invoice, &runtime).await;
    assert_eq!(second.result.as_ref().unwrap()["status"], "levert");
    assert_eq!(
        posts.load(Ordering::SeqCst),
        1,
        "a delivered invoice is never posted again"
    );
}

#[tokio::test]
async fn a_lost_response_is_never_posted_again() {
    let idp = TestIdp::new();
    let Some(state) = test_state(&idp).await else {
        return;
    };
    let pool = &state.pool;
    let (company, invoice, _) = issued_invoice(pool).await;
    verified_sender(pool, company).await;
    let (provider, posts) = mock_provider(true, 502).await;
    let runtime = Runtime {
        peppol: Some(provider),
        ..Default::default()
    };

    let first = run_dispatch(pool, company, invoice, &runtime).await;
    assert_eq!(first.result.as_ref().unwrap()["status"], "uavklart");
    let second = run_dispatch(pool, company, invoice, &runtime).await;
    assert_eq!(second.result.as_ref().unwrap()["status"], "uavklart");
    assert_eq!(posts.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn a_worker_that_takes_over_never_repeats_a_post() {
    let idp = TestIdp::new();
    let Some(state) = test_state(&idp).await else {
        return;
    };
    let pool = &state.pool;
    let (company, invoice, xml) = issued_invoice(pool).await;
    verified_sender(pool, company).await;
    // A worker committed its attempt and died before recording an outcome.
    let id = dispatch(pool, company, invoice, xml).await;
    peppol::begin_attempt(pool, id, ResendPolicy::SafeOnly)
        .await
        .unwrap();
    let (provider, posts) = mock_provider(true, 200).await;
    let runtime = Runtime {
        peppol: Some(provider),
        ..Default::default()
    };

    let job = run_dispatch(pool, company, invoice, &runtime).await;
    assert_eq!(job.result.as_ref().unwrap()["status"], "uavklart");
    assert_eq!(posts.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn email_needs_proof_that_the_buyer_cannot_receive_ehf() {
    let idp = TestIdp::new();
    let Some(state) = test_state(&idp).await else {
        return;
    };
    let pool = &state.pool;
    let (company, invoice, _) = issued_invoice(pool).await;
    let (provider, posts) = mock_provider(false, 200).await;
    let runtime = Runtime {
        peppol: Some(provider),
        directory_answer: Some(DirectoryAnswer::Unknown),
        ..Default::default()
    };

    let held = run_dispatch(pool, company, invoice, &runtime).await;
    assert_eq!(held.status, "failed");
    assert!(held.error.unwrap().contains("ikke sendt"));
    assert_eq!(posts.load(Ordering::SeqCst), 0);
    assert!(
        regnmed_db::list_dispatches(pool, company, invoice)
            .await
            .unwrap()
            .is_empty(),
        "an unknown lookup sends nothing on any channel"
    );
}

#[tokio::test]
async fn two_clicks_are_one_open_job() {
    let idp = TestIdp::new();
    let Some(state) = test_state(&idp).await else {
        return;
    };
    let pool = &state.pool;
    let (company, invoice, _) = issued_invoice(pool).await;
    let mut ids = std::collections::HashSet::new();
    for _ in 0..3 {
        ids.insert(
            job::create_deduped(
                pool,
                company,
                Kind::InvoiceDispatch,
                &invoice.to_string(),
                &json!({ "invoice_id": invoice }),
                "test",
                None,
            )
            .await
            .unwrap(),
        );
    }
    assert_eq!(ids.len(), 1);
}

async fn sent_dispatch(pool: &PgPool) -> (Uuid, Uuid, Uuid, String) {
    let (company, invoice, xml) = issued_invoice(pool).await;
    let id = dispatch(pool, company, invoice, xml).await;
    peppol::begin_attempt(pool, id, ResendPolicy::SafeOnly)
        .await
        .unwrap();
    let document = format!("doc_{}", Uuid::new_v4().simple());
    peppol::link_provider_document(pool, &document, id, 1)
        .await
        .unwrap();
    (company, invoice, id, document)
}

fn status_event(document: &str, status: &str, category: Option<&str>) -> Vec<u8> {
    json!({
        "eventType": "document.delivery_status_changed",
        "documentId": document,
        "teamId": "team_ours",
        "channel": "peppol",
        "status": status,
        "failure": category.map(|c| json!({ "category": c, "message": "feil" })),
    })
    .to_string()
    .into_bytes()
}

#[tokio::test]
async fn webhook_events_are_routed_idempotently_in_any_order() {
    let idp = TestIdp::new();
    let Some(state) = test_state(&idp).await else {
        return;
    };
    let pool = &state.pool;
    let (_, _, id, document) = sent_dispatch(pool).await;
    let team = Some("team_ours");

    let delivered = status_event(&document, "delivered", None);
    for _ in 0..2 {
        assert_eq!(
            handle_provider_event(pool, team, &delivered, 1).await,
            EventAction::Ack
        );
    }
    // The earlier event arrives last and does not move the status back.
    let sent = json!({
        "eventType": "document.sent",
        "documentId": document,
        "teamId": "team_ours",
        "deliveryStatus": "pending",
    })
    .to_string();
    assert_eq!(
        handle_provider_event(pool, team, sent.as_bytes(), 1).await,
        EventAction::Ack
    );
    assert_eq!(
        peppol::latest_status(pool, id).await.unwrap(),
        Some((1, EventKind::Delivered))
    );
}

#[tokio::test]
async fn an_ambiguous_transport_failure_is_not_a_safe_resend() {
    let idp = TestIdp::new();
    let Some(state) = test_state(&idp).await else {
        return;
    };
    let pool = &state.pool;
    let (_, _, id, document) = sent_dispatch(pool).await;
    let event = status_event(&document, "failed", Some("transport"));
    assert_eq!(
        handle_provider_event(pool, None, &event, 1).await,
        EventAction::Ack
    );
    assert_eq!(
        peppol::begin_attempt(pool, id, ResendPolicy::SafeOnly)
            .await
            .unwrap(),
        BeginAttempt::Blocked {
            attempt: 1,
            status: EventKind::DeliveryFailed
        }
    );
}

#[tokio::test]
async fn unusable_or_foreign_events_cannot_loop_or_change_anything() {
    let idp = TestIdp::new();
    let Some(state) = test_state(&idp).await else {
        return;
    };
    let pool = &state.pool;
    let (_, _, id, document) = sent_dispatch(pool).await;
    assert_eq!(
        handle_provider_event(pool, None, b"not json", 1).await,
        EventAction::Drop
    );
    assert_eq!(
        handle_provider_event(pool, None, br#"{"eventType":"document.new_kind"}"#, 1).await,
        EventAction::Ack
    );
    let event = status_event(&document, "delivered", None);
    assert_eq!(
        handle_provider_event(pool, Some("team_other"), &event, 1).await,
        EventAction::Ack
    );
    assert_eq!(
        peppol::latest_status(pool, id).await.unwrap(),
        Some((1, EventKind::SendAttempt))
    );

    let unknown = status_event("doc_not_linked_yet", "delivered", None);
    assert_eq!(
        handle_provider_event(pool, None, &unknown, 1).await,
        EventAction::Retry
    );
    assert_eq!(
        handle_provider_event(pool, None, &unknown, 5).await,
        EventAction::Ack
    );
}

#[tokio::test]
async fn company_verification_reaches_the_sender() {
    let idp = TestIdp::new();
    let Some(state) = test_state(&idp).await else {
        return;
    };
    let pool = &state.pool;
    let (company, _, _) = issued_invoice(pool).await;
    let provider_id = format!("c_{}", Uuid::new_v4().simple());
    peppol::insert_sender(pool, company, &provider_id, false)
        .await
        .unwrap();
    for (status, expected) in [("verified", true), ("rejected", false)] {
        let event = json!({
            "eventType": "company.verification",
            "companyId": provider_id,
            "status": status,
        })
        .to_string();
        assert_eq!(
            handle_provider_event(pool, None, event.as_bytes(), 1).await,
            EventAction::Ack
        );
        assert_eq!(
            peppol::load_sender(pool, company)
                .await
                .unwrap()
                .unwrap()
                .is_verified,
            expected
        );
    }
}

#[tokio::test]
async fn reconciliation_recovers_a_lost_send_by_its_xml_hash() {
    let idp = TestIdp::new();
    let Some(state) = test_state(&idp).await else {
        return;
    };
    let pool = &state.pool;
    let (company, invoice, xml) = issued_invoice(pool).await;
    let provider_company = format!("c_{}", Uuid::new_v4().simple());
    peppol::insert_sender(pool, company, &provider_company, true)
        .await
        .unwrap();
    let id = dispatch(pool, company, invoice, xml).await;
    // A worker died after its POST, long enough ago to be stale.
    sqlx::query(
        "insert into dispatch_event (id, dispatch_id, event_key, attempt, kind, received_at)
         values ($1, $2, $3, 1, 'sendeforsok', now() - interval '20 minutes')",
    )
    .bind(Uuid::now_v7())
    .bind(id)
    .bind(own_event_key(id, 1, EventKind::SendAttempt))
    .execute(pool)
    .await
    .unwrap();
    let (_, bytes) = regnmed_db::get_attachment(pool, company, xml)
        .await
        .unwrap();
    let stored = String::from_utf8(bytes).unwrap();
    let lost = format!("doc_{}", Uuid::new_v4().simple());
    let listed = lost.clone();
    let app = Router::new()
        .route(
            "/api/v1/documents",
            axum::routing::get(move || {
                let listed = listed.clone();
                async move {
                    Json(json!({
                        "success": true,
                        "documents": [{ "id": listed }],
                        "pagination": { "totalPages": 1 },
                    }))
                }
            }),
        )
        .route(
            "/api/v1/documents/{id}",
            axum::routing::get(move || {
                let (stored, lost) = (stored.clone(), lost.clone());
                async move {
                    Json(json!({
                        "success": true,
                        "document": {
                            "id": lost,
                            "xml": stored,
                            "deliveries": [{ "channel": "peppol", "status": "delivered" }],
                        },
                    }))
                }
            }),
        );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let provider = Recommand::new("k", "s", Some(&base)).unwrap();

    // The lock is global to the test database, so wait for another test's run.
    for _ in 0..100 {
        peppol_worker::reconcile(pool, &provider).await.unwrap();
        if peppol::latest_status(pool, id).await.unwrap() == Some((1, EventKind::Delivered)) {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    panic!("reconciliation did not recover the send");
}

#[tokio::test]
async fn a_signed_webhook_is_queued_and_a_forged_one_is_not() {
    let idp = TestIdp::new();
    let Some(base) = test_state(&idp).await else {
        return;
    };
    let unavailable = webhook(&base, b"{}", "sha256=00", Some("k")).await;
    assert_eq!(unavailable, StatusCode::SERVICE_UNAVAILABLE);
    let Some(nats) = crate::dispatch::start_nats().await else {
        return;
    };
    let js = regnmed_api::mailq::connect(&nats.url, None, None)
        .await
        .unwrap();
    regnmed_mail::peppol_events::ensure_stream(&js)
        .await
        .unwrap();
    let state = regnmed_api::AppState {
        mailq: Some(js.clone()),
        peppol_webhook_secret: Some("webhook-secret".into()),
        ..base
    };
    let body = br#"{"eventType":"document.sent","documentId":"doc_1"}"#;
    assert_eq!(
        webhook(&state, body, "sha256=00", Some("key-1")).await,
        StatusCode::UNAUTHORIZED
    );
    let signature = format!("sha256={}", hmac_hex(b"webhook-secret", body));
    for _ in 0..2 {
        assert_eq!(
            webhook(&state, body, &signature, Some("key-1")).await,
            StatusCode::OK
        );
    }
    let mut stream = js
        .get_stream(regnmed_mail::peppol_events::STREAM)
        .await
        .unwrap();
    assert_eq!(
        stream.info().await.unwrap().state.messages,
        1,
        "a forged event is not stored and a repeat is one message"
    );
}

async fn webhook(
    state: &regnmed_api::AppState,
    body: &[u8],
    signature: &str,
    key: Option<&str>,
) -> StatusCode {
    let mut request =
        axum::http::Request::post("/webhooks/recommand").header("x-signature", signature);
    if let Some(key) = key {
        request = request.header("x-idempotency-key", key);
    }
    regnmed_api::router(state.clone())
        .oneshot(request.body(axum::body::Body::from(body.to_vec())).unwrap())
        .await
        .unwrap()
        .status()
}

/// HMAC-SHA256, written out so the test does not trust the code it checks.
fn hmac_hex(key: &[u8], message: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let mut block = [0u8; 64];
    block[..key.len()].copy_from_slice(key);
    let pad = |byte: u8| block.iter().map(|b| b ^ byte).collect::<Vec<_>>();
    let inner = Sha256::new()
        .chain_update(pad(0x36))
        .chain_update(message)
        .finalize();
    hex::encode(
        Sha256::new()
            .chain_update(pad(0x5c))
            .chain_update(inner)
            .finalize(),
    )
}

#[tokio::test]
async fn the_follow_up_only_duns_an_invoice_peppol_delivered() {
    use regnmed_db::subscription_follow_up as opf;

    let idp = TestIdp::new();
    let Some(state) = test_state(&idp).await else {
        return;
    };
    let pool = &state.pool;
    let (drift, invoice, xml) = issued_invoice(pool).await;
    let customer = regnmed_db::create_company(pool, &unique_orgnumber(), "Abonnent AS")
        .await
        .unwrap();
    sqlx::query(
        "insert into abonnement_faktura_run (id, company_id, ar, maned, invoice_id)
         values ($1, $2, 2026, 11, $3)",
    )
    .bind(Uuid::now_v7())
    .bind(customer)
    .bind(invoice)
    .execute(pool)
    .await
    .unwrap();
    let is_unsent = || async {
        opf::unsent_invoices(pool, drift)
            .await
            .unwrap()
            .iter()
            .any(|f| f.invoice_id == invoice)
    };
    let open = || async {
        opf::open_invoices(pool, drift)
            .await
            .unwrap()
            .into_iter()
            .find(|f| f.invoice_id == invoice)
            .unwrap()
    };
    let late = NaiveDate::from_ymd_opt(2027, 1, 15).unwrap();
    assert!(is_unsent().await);

    let id = dispatch(pool, drift, invoice, xml).await;
    peppol::begin_attempt(pool, id, ResendPolicy::SafeOnly)
        .await
        .unwrap();
    assert!(!is_unsent().await, "an attempt in flight is not sent again");
    outcome(pool, id, 1, EventKind::NotSent).await;
    assert!(
        is_unsent().await,
        "proof that it never left means try again"
    );

    peppol::begin_attempt(pool, id, ResendPolicy::SafeOnly)
        .await
        .unwrap();
    outcome(pool, id, 2, EventKind::Unresolved).await;
    assert!(!is_unsent().await, "an unknown outcome is not resent");
    assert!(
        opf::dispatches_needing_attention(pool, drift)
            .await
            .unwrap()
            .iter()
            .any(|d| d.status == "uavklart")
    );
    let unresolved = open().await;
    assert!(!unresolved.sent);
    assert!(
        opf::purr(pool, drift, &unresolved, late)
            .await
            .unwrap()
            .is_none(),
        "no fee on an invoice the buyer may never have received"
    );

    let document = format!("doc_{}", Uuid::new_v4().simple());
    peppol::record_event(
        pool,
        id,
        2,
        EventKind::Delivered,
        &provider_event_key(&document, EventKind::Delivered),
        "",
    )
    .await
    .unwrap();
    assert!(open().await.sent);
    assert!(!is_unsent().await);
}

#[tokio::test]
async fn a_taken_over_job_never_mails_twice() {
    let idp = TestIdp::new();
    let Some(state) = test_state(&idp).await else {
        return;
    };
    let pool = &state.pool;
    let (company, invoice, _) = issued_invoice(pool).await;
    let id = job::create_deduped(
        pool,
        company,
        Kind::InvoiceDispatch,
        &invoice.to_string(),
        &json!({ "invoice_id": invoice, "email": "kunde@example.test" }),
        "test",
        None,
    )
    .await
    .unwrap();
    // The first worker published and logged, then died before finishing.
    regnmed_db::log_dispatch(
        pool,
        id,
        company,
        Some(invoice),
        None,
        None,
        "kunde@example.test",
        "Faktura 1",
        "test",
    )
    .await
    .unwrap();
    let mails = Arc::new(AtomicUsize::new(0));
    let counter = mails.clone();
    let mail: regnmed_db::peppol_worker::SendMail = Arc::new(move |_, _, _, _| {
        counter.fetch_add(1, Ordering::SeqCst);
        Box::pin(async { Ok(()) })
    });
    let runtime = Runtime {
        mail: Some(mail),
        directory_answer: Some(DirectoryAnswer::Absent),
        ..Default::default()
    };
    let claimed = job::claim(pool, id, "successor").await.unwrap().unwrap();
    job::process(pool, &claimed, "successor", &runtime)
        .await
        .unwrap();
    assert_eq!(
        job::get(pool, company, id).await.unwrap().unwrap().status,
        "done"
    );
    assert_eq!(mails.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn the_channel_check_reports_an_existing_dispatch() {
    let idp = TestIdp::new();
    let Some(state) = test_state(&idp).await else {
        return;
    };
    let pool = &state.pool;
    let (company, invoice, xml) = issued_invoice(pool).await;
    dispatch(pool, company, invoice, xml).await;
    let id = job::create_deduped(
        pool,
        company,
        Kind::PeppolLookup,
        &invoice.to_string(),
        &json!({ "invoice_id": invoice }),
        "test",
        None,
    )
    .await
    .unwrap();
    let runtime = Runtime {
        directory_answer: Some(DirectoryAnswer::Absent),
        ..Default::default()
    };
    let claimed = job::claim(pool, id, "test-worker").await.unwrap().unwrap();
    job::process(pool, &claimed, "test-worker", &runtime)
        .await
        .unwrap();
    let result = job::get(pool, company, id)
        .await
        .unwrap()
        .unwrap()
        .result
        .unwrap();
    assert_eq!(result["kanal"], "peppol");
    assert_eq!(result["mottaker"], "0192:123456785");
}

#[tokio::test]
async fn delivered_status_does_not_depend_on_the_sender() {
    let idp = TestIdp::new();
    let Some(state) = test_state(&idp).await else {
        return;
    };
    let pool = &state.pool;
    let (company, invoice, id, document) = sent_dispatch(pool).await;
    peppol::record_event(
        pool,
        id,
        1,
        EventKind::Delivered,
        &provider_event_key(&document, EventKind::Delivered),
        "",
    )
    .await
    .unwrap();
    // No sender row and no provider: the answer is still that it was delivered.
    let done = run_dispatch(pool, company, invoice, &Runtime::default()).await;
    assert_eq!(done.status, "done", "{:?}", done.error);
    assert_eq!(done.result.unwrap()["status"], "levert");
}

#[tokio::test]
async fn a_dispatch_without_an_attempt_is_still_unsent() {
    use regnmed_db::subscription_follow_up as opf;

    let idp = TestIdp::new();
    let Some(state) = test_state(&idp).await else {
        return;
    };
    let pool = &state.pool;
    let (drift, invoice, xml) = issued_invoice(pool).await;
    let customer = regnmed_db::create_company(pool, &unique_orgnumber(), "Abonnent AS")
        .await
        .unwrap();
    sqlx::query(
        "insert into abonnement_faktura_run (id, company_id, ar, maned, invoice_id)
         values ($1, $2, 2026, 11, $3)",
    )
    .bind(Uuid::now_v7())
    .bind(customer)
    .bind(invoice)
    .execute(pool)
    .await
    .unwrap();
    dispatch(pool, drift, invoice, xml).await;
    assert!(
        opf::unsent_invoices(pool, drift)
            .await
            .unwrap()
            .iter()
            .any(|f| f.invoice_id == invoice)
    );
}
