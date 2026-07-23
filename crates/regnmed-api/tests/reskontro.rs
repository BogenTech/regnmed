//! End-to-end reskontro over the web API: flag a reskontro account,
//! create a customer, post an invoice + partial payment against the
//! party (hash v2), match them, verify the chain still holds, and check
//! the enforcement rules. Requires DATABASE_URL (skips otherwise).

mod common;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use chrono::NaiveDate;
use common::{TestIdp, test_state, unique_orgnr};
use regnmed_core::Ore;
use regnmed_core::voucher::{EntryDraft, VoucherDraft};
use tower::ServiceExt;
use uuid::Uuid;

use regnmed_api::{AppState, router};

async fn request(
    state: &AppState,
    method: &str,
    uri: &str,
    bearer: &str,
    body: Option<serde_json::Value>,
) -> (StatusCode, serde_json::Value) {
    let mut builder = Request::builder()
        .method(method)
        .uri(uri)
        .header("authorization", format!("Bearer {bearer}"));
    if body.is_some() {
        builder = builder.header("content-type", "application/json");
    }
    let response = router(state.clone())
        .oneshot(
            builder
                .body(Body::from(body.map(|b| b.to_string()).unwrap_or_default()))
                .unwrap(),
        )
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

fn entry(account: &str, ore: i64, party_no: Option<&str>) -> EntryDraft {
    EntryDraft {
        account_number: account.into(),
        amount: Ore(ore),
        vat_code: None,
        description: None,
        party_no: party_no.map(str::to_owned),
    }
}

fn voucher(date: NaiveDate, description: &str, entries: Vec<EntryDraft>) -> VoucherDraft {
    VoucherDraft {
        journal_code: "GL".into(),
        voucher_date: date,
        description: description.into(),
        reverses: None,
        entries,
    }
}

fn date(m: u32, d: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, m, d).unwrap()
}

#[tokio::test]
async fn full_reskontro_flow_with_hash_v2() {
    let idp = TestIdp::new();
    let Some(state) = test_state(&idp).await else {
        return;
    };
    let sub = format!("test|{}", Uuid::new_v4());
    let person = regnmed_db::ensure_person(&state.pool, &sub, Some("Kari Bokfører"), None)
        .await
        .unwrap();
    let company = regnmed_db::create_company(&state.pool, &unique_orgnr(), "Reskontro AS")
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
    ] {
        regnmed_db::ensure_account(&state.pool, company, number, name)
            .await
            .unwrap();
    }
    let token = idp.token(&sub, "Kari Bokfører");

    // Flag 1500 as kunde-reskontro and create a customer over the API.
    let (status, _) = request(
        &state,
        "PUT",
        &format!("/companies/{company}/accounts/1500/reskontro"),
        &token,
        Some(serde_json::json!({ "kind": "kunde" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, created) = request(
        &state,
        "POST",
        &format!("/companies/{company}/parties"),
        &token,
        Some(serde_json::json!({ "kind": "kunde", "name": "Kunde & Co AS", "orgnr": "911111111" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body: {created}");
    assert_eq!(
        created["party_no"], "10000",
        "auto-numbering starts at 10000"
    );
    let party_id = created["party_id"].as_str().unwrap().to_string();

    // A party is required on the reskontro account...
    let missing = regnmed_db::post_voucher(
        &state.pool,
        company,
        &voucher(
            date(1, 10),
            "Faktura uten kunde",
            vec![
                entry("1500", 12_500_00, None),
                entry("3000", -12_500_00, None),
            ],
        ),
        "test",
    )
    .await
    .expect_err("reskontro account without party must fail");
    assert!(
        missing.to_string().contains("requires a party"),
        "got: {missing}"
    );

    // ...and rejected on ordinary accounts.
    let misplaced = regnmed_db::post_voucher(
        &state.pool,
        company,
        &voucher(
            date(1, 10),
            "Feilplassert kunde",
            vec![
                entry("1920", 100_00, Some("10000")),
                entry("3000", -100_00, None),
            ],
        ),
        "test",
    )
    .await
    .expect_err("party on non-reskontro account must fail");
    assert!(
        misplaced.to_string().contains("not a reskontro"),
        "got: {misplaced}"
    );

    // Invoice (12 500 kr on the customer) and a partial payment (10 000).
    regnmed_db::post_voucher(
        &state.pool,
        company,
        &voucher(
            date(1, 10),
            "Faktura 1001",
            vec![
                entry("1500", 12_500_00, Some("10000")),
                entry("3000", -12_500_00, None),
            ],
        ),
        "test",
    )
    .await
    .unwrap();
    regnmed_db::post_voucher(
        &state.pool,
        company,
        &voucher(
            date(1, 25),
            "Innbetaling",
            vec![
                entry("1920", 10_000_00, None),
                entry("1500", -10_000_00, Some("10000")),
            ],
        ),
        "test",
    )
    .await
    .unwrap();

    // The chain (mixed v2 vouchers) verifies from genesis.
    let report = regnmed_db::verify_chain(&state.pool, company)
        .await
        .unwrap();
    assert_eq!(report.vouchers_checked, 2);

    // Spesifikasjon: the customer owes 2 500 kr.
    let (status, parties) = request(
        &state,
        "GET",
        &format!("/companies/{company}/parties?kind=kunde"),
        &token,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(parties["parties"][0]["saldo_ore"], 2_500_00);

    // Åpne poster: both items open; match payment against invoice.
    let (_, items) = request(
        &state,
        "GET",
        &format!("/companies/{company}/parties/{party_id}/items?open=true"),
        &token,
        None,
    )
    .await;
    let items = items["items"].as_array().unwrap().clone();
    assert_eq!(items.len(), 2);
    let invoice = items.iter().find(|i| i["amount_ore"] == 12_500_00).unwrap();
    let payment = items
        .iter()
        .find(|i| i["amount_ore"] == -10_000_00)
        .unwrap();

    let (status, matched) = request(
        &state,
        "POST",
        &format!("/companies/{company}/reskontro/matches"),
        &token,
        Some(serde_json::json!({
            "entry_a": invoice["entry_id"],
            "entry_b": payment["entry_id"],
            "amount_ore": 10_000_00,
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body: {matched}");

    // Over-matching the remainder is rejected.
    let (status, _) = request(
        &state,
        "POST",
        &format!("/companies/{company}/reskontro/matches"),
        &token,
        Some(serde_json::json!({
            "entry_a": invoice["entry_id"],
            "entry_b": payment["entry_id"],
            "amount_ore": 5_000_00,
        })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    // Only the invoice rest (2 500 kr) is still open.
    let (_, open) = request(
        &state,
        "GET",
        &format!("/companies/{company}/parties/{party_id}/items?open=true"),
        &token,
        None,
    )
    .await;
    let open = open["items"].as_array().unwrap();
    assert_eq!(open.len(), 1);
    assert_eq!(open[0]["remaining_ore"], 2_500_00);

    // SAF-T now carries the customer and tags the invoice line.
    let input = regnmed_db::load_saft_input(
        &state.pool,
        company,
        date(1, 1),
        NaiveDate::from_ymd_opt(2026, 12, 31).unwrap(),
        "Kari",
        "Bokfører",
    )
    .await
    .unwrap();
    assert_eq!(input.customers.len(), 1);
    assert_eq!(input.customers[0].party_no, "10000");
    assert_eq!(input.customers[0].closing_ore, 2_500_00);
    let xml = regnmed_core::saft::render(&input).unwrap();
    assert!(xml.contains("<CustomerID>10000</CustomerID>"));
}
