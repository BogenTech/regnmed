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
        ("1460", "Varelager"),
        ("4390", "Beholdningsendring"),
        ("1250", "Inventar"),
        ("6000", "Avskrivninger"),
        ("1920", "Bank"),
        ("3880", "Gevinst ved avgang"),
        ("7880", "Tap ved avgang"),
        ("7790", "Annen kostnad"),
        ("7100", "Bilgodtgjørelse"),
        ("2710", "Inngående mva"),
        ("2910", "Gjeld til ansatte"),
        ("8060", "Valutagevinst"),
        ("8160", "Valutatap"),
        ("1508", "Urealisert kursregulering"),
        ("2400", "Leverandørgjeld"),
        ("4300", "Varekostnad"),
        ("2050", "Annen egenkapital"),
        ("2800", "Avsatt utbytte"),
        ("5000", "Lønn til ansatte"),
        ("5090", "Feriepenger"),
        ("5400", "Arbeidsgiveravgift"),
        ("2600", "Forskuddstrekk"),
        ("2770", "Skyldig arbeidsgiveravgift"),
        ("2930", "Skyldig lønn"),
        ("2940", "Skyldige feriepenger"),
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
                valuta: None,
                valuta_kurs_micro: None,
                lines: vec![regnmed_db::InvoiceLineDraft {
                    description: "Konsulentbistand".into(),
                    account_number: "3000".into(),
                    quantity_milli: 1000,
                    unit_price_ore: price,
                    vat_code: Some("3".into()),
                    avdeling: None,
                    prosjekt: None,
                    product_id: None,
                }],
            },
            "Demo Bruker",
            None,
        )
        .await
        .unwrap();
    }
    // A lagerført product with stock so the Produkter section has data.
    regnmed_db::create_product(
        &state.pool,
        company,
        &regnmed_db::ProductDraft {
            nummer: "V1".into(),
            navn: "Kaffekopp".into(),
            salgspris_ore: 149_00,
            vat_code: Some("3".into()),
            konto: "3000".into(),
            lagerfort: true,
        },
    )
    .await
    .unwrap();
    regnmed_db::register_movement(
        &state.pool,
        company,
        "V1",
        today - chrono::Days::new(30),
        "kjop",
        50_000,
        Some(60_00),
        None,
        "Demo Bruker",
    )
    .await
    .unwrap();
    // Inbox documents so the attestering queue (#47) has something to
    // decide in the browser.
    for (filename, body) in [
        ("stort-innkjop.pdf", "kvittering: serverutstyr 40 000"),
        ("smatt-innkjop.pdf", "kvittering: kontorrekvisita 450"),
    ] {
        regnmed_db::upload_inbox_document(
            &state.pool,
            company,
            filename,
            "application/pdf",
            body.as_bytes(),
            "Demo Bruker",
            None,
        )
        .await
        .unwrap();
    }
    // Aksjeeierbok (#43): to eiere, en stiftelse, en overdragelse og et
    // utbyttevedtak — nok til å se boken, hendelsene og oppgaven.
    let kari = regnmed_db::aksjebok::create_aksjonaer(
        &state.pool,
        company,
        &regnmed_db::aksjebok::NyAksjonaer {
            kind: "person".into(),
            navn: "Kari Nordmann".into(),
            fodselsnummer: Some("26829398612".into()),
            orgnr: None,
            utenlandsk_id: None,
            adresse: Some("Haråsveien 13E".into()),
            postnummer: Some("0283".into()),
            poststed: Some("OSLO".into()),
            landkode: None,
            note: None,
        },
        "Demo Bruker",
    )
    .await
    .unwrap();
    let investor = regnmed_db::aksjebok::create_aksjonaer(
        &state.pool,
        company,
        &regnmed_db::aksjebok::NyAksjonaer {
            kind: "selskap".into(),
            navn: "Investor AS".into(),
            fodselsnummer: None,
            orgnr: Some("923609016".into()),
            utenlandsk_id: None,
            adresse: None,
            postnummer: None,
            poststed: None,
            landkode: None,
            note: None,
        },
        "Demo Bruker",
    )
    .await
    .unwrap();
    let dato = |y, m, d| chrono::NaiveDate::from_ymd_opt(y, m, d).unwrap();
    regnmed_db::aksjebok::record_hendelse(
        &state.pool,
        company,
        kari,
        "stiftelse",
        dato(2025, 1, 2),
        100,
        Some(10_000_000),
        None,
        None,
        Some("Stiftelse"),
        "Demo Bruker",
    )
    .await
    .unwrap();
    regnmed_db::aksjebok::record_hendelse(
        &state.pool,
        company,
        kari,
        "salg",
        dato(2026, 6, 1),
        40,
        Some(6_000_000),
        Some(investor),
        Some("kjop"),
        Some("Overdragelse"),
        "Demo Bruker",
    )
    .await
    .unwrap();
    regnmed_db::aksjebok::create_utbytte(
        &state.pool,
        company,
        dato(2026, 5, 20),
        50_000,
        Some("Ordinær generalforsamling"),
        "Demo Bruker",
    )
    .await
    .unwrap();

    // Ansatte + en kjørt måned, så Lønn-seksjonen har noe å vise.
    for (fnr, navn, stilling, lonn, trekk_bp, fp_bp) in [
        (
            "26829398612",
            "Kari Utvikler",
            "Utvikler",
            5_500_000i64,
            3500i32,
            1020i32,
        ),
        (
            "08888797336",
            "Ola Senior",
            "Fagansvarlig",
            6_200_000,
            3800,
            1250,
        ),
    ] {
        regnmed_db::lonn::create_ansatt(
            &state.pool,
            company,
            &regnmed_db::lonn::NyAnsatt {
                fodselsnummer: fnr.into(),
                navn: navn.into(),
                stilling: Some(stilling.into()),
                ansatt_fra: dato(2025, 1, 1),
                manedslonn_ore: Some(lonn),
                timelonn_ore: None,
                trekk_type: "prosent".into(),
                trekk_prosent_bp: Some(trekk_bp),
                trekk_tabell: None,
                feriepenger_bp: fp_bp,
                bank_account: None,
                note: None,
            },
            "Demo Bruker",
        )
        .await
        .unwrap();
    }
    let ansatte = regnmed_db::lonn::list_ansatte(&state.pool, company)
        .await
        .unwrap();
    regnmed_db::lonn::kjor_lonn(
        &state.pool,
        company,
        2026,
        6,
        dato(2026, 6, 20),
        "I",
        &ansatte
            .iter()
            .map(|a| regnmed_db::lonn::Lonnspost {
                employee_id: a.id,
                brutto_ore: None,
                // Juni: feriepenger utbetales, og de er trekkfrie.
                feriepenger_ore: 4_500_000,
            })
            .collect::<Vec<_>>(),
        "Demo Bruker",
    )
    .await
    .unwrap();

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
