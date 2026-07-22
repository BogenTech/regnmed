//! End-to-end faktura over the web API: issue an invoice (gap-free
//! number, valid KID, automatic ledger + reskontro posting), watch an
//! OCR payment identify it by KID, credit it, and verify the chain.
//! Requires DATABASE_URL (skips otherwise).

mod common;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use common::{TestIdp, test_state, unique_orgnr};
use tower::ServiceExt;
use uuid::Uuid;

use regnmed_api::{AppState, router};

async fn request(
    state: &AppState,
    method: &str,
    uri: &str,
    bearer: &str,
    body: Option<String>,
    json: bool,
) -> (StatusCode, serde_json::Value) {
    let mut builder = Request::builder()
        .method(method)
        .uri(uri)
        .header("authorization", format!("Bearer {bearer}"));
    if json {
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

fn record(parts: &[&str]) -> String {
    let line: String = parts.concat();
    assert_eq!(line.len(), 80, "fixture record must be 80 chars: {line}");
    line
}

/// An OCR file paying `amount` (17-digit øre) on `kid` (padded to 25).
fn ocr_file(transmission: &str, kid: &str, amount_17: &str) -> String {
    let kid_field = format!("{kid:>25}");
    [
        record(&[
            "NY000010",
            "00111222",
            transmission,
            "00008080",
            &"0".repeat(49),
        ]),
        record(&[
            "NY090020",
            "000988555",
            "0000001",
            "99991042764",
            &"0".repeat(45),
        ]),
        record(&[
            "NY091030", "0000001", "150226", "00", "20", "0", "00001", "0", amount_17, &kid_field,
            "000000",
        ]),
        record(&[
            "NY090088",
            "00000001",
            "00000003",
            amount_17,
            "150226",
            "150226",
            "150226",
            &"0".repeat(21),
        ]),
        record(&[
            "NY000089",
            "00000001",
            "00000005",
            amount_17,
            "150226",
            &"0".repeat(33),
        ]),
    ]
    .join("\n")
}

#[tokio::test]
async fn invoice_to_payment_to_credit_note_loop() {
    let idp = TestIdp::new();
    let Some(state) = test_state(&idp).await else {
        return;
    };
    let sub = format!("test|{}", Uuid::new_v4());
    let person = regnmed_db::ensure_person(&state.pool, &sub, Some("Kari Bokfører"), None)
        .await
        .unwrap();
    let company = regnmed_db::create_company(&state.pool, &unique_orgnr(), "Faktura AS")
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
        ("1920", "Bankinnskudd"),
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
        regnmed_db::create_party(&state.pool, company, "kunde", "Kunde & Co AS", None, None)
            .await
            .unwrap();
    let token = idp.token(&sub, "Kari Bokfører");

    // Issue: 2,5 timer à 4 000 kr + 25 % mva = 12 500 kr gross.
    let (status, issued) = request(
        &state,
        "POST",
        &format!("/companies/{company}/invoices"),
        &token,
        Some(
            serde_json::json!({
                "party_no": party_no,
                "invoice_date": "2026-02-01",
                "due_date": "2026-02-15",
                "lines": [{
                    "description": "Konsulentbistand",
                    "quantity_milli": 2500,
                    "unit_price_ore": 4_000_00,
                    "vat_code": "3",
                }],
            })
            .to_string(),
        ),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body: {issued}");
    assert_eq!(issued["invoice_no"], 1, "gap-free numbering starts at 1");
    assert_eq!(issued["net_ore"], 10_000_00);
    assert_eq!(issued["vat_ore"], 2_500_00);
    assert_eq!(issued["gross_ore"], 12_500_00);
    let kid = issued["kid"].as_str().unwrap().to_string();
    assert!(regnmed_core::kid::is_valid_mod10(&kid));
    let invoice_id = issued["invoice_id"].as_str().unwrap().to_string();

    // A bad invoice attempt (unknown vat code) must not burn a number.
    let (status, _) = request(
        &state,
        "POST",
        &format!("/companies/{company}/invoices"),
        &token,
        Some(
            serde_json::json!({
                "party_no": party_no,
                "invoice_date": "2026-02-02",
                "due_date": "2026-02-16",
                "lines": [{ "description": "X", "unit_price_ore": 100, "vat_code": "99" }],
            })
            .to_string(),
        ),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    // The ledger posted and the chain holds.
    let report = regnmed_db::verify_chain(&state.pool, company)
        .await
        .unwrap();
    assert_eq!(report.vouchers_checked, 1);

    // The invoice is open for its full gross.
    let (_, list) = request(
        &state,
        "GET",
        &format!("/companies/{company}/invoices?open=true"),
        &token,
        None,
        false,
    )
    .await;
    assert_eq!(list["invoices"][0]["remaining_ore"], 12_500_00);
    assert_eq!(list["invoices"][0]["kid"], kid);

    // An OCR payment on the invoice's KID identifies the invoice.
    let transmission = format!("{:07}", rand_7());
    let file = ocr_file(&transmission, &kid, "00000000001250000");
    let (status, _) = request(
        &state,
        "POST",
        &format!("/companies/{company}/ocr/files?account=1920"),
        &token,
        Some(file),
        false,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (_, payments) = request(
        &state,
        "GET",
        &format!("/companies/{company}/ocr/payments"),
        &token,
        None,
        false,
    )
    .await;
    let payment = payments["payments"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["kid"] == kid.as_str())
        .expect("payment listed");
    assert_eq!(payment["invoice_no"], 1, "KID resolved to the invoice");

    // Credit note: negates, posts, auto-matches — nothing open afterwards.
    let (status, credit) = request(
        &state,
        "POST",
        &format!("/companies/{company}/invoices/{invoice_id}/credit-note"),
        &token,
        None,
        false,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body: {credit}");
    assert_eq!(credit["invoice_no"], 2, "no gap despite the failed attempt");
    assert_eq!(credit["gross_ore"], -12_500_00);

    let (_, open) = request(
        &state,
        "GET",
        &format!("/companies/{company}/invoices?open=true"),
        &token,
        None,
        false,
    )
    .await;
    assert!(
        open["invoices"].as_array().unwrap().is_empty(),
        "invoice and kreditnota settle each other: {open}"
    );

    // Double-crediting is rejected; the chain still verifies (2 vouchers).
    let (status, _) = request(
        &state,
        "POST",
        &format!("/companies/{company}/invoices/{invoice_id}/credit-note"),
        &token,
        None,
        false,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let report = regnmed_db::verify_chain(&state.pool, company)
        .await
        .unwrap();
    assert_eq!(report.vouchers_checked, 2);
}

fn rand_7() -> u32 {
    u32::from_be_bytes(Uuid::new_v4().as_bytes()[..4].try_into().unwrap()) % 10_000_000
}
