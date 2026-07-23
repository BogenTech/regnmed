//! SAF-T migration over the web API: a file produced by our own exporter
//! is imported into an empty company — accounts, customers, opening
//! balance and history land in one transaction as chain-verified
//! vouchers; balances reconcile; re-import and non-admins are refused.
//! Requires DATABASE_URL (skips otherwise).

mod common;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use chrono::{NaiveDate, TimeZone, Utc};
use common::{TestIdp, test_state, unique_orgnr};
use regnmed_core::saft::{
    SaftAccount, SaftInput, SaftJournal, SaftLine, SaftParty, SaftTaxCode, SaftTransaction,
};
use tower::ServiceExt;
use uuid::Uuid;

use regnmed_api::{AppState, router};

fn date(y: i32, m: u32, d: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(y, m, d).unwrap()
}

/// A small but complete "foreign system" export: opening balances, one
/// invoice transaction with a customer, one bank fee.
fn foreign_saft() -> String {
    let line =
        |no: i32, account: &str, ore: i64, customer: Option<&str>, vat: Option<&str>| SaftLine {
            line_no: no,
            account_number: account.into(),
            description: None,
            amount_ore: ore,
            vat_code: vat.map(str::to_owned),
            tax_percent_bp: vat.map(|_| 2500),
            customer_id: customer.map(str::to_owned),
            supplier_id: None,
        };
    let input = SaftInput {
        orgnr: "923609016".into(),
        company_name: "Gammelt System AS".into(),
        contact_first_name: "Kari".into(),
        contact_last_name: "Nordmann".into(),
        file_created: date(2026, 7, 23),
        software_version: "old-system".into(),
        start: date(2026, 1, 1),
        end: date(2026, 12, 31),
        accounts: vec![
            SaftAccount {
                number: "1500".into(),
                name: "Kundefordringer".into(),
                created: date(2020, 1, 1),
                opening_ore: 2_000_00,
                closing_ore: 14_500_00,
            },
            SaftAccount {
                number: "1920".into(),
                name: "Bank".into(),
                created: date(2020, 1, 1),
                opening_ore: 8_000_00,
                closing_ore: 7_850_00,
            },
            SaftAccount {
                number: "2050".into(),
                name: "Annen egenkapital".into(),
                created: date(2020, 1, 1),
                opening_ore: -10_000_00,
                closing_ore: -10_000_00,
            },
            SaftAccount {
                number: "3000".into(),
                name: "Salgsinntekt".into(),
                created: date(2020, 1, 1),
                opening_ore: 0,
                closing_ore: -10_000_00,
            },
            SaftAccount {
                number: "2700".into(),
                name: "Utgående mva".into(),
                created: date(2020, 1, 1),
                opening_ore: 0,
                closing_ore: -2_500_00,
            },
            SaftAccount {
                number: "7770".into(),
                name: "Gebyr".into(),
                created: date(2020, 1, 1),
                opening_ore: 0,
                closing_ore: 150_00,
            },
        ],
        customers: vec![SaftParty {
            party_no: "10042".into(),
            name: "Gammel Kunde AS".into(),
            orgnr: Some("911111111".into()),
            balance_account: Some("1500".into()),
            opening_ore: 2_000_00,
            closing_ore: 14_500_00,
        }],
        suppliers: vec![],
        tax_codes: vec![SaftTaxCode {
            code: "3".into(),
            description: "Utgående mva".into(),
            percent_bp: 2500,
        }],
        journals: vec![SaftJournal {
            code: "SALG".into(),
            name: "Salgsjournal".into(),
            transactions: vec![
                SaftTransaction {
                    fiscal_year: 2026,
                    number: 77,
                    date: date(2026, 2, 10),
                    description: "Faktura 77".into(),
                    created_by: "old".into(),
                    created_at: Utc.with_ymd_and_hms(2026, 2, 10, 9, 0, 0).unwrap(),
                    reverses: None,
                    lines: vec![
                        line(1, "1500", 12_500_00, Some("10042"), None),
                        line(2, "3000", -10_000_00, None, Some("3")),
                        line(3, "2700", -2_500_00, None, None),
                    ],
                },
                SaftTransaction {
                    fiscal_year: 2026,
                    number: 78,
                    date: date(2026, 3, 5),
                    description: "Bankgebyr".into(),
                    created_by: "old".into(),
                    created_at: Utc.with_ymd_and_hms(2026, 3, 5, 9, 0, 0).unwrap(),
                    reverses: None,
                    lines: vec![
                        line(1, "7770", 150_00, None, None),
                        line(2, "1920", -150_00, None, None),
                    ],
                },
            ],
        }],
    };
    regnmed_core::saft::render(&input)
}

async fn request(
    state: &AppState,
    method: &str,
    uri: &str,
    bearer: &str,
    body: Option<String>,
) -> (StatusCode, serde_json::Value) {
    let response = router(state.clone())
        .oneshot(
            Request::builder()
                .method(method)
                .uri(uri)
                .header("authorization", format!("Bearer {bearer}"))
                .body(Body::from(body.unwrap_or_default()))
                .unwrap(),
        )
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

#[tokio::test]
async fn migrates_a_foreign_saft_file_into_an_empty_company() {
    let idp = TestIdp::new();
    let Some(state) = test_state(&idp).await else {
        return;
    };
    let admin_sub = format!("test|{}", Uuid::new_v4());
    let viewer_sub = format!("test|{}", Uuid::new_v4());
    let admin = regnmed_db::ensure_person(&state.pool, &admin_sub, Some("Milla Migrerer"), None)
        .await
        .unwrap();
    let viewer = regnmed_db::ensure_person(&state.pool, &viewer_sub, Some("Lars Leser"), None)
        .await
        .unwrap();
    let company = regnmed_db::create_company(&state.pool, &unique_orgnr(), "Migrert AS")
        .await
        .unwrap();
    regnmed_db::ensure_company_member(&state.pool, company, admin, "admin")
        .await
        .unwrap();
    regnmed_db::ensure_company_member(&state.pool, company, viewer, "bokforing")
        .await
        .unwrap();
    let admin_token = idp.token(&admin_sub, "Milla Migrerer");
    let viewer_token = idp.token(&viewer_sub, "Lars Leser");
    let file = foreign_saft();

    // Non-admin is refused.
    let (status, _) = request(
        &state,
        "POST",
        &format!("/companies/{company}/import/saft"),
        &viewer_token,
        Some(file.clone()),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    // Admin imports: 6 accounts, 1 customer, opening + 2 history vouchers.
    let (status, report) = request(
        &state,
        "POST",
        &format!("/companies/{company}/import/saft"),
        &admin_token,
        Some(file.clone()),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body: {report}");
    assert_eq!(report["accounts"], 6);
    assert_eq!(report["customers"], 1);
    assert_eq!(report["vouchers"], 2);
    assert_eq!(report["opening_posted"], true);

    // The chain verifies from genesis over the imported history.
    let chain = regnmed_db::verify_chain(&state.pool, company)
        .await
        .unwrap();
    assert_eq!(chain.vouchers_checked, 3, "opening + 2 history vouchers");

    // Trial balance equals the foreign system's closing balances.
    for (account, expected) in [
        ("1500", 14_500_00i64),
        ("1920", 7_850_00),
        ("3000", -10_000_00),
        ("2700", -2_500_00),
        ("7770", 150_00),
        ("2050", -10_000_00),
    ] {
        let balance: i64 = sqlx::query_scalar(
            "select coalesce(sum(e.amount_ore), 0)::bigint
             from entry e join account a on a.id = e.account_id
             where a.company_id = $1 and a.number = $2",
        )
        .bind(company)
        .bind(account)
        .fetch_one(&state.pool)
        .await
        .unwrap();
        assert_eq!(balance, expected, "konto {account}");
    }

    // The customer exists with the source system's number; the opening
    // balance on 1500 deferred its reskontro flag (warned, not hidden).
    let (_, parties) = request(
        &state,
        "GET",
        &format!("/companies/{company}/parties"),
        &admin_token,
        None,
    )
    .await;
    assert_eq!(parties["parties"][0]["party_no"], "10042");
    assert!(
        report["warnings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|w| w.as_str().unwrap().contains("1500")),
        "deferred reskontro flag is warned about: {report}"
    );

    // Re-import into the now non-empty ledger is refused.
    let (status, body) = request(
        &state,
        "POST",
        &format!("/companies/{company}/import/saft"),
        &admin_token,
        Some(file),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "body: {body}");
}
