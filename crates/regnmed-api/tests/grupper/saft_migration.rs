//! SAF-T migration over the web API: a file produced by our own exporter
//! is imported into an empty company — accounts, customers, opening
//! balance and history land in one transaction as chain-verified
//! vouchers; balances reconcile; re-import and non-admins are refused.
//! Multi-year history (one file per fiscal year, the Conta shape) is
//! imported file by file: follow-up openings must reconcile against the
//! imported history, mismatches are refused with the difference named,
//! and one ordinary voucher closes the import door for good.
//! Requires DATABASE_URL (skips otherwise).

use crate::common::{TestIdp, test_state, unique_orgnr};
use axum::body::Body;
use axum::http::{Request, StatusCode};
use chrono::{NaiveDate, TimeZone, Utc};
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
            avdeling: None,
            prosjekt: None,
            valuta: None,
            valutabelop_cent: None,
            kurs_micro: None,
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
        analysis_types: vec![],
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
    regnmed_core::saft::render(&input).unwrap()
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

    // Re-importing the same file is refused by the import log: the
    // ledger is still IMP-only so the door is open, but identical
    // content can never land twice.
    let (status, body) = request(
        &state,
        "POST",
        &format!("/companies/{company}/import/saft"),
        &admin_token,
        Some(file),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "body: {body}");
    assert!(
        body.to_string().contains("allerede importert"),
        "the dedup guard names itself: {body}"
    );
}

/// One SAF-T file per fiscal year (the Conta export shape). The follow-up
/// file's openings mirror reality: balance accounts carry the closing
/// balance over, resultat accounts open at zero WITHOUT a counterpart —
/// the openings do not sum to zero, and no year-end closing is posted.
fn year_saft(
    start: NaiveDate,
    end: NaiveDate,
    accounts: Vec<SaftAccount>,
    transactions: Vec<SaftTransaction>,
) -> String {
    let input = SaftInput {
        orgnr: "923609016".into(),
        company_name: "Gammelt System AS".into(),
        contact_first_name: "Kari".into(),
        contact_last_name: "Nordmann".into(),
        file_created: end,
        software_version: "old-system".into(),
        start,
        end,
        accounts,
        customers: vec![],
        suppliers: vec![],
        tax_codes: vec![],
        analysis_types: vec![],
        journals: vec![SaftJournal {
            code: "GEN".into(),
            name: "Hovedbok".into(),
            transactions,
        }],
    };
    regnmed_core::saft::render(&input).unwrap()
}

fn acct(number: &str, name: &str, opening_ore: i64, closing_ore: i64) -> SaftAccount {
    SaftAccount {
        number: number.into(),
        name: name.into(),
        created: date(2020, 1, 1),
        opening_ore,
        closing_ore,
    }
}

fn plain_tx(number: i64, on: NaiveDate, text: &str, lines: &[(&str, i64)]) -> SaftTransaction {
    use chrono::Datelike;
    SaftTransaction {
        fiscal_year: on.year(),
        number,
        date: on,
        description: text.into(),
        created_by: "old".into(),
        created_at: Utc.with_ymd_and_hms(on.year(), 1, 2, 9, 0, 0).unwrap(),
        reverses: None,
        lines: lines
            .iter()
            .enumerate()
            .map(|(i, (account, ore))| SaftLine {
                line_no: i as i32 + 1,
                account_number: (*account).into(),
                description: None,
                amount_ore: *ore,
                vat_code: None,
                tax_percent_bp: None,
                customer_id: None,
                supplier_id: None,
                avdeling: None,
                prosjekt: None,
                valuta: None,
                valutabelop_cent: None,
                kurs_micro: None,
            })
            .collect(),
    }
}

#[tokio::test]
async fn imports_one_file_per_year_and_refuses_mismatched_openings() {
    let idp = TestIdp::new();
    let Some(state) = test_state(&idp).await else {
        return;
    };
    let admin_sub = format!("test|{}", Uuid::new_v4());
    let admin = regnmed_db::ensure_person(&state.pool, &admin_sub, Some("Milla Migrerer"), None)
        .await
        .unwrap();
    let company = regnmed_db::create_company(&state.pool, &unique_orgnr(), "Flerårig AS")
        .await
        .unwrap();
    regnmed_db::ensure_company_member(&state.pool, company, admin, "admin")
        .await
        .unwrap();
    let admin_token = idp.token(&admin_sub, "Milla Migrerer");
    let import_uri = format!("/companies/{company}/import/saft");

    // 2025: opening equity + bank, one sale during the year.
    let file_2025 = year_saft(
        date(2025, 1, 1),
        date(2025, 12, 31),
        vec![
            acct("1920", "Bank", 10_000_00, 15_000_00),
            acct("2050", "Annen egenkapital", -10_000_00, -10_000_00),
            acct("3000", "Salgsinntekt", 0, -5_000_00),
        ],
        vec![plain_tx(
            1,
            date(2025, 6, 1),
            "Salg",
            &[("1920", 5_000_00), ("3000", -5_000_00)],
        )],
    );
    let (status, report) =
        request(&state, "POST", &import_uri, &admin_token, Some(file_2025)).await;
    assert_eq!(status, StatusCode::OK, "2025: {report}");
    assert_eq!(report["opening_posted"], true);
    assert_eq!(report["opening_reconciled"], false);

    // 2026 Jan–Apr: balance accounts carry over, 3000 opens at zero with
    // no counterpart (openings sum to 5 000, like a real Conta file).
    let file_2026 = year_saft(
        date(2026, 1, 1),
        date(2026, 4, 30),
        vec![
            acct("1920", "Bank", 15_000_00, 14_900_00),
            acct("2050", "Annen egenkapital", -10_000_00, -10_000_00),
            acct("3000", "Salgsinntekt", 0, 0),
            acct("7770", "Gebyr", 0, 100_00),
        ],
        vec![plain_tx(
            1,
            date(2026, 2, 1),
            "Bankgebyr",
            &[("7770", 100_00), ("1920", -100_00)],
        )],
    );
    let (status, report) =
        request(&state, "POST", &import_uri, &admin_token, Some(file_2026)).await;
    assert_eq!(status, StatusCode::OK, "2026: {report}");
    assert_eq!(report["opening_posted"], false, "no second Åpningsbalanse");
    assert_eq!(report["opening_reconciled"], true);
    assert_eq!(report["vouchers"], 1);

    // 2026 May–Aug: a year delivered in several files. Resultat accounts
    // open at the year-to-date sum, not zero and not the all-time sum.
    let file_2026_b = year_saft(
        date(2026, 5, 1),
        date(2026, 8, 31),
        vec![
            acct("1920", "Bank", 14_900_00, 14_850_00),
            acct("2050", "Annen egenkapital", -10_000_00, -10_000_00),
            acct("7770", "Gebyr", 100_00, 150_00),
        ],
        vec![plain_tx(
            1,
            date(2026, 6, 15),
            "Bankgebyr",
            &[("7770", 50_00), ("1920", -50_00)],
        )],
    );
    let (status, report) =
        request(&state, "POST", &import_uri, &admin_token, Some(file_2026_b)).await;
    assert_eq!(status, StatusCode::OK, "2026 May–Aug: {report}");

    // The chain verifies from genesis across all three files.
    let chain = regnmed_db::verify_chain(&state.pool, company)
        .await
        .unwrap();
    assert_eq!(chain.vouchers_checked, 4, "opening + one voucher per file");
    for (account, expected) in [
        ("1920", 14_850_00i64),
        ("2050", -10_000_00),
        ("3000", -5_000_00),
        ("7770", 150_00),
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

    // A file whose period nets to ZERO on every account is the
    // reconciliation's blind spot — a second import would reconcile
    // cleanly and double-post. The import log must catch it by content.
    let zero_net = year_saft(
        date(2026, 9, 1),
        date(2026, 10, 31),
        vec![
            acct("1920", "Bank", 14_850_00, 14_850_00),
            acct("2050", "Annen egenkapital", -10_000_00, -10_000_00),
            acct("7770", "Gebyr", 150_00, 150_00),
        ],
        vec![
            plain_tx(
                1,
                date(2026, 9, 10),
                "Gebyr",
                &[("7770", 100_00), ("1920", -100_00)],
            ),
            plain_tx(
                2,
                date(2026, 9, 20),
                "Gebyr refundert",
                &[("7770", -100_00), ("1920", 100_00)],
            ),
        ],
    );
    let (status, report) = request(
        &state,
        "POST",
        &import_uri,
        &admin_token,
        Some(zero_net.clone()),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "zero-net file: {report}");
    assert_eq!(report["vouchers"], 2);
    let (status, body) = request(&state, "POST", &import_uri, &admin_token, Some(zero_net)).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "body: {body}");
    assert!(
        body.to_string().contains("allerede importert"),
        "identical content is refused by the log, not by luck: {body}"
    );

    // A continuation file with a bank opening 850 kr off: refused, and
    // the refusal names the account and the exact difference.
    let file_wrong = year_saft(
        date(2026, 9, 1),
        date(2026, 12, 31),
        vec![
            acct("1920", "Bank", 14_000_00, 14_000_00),
            acct("2050", "Annen egenkapital", -10_000_00, -10_000_00),
            acct("7770", "Gebyr", 150_00, 150_00),
        ],
        vec![],
    );
    let (status, body) = request(&state, "POST", &import_uri, &admin_token, Some(file_wrong)).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "body: {body}");
    let message = body.to_string();
    assert!(
        message.contains("konto 1920"),
        "names the account: {message}"
    );
    assert!(
        message.contains("-85000"),
        "names the difference: {message}"
    );

    // One ordinary voucher closes the import door for good.
    sqlx::query(
        "insert into journal (id, company_id, code, name) values ($1, $2, 'BILAG', 'Bilag')",
    )
    .bind(Uuid::now_v7())
    .bind(company)
    .execute(&state.pool)
    .await
    .unwrap();
    let draft = regnmed_core::voucher::VoucherDraft {
        journal_code: "BILAG".into(),
        voucher_date: date(2026, 9, 1),
        description: "Ordinært bilag".into(),
        reverses: None,
        entries: vec![
            regnmed_core::voucher::EntryDraft {
                account_number: "7770".into(),
                amount: regnmed_core::Ore(10_00),
                vat_code: None,
                description: None,
                party_no: None,
                avdeling: None,
                prosjekt: None,
                valuta: None,
            },
            regnmed_core::voucher::EntryDraft {
                account_number: "1920".into(),
                amount: regnmed_core::Ore(-10_00),
                vat_code: None,
                description: None,
                party_no: None,
                avdeling: None,
                prosjekt: None,
                valuta: None,
            },
        ],
    };
    regnmed_db::post_voucher(&state.pool, company, &draft, "Milla Migrerer")
        .await
        .unwrap();
    let next_file = year_saft(
        date(2026, 9, 1),
        date(2026, 12, 31),
        vec![
            acct("1920", "Bank", 14_840_00, 14_840_00),
            acct("2050", "Annen egenkapital", -10_000_00, -10_000_00),
            acct("7770", "Gebyr", 160_00, 160_00),
        ],
        vec![],
    );
    let (status, body) = request(&state, "POST", &import_uri, &admin_token, Some(next_file)).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "body: {body}");
    assert!(
        body.to_string().contains("importjournalen"),
        "explains that ordinary bookkeeping closed the door: {body}"
    );
}
