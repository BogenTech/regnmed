//! Årsavslutning (#84): resultatdisponering og skattekostnad as an
//! ordinary voucher. Requires DATABASE_URL (skips otherwise).

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

fn d(y: i32, m: u32, day: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(y, m, day).unwrap()
}

fn e(konto: &str, ore: i64) -> EntryDraft {
    EntryDraft {
        account_number: konto.into(),
        amount: Ore(ore),
        vat_code: None,
        description: None,
        party_no: None,
        avdeling: None,
        prosjekt: None,
        valuta: None,
    }
}

async fn selskap_med_overskudd(pool: &PgPool) -> Uuid {
    let company = regnmed_db::create_company(pool, &unique_orgnr(), "Årsoppgjør AS")
        .await
        .unwrap();
    regnmed_db::ensure_journal(pool, company, "GL", "Hovedbok")
        .await
        .unwrap();
    for (nr, navn) in [("1920", "Bank"), ("3000", "Salg"), ("6300", "Leie")] {
        regnmed_db::ensure_account(pool, company, nr, navn)
            .await
            .unwrap();
    }
    // 100 000 inntekt, 40 000 kostnad -> 60 000 overskudd i 2025.
    regnmed_db::post_voucher(
        pool,
        company,
        &VoucherDraft {
            journal_code: "GL".into(),
            voucher_date: d(2025, 6, 1),
            description: "Salg".into(),
            reverses: None,
            entries: vec![e("1920", 100_000_00), e("3000", -100_000_00)],
        },
        "test",
    )
    .await
    .unwrap();
    regnmed_db::post_voucher(
        pool,
        company,
        &VoucherDraft {
            journal_code: "GL".into(),
            voucher_date: d(2025, 9, 1),
            description: "Husleie".into(),
            reverses: None,
            entries: vec![e("6300", 40_000_00), e("1920", -40_000_00)],
        },
        "test",
    )
    .await
    .unwrap();
    company
}

async fn saldo(pool: &PgPool, company: Uuid, nr: &str) -> i64 {
    sqlx::query_scalar::<_, Option<i64>>(
        "select sum(e.amount_ore)::bigint from entry e
         join account a on a.id = e.account_id
         where a.company_id = $1 and a.number = $2",
    )
    .bind(company)
    .bind(nr)
    .fetch_one(pool)
    .await
    .unwrap()
    .unwrap_or(0)
}

/// The whole point of #84: after disposition the profit sits in equity
/// and NOT also in udisponert resultat, while last year's P&L still
/// reads the profit it earned.
#[tokio::test]
async fn closing_a_year_moves_the_result_to_equity_without_double_counting() {
    let Some(pool) = pool().await else { return };
    let company = selskap_med_overskudd(&pool).await;

    let for_skatt = regnmed_db::arsavslutning::resultat_for_aret(&pool, company, 2025)
        .await
        .unwrap();
    assert_eq!(for_skatt, 60_000_00);

    let avsluttet = regnmed_db::arsavslutning::avslutt_ar(&pool, company, 2025, 13_200_00, "test")
        .await
        .unwrap();
    assert_eq!(avsluttet.resultat_for_skatt_ore, 60_000_00);
    assert_eq!(avsluttet.disponert_ore, 46_800_00, "etter skatt");

    // The tax is accrued, the result is in equity.
    assert_eq!(
        saldo(&pool, company, "8300").await,
        13_200_00,
        "skattekostnad"
    );
    assert_eq!(
        saldo(&pool, company, "2500").await,
        -13_200_00,
        "betalbar skatt"
    );
    assert_eq!(
        saldo(&pool, company, "2050").await,
        -46_800_00,
        "opptjent EK"
    );
    assert_eq!(
        saldo(&pool, company, "8800").await,
        46_800_00,
        "disponeringen"
    );

    // The balanse: udisponert is empty, and it still balances.
    let lines = regnmed_db::saldo_lines(&pool, company, None, d(2025, 12, 31), None, None)
        .await
        .unwrap();
    let b = regnmed_core::regnskap::balanse(&lines);
    assert_eq!(
        b.udisponert_resultat_ore, 0,
        "resultatet er disponert — det kan ikke også ligge udisponert"
    );
    assert_eq!(b.differanse_ore(), 0);

    // …and the year's resultatregnskap is untouched by its own closing.
    let arets = regnmed_db::saldo_lines(
        &pool,
        company,
        Some(d(2025, 1, 1)),
        d(2025, 12, 31),
        None,
        None,
    )
    .await
    .unwrap();
    let r = regnmed_core::regnskap::resultat(&arets);
    assert_eq!(
        r.arsresultat_ore,
        60_000_00 - 13_200_00,
        "resultat etter skattekostnad, uten disponeringen"
    );

    regnmed_db::verify_chain(&pool, company).await.unwrap();
}

/// Ordering: the closing LOCKS the year, and a locked year cannot be
/// closed (its own voucher could not be posted). Both directions.
#[tokio::test]
async fn the_closing_locks_the_year_and_a_locked_year_refuses() {
    let Some(pool) = pool().await else { return };
    let company = selskap_med_overskudd(&pool).await;

    assert!(
        regnmed_db::current_period_lock(&pool, company)
            .await
            .unwrap()
            .is_none(),
        "ingen lås før avslutningen"
    );
    regnmed_db::arsavslutning::avslutt_ar(&pool, company, 2025, 0, "test")
        .await
        .unwrap();
    assert_eq!(
        regnmed_db::current_period_lock(&pool, company)
            .await
            .unwrap(),
        Some(d(2025, 12, 31)),
        "avslutningen låser året selv"
    );

    // Twice is refused — a correction is a reversing voucher.
    let feil = regnmed_db::arsavslutning::avslutt_ar(&pool, company, 2025, 0, "test")
        .await
        .expect_err("et år kan ikke disponeres to ganger");
    assert!(feil.to_string().contains("allerede avsluttet"), "{feil}");

    // A year already locked cannot be closed: its voucher is dated
    // inside the lock, and the message must say so rather than let the
    // trigger surface two layers down.
    let company2 = selskap_med_overskudd(&pool).await;
    regnmed_db::set_period_lock(&pool, company2, d(2025, 12, 31), "test", false)
        .await
        .unwrap();
    let feil = regnmed_db::arsavslutning::avslutt_ar(&pool, company2, 2025, 0, "test")
        .await
        .expect_err("låst år kan ikke avsluttes");
    assert!(feil.to_string().contains("låst til og med"), "{feil}");
}
