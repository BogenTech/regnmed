//! Integration test of the mva-spesifikasjon and the dated-rate lookup:
//! grunnlag and beregnet avgift per code and termin, historical rates
//! applied by voucher date, and the SAF-T loader carrying the same dated
//! rate per line. Requires DATABASE_URL (skips otherwise).

use chrono::NaiveDate;
use regnmed_core::Ore;
use regnmed_core::voucher::{EntryDraft, VoucherDraft};
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

fn date(y: i32, m: u32, d: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(y, m, d).unwrap()
}

fn entry(account: &str, ore: i64, vat_code: Option<&str>) -> EntryDraft {
    EntryDraft {
        account_number: account.into(),
        amount: Ore(ore),
        vat_code: vat_code.map(str::to_owned),
        description: None,
        party_no: None,
        department: None,
        project: None,
        currency: None,
    }
}

fn voucher(day: NaiveDate, description: &str, entries: Vec<EntryDraft>) -> VoucherDraft {
    VoucherDraft {
        journal_code: "GL".into(),
        voucher_date: day,
        description: description.into(),
        reverses: None,
        entries,
    }
}

#[tokio::test]
async fn spesifikasjon_reports_grunnlag_and_avgift_per_termin() {
    let Some(pool) = pool().await else { return };
    let company = regnmed_db::create_company(&pool, &unique_orgnr(), "Mva Test AS")
        .await
        .unwrap();
    regnmed_db::ensure_journal(&pool, company, "GL", "Hovedbok")
        .await
        .unwrap();
    for (number, name) in [
        ("1920", "Bankinnskudd"),
        ("3000", "Salgsinntekt"),
        ("2700", "Utgående mva"),
        ("2710", "Inngående mva"),
        ("4300", "Varekjøp"),
    ] {
        regnmed_db::ensure_account(&pool, company, number, name)
            .await
            .unwrap();
    }

    // Termin 1 2026: a sale (code 3, 25 %) and a purchase (code 1, 25 %).
    regnmed_db::post_voucher(
        &pool,
        company,
        &voucher(
            date(2026, 1, 10),
            "Salg",
            vec![
                entry("1920", 12_500_00, None),
                entry("3000", -10_000_00, Some("3")),
                entry("2700", -2_500_00, None),
            ],
        ),
        "test",
    )
    .await
    .unwrap();
    regnmed_db::post_voucher(
        &pool,
        company,
        &voucher(
            date(2026, 2, 5),
            "Varekjøp",
            vec![
                entry("4300", 8_000_00, Some("1")),
                entry("2710", 2_000_00, None),
                entry("1920", -10_000_00, None),
            ],
        ),
        "test",
    )
    .await
    .unwrap();
    // Termin 2: must not appear in termin 1's report.
    regnmed_db::post_voucher(
        &pool,
        company,
        &voucher(
            date(2026, 3, 15),
            "Salg",
            vec![
                entry("1920", 1_250_00, None),
                entry("3000", -1_000_00, Some("3")),
                entry("2700", -250_00, None),
            ],
        ),
        "test",
    )
    .await
    .unwrap();
    // 2017: lav sats was 10 %, not today's 12 % — the dated rate must win.
    regnmed_db::post_voucher(
        &pool,
        company,
        &voucher(
            date(2017, 5, 1),
            "Persontransport",
            vec![
                entry("1920", 1_100_00, None),
                entry("3000", -1_000_00, Some("33")),
                entry("2700", -100_00, None),
            ],
        ),
        "test",
    )
    .await
    .unwrap();

    let termin1 = regnmed_core::mva::Termin::new(2026, 1).unwrap();
    let lines = regnmed_db::mva_spesifikasjon(&pool, company, termin1.start(), termin1.end())
        .await
        .unwrap();
    assert_eq!(lines.len(), 2, "codes 1 and 3, nothing from termin 2");

    let inn = lines.iter().find(|l| l.code == "1").unwrap();
    assert_eq!(inn.grunnlag_ore, 8_000_00);
    assert_eq!(inn.avgift_ore, 2_000_00);
    assert_eq!(inn.rate_bp, 2500);

    let utg = lines.iter().find(|l| l.code == "3").unwrap();
    assert_eq!(utg.grunnlag_ore, -10_000_00, "sales base is a credit");
    assert_eq!(utg.avgift_ore, -2_500_00);

    // 2017 report: 10 % on code 33, from the dated rate table.
    let lines_2017 =
        regnmed_db::mva_spesifikasjon(&pool, company, date(2017, 1, 1), date(2017, 12, 31))
            .await
            .unwrap();
    let low = lines_2017.iter().find(|l| l.code == "33").unwrap();
    assert_eq!(low.rate_bp, 1000, "2017 lav sats was 10 %");
    assert_eq!(low.avgift_ore, -100_00);

    // The SAF-T loader must carry the same dated rate per line.
    let input = regnmed_db::load_saft_input(
        &pool,
        company,
        date(2017, 1, 1),
        date(2017, 12, 31),
        "Kari",
        "Nordmann",
    )
    .await
    .unwrap();
    let saft_line = input
        .journals
        .iter()
        .flat_map(|j| &j.transactions)
        .flat_map(|t| &t.lines)
        .find(|l| l.vat_code.as_deref() == Some("33"))
        .expect("2017 voucher line with code 33");
    assert_eq!(saft_line.tax_percent_bp, Some(1000));
}

/// The computed tax on omvendt avgiftsplikt reaches the HOVEDBOK, not
/// just the report (#82, bokføringsforskriften §3-1 nr. 8).
///
/// The buyer posts only what they were invoiced — a foreign supplier
/// charges no Norwegian mva, so the bilag is cost against gjeld and
/// nothing else. The two avgiftslinjene are ours to add, in the posting
/// transaction, or the melding reports a "bokført beløp" that is booked
/// nowhere.
#[tokio::test]
async fn omvendt_avgiftsplikt_posts_the_computed_tax_into_the_ledger() {
    let Some(pool) = pool().await else { return };
    let company = regnmed_db::create_company(&pool, &unique_orgnr(), "Innførsel AS")
        .await
        .unwrap();
    regnmed_db::ensure_journal(&pool, company, "GL", "Hovedbok")
        .await
        .unwrap();
    for (number, name) in [("2400", "Leverandørgjeld"), ("6800", "Konsulenttjenester")] {
        regnmed_db::ensure_account(&pool, company, number, name)
            .await
            .unwrap();
    }

    // Kode 86: tjeneste kjøpt fra utlandet MED fradragsrett.
    regnmed_db::post_voucher(
        &pool,
        company,
        &voucher(
            date(2026, 3, 5),
            "Konsulent fra utlandet",
            vec![
                entry("6800", 100_000_00, Some("86")),
                entry("2400", -100_000_00, None),
            ],
        ),
        "test",
    )
    .await
    .unwrap();

    let saldo = |number: &'static str| {
        let pool = pool.clone();
        async move {
            sqlx::query_scalar::<_, Option<i64>>(
                "select sum(e.amount_ore)::bigint from entry e
                 join account a on a.id = e.account_id
                 where a.company_id = $1 and a.number = $2",
            )
            .bind(company)
            .bind(number)
            .fetch_one(&pool)
            .await
            .unwrap()
            .unwrap_or(0)
        }
    };

    assert_eq!(
        saldo("2701").await,
        -25_000_00,
        "beregnet utgående avgift skal stå som gjeld i hovedboken"
    );
    assert_eq!(saldo("2711").await, 25_000_00, "og fradraget for den");
    assert_eq!(
        saldo("6800").await,
        100_000_00,
        "full fradragsrett endrer ikke kostnaden"
    );

    // Kode 87, samme kjøp UTEN fradragsrett: avgiften er en kostnad.
    regnmed_db::post_voucher(
        &pool,
        company,
        &voucher(
            date(2026, 3, 6),
            "Konsulent fra utlandet, uten fradragsrett",
            vec![
                entry("6800", 100_000_00, Some("87")),
                entry("2400", -100_000_00, None),
            ],
        ),
        "test",
    )
    .await
    .unwrap();
    assert_eq!(
        saldo("2701").await,
        -50_000_00,
        "avgiften skal beregnes også uten fradragsrett"
    );
    assert_eq!(saldo("2711").await, 25_000_00, "men ikke føres til fradrag");
    assert_eq!(
        saldo("6800").await,
        225_000_00,
        "ikke-fradragsberettiget avgift er en del av kostnaden"
    );

    // The chain still verifies: the added entries are hashed with the
    // voucher, not smuggled past it.
    regnmed_db::verify_chain(&pool, company)
        .await
        .expect("kjeden skal verifisere med de tillagte linjene");
}

/// A company that is not mva-registered may not charge output VAT
/// (mval. §15-11 second paragraph) — and must be told so, not quietly
/// handed a zero.
///
/// This is the case the ledger met first in real use: BOGENTECH AS is
/// registered in Foretaksregisteret but NOT in Merverdiavgiftsregisteret,
/// so the invoice path's silent acceptance of a vat code would have
/// issued a document demanding tax the seller has no right to collect
/// (#99). The second half matters as much as the first: once the
/// registration IS recorded, the very same draft must go through, or
/// the guard is just a wall.
#[tokio::test]
async fn an_unregistered_company_cannot_charge_output_vat() {
    let Some(pool) = pool().await else { return };
    let company = regnmed_db::create_company(&pool, &unique_orgnr(), "Uregistrert AS")
        .await
        .unwrap();
    regnmed_db::ensure_journal(&pool, company, "GL", "Hovedbok")
        .await
        .unwrap();
    for (nr, navn) in [
        ("1500", "Kundefordringer"),
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
        Some("Øvre Bogenvei 67, 3152 Tolvsrød"),
        None,
        Some("AS"),
        None,
        None,
    )
    .await
    .unwrap();
    let (party_id, party_no) =
        regnmed_db::create_party(&pool, company, "kunde", "Kunden", None, None)
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

    let dato = date(2026, 10, 28);
    let draft = regnmed_db::InvoiceDraft {
        kontant_betalingsmiddel: None,
        party_no: party_no.clone(),
        invoice_date: dato,
        due_date: dato,
        delivery_date: dato,
        delivery_place: None,
        journal_code: "GL".into(),
        receivable_account: "1500".into(),
        vat_account: "2700".into(),
        currency: None,
        valuta_kurs_micro: None,
        lines: vec![regnmed_db::InvoiceLineDraft {
            description: "Serverleie".into(),
            account_number: "3000".into(),
            quantity_milli: 1000,
            unit_price_ore: 5_565_29,
            vat_code: Some("3".into()),
            department: None,
            project: None,
            product_id: None,
        }],
    };

    let err = regnmed_db::create_invoice(&pool, company, &draft, "test", None)
        .await
        .expect_err("an unregistered seller must not be able to charge mva");
    assert!(
        err.to_string().contains("ikke mva-registrert"),
        "the refusal must name the reason, got: {err}"
    );

    // Same door, same guard: a tilbud is where the number is first
    // promised to the customer.
    let tilbud = regnmed_db::create_salgsdokument(
        &pool,
        company,
        "tilbud",
        &party_no,
        dato,
        &[regnmed_db::SalgsLineDraft {
            description: "Serverleie".into(),
            account_number: "3000".into(),
            quantity_milli: 1000,
            unit_price_ore: 5_565_29,
            vat_code: Some("3".into()),
            department: None,
            project: None,
            product_id: None,
        }],
        "test",
    )
    .await;
    assert!(
        tilbud.is_err(),
        "a tilbud may not promise mva the seller cannot charge"
    );

    // Uncoded lines were always legitimate: this is exactly how an
    // unregistered company invoices, with the unrecoverable input VAT
    // carried in the price and never named as tax.
    let mut uten_kode = draft.clone();
    uten_kode.lines[0].vat_code = None;
    let utstedt = regnmed_db::create_invoice(&pool, company, &uten_kode, "test", None)
        .await
        .expect("an unregistered company must still be able to invoice");
    assert_eq!(utstedt.gross_ore, 5_565_29, "no VAT may be added");

    // Code 7 ("ingen mva-behandling", income side) is how an
    // unregistered company's line is CORRECTLY coded, and the
    // abonnement rail picks it on purpose — so the guard must let it
    // through. Refusing every code broke that rail; this pins it.
    let mut kode_sju = draft.clone();
    kode_sju.lines[0].vat_code = Some("7".into());
    let utenfor = regnmed_db::create_invoice(&pool, company, &kode_sju, "test", None)
        .await
        .expect("code 7 says the sale is outside the vat act — always allowed");
    assert_eq!(utenfor.gross_ore, 5_565_29, "code 7 adds no tax");

    // And once the registration is on record, the original draft stands.
    regnmed_db::record_registrering(
        &pool,
        company,
        date(2026, 10, 1),
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
    regnmed_db::create_invoice(&pool, company, &draft, "test", None)
        .await
        .expect("a registered seller charges mva as before");
}

/// The deduction side of the same rule: input VAT is only deductible by
/// a registered buyer (mval. §8-1).
///
/// Left unguarded, approving an utlegg would split the receipt into net
/// plus a deduction that does not exist, which understates the cost and
/// puts a claim against the state in the ledger. For an unregistered
/// company the whole gross is the cost — which is also why the
/// on-charge has to cover it (#99).
#[tokio::test]
async fn an_unregistered_company_cannot_deduct_input_vat_on_an_utlegg() {
    let Some(pool) = pool().await else { return };
    let company = regnmed_db::create_company(&pool, &unique_orgnr(), "Utlegg Uregistrert AS")
        .await
        .unwrap();
    regnmed_db::ensure_journal(&pool, company, "GL", "Hovedbok")
        .await
        .unwrap();
    for (nr, navn) in [
        ("2710", "Inngående mva"),
        ("2910", "Mellomregning ansatte"),
        ("6420", "Leie datasystemer"),
    ] {
        regnmed_db::ensure_account(&pool, company, nr, navn)
            .await
            .unwrap();
    }
    let person = regnmed_db::ensure_person(&pool, "sub-utlegg-mva", Some("André"), None)
        .await
        .unwrap();
    let dato = date(2026, 10, 2);
    let krav = regnmed_db::create_utlegg(
        &pool,
        company,
        person,
        dato,
        "ServeTheWorld faktura 1202053",
        397_50,
        "stw.pdf",
        "application/pdf",
        b"%PDF-1.4 kvittering",
        "test",
    )
    .await
    .unwrap();

    let err = regnmed_db::approve_expense(
        &pool,
        company,
        krav,
        "6420",
        Some("1"),
        "2710",
        "2910",
        "test",
        person,
    )
    .await
    .expect_err("an unregistered buyer must not deduct inngående mva");
    assert!(
        err.to_string().contains("ikke mva-registrert"),
        "the refusal must name the reason, got: {err}"
    );

    // Without a code the claim posts at its gross amount: the VAT is a
    // real cost, so it belongs on the cost account.
    regnmed_db::approve_expense(
        &pool, company, krav, "6420", None, "2710", "2910", "test", person,
    )
    .await
    .expect("approval without a vat code must still work");
    let kostnad = sqlx::query_scalar::<_, Option<i64>>(
        "select sum(e.amount_ore)::bigint from entry e
         join account a on a.id = e.account_id
         where a.company_id = $1 and a.number = '6420'",
    )
    .bind(company)
    .fetch_one(&pool)
    .await
    .unwrap()
    .unwrap_or(0);
    assert_eq!(kostnad, 397_50, "the whole gross is the cost");
}
