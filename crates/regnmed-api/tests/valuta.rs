//! Flervaluta end to end: dated kurser (manual + validation), an EUR
//! invoice posted in NOK with hash-covered valutainformasjon (format
//! v4), realized agio posted in the same transaction as the valuta
//! match, urealisert kursregulering with its reversal, the posting
//! sanity bound against unit mistakes, and SAF-T currency fields.
//! Requires DATABASE_URL (skips otherwise).

mod common;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use common::{TestIdp, test_state, unique_orgnr};
use tower::ServiceExt;
use uuid::Uuid;

use regnmed_api::{AppState, router};
use regnmed_core::Ore;
use regnmed_core::valuta::Valuta;
use regnmed_core::voucher::{EntryDraft, VoucherDraft};

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

async fn saldo(state: &AppState, company: Uuid, konto: &str) -> i64 {
    sqlx::query_scalar(
        "select coalesce(sum(e.amount_ore), 0)::bigint
         from entry e join account a on a.id = e.account_id
         where a.company_id = $1 and a.number = $2",
    )
    .bind(company)
    .bind(konto)
    .fetch_one(&state.pool)
    .await
    .unwrap()
}

fn entry(konto: &str, amount: i64, valuta: Option<Valuta>) -> EntryDraft {
    EntryDraft {
        account_number: konto.into(),
        amount: Ore(amount),
        vat_code: None,
        description: None,
        party_no: None,
        avdeling: None,
        prosjekt: None,
        valuta,
    }
}

#[tokio::test]
async fn valuta_invoice_agio_and_regulation() {
    let idp = TestIdp::new();
    let Some(state) = test_state(&idp).await else {
        return;
    };
    let sub = format!("test|{}", Uuid::new_v4());
    let person = regnmed_db::ensure_person(&state.pool, &sub, Some("Valuta Ansvarlig"), None)
        .await
        .unwrap();
    let company = regnmed_db::create_company(&state.pool, &unique_orgnr(), "Eksport AS")
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
        ("1920", "Bank"),
        ("8060", "Valutagevinst"),
        ("8160", "Valutatap"),
        ("1508", "Urealisert kursregulering"),
    ] {
        regnmed_db::ensure_account(&state.pool, company, number, name)
            .await
            .unwrap();
    }
    regnmed_db::set_account_reskontro(&state.pool, company, "1500", Some("kunde"))
        .await
        .unwrap();
    let (_, party_no) =
        regnmed_db::create_party(&state.pool, company, "kunde", "Sveits Kunde AG", None, None)
            .await
            .unwrap();
    let token = idp.token(&sub, "Valuta Ansvarlig");
    let base = format!("/companies/{company}");

    // Rates: manual registration with kilde; garbage rejected.
    let (status, _) = request(
        &state,
        "POST",
        &format!("{base}/currency/rates"),
        &token,
        Some(
            serde_json::json!({ "valuta": "CHF", "dato": "2026-07-01", "kurs": "11,50" })
                .to_string(),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = request(
        &state,
        "POST",
        &format!("{base}/currency/rates"),
        &token,
        Some(
            serde_json::json!({ "valuta": "CHF", "dato": "2026-07-01", "kurs": "abc" }).to_string(),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (_, rates) = request(
        &state,
        "GET",
        &format!("{base}/currency/rates"),
        &token,
        None,
    )
    .await;
    let chf = rates["rates"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["valuta"] == "CHF")
        .expect("CHF in the rate list");
    // The table is GLOBAL and append-only; this test's own inserts are
    // idempotent (on conflict do nothing), so the newest CHF rate is
    // deterministic across re-runs: the 2026-12-31 year-end rate once
    // the full test has run at least once, else 2026-07-01.
    assert!(chf["kurs"] == "11.500000" || chf["kurs"] == "12.000000");

    // EUR invoice: 100 EUR à kurs 11,50 → 1 150 kr posted, document in
    // cent, valutainformasjon on the entries and inside the v4 hash.
    let (status, issued) = request(
        &state,
        "POST",
        &format!("{base}/invoices"),
        &token,
        Some(
            serde_json::json!({
                "party_no": party_no, "invoice_date": "2026-07-02", "due_date": "2026-07-16",
                "valuta": "CHF",
                "lines": [{ "description": "Konsulentbistand", "unit_price_ore": 100_00,
                            "vat_code": "52" }],
            })
            .to_string(),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body: {issued}");
    assert_eq!(issued["gross_ore"], 100_00, "dokumentbeløp i cent");
    assert_eq!(issued["gross_nok_ore"], 1_150_00, "bokført i NOK");
    let invoice_id = issued["invoice_id"].as_str().unwrap().to_string();
    let receivable: (Uuid, Option<String>, Option<i64>, Option<i64>) = sqlx::query_as(
        "select e.id, e.valuta, e.valutabelop_cent, e.kurs_micro
         from invoice i join entry e on e.id = i.receivable_entry_id where i.id = $1",
    )
    .bind(Uuid::parse_str(&invoice_id).unwrap())
    .fetch_one(&state.pool)
    .await
    .unwrap();
    assert_eq!(receivable.1.as_deref(), Some("CHF"));
    assert_eq!(receivable.2, Some(100_00));
    assert_eq!(receivable.3, Some(11_500_000));
    assert_eq!(saldo(&state, company, "1500").await, 1_150_00);

    // Unit mistakes are refused at posting (kurs off by ten).
    let bad = VoucherDraft {
        journal_code: "GL".into(),
        voucher_date: chrono::NaiveDate::from_ymd_opt(2026, 7, 3).unwrap(),
        description: "Feil enheter".into(),
        reverses: None,
        entries: vec![
            entry(
                "1920",
                1_150_00,
                Some(Valuta {
                    valuta: "CHF".into(),
                    belop_cent: 100_00,
                    kurs_micro: 115_000_000,
                }),
            ),
            entry("3000", -1_150_00, None),
        ],
    };
    let result = regnmed_db::post_voucher(&state.pool, company, &bad, "test").await;
    assert!(
        result.is_err() && format!("{:#}", result.unwrap_err()).contains("enhetene"),
        "kurs off by 10 must be refused"
    );

    // Payment: 100 EUR at 11,60 → 1 160 kr into the bank.
    let payment = VoucherDraft {
        journal_code: "GL".into(),
        voucher_date: chrono::NaiveDate::from_ymd_opt(2026, 7, 10).unwrap(),
        description: "Innbetaling EUR".into(),
        reverses: None,
        entries: vec![
            entry(
                "1920",
                1_160_00,
                Some(Valuta {
                    valuta: "CHF".into(),
                    belop_cent: 100_00,
                    kurs_micro: 11_600_000,
                }),
            ),
            EntryDraft {
                party_no: Some(party_no.clone()),
                ..entry(
                    "1500",
                    -1_160_00,
                    Some(Valuta {
                        valuta: "CHF".into(),
                        belop_cent: -100_00,
                        kurs_micro: 11_600_000,
                    }),
                )
            },
        ],
    };
    let posted = regnmed_db::post_voucher(&state.pool, company, &payment, "test")
        .await
        .unwrap();
    let payment_entry: Uuid =
        sqlx::query_scalar("select id from entry where voucher_id = $1 and party_id is not null")
            .bind(posted.id)
            .fetch_one(&state.pool)
            .await
            .unwrap();

    // Valuta match: agio posts in the SAME transaction; both sides
    // close to exactly zero and the gevinst lands on 8060.
    let (status, matched) = request(
        &state,
        "POST",
        &format!("{base}/reskontro/matches"),
        &token,
        Some(
            serde_json::json!({
                "entry_a": receivable.0, "entry_b": payment_entry, "valuta_cent": 100_00,
            })
            .to_string(),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body: {matched}");
    assert_eq!(matched["agio_ore"], 1_000, "10 kr gevinst");
    assert_eq!(saldo(&state, company, "8060").await, -1_000);
    assert_eq!(saldo(&state, company, "1500").await, 0, "reskontro i null");
    let open_items: i64 = sqlx::query_scalar(
        "select count(*) from entry e
         join voucher v on v.id = e.voucher_id
         where v.company_id = $1 and e.party_id is not null
           and e.amount_ore <> coalesce((select sum(m.amount_ore) from reskontro_match m
                                         where m.entry_a = e.id), 0)
                             + coalesce((select sum(-m.amount_ore) from reskontro_match m
                                         where m.entry_b = e.id), 0)",
    )
    .bind(company)
    .fetch_one(&state.pool)
    .await
    .unwrap();
    assert_eq!(open_items, 0, "every reskontro item fully closed");

    // Open 200 EUR invoice + year-end kurs 12,00 → urealisert +100 kr,
    // posted with its reversal the day after in one transaction.
    let (status, _) = request(
        &state,
        "POST",
        &format!("{base}/invoices"),
        &token,
        Some(
            serde_json::json!({
                "party_no": party_no, "invoice_date": "2026-07-02", "due_date": "2026-08-01",
                "valuta": "CHF",
                "lines": [{ "description": "Lisens", "unit_price_ore": 200_00, "vat_code": "52" }],
            })
            .to_string(),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = request(
        &state,
        "POST",
        &format!("{base}/currency/rates"),
        &token,
        Some(
            serde_json::json!({ "valuta": "CHF", "dato": "2026-12-31", "kurs": "12.00" })
                .to_string(),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, regulering) = request(
        &state,
        "POST",
        &format!("{base}/currency/regulate"),
        &token,
        Some(serde_json::json!({ "dato": "2026-12-31", "balansekonto": "1508" }).to_string()),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body: {regulering}");
    assert_eq!(regulering["diff_ore"], 100_00, "200 EUR × (12,00 − 11,50)");
    assert!(regulering["voucher"].is_string() && regulering["reversal"].is_string());
    assert_eq!(saldo(&state, company, "1508").await, 0, "reversalen nuller");
    let reversal_links: i64 = sqlx::query_scalar(
        "select count(*) from voucher where company_id = $1 and reverses_voucher_id is not null",
    )
    .bind(company)
    .fetch_one(&state.pool)
    .await
    .unwrap();
    assert_eq!(reversal_links, 1);

    // The chain (v4) verifies over the lot.
    let report = regnmed_db::verify_chain(&state.pool, company)
        .await
        .unwrap();
    assert_eq!(
        report.vouchers_checked, 6,
        "faktura, betaling, agio, faktura 2, regulering, reversal"
    );

    // SAF-T carries the currency fields per spec.
    let input = regnmed_db::saft::load_saft_input(
        &state.pool,
        company,
        chrono::NaiveDate::from_ymd_opt(2026, 1, 1).unwrap(),
        chrono::NaiveDate::from_ymd_opt(2026, 12, 31).unwrap(),
        "Valuta",
        "Ansvarlig",
    )
    .await
    .unwrap();
    let xml = regnmed_core::saft::render(&input).unwrap();
    assert!(xml.contains("<CurrencyCode>CHF</CurrencyCode>"));
    assert!(xml.contains("<CurrencyAmount>100.00</CurrencyAmount>"));
    assert!(xml.contains("<ExchangeRate>11.500000</ExchangeRate>"));
    // XSD validation when xmllint is available (CI has it).
    let dir = std::env::temp_dir().join(format!("saft-valuta-{company}"));
    std::fs::create_dir_all(&dir).unwrap();
    let xml_path = dir.join("audit.xml");
    std::fs::write(&xml_path, &xml).unwrap();
    let xsd = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../docs/saft/Norwegian_SAF-T_Financial_Schema_v_1.30.xsd"
    );
    match std::process::Command::new("xmllint")
        .args(["--noout", "--schema", xsd])
        .arg(&xml_path)
        .output()
    {
        Ok(output) => assert!(
            output.status.success(),
            "XSD validation failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ),
        Err(_) => eprintln!("xmllint not installed — skipping XSD validation"),
    }
}
