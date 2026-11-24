//! Årsavslutning (#84): resultatdisponering og skattekostnad as an
//! ordinary voucher. Requires DATABASE_URL (skips otherwise).

use chrono::{Datelike, NaiveDate};
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

fn unique_orgnumber() -> String {
    let n = u32::from_be_bytes(Uuid::new_v4().as_bytes()[..4].try_into().unwrap());
    format!("{:09}", u64::from(n) % 1_000_000_000)
}

fn d(y: i32, m: u32, day: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(y, m, day).unwrap()
}

fn e(account: &str, ore: i64) -> EntryDraft {
    EntryDraft {
        account_number: account.into(),
        amount: Ore(ore),
        vat_code: None,
        description: None,
        party_no: None,
        department: None,
        project: None,
        currency: None,
    }
}

async fn selskap_med_overskudd(pool: &PgPool) -> Uuid {
    let company = regnmed_db::create_company(pool, &unique_orgnumber(), "Årsoppgjør AS")
        .await
        .unwrap();
    regnmed_db::ensure_journal(pool, company, "GL", "Hovedbok")
        .await
        .unwrap();
    for (number, name) in [("1920", "Bank"), ("3000", "Salg"), ("6300", "Leie")] {
        regnmed_db::ensure_account(pool, company, number, name)
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

async fn balance(pool: &PgPool, company: Uuid, number: &str) -> i64 {
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

/// The whole point of #84: after disposition the profit sits in equity
/// and NOT also in udisponert resultat, while last year's P&L still
/// reads the profit it earned.
#[tokio::test]
async fn closing_a_year_moves_the_result_to_equity_without_double_counting() {
    let Some(pool) = pool().await else { return };
    let company = selskap_med_overskudd(&pool).await;

    let for_skatt = regnmed_db::year_end::result_for_the_year(&pool, company, 2025)
        .await
        .unwrap();
    assert_eq!(for_skatt, 60_000_00);

    let closed = regnmed_db::year_end::close_year(
        &pool, company, 2025, 13_200_00, "test", false, false, None,
    )
    .await
    .unwrap();
    assert_eq!(closed.closing.result_before_tax_ore, 60_000_00);
    assert_eq!(closed.closing.allocated_ore, 46_800_00, "etter skatt");

    // The tax is accrued, the result is in equity.
    assert_eq!(
        balance(&pool, company, "8300").await,
        13_200_00,
        "skattekostnad"
    );
    assert_eq!(
        balance(&pool, company, "2500").await,
        -13_200_00,
        "betalbar skatt"
    );
    assert_eq!(
        balance(&pool, company, "2050").await,
        -46_800_00,
        "opptjent EK"
    );
    assert_eq!(
        balance(&pool, company, "8800").await,
        46_800_00,
        "disponeringen"
    );

    // The balanse: udisponert is empty, and it still balances.
    let lines = regnmed_db::balance_lines(&pool, company, None, d(2025, 12, 31), None, None)
        .await
        .unwrap();
    let b = regnmed_core::statements::balance_sheet(&lines);
    assert_eq!(
        b.undistributed_result_ore, 0,
        "resultatet er disponert — det kan ikke også ligge udisponert"
    );
    assert_eq!(b.difference_ore(), 0);

    // …and the year's resultatregnskap is untouched by its own closing.
    let arets = regnmed_db::balance_lines(
        &pool,
        company,
        Some(d(2025, 1, 1)),
        d(2025, 12, 31),
        None,
        None,
    )
    .await
    .unwrap();
    let r = regnmed_core::statements::result(&arets);
    assert_eq!(
        r.annual_result_ore,
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
    regnmed_db::year_end::close_year(&pool, company, 2025, 0, "test", false, false, None)
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
    let error =
        regnmed_db::year_end::close_year(&pool, company, 2025, 0, "test", false, false, None)
            .await
            .expect_err("et år kan ikke disponeres to ganger");
    assert!(error.to_string().contains("allerede avsluttet"), "{error}");

    // A year already locked cannot be closed: its voucher is dated
    // inside the lock, and the message must say so rather than let the
    // trigger surface two layers down.
    let company2 = selskap_med_overskudd(&pool).await;
    regnmed_db::set_period_lock(&pool, company2, d(2025, 12, 31), "test", false)
        .await
        .unwrap();
    let error =
        regnmed_db::year_end::close_year(&pool, company2, 2025, 0, "test", false, false, None)
            .await
            .expect_err("et låst år krever en administrator");
    assert!(error.to_string().contains("låst til og med"), "{error}");
    assert!(error.to_string().contains("administrator"), "{error}");
}

/// The closing is done in spring, with the first months of the new year
/// already locked for VAT. An admin closes it anyway: the lock goes back
/// to 30.12, the voucher is posted, and the lock comes back — all three
/// in the history, in one transaction.
#[tokio::test]
async fn an_admin_closes_a_year_behind_a_later_lock_and_the_lock_comes_back() {
    let Some(pool) = pool().await else { return };
    let company = selskap_med_overskudd(&pool).await;
    regnmed_db::set_period_lock(&pool, company, d(2026, 3, 31), "test", false)
        .await
        .unwrap();

    // Preview: the lines, nothing kept.
    let preview =
        regnmed_db::year_end::close_year(&pool, company, 2025, 13_200_00, "test", true, true, None)
            .await
            .unwrap();
    assert_eq!(
        preview.lines,
        vec![
            ("8300".to_string(), 13_200_00, "Skattekostnad".to_string()),
            ("2500".to_string(), -13_200_00, "Betalbar skatt".to_string()),
            (
                "8800".to_string(),
                46_800_00,
                "Disponering av årets resultat".to_string()
            ),
            (
                "2050".to_string(),
                -46_800_00,
                "Overført til annen egenkapital".to_string()
            ),
        ]
    );
    assert_eq!(preview.relocked_through, Some(d(2026, 3, 31)));
    assert_eq!(
        balance(&pool, company, "2050").await,
        0,
        "forhåndsvisningen beholder ingenting"
    );
    assert_eq!(
        regnmed_db::period_lock_history(&pool, company)
            .await
            .unwrap()
            .len(),
        1,
        "heller ikke i låsesporet"
    );

    // A result that moved since the preview is refused.
    let error = regnmed_db::year_end::close_year(
        &pool,
        company,
        2025,
        13_200_00,
        "test",
        true,
        false,
        Some(59_000_00),
    )
    .await
    .expect_err("et endret resultat avsluttes ikke");
    assert!(error.to_string().contains("endret seg"), "{error}");

    let closed = regnmed_db::year_end::close_year(
        &pool,
        company,
        2025,
        13_200_00,
        "Astrid",
        true,
        false,
        Some(60_000_00),
    )
    .await
    .unwrap();
    assert_eq!(closed.closing.allocated_ore, 46_800_00);
    assert_eq!(balance(&pool, company, "2050").await, -46_800_00);
    assert_eq!(
        regnmed_db::current_period_lock(&pool, company)
            .await
            .unwrap(),
        Some(d(2026, 3, 31)),
        "låsen står der den sto"
    );
    let history = regnmed_db::period_lock_history(&pool, company)
        .await
        .unwrap();
    let spor: Vec<(NaiveDate, bool)> = history
        .iter()
        .map(|h| (h.locked_through, h.reopened))
        .collect();
    assert_eq!(
        spor,
        vec![
            (d(2026, 3, 31), false),
            (d(2025, 12, 30), true),
            (d(2026, 3, 31), false),
        ],
        "gjenåpningen og gjenlåsingen står i sporet, nyeste først"
    );
    assert!(
        history[1].set_by.contains("Astrid"),
        "{}",
        history[1].set_by
    );
    regnmed_db::verify_chain(&pool, company).await.unwrap();
}

/// A year that has not ended cannot be closed: it would lock December
/// before December happened.
#[tokio::test]
async fn a_year_that_has_not_ended_refuses() {
    let Some(pool) = pool().await else { return };
    let company = selskap_med_overskudd(&pool).await;
    let this_year = chrono::Local::now().date_naive().year();
    let error =
        regnmed_db::year_end::close_year(&pool, company, this_year, 0, "test", true, true, None)
            .await
            .expect_err("et år som ikke er over kan ikke avsluttes");
    assert!(error.to_string().contains("ikke over ennå"), "{error}");
}

/// The next year: income-statement accounts open at zero in the trial
/// balance, the kontospesifikasjon and the SAF-T export, while balance
/// accounts carry over. Before the closing, the openings sum to the
/// undistributed result; after it, to zero.
#[tokio::test]
async fn income_statement_accounts_open_the_next_year_at_zero() {
    let Some(pool) = pool().await else { return };
    let company = selskap_med_overskudd(&pool).await;
    regnmed_db::post_voucher(
        &pool,
        company,
        &VoucherDraft {
            journal_code: "GL".into(),
            voucher_date: d(2026, 2, 1),
            description: "Salg 2026".into(),
            reverses: None,
            entries: vec![e("1920", 10_000_00), e("3000", -10_000_00)],
        },
        "test",
    )
    .await
    .unwrap();
    let opening = |rows: &[regnmed_db::TrialBalanceRow], number: &str| {
        rows.iter()
            .find(|r| r.number == number)
            .map_or(0, |r| r.opening_ore)
    };
    let sum =
        |rows: &[regnmed_db::TrialBalanceRow]| rows.iter().map(|r| r.opening_ore).sum::<i64>();

    // 2025 not closed: the 60 000 profit has no home yet.
    let year = regnmed_db::trial_balance(&pool, company, d(2026, 1, 1), d(2026, 12, 31))
        .await
        .unwrap();
    assert_eq!(opening(&year, "3000"), 0, "salg starter året på null");
    assert_eq!(opening(&year, "6300"), 0);
    assert_eq!(opening(&year, "1920"), 60_000_00, "banken bæres over");
    assert_eq!(sum(&year), 60_000_00, "udisponert resultat fra 2025");

    regnmed_db::year_end::close_year(&pool, company, 2025, 13_200_00, "test", false, false, None)
        .await
        .unwrap();

    let year = regnmed_db::trial_balance(&pool, company, d(2026, 1, 1), d(2026, 12, 31))
        .await
        .unwrap();
    for number in ["3000", "6300", "8300", "8800"] {
        assert_eq!(opening(&year, number), 0, "{number}");
    }
    assert_eq!(opening(&year, "2050"), -46_800_00);
    assert_eq!(sum(&year), 0, "avsluttet år: inngående går i null");
    let salg = year.iter().find(|r| r.number == "3000").unwrap();
    assert_eq!(salg.closing_ore, -10_000_00);

    // Mid-year, an income account opens at the year so far.
    let march = regnmed_db::trial_balance(&pool, company, d(2026, 3, 1), d(2026, 3, 31))
        .await
        .unwrap();
    assert_eq!(opening(&march, "3000"), -10_000_00);

    let posts = regnmed_db::account_specification(
        &pool,
        company,
        Some("3000"),
        d(2026, 1, 1),
        d(2026, 12, 31),
    )
    .await
    .unwrap();
    assert_eq!(posts.len(), 1);
    assert_eq!(posts[0].balance_ore, -10_000_00, "løpende saldo fra null");

    let saft = regnmed_db::load_saft_input(
        &pool,
        company,
        d(2026, 1, 1),
        d(2026, 12, 31),
        "Ola",
        "Nordmann",
    )
    .await
    .unwrap();
    let account = |number: &str| saft.accounts.iter().find(|a| a.number == number).unwrap();
    assert_eq!(
        (account("3000").opening_ore, account("3000").closing_ore),
        (0, -10_000_00)
    );
    assert_eq!(
        (account("6300").opening_ore, account("6300").closing_ore),
        (0, 0)
    );
    assert_eq!(account("1920").opening_ore, 60_000_00);
    assert_eq!(account("2050").opening_ore, -46_800_00);

    // Across New Year an income account's closing would mix two years.
    let error = regnmed_db::load_saft_input(
        &pool,
        company,
        d(2025, 11, 1),
        d(2026, 2, 28),
        "Ola",
        "Nordmann",
    )
    .await
    .expect_err("a selection spanning two fiscal years is refused");
    assert!(error.to_string().contains("one fiscal year"), "{error}");
}
