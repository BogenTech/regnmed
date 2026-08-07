//! Kontantsalg (#89, bokføringsforskriften §5-3): a salgsdokument for a
//! ytelse paid on delivery. Requires DATABASE_URL (skips otherwise).

use chrono::NaiveDate;
use sqlx::PgPool;
use uuid::Uuid;

async fn pool() -> Option<PgPool> {
    dotenvy::dotenv().ok();
    let Ok(url) = std::env::var("DATABASE_URL") else {
        eprintln!("skipping: DATABASE_URL not set");
        return None;
    };
    let pool = regnmed_db::connect(&url).await.expect("connect to dev db");
    regnmed_db::MIGRATOR.run(&pool).await.expect("migrate");
    Some(pool)
}

fn unique_orgnr() -> String {
    let n = u32::from_be_bytes(Uuid::new_v4().as_bytes()[..4].try_into().unwrap());
    format!("{:09}", u64::from(n) % 1_000_000_000)
}

fn d(y: i32, m: u32, day: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(y, m, day).unwrap()
}

async fn saldo(pool: &PgPool, company: Uuid, number: &str) -> i64 {
    sqlx::query_scalar::<_, Option<i64>>(
        "select sum(e.amount_ore)::bigint from entry e
         join account a on a.id = e.account_id
         where a.company_id = $1 and a.number = $2",
    )
    .bind(company)
    .bind(number)
    .fetch_one(pool)
    .await
    .unwrap()
    .unwrap_or(0)
}

/// Kontantfaktura (#89, bokføringsforskriften §5-3): the receivable
/// arises and is settled in ONE transaction.
///
/// The tempting shortcut is to post the sale straight against bank and
/// skip 1500 — and that is exactly what must not happen. The reskontro
/// doctrine says a customer's postings carry a party, and a side door
/// past it would make the revisor's tie-out quietly incomplete. So the
/// claim exists, carries the party, and is closed the instant it exists.
#[tokio::test]
async fn a_kontantfaktura_settles_its_own_receivable_through_the_reskontro() {
    let Some(pool) = pool().await else { return };
    let company = regnmed_db::create_company(&pool, &unique_orgnr(), "Kontantsalg AS")
        .await
        .unwrap();
    regnmed_db::ensure_journal(&pool, company, "GL", "Hovedbok")
        .await
        .unwrap();
    for (nr, navn) in [
        ("1500", "Kundefordringer"),
        ("1900", "Kontanter"),
        ("2700", "Utgående mva"),
        ("3000", "Salgsinntekt"),
    ] {
        regnmed_db::ensure_account(&pool, company, nr, navn)
            .await
            .unwrap();
    }
    regnmed_db::set_account_reskontro(&pool, company, "1500", Some("kunde"))
        .await
        .unwrap();
    regnmed_db::update_company_settings(
        &pool,
        company,
        Some("Storgata 1, 0155 Oslo"),
        None,
        Some("AS"),
        None,
    )
    .await
    .unwrap();
    regnmed_db::record_registrering(
        &pool,
        company,
        d(2000, 1, 1),
        regnmed_db::Registrering {
            mva_registrert: true,
            foretaksregistrert: true,
        },
        "manuell",
        Some("testfixtur"),
        "test",
    )
    .await
    .unwrap();
    let (party_id, party_no) =
        regnmed_db::create_party(&pool, company, "kunde", "Kontantkunde", None, None)
            .await
            .unwrap();
    regnmed_db::update_party_contact(
        &pool,
        company,
        party_id,
        Some("Torget 2, 0155 Oslo"),
        None,
        None,
    )
    .await
    .unwrap();

    let dato = d(2026, 4, 3);
    let utstedt = regnmed_db::invoice::create_kontantfaktura(
        &pool,
        company,
        &regnmed_db::InvoiceDraft {
            kontant_betalingsmiddel: None, // create_kontantfaktura sets it
            party_no: party_no.clone(),
            invoice_date: dato,
            due_date: dato,
            delivery_date: dato,
            delivery_place: None,
            journal_code: "GL".into(),
            receivable_account: "1500".into(),
            vat_account: "2700".into(),
            valuta: None,
            valuta_kurs_micro: None,
            lines: vec![regnmed_db::InvoiceLineDraft {
                description: "Kaffe og kake".into(),
                account_number: "3000".into(),
                quantity_milli: 1000,
                unit_price_ore: 200_00,
                vat_code: Some("3".into()),
                avdeling: None,
                prosjekt: None,
                product_id: None,
            }],
        },
        "1900",
        "Kontant",
        "test",
    )
    .await
    .unwrap();

    // The claim was raised and settled: 1500 nets to zero, the cash is in.
    assert_eq!(
        saldo(&pool, company, "1500").await,
        0,
        "fordringen er gjort opp"
    );
    assert_eq!(
        saldo(&pool, company, "1900").await,
        250_00,
        "brutto i kassen"
    );
    assert_eq!(saldo(&pool, company, "2700").await, -50_00, "mva beregnet");

    // Both sides carry the party, so the customer's history is complete
    // and the reskontro tie-out still sees everything.
    let med_part: i64 = sqlx::query_scalar(
        "select count(*)::bigint from entry e
         join account a on a.id = e.account_id
         where a.company_id = $1 and a.number = '1500' and e.party_id is not null",
    )
    .bind(company)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(med_part, 2, "krav og oppgjør, begge med part");

    // And the open item is closed — nothing is left outstanding.
    let apen: i64 = sqlx::query_scalar(
        "select coalesce(sum(e.amount_ore), 0)::bigint from entry e
         join account a on a.id = e.account_id
         where a.company_id = $1 and a.number = '1500'
           and not exists (select 1 from reskontro_match m
                           where m.entry_a = e.id or m.entry_b = e.id)",
    )
    .bind(company)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(apen, 0, "ingen åpen post etter et kontantsalg");

    // The stored salgsdokument is a KONTANTFAKTURA, not a faktura with a
    // KID for money already received.
    let pdf_id = regnmed_db::invoice_pdf_attachment_id(&pool, company, utstedt.invoice_id)
        .await
        .unwrap()
        .expect("PDF lagret ved utstedelse");
    let (_, bytes) = regnmed_db::get_attachment(&pool, company, pdf_id)
        .await
        .unwrap();
    let text = String::from_utf8_lossy(&bytes).to_string();
    assert!(text.contains("KONTANTFAKTURA"), "dokumenttypen");
    assert!(!text.contains("KID"), "ingen KID på et betalt salg");
}

/// Kassaoppgjør (#89, §5-3/§5-4): the day's Z-report as one voucher with
/// the mva split, and the till discrepancy as its OWN bilag.
#[tokio::test]
async fn a_dagsoppgjor_posts_the_day_and_its_difference_separately() {
    let Some(pool) = pool().await else { return };
    let company = regnmed_db::create_company(&pool, &unique_orgnr(), "Kafeen AS")
        .await
        .unwrap();
    regnmed_db::ensure_journal(&pool, company, "GL", "Hovedbok")
        .await
        .unwrap();
    for (nr, navn) in [
        ("1571", "Kortfordring"),
        ("1900", "Kontanter"),
        ("2700", "Utgående mva"),
        ("3000", "Salg 25 %"),
        ("3010", "Salg 15 %"),
        ("7830", "Kassadifferanse"),
    ] {
        regnmed_db::ensure_account(&pool, company, nr, navn)
            .await
            .unwrap();
    }

    let bokfort = regnmed_db::kassa::bokfor_dagsoppgjor(
        &pool,
        company,
        &regnmed_db::kassa::DagsoppgjorInn {
            dato: d(2026, 4, 3),
            z_nummer: "0042".into(),
            salg: vec![
                ("3000".into(), Some("3".into()), 12_500_00),
                ("3010".into(), Some("31".into()), 2_300_00),
            ],
            betaling: vec![("1900".into(), 4_800_00), ("1571".into(), 10_000_00)],
            mva_konto: "2700".into(),
            kontantkonto: Some("1900".into()),
            // 50 kr short in the till.
            opptalt_kontant_ore: Some(4_750_00),
            differansekonto: "7830".into(),
        },
        Some(("z-0042.txt", "text/plain", b"Z-rapport 0042")),
        "test",
    )
    .await
    .unwrap();

    assert_eq!(bokfort.differanse_ore, -50_00);
    assert!(bokfort.differanse.is_some(), "differansen får eget bilag");

    // The day: income net per rate, mva as the sum of the parts.
    assert_eq!(saldo(&pool, company, "3000").await, -10_000_00);
    assert_eq!(saldo(&pool, company, "3010").await, -2_000_00);
    assert_eq!(saldo(&pool, company, "2700").await, -2_800_00);
    assert_eq!(saldo(&pool, company, "1571").await, 10_000_00);
    // Cash: registered 4 800 less the 50 that was missing.
    assert_eq!(saldo(&pool, company, "1900").await, 4_750_00);
    assert_eq!(
        saldo(&pool, company, "7830").await,
        50_00,
        "differansen er kostnadsført, ikke jevnet ut i salget"
    );

    // Two vouchers, not one: the discrepancy is a finding about the day.
    let bilag: i64 =
        sqlx::query_scalar("select count(*)::bigint from voucher where company_id = $1")
            .bind(company)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(bilag, 2);

    // The Z-report hangs on the settlement, per §5-4.
    let vedlegg: i64 = sqlx::query_scalar(
        "select count(*)::bigint from attachment a
         join voucher v on v.id = a.voucher_id
         where v.company_id = $1 and v.description like 'Kassaoppgjør Z-0042%'",
    )
    .bind(company)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(vedlegg, 1);

    regnmed_db::verify_chain(&pool, company).await.unwrap();
}
