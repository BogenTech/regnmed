//! Tilbud → ordre → faktura end to end: gap-free series per kind, the
//! one-way status trapp, lossless conversion with chain links, the
//! ordinary atomic invoice path at the end, and the guards (edit after
//! akseptert, double ordre, double faktura). Requires DATABASE_URL
//! (skips otherwise).

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
async fn tilbud_ordre_faktura_chain() {
    let idp = TestIdp::new();
    let Some(state) = test_state(&idp).await else {
        return;
    };
    let sub = format!("test|{}", Uuid::new_v4());
    let person = regnmed_db::ensure_person(&state.pool, &sub, Some("Kari Bokfører"), None)
        .await
        .unwrap();
    let company = regnmed_db::create_company(&state.pool, &unique_orgnr(), "Tilbud & Co AS")
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
    let (_, party_no) =
        regnmed_db::create_party(&state.pool, company, "kunde", "Kjøper AS", None, None)
            .await
            .unwrap();
    let token = idp.token(&sub, "Kari Bokfører");

    // Tilbud 1: utkast, editable.
    let (status, quote) = request(
        &state,
        "POST",
        &format!("/companies/{company}/quotes"),
        &token,
        Some(
            serde_json::json!({
                "party_no": party_no,
                "lines": [{ "description": "Prosjektering", "unit_price_ore": 50_000_00, "vat_code": "3" }],
            })
            .to_string(),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body: {quote}");
    assert_eq!(quote["doc_no"], 1, "own gap-free series");
    let quote_id = quote["id"].as_str().unwrap().to_string();

    // Edit while utkast: price negotiated down.
    let (status, _) = request(
        &state,
        "PUT",
        &format!("/companies/{company}/quotes/{quote_id}"),
        &token,
        Some(
            serde_json::json!({
                "lines": [{ "description": "Prosjektering", "unit_price_ore": 45_000_00, "vat_code": "3" }],
            })
            .to_string(),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // Tilbud PDF renders with the current state.
    let (status, pdf) = send(
        &state,
        "GET",
        &format!("/companies/{company}/quotes/{quote_id}/pdf"),
        &token,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(pdf.starts_with(b"%PDF-1.4"));
    assert!(find(&pdf, b"TILBUD").is_some());
    assert!(find(&pdf, b"Tilbudsnr").is_some());
    assert!(
        find(&pdf, b"45 000,00").is_some(),
        "edited price on the PDF"
    );
    assert!(find(&pdf, b"KID").is_none(), "a tilbud is not payable");
    assert!(find(&pdf, b"BETALINGSINFORMASJON").is_none());

    // utkast → sendt → akseptert; avslått/backwards is rejected after.
    for s in ["sendt", "akseptert"] {
        let (status, body) = request(
            &state,
            "POST",
            &format!("/companies/{company}/quotes/{quote_id}/status"),
            &token,
            Some(serde_json::json!({ "status": s }).to_string()),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "→ {s}: {body}");
    }
    let (status, _) = request(
        &state,
        "POST",
        &format!("/companies/{company}/quotes/{quote_id}/status"),
        &token,
        Some(serde_json::json!({ "status": "avslatt" }).to_string()),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "trappen er enveis");

    // Editing an accepted tilbud is rejected.
    let (status, _) = request(
        &state,
        "PUT",
        &format!("/companies/{company}/quotes/{quote_id}"),
        &token,
        Some(
            serde_json::json!({
                "lines": [{ "description": "X", "unit_price_ore": 1, "vat_code": "3" }],
            })
            .to_string(),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    // Akseptert tilbud → ordre (lines copied); a second ordre from the
    // same tilbud is rejected.
    let (status, order) = request(
        &state,
        "POST",
        &format!("/companies/{company}/quotes/{quote_id}/order"),
        &token,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body: {order}");
    assert_eq!(order["doc_no"], 1, "ordre series starts at 1");
    let order_id = order["order_id"].as_str().unwrap().to_string();
    let (status, _) = request(
        &state,
        "POST",
        &format!("/companies/{company}/quotes/{quote_id}/order"),
        &token,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "one ordre per tilbud");

    // Ordrebekreftelse PDF.
    let (status, ordre_pdf) = send(
        &state,
        "GET",
        &format!("/companies/{company}/orders/{order_id}/pdf"),
        &token,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(find(&ordre_pdf, b"ORDREBEKREFTELSE").is_some());

    // Ordre → faktura through the ordinary path; chain + PDF verified;
    // links carried; second conversion rejected.
    let (status, issued) = request(
        &state,
        "POST",
        &format!("/companies/{company}/orders/{order_id}/invoice"),
        &token,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body: {issued}");
    assert_eq!(issued["invoice_no"], 1);
    assert_eq!(issued["gross_ore"], 56_250_00, "45 000 + 25 % mva");
    let report = regnmed_db::verify_chain(&state.pool, company)
        .await
        .unwrap();
    assert_eq!(report.vouchers_checked, 1);
    assert_eq!(
        regnmed_db::verify_attachments(&state.pool, company)
            .await
            .unwrap(),
        1,
        "the invoice got its stored PDF"
    );
    let (status, _) = request(
        &state,
        "POST",
        &format!("/companies/{company}/orders/{order_id}/invoice"),
        &token,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "one ordre → one faktura");

    // The listing carries the whole chain: T-1 → O-1 → faktura 1.
    let (_, orders) = request(
        &state,
        "GET",
        &format!("/companies/{company}/orders"),
        &token,
        None,
    )
    .await;
    let row = &orders["documents"][0];
    assert_eq!(row["status"], "fakturert");
    assert_eq!(row["tilbud_no"], 1);
    assert_eq!(row["invoice_no"], 1);

    // Avslått path: a rejected tilbud is history, not a hole — the next
    // tilbud takes number 3.
    let (_, rejected) = request(
        &state,
        "POST",
        &format!("/companies/{company}/quotes"),
        &token,
        Some(
            serde_json::json!({
                "party_no": party_no,
                "lines": [{ "description": "Noe annet", "unit_price_ore": 1_000_00 }],
            })
            .to_string(),
        ),
    )
    .await;
    assert_eq!(rejected["doc_no"], 2);
    let rejected_id = rejected["id"].as_str().unwrap();
    let (status, _) = request(
        &state,
        "POST",
        &format!("/companies/{company}/quotes/{rejected_id}/status"),
        &token,
        Some(serde_json::json!({ "status": "avslatt" }).to_string()),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = request(
        &state,
        "POST",
        &format!("/companies/{company}/quotes/{rejected_id}/order"),
        &token,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "avslått blir aldri ordre");
    let (_, third) = request(
        &state,
        "POST",
        &format!("/companies/{company}/quotes"),
        &token,
        Some(
            serde_json::json!({
                "party_no": party_no,
                "lines": [{ "description": "Tredje", "unit_price_ore": 100 }],
            })
            .to_string(),
        ),
    )
    .await;
    assert_eq!(third["doc_no"], 3, "rejected tilbud is history, not a hole");

    // A direct ordre (no tilbud) also works.
    let (status, direct) = request(
        &state,
        "POST",
        &format!("/companies/{company}/orders"),
        &token,
        Some(
            serde_json::json!({
                "party_no": party_no,
                "lines": [{ "description": "Hasteordre", "unit_price_ore": 2_000_00, "vat_code": "3" }],
            })
            .to_string(),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body: {direct}");
    assert_eq!(direct["doc_no"], 2, "ordre series continues");
}
