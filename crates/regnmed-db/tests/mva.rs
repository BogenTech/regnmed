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
