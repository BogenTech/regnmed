//! Faktura-PDF end to end: firmaopplysninger set over the API, an
//! issued invoice stores its salgsdokument as a voucher attachment IN
//! the issuing transaction, the PDF endpoint serves it hash-checked,
//! kreditnotaer get their own document, and purringer render to PDF on
//! demand. Requires DATABASE_URL (skips otherwise).

mod common;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use common::{TestIdp, test_state, unique_orgnr};
use tower::ServiceExt;
use uuid::Uuid;

use regnmed_api::{AppState, router};

async fn send(
    state: &AppState,
    method: &str,
    uri: &str,
    bearer: &str,
    body: Option<String>,
) -> (StatusCode, Vec<u8>) {
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
    (status, bytes.to_vec())
}

async fn request(
    state: &AppState,
    method: &str,
    uri: &str,
    bearer: &str,
    body: Option<String>,
) -> (StatusCode, serde_json::Value) {
    let (status, bytes) = send(state, method, uri, bearer, body).await;
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null),
    )
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

#[tokio::test]
async fn invoice_pdf_is_stored_served_and_verified() {
    let idp = TestIdp::new();
    let Some(state) = test_state(&idp).await else {
        return;
    };
    let sub = format!("test|{}", Uuid::new_v4());
    let person = regnmed_db::ensure_person(&state.pool, &sub, Some("Kari Bokfører"), None)
        .await
        .unwrap();
    let company = regnmed_db::create_company(&state.pool, &unique_orgnr(), "PDF & Co AS")
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

    // Firmaopplysninger over the API (admin) — printed on the PDF.
    let (status, _) = request(
        &state,
        "PUT",
        &format!("/companies/{company}/settings"),
        &token,
        Some(
            serde_json::json!({
                "address": "Storgata 1, 0155 Oslo",
                "bank_account": "1234.56.78903",
                "orgform": "AS",
            })
            .to_string(),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    // Party contact.
    let (status, _) = request(
        &state,
        "PUT",
        &format!("/companies/{company}/parties/{party_id}/contact"),
        &token,
        Some(
            serde_json::json!({ "address": "Kundeveien 2, 5003 Bergen", "email": "post@kunde.example" })
                .to_string(),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // Issue an invoice: the PDF must exist as an attachment already.
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
                "lines": [{
                    "description": "Konsulentbistand",
                    "unit_price_ore": 10_000_00,
                    "vat_code": "3",
                }],
            })
            .to_string(),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body: {issued}");
    let invoice_id = issued["invoice_id"].as_str().unwrap().to_string();

    let (status, pdf) = send(
        &state,
        "GET",
        &format!("/companies/{company}/invoices/{invoice_id}/pdf"),
        &token,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(pdf.starts_with(b"%PDF-1.4"), "a real PDF is served");
    for expected in [
        &b"FAKTURA"[..],
        b"PDF & Co AS",
        b"Storgata 1, 0155 Oslo",
        b"Foretaksregisteret",
        b"Kundeveien 2, 5003 Bergen",
        b"Kontonummer: 1234.56.78903",
        b"Konsulentbistand",
        b"12 500,00",
    ] {
        assert!(
            find(&pdf, expected).is_some(),
            "PDF missing {:?}",
            String::from_utf8_lossy(expected)
        );
    }

    // The attachment is part of oppbevaringen and passes verification.
    let attachments = regnmed_db::verify_attachments(&state.pool, company)
        .await
        .unwrap();
    assert_eq!(attachments, 1);

    // Kreditnota gets its own stored document.
    let (status, credit) = request(
        &state,
        "POST",
        &format!("/companies/{company}/invoices/{invoice_id}/credit-note"),
        &token,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body: {credit}");
    let credit_id = credit["invoice_id"].as_str().unwrap();
    let (status, credit_pdf) = send(
        &state,
        "GET",
        &format!("/companies/{company}/invoices/{credit_id}/pdf"),
        &token,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(find(&credit_pdf, b"KREDITNOTA").is_some());
    assert!(find(&credit_pdf, b"Krediterer faktura").is_some());

    // A purring renders to PDF from its stored text.
    // (Invoice 1 is settled by the kreditnota, so remind on a fresh one.)
    let (_, unpaid) = request(
        &state,
        "POST",
        &format!("/companies/{company}/invoices"),
        &token,
        Some(
            serde_json::json!({
                "party_no": party_no,
                "invoice_date": "2026-01-10",
                "due_date": "2026-01-24",
                "lines": [{ "description": "Gammel jobb", "unit_price_ore": 1_000_00 }],
            })
            .to_string(),
        ),
    )
    .await;
    let unpaid_id = unpaid["invoice_id"].as_str().unwrap();
    let today: chrono::NaiveDate = sqlx::query_scalar("select current_date")
        .fetch_one(&state.pool)
        .await
        .unwrap();
    let (status, reminder) = request(
        &state,
        "POST",
        &format!("/companies/{company}/invoices/{unpaid_id}/reminders"),
        &token,
        Some(
            serde_json::json!({
                "steg": "paminnelse",
                "frist_date": (today + chrono::Days::new(14)).to_string(),
            })
            .to_string(),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body: {reminder}");
    let reminder_id = reminder["reminder_id"].as_str().unwrap();
    let (status, purring_pdf) = send(
        &state,
        "GET",
        &format!("/companies/{company}/invoices/{unpaid_id}/reminders/{reminder_id}?format=pdf"),
        &token,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(purring_pdf.starts_with(b"%PDF-1.4"));
    assert!(find(&purring_pdf, b"/BaseFont /Courier").is_some());

    // Settings are guarded: a bokforing member cannot change them.
    let clerk_sub = format!("test|{}", Uuid::new_v4());
    let clerk = regnmed_db::ensure_person(&state.pool, &clerk_sub, Some("Bo Bokfører"), None)
        .await
        .unwrap();
    regnmed_db::ensure_company_member(&state.pool, company, clerk, "bokforing")
        .await
        .unwrap();
    let clerk_token = idp.token(&clerk_sub, "Bo Bokfører");
    let (status, _) = request(
        &state,
        "PUT",
        &format!("/companies/{company}/settings"),
        &clerk_token,
        Some(serde_json::json!({ "address": "X" }).to_string()),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}
