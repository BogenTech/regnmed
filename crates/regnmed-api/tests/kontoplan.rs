//! Kontoplan wizard + manual åpningsbalanse over the web API: a SAF-T
//! file with a 5-digit chart is refused raw, analyzed with suggestions,
//! then imported with the reviewed mapping (two foreign accounts merge
//! onto one NS 4102 account); a company without SAF-T enters its
//! opening balance manually — zero-sum enforced, empty-ledger only.
//! Requires DATABASE_URL (skips otherwise).

mod common;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use chrono::{NaiveDate, TimeZone, Utc};
use common::{TestIdp, test_state, unique_orgnr};
use regnmed_core::saft::{SaftAccount, SaftInput, SaftJournal, SaftLine, SaftTransaction};
use serde_json::json;
use tower::ServiceExt;
use uuid::Uuid;

use regnmed_api::{AppState, router};

fn date(y: i32, m: u32, d: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(y, m, d).unwrap()
}

/// A foreign system with a 5-digit chart: 19200 bank, 30000 salg, and
/// two receivable accounts (15000/15001) that should merge onto 1500.
fn foreign_saft_5digit() -> String {
    let account = |number: &str, name: &str, opening: i64, closing: i64| SaftAccount {
        number: number.into(),
        name: name.into(),
        created: date(2020, 1, 1),
        opening_ore: opening,
        closing_ore: closing,
    };
    let line = |no: i32, account: &str, ore: i64| SaftLine {
        line_no: no,
        account_number: account.into(),
        description: None,
        amount_ore: ore,
        vat_code: None,
        tax_percent_bp: None,
        customer_id: None,
        supplier_id: None,
    };
    let input = SaftInput {
        orgnr: "923609016".into(),
        company_name: "Femsifret AS".into(),
        contact_first_name: "Kari".into(),
        contact_last_name: "Nordmann".into(),
        file_created: date(2026, 7, 23),
        software_version: "old".into(),
        start: date(2026, 1, 1),
        end: date(2026, 12, 31),
        accounts: vec![
            account("19200", "Driftskonto", 5_000_00, 4_000_00),
            account("15000", "Kunder Norge", 1_000_00, 1_500_00),
            account("15001", "Kunder utland", 500_00, 1_000_00),
            account("20500", "Egenkapital", -6_500_00, -6_500_00),
            account("30000", "Salg", 0, -1_000_00),
        ],
        customers: vec![],
        suppliers: vec![],
        tax_codes: vec![],
        journals: vec![SaftJournal {
            code: "S".into(),
            name: "Salg".into(),
            transactions: vec![SaftTransaction {
                fiscal_year: 2026,
                number: 1,
                date: date(2026, 3, 1),
                description: "Salg inn/utland".into(),
                created_by: "old".into(),
                created_at: Utc.with_ymd_and_hms(2026, 3, 1, 9, 0, 0).unwrap(),
                reverses: None,
                lines: vec![
                    line(1, "15000", 500_00),
                    line(2, "15001", 500_00),
                    line(3, "30000", -1_000_00),
                ],
            }],
        }],
    };
    regnmed_core::saft::render(&input)
}

async fn request(
    state: &AppState,
    method: &str,
    uri: &str,
    bearer: &str,
    content_type: Option<&str>,
    body: String,
) -> (StatusCode, serde_json::Value) {
    let mut builder = Request::builder()
        .method(method)
        .uri(uri)
        .header("authorization", format!("Bearer {bearer}"));
    if let Some(ct) = content_type {
        builder = builder.header("content-type", ct);
    }
    let response = router(state.clone())
        .oneshot(builder.body(Body::from(body)).unwrap())
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

async fn balance(pool: &sqlx::PgPool, company: Uuid, account: &str) -> i64 {
    sqlx::query_scalar(
        "select coalesce(sum(e.amount_ore), 0)::bigint
         from entry e join account a on a.id = e.account_id
         where a.company_id = $1 and a.number = $2",
    )
    .bind(company)
    .bind(account)
    .fetch_one(pool)
    .await
    .unwrap()
}

#[tokio::test]
async fn foreign_chart_maps_through_the_wizard() {
    let idp = TestIdp::new();
    let Some(state) = test_state(&idp).await else {
        return;
    };
    let sub = format!("test|{}", Uuid::new_v4());
    let admin = regnmed_db::ensure_person(&state.pool, &sub, Some("Mona Mapper"), None)
        .await
        .unwrap();
    let company = regnmed_db::create_company(&state.pool, &unique_orgnr(), "Mappet AS")
        .await
        .unwrap();
    regnmed_db::ensure_company_member(&state.pool, company, admin, "admin")
        .await
        .unwrap();
    let token = idp.token(&sub, "Mona Mapper");
    let xml = foreign_saft_5digit();

    // Raw import of a 5-digit chart is refused, as before.
    let (status, body) = request(
        &state,
        "POST",
        &format!("/companies/{company}/import/saft"),
        &token,
        None,
        xml.clone(),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");

    // Analysis suggests the truncations.
    let (status, analysis) = request(
        &state,
        "POST",
        &format!("/companies/{company}/import/saft/analyze"),
        &token,
        None,
        xml.clone(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{analysis}");
    assert_eq!(analysis["needs_mapping"], true);
    let suggestion = |id: &str| -> serde_json::Value {
        analysis["accounts"]
            .as_array()
            .unwrap()
            .iter()
            .find(|a| a["account_id"] == id)
            .unwrap()
            .clone()
    };
    assert_eq!(suggestion("19200")["suggested"], "1920");
    assert_eq!(suggestion("19200")["reason"], "avkortet");
    assert_eq!(suggestion("15000")["suggested"], "1500");
    assert_eq!(suggestion("15001")["suggested"], "1500", "merge suggested");

    // Import with the reviewed mapping — including the deliberate merge.
    let envelope = json!({
        "file": xml,
        "mapping": {
            "19200": "1920", "15000": "1500", "15001": "1500",
            "20500": "2050", "30000": "3000",
        },
    });
    let (status, report) = request(
        &state,
        "POST",
        &format!("/companies/{company}/import/saft"),
        &token,
        Some("application/json"),
        envelope.to_string(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{report}");
    assert_eq!(
        report["accounts"], 4,
        "five foreign accounts → four NS 4102"
    );
    assert_eq!(report["vouchers"], 1);
    assert_eq!(report["opening_posted"], true);

    // Balances land merged on the mapped accounts, chain verifies.
    assert_eq!(balance(&state.pool, company, "1500").await, 2_500_00);
    assert_eq!(balance(&state.pool, company, "1920").await, 5_000_00);
    assert_eq!(balance(&state.pool, company, "3000").await, -1_000_00);
    let chain = regnmed_db::verify_chain(&state.pool, company)
        .await
        .unwrap();
    assert_eq!(chain.vouchers_checked, 2, "opening + history");
}

#[tokio::test]
async fn manual_opening_balance_posts_once_and_balances() {
    let idp = TestIdp::new();
    let Some(state) = test_state(&idp).await else {
        return;
    };
    let sub = format!("test|{}", Uuid::new_v4());
    let admin = regnmed_db::ensure_person(&state.pool, &sub, Some("Ove Oppstart"), None)
        .await
        .unwrap();
    let company = regnmed_db::create_company(&state.pool, &unique_orgnr(), "Manuell AS")
        .await
        .unwrap();
    regnmed_db::ensure_company_member(&state.pool, company, admin, "admin")
        .await
        .unwrap();
    for (number, name) in [("1920", "Bank"), ("2000", "Aksjekapital")] {
        regnmed_db::ensure_account(&state.pool, company, number, name)
            .await
            .unwrap();
    }
    let token = idp.token(&sub, "Ove Oppstart");
    let uri = format!("/companies/{company}/opening-balance");

    // Unbalanced is refused with the discrepancy named.
    let (status, body) = request(
        &state,
        "POST",
        &uri,
        &token,
        Some("application/json"),
        json!({"date": "2026-01-01", "lines": [
            {"account": "1920", "amount_ore": 100_000_00},
            {"account": "2000", "amount_ore": -99_000_00},
        ]})
        .to_string(),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(body["error"].as_str().unwrap().contains("null"), "{body}");

    // Balanced posts as an Åpningsbalanse voucher.
    let (status, posted) = request(
        &state,
        "POST",
        &uri,
        &token,
        Some("application/json"),
        json!({"date": "2026-01-01", "lines": [
            {"account": "1920", "amount_ore": 100_000_00},
            {"account": "2000", "amount_ore": -100_000_00},
        ]})
        .to_string(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{posted}");
    assert_eq!(balance(&state.pool, company, "1920").await, 100_000_00);

    // The ledger is no longer empty — a second opening is refused.
    let (status, _) = request(
        &state,
        "POST",
        &uri,
        &token,
        Some("application/json"),
        json!({"date": "2026-01-01", "lines": [
            {"account": "1920", "amount_ore": 1_00},
            {"account": "2000", "amount_ore": -1_00},
        ]})
        .to_string(),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}
