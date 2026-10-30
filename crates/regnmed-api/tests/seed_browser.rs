//! Throwaway seeding helper for manual browser verification — NOT a CI
//! test (no-op unless SEED_BROWSER is set). Writes a static JWKS +
//! signed token to $SEED_BROWSER/ and seeds an overdue-invoice demo.

mod common;

use common::{TestIdp, test_state, unique_orgnumber};
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
    let company = regnmed_db::create_company(&state.pool, &unique_orgnumber(), "Purredemo AS")
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
    regnmed_db::set_account_subledger(&state.pool, company, "1500", Some("kunde"))
        .await
        .unwrap();
    // Salgsdokumentet krever adresse for begge parter og en registrert
    // status (§5-1-2) — demoen må se ut som et ekte selskap.
    regnmed_db::update_company_settings(
        &state.pool,
        company,
        Some("Storgata 1, 0155 Oslo"),
        Some("1234.56.78903"),
        Some("AS"),
        None,
        None,
    )
    .await
    .unwrap();
    regnmed_db::record_registrering(
        &state.pool,
        company,
        chrono::NaiveDate::from_ymd_opt(2020, 1, 1).unwrap(),
        regnmed_db::Registrering {
            vat_registered: true,
            foretaksregistrert: true,
        },
        "brreg",
        Some("demo"),
        "seed",
    )
    .await
    .unwrap();
    let (party_id, party_no) =
        regnmed_db::create_party(&state.pool, company, "kunde", "Sen Betaler AS", None, None)
            .await
            .unwrap();
    regnmed_db::update_party_contact(
        &state.pool,
        company,
        party_id,
        Some("Lilleveien 3, 5003 Bergen"),
        Some("post@senbetaler.invalid"),
        None,
    )
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
                cash_payment_method: None,
                party_no: party_no.clone(),
                invoice_date,
                due_date,
                delivery_date: invoice_date,
                delivery_place: None,
                journal_code: "GL".into(),
                receivable_account: "1500".into(),
                vat_account: "2700".into(),
                currency: None,
                exchange_rate_micro: None,
                lines: vec![regnmed_db::InvoiceLineDraft {
                    description: "Konsulentbistand".into(),
                    account_number: "3000".into(),
                    quantity_milli: 1000,
                    unit_price_ore: price,
                    vat_code: Some("3".into()),
                    department: None,
                    project: None,
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
            number: "V1".into(),
            name: "Kaffekopp".into(),
            salgspris_ore: 149_00,
            vat_code: Some("3".into()),
            account: "3000".into(),
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
    // Aksjeeierbok (#43): two owners, a stiftelse, a transfer and a
    // dividend decision — enough to see the book, the events and the oppgave.
    let kari = regnmed_db::share_register::create_shareholder(
        &state.pool,
        company,
        &regnmed_db::share_register::NewShareholder {
            kind: "person".into(),
            name: "Kari Nordmann".into(),
            nin: Some("26829398612".into()),
            orgnumber: None,
            utenlandsk_id: None,
            address: Some("Haråsveien 13E".into()),
            postal_code: Some("0283".into()),
            postal_city: Some("OSLO".into()),
            country_code: None,
            note: None,
        },
        "Demo Bruker",
    )
    .await
    .unwrap();
    let investor = regnmed_db::share_register::create_shareholder(
        &state.pool,
        company,
        &regnmed_db::share_register::NewShareholder {
            kind: "selskap".into(),
            name: "Investor AS".into(),
            nin: None,
            orgnumber: Some("923609016".into()),
            utenlandsk_id: None,
            address: None,
            postal_code: None,
            postal_city: None,
            country_code: None,
            note: None,
        },
        "Demo Bruker",
    )
    .await
    .unwrap();
    let date = |y, m, d| chrono::NaiveDate::from_ymd_opt(y, m, d).unwrap();
    regnmed_db::share_register::record_hendelse(
        &state.pool,
        company,
        kari,
        "stiftelse",
        date(2025, 1, 2),
        100,
        Some(10_000_000),
        None,
        None,
        Some("Stiftelse"),
        "Demo Bruker",
    )
    .await
    .unwrap();
    regnmed_db::share_register::record_hendelse(
        &state.pool,
        company,
        kari,
        "salg",
        date(2026, 6, 1),
        40,
        Some(6_000_000),
        Some(investor),
        Some("kjop"),
        Some("Overdragelse"),
        "Demo Bruker",
    )
    .await
    .unwrap();
    regnmed_db::share_register::create_utbytte(
        &state.pool,
        company,
        date(2026, 5, 20),
        50_000,
        Some("Ordinær generalforsamling"),
        "Demo Bruker",
    )
    .await
    .unwrap();

    // Employees + one month run, so the Lønn section has something to show.
    for (fnr, name, stilling, lonn, trekk_bp, fp_bp) in [
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
        regnmed_db::payroll::create_employee(
            &state.pool,
            company,
            &regnmed_db::payroll::NewEmployee {
                nin: fnr.into(),
                name: name.into(),
                stilling: Some(stilling.into()),
                employed_from: date(2025, 1, 1),
                manedslonn_ore: Some(lonn),
                timelonn_ore: None,
                withholding_type: "prosent".into(),
                withholding_percent_bp: Some(trekk_bp),
                withholding_table: None,
                holiday_pay_bp: fp_bp,
                bank_account: None,
                note: None,
            },
            "Demo Bruker",
        )
        .await
        .unwrap();
    }
    let ansatte = regnmed_db::payroll::list_ansatte(&state.pool, company)
        .await
        .unwrap();
    // May: an ordinary month. Feriepenger are accrued, and the avgift on
    // them accrues — so June actually has something to draw down.
    regnmed_db::payroll::run_payroll(
        &state.pool,
        company,
        2026,
        5,
        date(2026, 5, 20),
        "I",
        &ansatte
            .iter()
            .map(|a| regnmed_db::payroll::PayrollEntry {
                employee_id: a.id,
                gross_ore: None,
                holiday_pay_ore: 0,
                from_timesheet: false,
            })
            .collect::<Vec<_>>(),
        "Demo Bruker",
    )
    .await
    .unwrap();
    regnmed_db::payroll::run_payroll(
        &state.pool,
        company,
        2026,
        6,
        date(2026, 6, 20),
        "I",
        &ansatte
            .iter()
            .map(|a| regnmed_db::payroll::PayrollEntry {
                employee_id: a.id,
                gross_ore: None,
                // June: feriepenger are paid out, and they carry no withholding.
                holiday_pay_ore: 4_500_000,
                from_timesheet: false,
            })
            .collect::<Vec<_>>(),
        "Demo Bruker",
    )
    .await
    .unwrap();

    // One hourly employee, linked to the demo user who logs hours, with
    // July hours and the month LOCKED — so "from hours" can actually be used.
    let timo = regnmed_db::payroll::create_employee(
        &state.pool,
        company,
        &regnmed_db::payroll::NewEmployee {
            nin: "25927898821".into(),
            name: "Timo Timeansatt".into(),
            stilling: Some("Konsulent".into()),
            employed_from: date(2025, 1, 1),
            manedslonn_ore: None,
            timelonn_ore: Some(45_000),
            withholding_type: "prosent".into(),
            withholding_percent_bp: Some(3000),
            withholding_table: None,
            holiday_pay_bp: 1020,
            bank_account: None,
            note: None,
        },
        "Demo Bruker",
    )
    .await
    .unwrap();
    sqlx::query("update employee set person_id = $2 where id = $1")
        .bind(timo)
        .bind(person)
        .execute(&state.pool)
        .await
        .unwrap();
    for (dag, minutes) in [(1u32, 450i32), (2, 480), (3, 390), (6, 480), (7, 420)] {
        regnmed_db::timesheet::create_time_entry(
            &state.pool,
            company,
            person,
            &regnmed_db::timesheet::TimeEntryDraft {
                date: date(2026, 7, dag),
                minutes,
                description: "Konsulentarbeid".into(),
                project: None,
                billable: Some(true),
                hourly_rate_ore: Some(120_000),
            },
            true,
            "Demo Bruker",
        )
        .await
        .unwrap();
    }
    regnmed_db::timesheet::set_timesheet_lock(
        &state.pool,
        company,
        date(2026, 7, 31),
        "Demo Bruker",
        Some("Låst for lønn"),
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
    // A second identity with no access at all — the registration flow's
    // brand-new-user view can only be seen by someone who owns nothing.
    std::fs::write(
        format!("{out_dir}/token-ny.txt"),
        idp.token(&format!("browser|{}", Uuid::new_v4()), "Ny Bruker"),
    )
    .unwrap();
    // A platform systemadmin (docs/auth.md §8) with no company access —
    // the Plattform view is only reachable with an active platform role.
    let platform_sub = format!("browser|{}", Uuid::new_v4());
    let platform_person = regnmed_db::ensure_person(
        &state.pool,
        &platform_sub,
        Some("Plattform Demo"),
        Some("plattform@test.invalid"),
    )
    .await
    .unwrap();
    regnmed_db::grant_platform_role(
        &state.pool,
        platform_person,
        "systemadmin",
        (chrono::Utc::now() + chrono::Duration::days(90)).date_naive(),
        "demo",
        None,
    )
    .await
    .unwrap();
    std::fs::write(
        format!("{out_dir}/token-plattform.txt"),
        idp.token(&platform_sub, "Plattform Demo"),
    )
    .unwrap();
    std::fs::write(format!("{out_dir}/company.txt"), company.to_string()).unwrap();
    println!("seeded company {company}");
}
