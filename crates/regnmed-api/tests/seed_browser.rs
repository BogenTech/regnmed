//! Throwaway seeding helper for manual browser verification — NOT a CI
//! test (no-op unless SEED_BROWSER is set). Writes a static JWKS +
//! signed token to $SEED_BROWSER/ and seeds an overdue-invoice demo.

mod common;

use common::{TestIdp, test_state, unique_orgnr};
use uuid::Uuid;

#[tokio::test]
async fn seed_browser_demo() {
    let Ok(out_dir) = std::env::var("SEED_BROWSER") else {
        return;
    };
    let idp = TestIdp::new();
    let Some(state) = test_state(&idp).await else {
        return;
    };
    let sub = format!("browser|{}", Uuid::new_v4());
    let person = regnmed_db::ensure_person(&state.pool, &sub, Some("Demo Bruker"), None)
        .await
        .unwrap();
    let company = regnmed_db::create_company(&state.pool, &unique_orgnr(), "Purredemo AS")
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
        ("3950", "Annen driftsrelatert inntekt"),
        ("8050", "Annen renteinntekt"),
    ] {
        regnmed_db::ensure_account(&state.pool, company, number, name)
            .await
            .unwrap();
    }
    regnmed_db::set_account_reskontro(&state.pool, company, "1500", Some("kunde"))
        .await
        .unwrap();
    let (_, party_no) =
        regnmed_db::create_party(&state.pool, company, "kunde", "Sen Betaler AS", None, None)
            .await
            .unwrap();
    let today = chrono::Utc::now().date_naive();
    for (invoice_date, due_date, price) in [
        (
            chrono::NaiveDate::from_ymd_opt(2026, 1, 10).unwrap(),
            chrono::NaiveDate::from_ymd_opt(2026, 1, 24).unwrap(),
            10_000_00,
        ),
        (
            today - chrono::Days::new(20),
            today - chrono::Days::new(5),
            1_000_00,
        ),
    ] {
        regnmed_db::create_invoice(
            &state.pool,
            company,
            &regnmed_db::InvoiceDraft {
                party_no: party_no.clone(),
                invoice_date,
                due_date,
                journal_code: "GL".into(),
                receivable_account: "1500".into(),
                vat_account: "2700".into(),
                lines: vec![regnmed_db::InvoiceLineDraft {
                    description: "Konsulentbistand".into(),
                    account_number: "3000".into(),
                    quantity_milli: 1000,
                    unit_price_ore: price,
                    vat_code: Some("3".into()),
                }],
            },
            "Demo Bruker",
            None,
        )
        .await
        .unwrap();
    }
    std::fs::write(
        format!("{out_dir}/jwks.json"),
        serde_json::to_string(&idp.jwks).unwrap(),
    )
    .unwrap();
    std::fs::write(
        format!("{out_dir}/token.txt"),
        idp.token(&sub, "Demo Bruker"),
    )
    .unwrap();
    std::fs::write(format!("{out_dir}/company.txt"), company.to_string()).unwrap();
    println!("seeded company {company}");
}
