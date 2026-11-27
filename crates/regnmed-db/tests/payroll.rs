//! Payroll runs against a real Postgres (#46, docs/lonn.md).
//!
//! What must hold: the bilag balances, the forskuddstrekk follows the
//! rules (feriepenger trekkfrie, half tax in December), the
//! arbeidsgiveravgift comes from the satsregister, feriepenger paid out
//! draw down the LIABILITY instead of becoming a new cost, the same month
//! cannot be run twice, and a run cannot be changed afterwards.
//!
//! Requires DATABASE_URL; skips otherwise.

use chrono::NaiveDate;
use regnmed_db::payroll::{self, NewEmployee, PayrollEntry};
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

fn date(y: i32, m: u32, d: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(y, m, d).unwrap()
}

/// MOD11-valid, like every real company's (onboarding checks it) — the
/// a-melding renderer refuses anything else.
fn valid_orgnumber() -> String {
    std::iter::repeat_with(unique_orgnumber)
        .find(|n| regnmed_core::orgnumber::is_valid(n))
        .unwrap()
}

async fn company(pool: &PgPool) -> Uuid {
    let company = regnmed_db::create_company(pool, &valid_orgnumber(), "Lønnstest AS")
        .await
        .unwrap();
    regnmed_db::ensure_journal(pool, company, "GL", "Hovedbok")
        .await
        .unwrap();
    for (number, name) in [
        ("5000", "Lønn til ansatte"),
        ("5090", "Feriepenger"),
        ("5400", "Arbeidsgiveravgift"),
        ("2600", "Forskuddstrekk"),
        ("2770", "Skyldig arbeidsgiveravgift"),
        ("2930", "Skyldig lønn"),
        ("2940", "Skyldige feriepenger"),
        ("1920", "Bankinnskudd"),
    ] {
        regnmed_db::ensure_account(pool, company, number, name)
            .await
            .unwrap();
    }
    company
}

async fn employee(pool: &PgPool, company: Uuid, nin: &str, name: &str, monthly_ore: i64) -> Uuid {
    payroll::create_employee(
        pool,
        company,
        &NewEmployee {
            nin: nin.into(),
            name: name.into(),
            stilling: Some("Utvikler".into()),
            employed_from: date(2025, 1, 1),
            monthly_salary_ore: Some(monthly_ore),
            hourly_wage_ore: None,
            withholding_type: "prosent".into(),
            withholding_percent_bp: Some(3500),
            withholding_table: None,
            holiday_pay_bp: 1020,
            bank_account: None,
            note: None,
            details: Default::default(),
        },
        "Test",
    )
    .await
    .unwrap()
}

/// Sum of a voucher's entries — must be exactly zero, always.
async fn voucher_sum(pool: &PgPool, voucher_id: Uuid) -> i64 {
    sqlx::query_scalar(
        "select coalesce(sum(amount_ore), 0)::bigint from entry where voucher_id = $1",
    )
    .bind(voucher_id)
    .fetch_one(pool)
    .await
    .unwrap()
}

async fn account_amount(pool: &PgPool, voucher_id: Uuid, account: &str) -> i64 {
    sqlx::query_scalar(
        "select coalesce(sum(e.amount_ore), 0)::bigint from entry e
         join account a on a.id = e.account_id
         where e.voucher_id = $1 and a.number = $2",
    )
    .bind(voucher_id)
    .bind(account)
    .fetch_one(pool)
    .await
    .unwrap()
}

#[tokio::test]
async fn a_payroll_run_posts_as_one_balanced_bilag() {
    let Some(pool) = pool().await else { return };
    let company = company(&pool).await;
    // 50 000 kr, 35 % trekk, sone I (14,1 %).
    let a = employee(&pool, company, "26829398612", "Kari Utvikler", 5_000_000).await;

    let run = payroll::run_payroll(
        &pool,
        company,
        2026,
        3,
        date(2026, 3, 25),
        "I",
        &[PayrollEntry {
            employee_id: a,
            gross_ore: None,
            holiday_pay_ore: 0,
            holiday_pay_supplement_ore: None,
            from_timesheet: false,
            holiday_pay_exempt: false,
        }],
        "Test",
    )
    .await
    .unwrap();

    assert_eq!(run.sum.gross_ore, 5_000_000);
    assert_eq!(run.sum.withholding_ore, 1_750_000);
    assert_eq!(run.sum.net_ore, 3_250_000);
    // 10,2 % feriepengeavsetning of the gross pay.
    assert_eq!(run.sum.holiday_pay_accrual_ore, 510_000);
    // 14,1 % arbeidsgiveravgift, from the satsregister — not from the code.
    assert_eq!(run.sum.employer_contribution_ore, 705_000);

    // The bilag balances. Nothing else matters if this fails.
    assert_eq!(voucher_sum(&pool, run.voucher_id).await, 0);

    assert_eq!(
        account_amount(&pool, run.voucher_id, "5000").await,
        5_000_000
    );
    assert_eq!(
        account_amount(&pool, run.voucher_id, "2600").await,
        -1_750_000
    );
    assert_eq!(
        account_amount(&pool, run.voucher_id, "2930").await,
        -3_250_000
    );
    assert_eq!(account_amount(&pool, run.voucher_id, "5090").await, 510_000);
    assert_eq!(
        account_amount(&pool, run.voucher_id, "2940").await,
        -510_000
    );
    assert_eq!(account_amount(&pool, run.voucher_id, "5400").await, 705_000);
    assert_eq!(
        account_amount(&pool, run.voucher_id, "2770").await,
        -705_000
    );
}

/// A company missing a payroll account is told which ones, in Norwegian,
/// before anything is posted — not the ledger's English "no active
/// account" for the first one it trips on.
#[tokio::test]
async fn a_run_names_every_missing_account_before_posting() {
    let Some(pool) = pool().await else { return };
    let company = company(&pool).await;
    let a = employee(&pool, company, "26829398612", "Kari Utvikler", 5_000_000).await;
    sqlx::query(
        "update account set active = false where company_id = $1 and number in ('5000', '2930')",
    )
    .bind(company)
    .execute(&pool)
    .await
    .unwrap();

    let err = payroll::run_payroll(
        &pool,
        company,
        2026,
        3,
        date(2026, 3, 25),
        "I",
        &[PayrollEntry {
            employee_id: a,
            gross_ore: None,
            holiday_pay_ore: 0,
            holiday_pay_supplement_ore: None,
            from_timesheet: false,
            holiday_pay_exempt: false,
        }],
        "Test",
    )
    .await
    .unwrap_err()
    .to_string();

    assert!(err.contains("konto 2930, 5000"), "{err}");
    assert!(err.contains("Hovedbok → Kontoplan"), "{err}");
    let runs: i64 = sqlx::query_scalar("select count(*) from payroll_run where company_id = $1")
        .bind(company)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(runs, 0);
}

/// Feriepenger paid out are not a new cost — they draw down the liability
/// accrued in the year they were earned. Get this wrong and feriepenger
/// are expensed twice, making the result systematically too low.
#[tokio::test]
async fn holiday_pay_paid_out_reduces_the_liability_and_carries_no_withholding() {
    let Some(pool) = pool().await else { return };
    let company = company(&pool).await;
    let a = employee(&pool, company, "08888797336", "Ola Ferierende", 3_000_000).await;

    let run = payroll::run_payroll(
        &pool,
        company,
        2026,
        6,
        date(2026, 6, 20),
        "I",
        &[PayrollEntry {
            employee_id: a,
            gross_ore: None,
            holiday_pay_ore: 4_000_000,
            holiday_pay_supplement_ore: None,
            from_timesheet: false,
            holiday_pay_exempt: false,
        }],
        "Test",
    )
    .await
    .unwrap();

    // Withhold from ordinary pay only: 35 % of 30 000, not of 70 000.
    assert_eq!(run.sum.withholding_ore, 1_050_000);
    assert_eq!(run.sum.net_ore, 3_000_000 + 4_000_000 - 1_050_000);
    assert_eq!(voucher_sum(&pool, run.voucher_id).await, 0);

    // 5000 carries ONLY ordinary pay — the feriepenger are not a new cost.
    assert_eq!(
        account_amount(&pool, run.voucher_id, "5000").await,
        3_000_000
    );
    // 2940 is debited 4 000 000 (drawdown) and credited 306 000 (new accrual).
    assert_eq!(
        account_amount(&pool, run.voucher_id, "2940").await,
        4_000_000 - 306_000
    );
    // The avgift falls on everything actually paid out, feriepenger included.
    assert_eq!(run.sum.employer_contribution_ore, 987_000); // 14,1 % av 70 000
}

async fn employee_at_rate(pool: &PgPool, company: Uuid, nin: &str, rate_bp: i32) -> Uuid {
    payroll::create_employee(
        pool,
        company,
        &NewEmployee {
            nin: nin.into(),
            name: format!("Ansatt {rate_bp}"),
            stilling: None,
            employed_from: date(2025, 1, 1),
            monthly_salary_ore: None,
            hourly_wage_ore: None,
            withholding_type: "prosent".into(),
            withholding_percent_bp: Some(3500),
            withholding_table: None,
            holiday_pay_bp: rate_bp,
            bank_account: None,
            note: None,
            details: Default::default(),
        },
        "Test",
    )
    .await
    .unwrap()
}

#[tokio::test]
async fn the_over_60_addition_is_capped_at_6g_across_the_earning_year() {
    let Some(pool) = pool().await else { return };
    let company = company(&pool).await;
    // Born 1967: turns 59 in 2026, so 2026 pay carries the addition.
    let statutory = employee_at_rate(&pool, company, "15866710020", 1250).await;
    let tariff = employee_at_rate(&pool, company, "16866710140", 1430).await;
    let plain_tariff = employee_at_rate(&pool, company, "17866710260", 1200).await;
    // Born 1968: 12,5 % here is an agreement, not the statutory addition.
    let too_young = employee_at_rate(&pool, company, "15866810041", 1250).await;
    let all = [statutory, tariff, plain_tariff, too_young];

    let mut accrued = std::collections::HashMap::new();
    // 500 000 + 300 000 + 100 000 + 100 000 kr, crossing 6 G (819 294 kr)
    // in the third run. The last run is December's pay paid in January.
    for (month, paid, gross) in [
        (1, date(2026, 1, 20), 50_000_000),
        (2, date(2026, 2, 20), 30_000_000),
        (3, date(2026, 3, 20), 10_000_000),
        (4, date(2026, 4, 20), 10_000_000),
        (12, date(2027, 1, 5), 10_000_000),
    ] {
        let entries: Vec<_> = all
            .iter()
            .map(|&employee_id| PayrollEntry {
                employee_id,
                gross_ore: Some(gross),
                holiday_pay_ore: 0,
                holiday_pay_supplement_ore: None,
                from_timesheet: false,
                holiday_pay_exempt: false,
            })
            .collect();
        let run = payroll::run_payroll(&pool, company, 2026, month, paid, "I", &entries, "Test")
            .await
            .unwrap();
        assert_eq!(voucher_sum(&pool, run.voucher_id).await, 0);
        assert_eq!(
            run.warnings.iter().filter(|w| w.contains("6G")).count(),
            if month == 3 || month == 4 { 2 } else { 0 },
            "{:?}",
            run.warnings
        );
        for line in &run.lines {
            if month == 12 {
                // A new earning year: the count starts over, uncapped.
                let rate = match line.employee_id {
                    e if e == tariff => 1430,
                    e if e == plain_tariff => 1200,
                    _ => 1250,
                };
                assert_eq!(line.holiday_pay_accrual_ore, gross * rate / 10_000);
            } else {
                *accrued.entry(line.employee_id).or_insert(0) += line.holiday_pay_accrual_ore;
            }
        }
    }

    // 1 000 000 kr in 2026. 2,3 % of 819 294 kr = 18 843,76 kr.
    assert_eq!(accrued[&statutory], 10_200_000 + 1_884_376);
    assert_eq!(accrued[&tariff], 12_000_000 + 1_884_376);
    assert_eq!(accrued[&plain_tariff], 12_000_000);
    assert_eq!(accrued[&too_young], 12_500_000);
}

#[tokio::test]
async fn raising_the_rate_mid_year_catches_up_the_addition_and_keeps_old_payslips() {
    let Some(pool) = pool().await else { return };
    let company = company(&pool).await;
    // Born 1967: turns 59 in 2026.
    let id = employee_at_rate(&pool, company, "15866710020", 1020).await;
    let pay = |month, gross| {
        let pool = pool.clone();
        async move {
            payroll::run_payroll(
                &pool,
                company,
                2026,
                month,
                date(2026, month, 20),
                "I",
                &[PayrollEntry {
                    employee_id: id,
                    gross_ore: Some(gross),
                    holiday_pay_ore: 0,
                    holiday_pay_supplement_ore: None,
                    from_timesheet: false,
                    holiday_pay_exempt: false,
                }],
                "Test",
            )
            .await
            .unwrap()
        }
    };

    let january = pay(1, 30_000_000).await;
    assert_eq!(january.lines[0].holiday_pay_accrual_ore, 3_060_000);
    assert!(
        january.warnings.iter().any(|w| w.contains("krever 12,5 %")),
        "{:?}",
        january.warnings
    );

    payroll::update_holiday_rate(&pool, company, id, 1250)
        .await
        .unwrap();

    // 12,5 % of February's pay + 2,3 % of January's.
    let february = pay(2, 30_000_000).await;
    assert_eq!(
        february.lines[0].holiday_pay_accrual_ore,
        3_750_000 + 690_000
    );
    assert!(!february.warnings.iter().any(|w| w.contains("krever")));
    assert!(
        february.warnings.iter().any(|w| w.contains("6900,00 kr")),
        "{:?}",
        february.warnings
    );

    // Crosses 6 G (819 294 kr): the year ends at 10,2 % of 900 000 kr + 2,3 %
    // of 6 G, so pay from before the raise is neither missed nor over-cut.
    let march = pay(3, 30_000_000).await;
    assert_eq!(
        3_060_000
            + february.lines[0].holiday_pay_accrual_ore
            + march.lines[0].holiday_pay_accrual_ore,
        9_180_000 + 1_884_376
    );

    let slip = |run| payroll::payslip(&pool, company, run, id);
    assert_eq!(slip(january.id).await.unwrap().holiday_pay_bp, 1020);
    assert_eq!(slip(february.id).await.unwrap().holiday_pay_bp, 1250);
}

#[tokio::test]
async fn pay_at_an_agreed_rate_does_not_use_up_the_6g_of_a_later_addition() {
    let Some(pool) = pool().await else { return };
    let company = company(&pool).await;
    let id = employee_at_rate(&pool, company, "15866710020", 1300).await;
    run(&pool, company, id, 1, "I", Some(90_000_000), 0).await;
    payroll::update_holiday_rate(&pool, company, id, 1430)
        .await
        .unwrap();
    let february = run(&pool, company, id, 2, "I", Some(10_000_000), 0).await;
    assert_eq!(february.lines[0].holiday_pay_cap_cut_ore, 0);
    assert_eq!(february.lines[0].holiday_pay_accrual_ore, 1_430_000);
}

#[tokio::test]
async fn simultaneous_runs_cannot_both_post_the_catch_up() {
    let Some(pool) = pool().await else { return };
    let company = company(&pool).await;
    let id = employee_at_rate(&pool, company, "15866710020", 1020).await;
    let january = run(&pool, company, id, 1, "I", Some(30_000_000), 0).await;
    payroll::update_holiday_rate(&pool, company, id, 1250)
        .await
        .unwrap();

    let attempt = |month| {
        let pool = pool.clone();
        async move {
            payroll::run_payroll(
                &pool,
                company,
                2026,
                month,
                date(2026, month, 20),
                "I",
                &[PayrollEntry {
                    employee_id: id,
                    gross_ore: Some(10_000_000),
                    holiday_pay_ore: 0,
                    holiday_pay_supplement_ore: None,
                    from_timesheet: false,
                    holiday_pay_exempt: false,
                }],
                "Test",
            )
            .await
        }
    };
    let (february, march) = tokio::join!(attempt(2), attempt(3));
    // The loser is refused and simply runs again.
    let february = match february {
        Ok(run) => run,
        Err(_) => attempt(2).await.unwrap(),
    };
    let march = match march {
        Ok(run) => run,
        Err(_) => attempt(3).await.unwrap(),
    };
    assert_eq!(
        january.lines[0].holiday_pay_accrual_ore
            + february.lines[0].holiday_pay_accrual_ore
            + march.lines[0].holiday_pay_accrual_ore,
        6_250_000 // 12,5 % of 500 000 kr, with the catch-up counted once.
    );
}

/// A line from before migration 0105 has no rate of its own.
#[tokio::test]
async fn a_line_without_a_stored_rate_reads_the_rate_from_before_any_edit() {
    let Some(pool) = pool().await else { return };
    let company = company(&pool).await;
    let id = employee_at_rate(&pool, company, "15866710020", 1020).await;
    sqlx::query("update employee set legacy_holiday_pay_bp = 1020 where id = $1")
        .bind(id)
        .execute(&pool)
        .await
        .unwrap();
    let january = run(&pool, company, id, 1, "I", Some(30_000_000), 0).await;
    let old_run = Uuid::new_v4();
    sqlx::query(
        "insert into payroll_run (id, company_id, ar, maned, utbetalt_dato, sone, brutto_ore,
                                  forskuddstrekk_ore, netto_ore, feriepengeavsetning_ore,
                                  aga_ore, aga_feriepenger_ore, voucher_id, created_by)
         select $1, company_id, ar, 2, date '2026-02-20', sone, brutto_ore, forskuddstrekk_ore,
                netto_ore, feriepengeavsetning_ore, aga_ore, aga_feriepenger_ore, voucher_id,
                created_by
         from payroll_run where id = $2",
    )
    .bind(old_run)
    .bind(january.id)
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "insert into payroll_line (id, run_id, employee_id, brutto_ore, trekkgrunnlag_ore,
                                   forskuddstrekk_ore, netto_ore, feriepengeavsetning_ore,
                                   aga_ore, aga_feriepenger_ore)
         values ($1, $2, $3, 30000000, 30000000, 0, 30000000, 3060000, 0, 0)",
    )
    .bind(Uuid::new_v4())
    .bind(old_run)
    .bind(id)
    .execute(&pool)
    .await
    .unwrap();

    payroll::update_holiday_rate(&pool, company, id, 1250)
        .await
        .unwrap();
    let slip = payroll::payslip(&pool, company, old_run, id).await.unwrap();
    assert_eq!(slip.holiday_pay_bp, 1020);
    // March catches up the addition on both January and the stored-rate-less
    // February line.
    let march = run(&pool, company, id, 3, "I", Some(10_000_000), 0).await;
    assert_eq!(
        march.lines[0].holiday_pay_accrual_ore,
        1_250_000 + 2 * 690_000
    );
}

#[tokio::test]
async fn the_holiday_rate_edit_stays_inside_the_company_and_the_valid_range() {
    let Some(pool) = pool().await else { return };
    let owner = company(&pool).await;
    let other = company(&pool).await;
    let id = employee_at_rate(&pool, owner, "15866710020", 1020).await;

    assert!(
        payroll::update_holiday_rate(&pool, other, id, 1250)
            .await
            .is_err()
    );
    assert!(
        payroll::update_holiday_rate(&pool, owner, id, 5001)
            .await
            .is_err()
    );
    let rate = payroll::list_ansatte(&pool, owner).await.unwrap()[0].holiday_pay_bp;
    assert_eq!(rate, 1020);
}

#[tokio::test]
async fn over_60_holiday_pay_withholds_only_the_supplement() {
    let Some(pool) = pool().await else { return };
    let company = company(&pool).await;
    // Born 1967: 2026 pay carries the addition, paid out in 2027.
    let statutory = employee_at_rate(&pool, company, "15866710020", 1250).await;
    let tariff = employee_at_rate(&pool, company, "16866710140", 1430).await;
    let ordinary = employee_at_rate(&pool, company, "17866710260", 1020).await;
    // Born 1968: 12,5 % is an agreement, with no addition to withhold on.
    let too_young = employee_at_rate(&pool, company, "15866810041", 1250).await;
    let all = [statutory, tariff, ordinary, too_young];
    for id in all {
        payroll::update_employment_details(&pool, company, id, &complete_details("974760673"))
            .await
            .unwrap();
    }
    let entry = |employee_id, gross, holiday, stated| PayrollEntry {
        employee_id,
        gross_ore: Some(gross),
        holiday_pay_ore: holiday,
        holiday_pay_supplement_ore: stated,
        from_timesheet: false,
        holiday_pay_exempt: false,
    };
    // 1 000 000 kr in 2026, above 6 G (819 294 kr).
    let earned: Vec<_> = all
        .iter()
        .map(|&id| entry(id, 100_000_000, 0, None))
        .collect();
    let first = payroll::run_payroll(
        &pool,
        company,
        2026,
        1,
        date(2026, 1, 20),
        "I",
        &earned,
        "Test",
    )
    .await
    .unwrap();
    let accrual = |id| {
        first
            .lines
            .iter()
            .find(|l| l.employee_id == id)
            .unwrap()
            .holiday_pay_accrual_ore
    };
    assert_eq!(accrual(statutory), 10_200_000 + 1_884_376);
    assert_eq!(accrual(tariff), 12_000_000 + 1_884_376);

    let preview = |entries: Vec<PayrollEntry>| {
        let pool = pool.clone();
        async move {
            payroll::preview_payroll(&pool, company, 2027, 6, date(2027, 6, 20), "I", &entries)
                .await
        }
    };
    // Part of the balance: refused until the supplement is stated.
    let refused = preview(vec![entry(statutory, 0, 5_000_000, None)])
        .await
        .unwrap_err()
        .to_string();
    assert!(refused.contains("tillegget på 2,3"), "{refused}");
    let stated = preview(vec![entry(statutory, 0, 5_000_000, Some(500_000))])
        .await
        .unwrap();
    assert_eq!(stated.lines[0].calculation.withholding_basis_ore, 500_000);
    assert_eq!(stated.lines[0].calculation.withholding_ore, 175_000);
    let too_much = preview(vec![entry(statutory, 0, 5_000_000, Some(5_000_001))])
        .await
        .unwrap_err()
        .to_string();
    assert!(too_much.contains("større enn"), "{too_much}");
    // A card that exempts nothing ignores the split but not a bad value.
    let no_withholding = payroll::create_employee(
        &pool,
        company,
        &NewEmployee {
            nin: "03048810003".into(),
            name: "Uten Trekk".into(),
            stilling: None,
            employed_from: date(2025, 1, 1),
            monthly_salary_ore: None,
            hourly_wage_ore: None,
            withholding_type: "ingen".into(),
            withholding_percent_bp: None,
            withholding_table: None,
            holiday_pay_bp: 1020,
            bank_account: None,
            note: None,
            details: complete_details("974760673"),
        },
        "Test",
    )
    .await
    .unwrap();
    let invalid = preview(vec![entry(no_withholding, 0, 100_000, Some(100_001))])
        .await
        .unwrap_err()
        .to_string();
    assert!(invalid.contains("større enn"), "{invalid}");

    // Marked exempt: the supplement is withheld all the same.
    let marked = preview(vec![PayrollEntry {
        holiday_pay_exempt: true,
        ..entry(statutory, 0, accrual(statutory), None)
    }])
    .await
    .unwrap();
    assert_eq!(marked.lines[0].calculation.withholding_basis_ore, 1_884_376);
    assert!(
        marked
            .warnings
            .iter()
            .any(|w| w.contains("merket trekkfri")),
        "{:?}",
        marked.warnings
    );

    // The whole of last year's accrual: the split is computed.
    let payout: Vec<_> = all
        .iter()
        .map(|&id| entry(id, 0, accrual(id), None))
        .collect();
    let run = payroll::run_payroll(
        &pool,
        company,
        2027,
        6,
        date(2027, 6, 20),
        "I",
        &payout,
        "Test",
    )
    .await
    .unwrap();
    assert_eq!(voucher_sum(&pool, run.voucher_id).await, 0);
    for line in &run.lines {
        let (basis, withheld) = if [statutory, tariff].contains(&line.employee_id) {
            // 35 % of 18 843,76 kr, rounded down to whole kroner.
            (1_884_376, 659_500)
        } else if line.employee_id == too_young {
            // An agreed 12,5 %: only what exceeds 12,342 % of the basis.
            (158_000, 55_300)
        } else {
            (0, 0)
        };
        assert_eq!(line.calculation.withholding_basis_ore, basis);
        assert_eq!(line.calculation.withholding_ore, withheld);
    }

    // One feriepenger line per employee, supplement included.
    let draft = payroll::amelding_for_run(&pool, company, run.id, chrono::Utc::now())
        .await
        .unwrap();
    assert!(draft.hindrances.is_empty(), "{:?}", draft.hindrances);
    let xml = regnmed_core::amelding::render(&draft.melding).unwrap();
    assert_eq!(
        xml.matches("<beskrivelse>feriepenger</beskrivelse>")
            .count(),
        4
    );
}

#[tokio::test]
async fn the_over_60_supplement_is_withheld_on_leaving_and_when_marked_exempt() {
    let Some(pool) = pool().await else { return };
    let company = company(&pool).await;
    // Born 1967: both years below carry the addition.
    let id = employee_at_rate(&pool, company, "15866710020", 1250).await;
    for (year, month) in [(2026, 1), (2027, 2)] {
        let entry = PayrollEntry {
            employee_id: id,
            gross_ore: Some(10_000_000),
            holiday_pay_ore: 0,
            holiday_pay_supplement_ore: None,
            from_timesheet: false,
            holiday_pay_exempt: false,
        };
        payroll::run_payroll(
            &pool,
            company,
            year,
            month,
            date(year, month, 20),
            "I",
            &[entry],
            "Test",
        )
        .await
        .unwrap();
    }
    payroll::end_employment(&pool, company, id, Some(date(2027, 3, 31)))
        .await
        .unwrap();
    let payout = |stated, exempt| {
        let pool = pool.clone();
        async move {
            let entry = PayrollEntry {
                employee_id: id,
                gross_ore: Some(0),
                // 1 250 000 from 2026 + 1 250 000 earned in 2027.
                holiday_pay_ore: 2_500_000,
                holiday_pay_supplement_ore: stated,
                from_timesheet: false,
                holiday_pay_exempt: exempt,
            };
            payroll::preview_payroll(&pool, company, 2027, 3, date(2027, 3, 20), "I", &[entry])
                .await
        }
    };

    // This year's part is withheld in full, last year's only on its 2,3 %.
    let leaving = payout(None, false).await.unwrap();
    assert_eq!(
        leaving.lines[0].calculation.withholding_basis_ore,
        1_250_000 + 230_000
    );

    // Marked exempt: the payment holds two years' supplements, so the
    // operator must say how much.
    let refused = payout(None, true).await.unwrap_err().to_string();
    assert!(refused.contains("tillegget på 2,3"), "{refused}");
    let stated = payout(Some(460_000), true).await.unwrap();
    assert_eq!(stated.lines[0].calculation.withholding_basis_ore, 460_000);
    assert_eq!(stated.lines[0].calculation.withholding_ore, 161_000);
    assert_eq!(
        stated.lines[0].calculation.method,
        "35,00 %, feriepenger merket trekkfrie unntatt tillegget over 60 år"
    );
    // The operator can still record that none is withheld.
    let none = payout(Some(0), true).await.unwrap();
    assert_eq!(none.lines[0].calculation.withholding_ore, 0);
}

#[tokio::test]
async fn a_leaver_is_capped_at_the_g_on_the_leaving_date_and_earlier_cuts_are_netted() {
    let Some(pool) = pool().await else { return };
    let company = company(&pool).await;
    let id = employee_at_rate(&pool, company, "15866710020", 1250).await;
    let entry = |gross| {
        [PayrollEntry {
            employee_id: id,
            gross_ore: Some(gross),
            holiday_pay_ore: 0,
            holiday_pay_supplement_ore: None,
            from_timesheet: false,
            holiday_pay_exempt: false,
        }]
    };

    // 850 000 kr against 6 G of 819 294 kr: 2,3 % of 30 706 kr = 706,24 kr.
    let first = payroll::run_payroll(
        &pool,
        company,
        2026,
        1,
        date(2026, 1, 20),
        "I",
        &entry(85_000_000),
        "Test",
    )
    .await
    .unwrap();
    assert_eq!(first.lines[0].holiday_pay_cap_cut_ore, 70_624);

    // Leaving before 1 May moves the cap to 6 × 130 160 = 780 960 kr.
    payroll::end_employment(&pool, company, id, Some(date(2026, 3, 31)))
        .await
        .unwrap();
    let last = payroll::run_payroll(
        &pool,
        company,
        2026,
        3,
        date(2026, 3, 20),
        "I",
        &entry(10_000_000),
        "Test",
    )
    .await
    .unwrap();
    // The year's cut is 2,3 % of 169 040 kr = 3 887,92 kr, less 706,24 kr.
    assert_eq!(last.lines[0].holiday_pay_cap_cut_ore, 388_792 - 70_624);
    // 10,2 % of 950 000 kr + 2,3 % of 780 960 kr.
    assert_eq!(
        first.lines[0].holiday_pay_accrual_ore + last.lines[0].holiday_pay_accrual_ore,
        9_690_000 + 1_796_208
    );
}

fn holiday_entry(id: Uuid, gross: i64, holiday: i64, exempt: bool) -> [PayrollEntry; 1] {
    [PayrollEntry {
        employee_id: id,
        gross_ore: Some(gross),
        holiday_pay_ore: holiday,
        holiday_pay_supplement_ore: None,
        from_timesheet: false,
        holiday_pay_exempt: exempt,
    }]
}

async fn run_in(
    pool: &PgPool,
    company: Uuid,
    id: Uuid,
    (year, month): (i32, u32),
    gross: i64,
    holiday: i64,
) -> payroll::PayrollRun {
    payroll::run_payroll(
        pool,
        company,
        year,
        month,
        date(year, month, 20),
        "I",
        &holiday_entry(id, gross, holiday, false),
        "Test",
    )
    .await
    .unwrap()
}

#[tokio::test]
async fn a_leaver_is_withheld_on_holiday_pay_earned_in_the_leaving_year() {
    let Some(pool) = pool().await else { return };
    let company = company(&pool).await;
    let id = employee_at_rate(&pool, company, "08888797336", 1020).await;
    run_in(&pool, company, id, (2025, 3), 10_000_000, 0).await;
    run_in(&pool, company, id, (2026, 3), 10_000_000, 0).await;
    payroll::end_employment(&pool, company, id, Some(date(2026, 4, 30)))
        .await
        .unwrap();

    // Owed: 10 200 kr from 2025, then 10 200 + 1 020 kr earned in 2026.
    let payout = 2_142_000;
    let on_death = payroll::preview_payroll(
        &pool,
        company,
        2026,
        4,
        date(2026, 4, 20),
        "I",
        &holiday_entry(id, 1_000_000, payout, true),
    )
    .await
    .unwrap();
    assert_eq!(on_death.sum.withholding_ore, 350_000, "§ 5-7-1 (1) d");

    let last = run_in(&pool, company, id, (2026, 4), 1_000_000, payout).await;
    let line = &last.lines[0];
    assert_eq!(line.holiday_pay_earning_year_ore, 1_122_000);
    assert_eq!(line.calculation.withholding_basis_ore, 2_122_000);
    // 35 % of the pay and of 2026's holiday pay; 2025's stays exempt.
    assert_eq!(line.calculation.withholding_ore, 742_700);
    assert!(last.warnings.iter().any(|w| w.contains("fratreden")));
    let stored: i64 = sqlx::query_scalar(
        "select holiday_pay_earning_year_ore from payroll_line where run_id = $1",
    )
    .bind(last.id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(stored, 1_122_000);
}

#[tokio::test]
async fn agreed_holiday_pay_above_the_limit_is_withheld_on_the_excess() {
    let Some(pool) = pool().await else { return };
    let company = company(&pool).await;
    let id = employee_at_rate(&pool, company, "15929998666", 1500).await;
    run_in(&pool, company, id, (2025, 3), 100_000_000, 0).await;

    let june = run_in(&pool, company, id, (2026, 6), 1_000_000, 15_000_000).await;
    // 15 % against 12,342 % of 1 000 000 kr: 26 580 kr is not exempt.
    assert_eq!(june.lines[0].calculation.withholding_basis_ore, 3_658_000);
    assert_eq!(june.lines[0].calculation.withholding_ore, 1_280_300);
    assert_eq!(june.lines[0].holiday_pay_earning_year_ore, 0);
    assert!(june.warnings.iter().any(|w| w.contains("21 prosent")));
}

#[tokio::test]
async fn the_excess_follows_the_rate_the_holiday_pay_was_accrued_at() {
    let Some(pool) = pool().await else { return };
    let company = company(&pool).await;
    // Born 1967: 14,3 % would carry the over-60 addition in 2026, but 2026
    // was accrued at an agreed 15 %, so today's rate must not excuse it.
    let id = employee_at_rate(&pool, company, "15866710020", 1500).await;
    run_in(&pool, company, id, (2026, 3), 100_000_000, 0).await;
    payroll::update_holiday_rate(&pool, company, id, 1430)
        .await
        .unwrap();

    let june = run_in(&pool, company, id, (2027, 6), 1_000_000, 15_000_000).await;
    assert_eq!(june.lines[0].calculation.withholding_basis_ore, 3_658_000);
    assert_eq!(june.lines[0].calculation.withholding_ore, 1_280_300);
}

#[tokio::test]
async fn a_table_card_withholds_holiday_pay_at_its_printed_percentage() {
    let Some(pool) = pool().await else { return };
    let company = company(&pool).await;
    let id = payroll::create_employee(
        &pool,
        company,
        &NewEmployee {
            nin: "15929998666".into(),
            name: "Tabell Slutter".into(),
            stilling: None,
            employed_from: date(2025, 1, 1),
            monthly_salary_ore: None,
            hourly_wage_ore: None,
            withholding_type: "tabell".into(),
            withholding_percent_bp: None,
            withholding_table: Some(8000),
            holiday_pay_bp: 1020,
            bank_account: None,
            note: None,
            details: Default::default(),
        },
        "Test",
    )
    .await
    .unwrap();
    run_in(&pool, company, id, (2026, 3), 10_000_000, 0).await;
    payroll::end_employment(&pool, company, id, Some(date(2026, 4, 30)))
        .await
        .unwrap();
    let last_run = holiday_entry(id, 1_000_000, 1_122_000, false);
    let refused =
        payroll::preview_payroll(&pool, company, 2026, 4, date(2026, 4, 20), "I", &last_run)
            .await
            .unwrap_err();
    assert!(refused.to_string().contains("trekkprosent"), "{refused}");

    let order = Uuid::new_v4();
    regnmed_db::tax_card::record_request(&pool, order, company, 2026, 1, "Test")
        .await
        .unwrap();
    regnmed_db::tax_card::record_order(
        &pool,
        order,
        company,
        2026,
        &format!("BR{}", order.as_u128() % 1_000_000_000),
        Some("d-1"),
        1,
        "Test",
    )
    .await
    .unwrap();
    regnmed_db::tax_card::record_answer(
        &pool,
        company,
        order,
        &[regnmed_db::tax_card::ReceivedCard {
            nin: "15929998666".into(),
            result: "skattekortopplysningerOK".into(),
            card_id: None,
            issued: None,
            raw: serde_json::json!({}),
            decision: Ok(regnmed_core::payroll::Withholding::Table(8000)),
            table_percent_bp: Some(4000),
        }],
    )
    .await
    .unwrap();

    let last = run_in(&pool, company, id, (2026, 4), 1_000_000, 1_122_000).await;
    let on_pay = regnmed_core::withholding_table::withholding_kr(
        2026,
        8000,
        regnmed_core::withholding_table::Period::Month,
        10_000,
    )
    .unwrap()
        * 100;
    assert_eq!(last.lines[0].calculation.withholding_ore, on_pay + 448_800);
}

#[tokio::test]
async fn an_exempt_mark_is_recorded_and_shown_on_the_payslip() {
    let Some(pool) = pool().await else { return };
    let company = company(&pool).await;
    let id = employee_at_rate(&pool, company, "08888797336", 1020).await;
    run_in(&pool, company, id, (2026, 3), 10_000_000, 0).await;
    payroll::end_employment(&pool, company, id, Some(date(2026, 4, 30)))
        .await
        .unwrap();
    let run = payroll::run_payroll(
        &pool,
        company,
        2026,
        4,
        date(2026, 4, 20),
        "I",
        &holiday_entry(id, 1_000_000, 1_122_000, true),
        "Test",
    )
    .await
    .unwrap();
    assert_eq!(run.sum.withholding_ore, 350_000);
    let stored: bool =
        sqlx::query_scalar("select holiday_pay_exempt from payroll_line where run_id = $1")
            .bind(run.id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(stored);
    let slip = payroll::payslip(&pool, company, run.id, id).await.unwrap();
    assert_eq!(
        slip.withholding_method.as_deref(),
        Some("35,00 %, feriepenger merket trekkfrie")
    );
}

#[tokio::test]
async fn an_exempt_mark_has_no_effect_on_a_twelve_month_card() {
    let Some(pool) = pool().await else { return };
    let company = company(&pool).await;
    let id = payroll::create_employee(
        &pool,
        company,
        &NewEmployee {
            nin: "15929998666".into(),
            name: "Tolv Måneder".into(),
            stilling: None,
            employed_from: date(2025, 1, 1),
            monthly_salary_ore: None,
            hourly_wage_ore: None,
            withholding_type: "tabell".into(),
            withholding_percent_bp: None,
            withholding_table: Some(7350),
            holiday_pay_bp: 1020,
            bank_account: None,
            note: None,
            details: Default::default(),
        },
        "Test",
    )
    .await
    .unwrap();
    payroll::end_employment(&pool, company, id, Some(date(2026, 4, 30)))
        .await
        .unwrap();
    let preview = |exempt| {
        let entry = holiday_entry(id, 1_000_000, 500_000, exempt);
        let pool = pool.clone();
        async move {
            payroll::preview_payroll(&pool, company, 2026, 4, date(2026, 4, 20), "I", &entry)
                .await
                .unwrap()
        }
    };
    let (plain, marked) = (preview(false).await, preview(true).await);
    assert_eq!(marked.sum.withholding_ore, plain.sum.withholding_ore);
    assert!(!marked.lines[0].holiday_pay_exempt);
    assert!(marked.warnings.iter().any(|w| w.contains("ingen virkning")));
    assert!(!plain.warnings.iter().any(|w| w.contains("merkes som")));
}

#[tokio::test]
async fn only_the_last_payday_before_the_end_date_is_a_leaving_payout() {
    let Some(pool) = pool().await else { return };
    let company = company(&pool).await;
    // A monthly wage: December's half trekk needs to know the pay form.
    let id = employee(&pool, company, "08888797336", "Ola Slutter", 1_000_000).await;
    run_in(&pool, company, id, (2026, 1), 10_000_000, 0).await;

    // Notice for October: June's payout is not caused by the leaving.
    payroll::end_employment(&pool, company, id, Some(date(2026, 10, 31)))
        .await
        .unwrap();
    let june = payroll::preview_payroll(
        &pool,
        company,
        2026,
        6,
        date(2026, 6, 20),
        "I",
        &holiday_entry(id, 1_000_000, 1_500_000, false),
    )
    .await
    .unwrap();
    assert_eq!(june.lines[0].holiday_pay_earning_year_ore, 0);
    assert_eq!(june.sum.withholding_ore, 350_000);

    // Ending 5 January makes 20 December the last payday before leaving.
    payroll::end_employment(&pool, company, id, Some(date(2027, 1, 5)))
        .await
        .unwrap();
    let december = run_in(&pool, company, id, (2026, 12), 1_000_000, 1_122_000).await;
    assert_eq!(december.lines[0].holiday_pay_earning_year_ore, 1_122_000);
    // Half trekk on the pay, the card's full 35 % on the holiday pay.
    assert_eq!(december.sum.withholding_ore, 175_000 + 392_700);
}

#[tokio::test]
async fn a_run_says_so_when_the_cap_rests_on_a_stale_g() {
    let Some(pool) = pool().await else { return };
    let company = company(&pool).await;
    let id = employee_at_rate(&pool, company, "15866710020", 1250).await;
    let run = payroll::preview_payroll(
        &pool,
        company,
        2099,
        1,
        date(2099, 1, 20),
        "I",
        &[PayrollEntry {
            employee_id: id,
            gross_ore: Some(10_000_000_000),
            holiday_pay_ore: 0,
            holiday_pay_supplement_ore: None,
            from_timesheet: false,
            holiday_pay_exempt: false,
        }],
    )
    .await
    .unwrap();
    assert!(
        run.warnings.iter().any(|w| w.contains("utdatert")),
        "{:?}",
        run.warnings
    );
}

#[tokio::test]
async fn december_withholds_half() {
    let Some(pool) = pool().await else { return };
    let company = company(&pool).await;
    let a = employee(&pool, company, "25927898821", "Nils Desember", 5_000_000).await;

    run_month(&pool, company, a, 2026, 11, date(2026, 11, 15), None, 0).await;
    let run = run_month(&pool, company, a, 2026, 12, date(2026, 12, 15), None, 0).await;
    assert_eq!(run.sum.withholding_ore, 875_000, "halv skatt i desember");
    assert!(run.lines[0].calculation.half_withholding);
    assert_eq!(voucher_sum(&pool, run.voucher_id).await, 0);
    assert!(
        !run.warnings.iter().any(|w| w.contains("5-7-2")),
        "{:?}",
        run.warnings
    );
}

#[allow(clippy::too_many_arguments)]
async fn run_month(
    pool: &PgPool,
    company: Uuid,
    employee_id: Uuid,
    year: i32,
    month: u32,
    paid: NaiveDate,
    gross_ore: Option<i64>,
    holiday_pay_ore: i64,
) -> payroll::PayrollRun {
    payroll::run_payroll(
        pool,
        company,
        year,
        month,
        paid,
        "I",
        &[PayrollEntry {
            employee_id,
            gross_ore,
            holiday_pay_ore,
            holiday_pay_supplement_ore: None,
            from_timesheet: false,
            holiday_pay_exempt: false,
        }],
        "Test",
    )
    .await
    .unwrap()
}

/// § 5-7-2 (4): a bonus in December is halved only up to average earnings.
/// A June with feriepenger and ferietrekk does not pull the average down.
#[tokio::test]
async fn december_halves_only_up_to_average_earnings() {
    let Some(pool) = pool().await else { return };
    let company = company(&pool).await;
    let a = employee(&pool, company, "25927898821", "Bo Nus", 5_000_000).await;

    run_month(&pool, company, a, 2026, 5, date(2026, 5, 15), None, 0).await;
    let june = date(2026, 6, 15);
    run_month(&pool, company, a, 2026, 6, june, Some(1_000_000), 100_000).await;
    run_month(&pool, company, a, 2026, 11, date(2026, 11, 15), None, 0).await;

    let run = run_month(
        &pool,
        company,
        a,
        2026,
        12,
        date(2026, 12, 15),
        Some(15_000_000),
        0,
    )
    .await;
    assert_eq!(run.sum.withholding_ore, 875_000 + 3_500_000);
    assert!(
        run.warnings.iter().any(|w| w.contains("fjerde ledd")),
        "{:?}",
        run.warnings
    );
}

/// § 5-7-2 (2) exempts two specific weeks, which a monthly run cannot
/// separate, so hourly pay is withheld in full and the run says why.
#[tokio::test]
async fn hourly_pay_is_withheld_in_full_in_december() {
    let Some(pool) = pool().await else { return };
    let company = company(&pool).await;
    let a = payroll::create_employee(
        &pool,
        company,
        &NewEmployee {
            nin: "26829398612".into(),
            name: "Tim Lønn".into(),
            stilling: None,
            employed_from: date(2025, 1, 1),
            monthly_salary_ore: None,
            hourly_wage_ore: Some(45_000),
            withholding_type: "prosent".into(),
            withholding_percent_bp: Some(3500),
            withholding_table: None,
            holiday_pay_bp: 1020,
            bank_account: None,
            note: None,
            details: Default::default(),
        },
        "Test",
    )
    .await
    .unwrap();
    let nov = date(2026, 11, 15);
    run_month(&pool, company, a, 2026, 11, nov, Some(4_000_000), 0).await;
    let dec = date(2026, 12, 15);
    let run = run_month(&pool, company, a, 2026, 12, dec, Some(4_000_000), 0).await;
    assert_eq!(run.sum.withholding_ore, 1_400_000);
    assert!(!run.lines[0].calculation.half_withholding);
    assert!(
        run.warnings.iter().any(|w| w.contains("annet ledd")),
        "{:?}",
        run.warnings
    );
}

#[tokio::test]
async fn december_without_earlier_pay_is_withheld_in_full() {
    let Some(pool) = pool().await else { return };
    let company = company(&pool).await;
    let a = employee(&pool, company, "25927898821", "Ny Ansatt", 5_000_000).await;
    let run = run_month(&pool, company, a, 2026, 12, date(2026, 12, 15), None, 0).await;
    assert_eq!(run.sum.withholding_ore, 1_750_000);
    assert!(
        run.warnings
            .iter()
            .any(|w| w.contains("ingen tidligere lønnsmåned")),
        "{:?}",
        run.warnings
    );
}

/// Payday moved across New Year: December 2026 paid in January 2027 keeps
/// its half and averages 2026, but December 2027 paid in December 2027
/// would be a second half in one income year.
#[tokio::test]
async fn one_payment_year_gets_one_december_half() {
    let Some(pool) = pool().await else { return };
    let company = company(&pool).await;
    let a = employee(&pool, company, "25927898821", "Flytt Lønnsdag", 5_000_000).await;

    run_month(&pool, company, a, 2026, 11, date(2026, 11, 15), None, 0).await;
    let first = run_month(&pool, company, a, 2026, 12, date(2027, 1, 15), None, 0).await;
    assert_eq!(first.sum.withholding_ore, 875_000);

    run_month(&pool, company, a, 2027, 11, date(2027, 11, 15), None, 0).await;
    let second = run_month(&pool, company, a, 2027, 12, date(2027, 12, 15), None, 0).await;
    assert_eq!(second.sum.withholding_ore, 1_750_000);
    assert!(!second.lines[0].calculation.half_withholding);
    assert!(
        second.warnings.iter().any(|w| w.contains("allerede fått")),
        "{:?}",
        second.warnings
    );
}

#[tokio::test]
async fn sone_v_is_a_zero_rate_and_sone_ia_is_refused() {
    let Some(pool) = pool().await else { return };
    let company = company(&pool).await;
    let a = employee(&pool, company, "26829398612", "Finnmarking", 4_000_000).await;

    let run = payroll::run_payroll(
        &pool,
        company,
        2026,
        4,
        date(2026, 4, 25),
        "V",
        &[PayrollEntry {
            employee_id: a,
            gross_ore: None,
            holiday_pay_ore: 0,
            holiday_pay_supplement_ore: None,
            from_timesheet: false,
            holiday_pay_exempt: false,
        }],
        "Test",
    )
    .await
    .unwrap();
    assert_eq!(run.sum.employer_contribution_ore, 0, "sone V er nullsats");
    assert_eq!(voucher_sum(&pool, run.voucher_id).await, 0);

    // Sone Ia is refused: the fribeløp cannot be read out of a single rate.
    let error = payroll::run_payroll(
        &pool,
        company,
        2026,
        5,
        date(2026, 5, 25),
        "Ia",
        &[PayrollEntry {
            employee_id: a,
            gross_ore: None,
            holiday_pay_ore: 0,
            holiday_pay_supplement_ore: None,
            from_timesheet: false,
            holiday_pay_exempt: false,
        }],
        "Test",
    )
    .await
    .unwrap_err();
    assert!(error.to_string().contains("fribeløpet"), "{error}");
}

/// Tabelltrekk follows Skatteetaten's own routine and lands in the run
/// and the bilag, for 10,5- and 12-month tables alike.
#[tokio::test]
async fn tabelltrekk_follows_the_table_for_10_5_and_12_month_tables() {
    let Some(pool) = pool().await else { return };
    let company = company(&pool).await;
    let employee = |nin: &'static str, table: i32| NewEmployee {
        nin: nin.into(),
        name: format!("Tabell {table}"),
        stilling: None,
        employed_from: date(2025, 1, 1),
        monthly_salary_ore: Some(5_000_000),
        hourly_wage_ore: None,
        withholding_type: "tabell".into(),
        withholding_percent_bp: None,
        withholding_table: Some(table),
        holiday_pay_bp: 1020,
        bank_account: None,
        note: None,
        details: Default::default(),
    };
    let run = |id| PayrollEntry {
        employee_id: id,
        gross_ore: None,
        holiday_pay_ore: 0,
        holiday_pay_supplement_ore: None,
        from_timesheet: false,
        holiday_pay_exempt: false,
    };
    let ordinary = payroll::create_employee(&pool, company, &employee("08888797336", 8000), "Test")
        .await
        .unwrap();
    let twelve = payroll::create_employee(&pool, company, &employee("15929998666", 7350), "Test")
        .await
        .unwrap();

    let expected_kr = regnmed_core::withholding_table::withholding_kr(
        2026,
        8000,
        regnmed_core::withholding_table::Period::Month,
        50_000,
    )
    .unwrap();
    let posted = payroll::run_payroll(
        &pool,
        company,
        2026,
        3,
        date(2026, 3, 25),
        "I",
        &[run(ordinary)],
        "Test",
    )
    .await
    .unwrap();
    assert_eq!(posted.sum.withholding_ore, expected_kr * 100);
    assert!(
        expected_kr > 10_000 && expected_kr < 20_000,
        "{expected_kr}"
    );

    // A 12-month table is computed too -- the table routine handles both.
    let april = payroll::run_payroll(
        &pool,
        company,
        2026,
        4,
        date(2026, 4, 25),
        "I",
        &[run(twelve)],
        "Test",
    )
    .await
    .unwrap();
    let expected_twelve = regnmed_core::withholding_table::withholding_kr(
        2026,
        7350,
        regnmed_core::withholding_table::Period::Month,
        50_000,
    )
    .unwrap();
    assert_eq!(april.sum.withholding_ore, expected_twelve * 100);

    // December's pay paid in January is taxed in January's year, whose
    // tables are not in regnmed yet: refused, never last year's.
    let error = payroll::run_payroll(
        &pool,
        company,
        2026,
        12,
        date(2027, 1, 5),
        "I",
        &[run(ordinary)],
        "Test",
    )
    .await
    .unwrap_err();
    assert!(error.to_string().contains("2027"), "{error}");
}

#[tokio::test]
async fn the_same_month_cannot_be_run_twice_and_runs_are_immutable() {
    let Some(pool) = pool().await else { return };
    let company = company(&pool).await;
    let a = employee(&pool, company, "26829398612", "Kari", 4_000_000).await;
    let post = || {
        vec![PayrollEntry {
            employee_id: a,
            gross_ore: None,
            holiday_pay_ore: 0,
            holiday_pay_supplement_ore: None,
            from_timesheet: false,
            holiday_pay_exempt: false,
        }]
    };

    let first = payroll::run_payroll(
        &pool,
        company,
        2026,
        7,
        date(2026, 7, 25),
        "I",
        &post(),
        "Test",
    )
    .await
    .unwrap();
    assert!(payroll::kjort_maned(&pool, company, 2026, 7).await.unwrap());

    let error = payroll::run_payroll(
        &pool,
        company,
        2026,
        7,
        date(2026, 7, 25),
        "I",
        &post(),
        "Test",
    )
    .await
    .unwrap_err();
    assert!(error.to_string().contains("allerede kjørt"), "{error}");

    // The database itself refuses to change or delete a run.
    let err = sqlx::query("update payroll_run set brutto_ore = 1 where id = $1")
        .bind(first.id)
        .execute(&pool)
        .await
        .unwrap_err();
    assert!(err.to_string().contains("innsettings-bare"), "{err}");

    let err = sqlx::query("delete from payroll_line where run_id = $1")
        .bind(first.id)
        .execute(&pool)
        .await
        .unwrap_err();
    assert!(err.to_string().contains("innsettings-bare"), "{err}");

    // The employee's identity is not editable either.
    let err = sqlx::query("update employee set fodselsnummer = '08888797336' where id = $1")
        .bind(a)
        .execute(&pool)
        .await
        .unwrap_err();
    assert!(err.to_string().contains("uforanderlig"), "{err}");
}

/// The list shows the birth date, not the fødselsnummer — the same
/// privacy choice as in the aksjeeierbok.
#[tokio::test]
async fn the_employee_list_shows_the_birth_date_not_the_nin() {
    let Some(pool) = pool().await else { return };
    let company = company(&pool).await;
    employee(&pool, company, "26829398612", "Kari Utvikler", 4_000_000).await;

    let employees = payroll::list_ansatte(&pool, company).await.unwrap();
    assert_eq!(employees.len(), 1);
    assert_eq!(employees[0].birth_date, Some(date(1993, 2, 26)));
    assert!(
        !format!("{:?}", employees[0]).contains("26829398612"),
        "fødselsnummeret skal ikke ligge i ansattlisten"
    );
}

#[tokio::test]
async fn an_invalid_nin_is_rejected_at_registration() {
    let Some(pool) = pool().await else { return };
    let company = company(&pool).await;
    let error = payroll::create_employee(
        &pool,
        company,
        &NewEmployee {
            nin: "26829398613".into(),
            name: "Feil Nummer".into(),
            stilling: None,
            employed_from: date(2025, 1, 1),
            monthly_salary_ore: Some(1),
            hourly_wage_ore: None,
            withholding_type: "ingen".into(),
            withholding_percent_bp: None,
            withholding_table: None,
            holiday_pay_bp: 1020,
            bank_account: None,
            note: None,
            details: Default::default(),
        },
        "Test",
    )
    .await
    .unwrap_err();
    assert!(error.to_string().contains("fødselsnummer"), "{error}");
}

/// The payslip is built from the insert-only payroll line, so it can be
/// reproduced forever — and it must explain the withholding, not merely
/// state it.
#[tokio::test]
async fn the_payslip_is_built_from_the_line_with_year_to_date() {
    let Some(pool) = pool().await else { return };
    let company = company(&pool).await;
    let a = employee(&pool, company, "26829398612", "Kari Utvikler", 5_000_000).await;
    let post = |fp: i64| {
        vec![PayrollEntry {
            employee_id: a,
            gross_ore: None,
            holiday_pay_ore: fp,
            holiday_pay_supplement_ore: None,
            from_timesheet: false,
            holiday_pay_exempt: false,
        }]
    };

    payroll::run_payroll(
        &pool,
        company,
        2026,
        5,
        date(2026, 5, 25),
        "I",
        &post(0),
        "Test",
    )
    .await
    .unwrap();
    let june = payroll::run_payroll(
        &pool,
        company,
        2026,
        6,
        date(2026, 6, 20),
        "I",
        &post(4_000_000),
        "Test",
    )
    .await
    .unwrap();

    let slip = payroll::payslip(&pool, company, june.id, a).await.unwrap();
    assert_eq!(slip.employee_name, "Kari Utvikler");
    // Birth date, not fødselsnummer — on the payslip too.
    assert_eq!(slip.employee_birth_date, Some(date(1993, 2, 26)));
    // Brutto on the slip is EVERYTHING paid out; the trekkgrunnlag is less.
    assert_eq!(slip.gross_ore, 9_000_000);
    assert_eq!(slip.withholding_basis_ore, 5_000_000);
    assert_eq!(slip.withholding_ore, 1_750_000);
    assert_eq!(slip.net_ore, 7_250_000);
    assert_eq!(slip.withholding_method.as_deref(), Some("35,00 %"));
    // Year to date through June: two months of pay + June's feriepenger.
    assert_eq!(slip.year_to_date_gross_ore, 5_000_000 + 9_000_000);
    assert_eq!(slip.year_to_date_withholding_ore, 3_500_000);
    assert_eq!(slip.ytd_holiday_pay_ore, 1_020_000);

    // And it renders to a PDF that explains the trekkfrihet.
    let pdf = regnmed_core::payslip::render_payslip(&slip);
    assert!(pdf.starts_with(b"%PDF-1.4"));
    let text = String::from_utf8_lossy(&pdf).to_string();
    assert!(text.contains("uten forskuddstrekk"), "{text}");
    assert!(!text.contains("26829398612"), "fnr skal ikke i slippen");

    // The May slip sees only May in the year-to-date figures.
    let mai_id = payroll::list_kjoringer(&pool, company, Some(2026))
        .await
        .unwrap()
        .into_iter()
        .find(|k| k.month == 5)
        .unwrap();
    assert_eq!(mai_id.employees.len(), 1, "listingen kjenner deltakerne");
    let mai = payroll::payslip(&pool, company, mai_id.id, a)
        .await
        .unwrap();
    assert_eq!(mai.year_to_date_gross_ore, 5_000_000);
}

/// December's pay paid in January belongs to the new year on the slip,
/// and still comes before January's pay even on the same payment date.
#[tokio::test]
async fn december_pay_paid_in_january_counts_in_the_new_year_to_date() {
    let Some(pool) = pool().await else { return };
    let company = company(&pool).await;
    let a = employee(&pool, company, "26829398612", "Kari Utvikler", 5_000_000).await;
    let post = [PayrollEntry {
        employee_id: a,
        gross_ore: None,
        holiday_pay_ore: 0,
        holiday_pay_supplement_ore: None,
        from_timesheet: false,
        holiday_pay_exempt: false,
    }];
    let mut slips = Vec::new();
    for (year, month, paid) in [
        (2026, 11, date(2026, 11, 25)),
        (2026, 12, date(2027, 1, 15)),
        (2027, 1, date(2027, 1, 15)),
        (2027, 2, date(2027, 3, 10)),
        (2027, 3, date(2027, 3, 1)),
    ] {
        let run = payroll::run_payroll(&pool, company, year, month, paid, "I", &post, "Test")
            .await
            .unwrap();
        slips.push(run.id);
    }
    let november = payroll::payslip(&pool, company, slips[0], a).await.unwrap();
    let december = payroll::payslip(&pool, company, slips[1], a).await.unwrap();
    let january = payroll::payslip(&pool, company, slips[2], a).await.unwrap();

    assert_eq!(november.year_to_date_gross_ore, 5_000_000);

    // December opens 2027: November stays behind in 2026.
    assert_eq!(december.year, 2026, "the pay period is still December 2026");
    assert_eq!(december.year_to_date_gross_ore, 5_000_000);
    assert_eq!(
        december.year_to_date_withholding_ore,
        december.withholding_ore
    );
    assert_eq!(december.ytd_holiday_pay_ore, 510_000);
    let pdf = regnmed_core::payslip::render_payslip(&december);
    let text = String::from_utf8_lossy(&pdf);
    assert!(text.contains("HITTIL I 2027"), "{text}");

    // January carries December, whose month number is the higher one.
    assert_eq!(january.year_to_date_gross_ore, 10_000_000);
    assert_eq!(
        january.year_to_date_withholding_ore,
        december.withholding_ore + january.withholding_ore
    );
    assert_eq!(january.ytd_holiday_pay_ore, 1_020_000);

    // Payment order decides: March was paid before February.
    let february = payroll::payslip(&pool, company, slips[3], a).await.unwrap();
    let march = payroll::payslip(&pool, company, slips[4], a).await.unwrap();
    assert_eq!(march.year_to_date_gross_ore, 15_000_000);
    assert_eq!(february.year_to_date_gross_ore, 20_000_000);
}

/// Hourly pay from the timesheet. The important part here is the
/// REFUSAL: hours that can still be changed must not be payable, because
/// the payroll run is insert-only and the two would then diverge
/// forever.
#[tokio::test]
async fn hourly_pay_requires_the_month_to_be_locked() {
    let Some(pool) = pool().await else { return };
    let company = company(&pool).await;

    let person = regnmed_db::ensure_person(
        &pool,
        &format!("test|{}", Uuid::new_v4()),
        Some("Timelønnet"),
        None,
    )
    .await
    .unwrap();
    let a = payroll::create_employee(
        &pool,
        company,
        &NewEmployee {
            nin: "25927898821".into(),
            name: "Timelønnet".into(),
            stilling: None,
            employed_from: date(2025, 1, 1),
            monthly_salary_ore: None,
            hourly_wage_ore: Some(45_000), // 450 kr/t
            withholding_type: "prosent".into(),
            withholding_percent_bp: Some(3000),
            withholding_table: None,
            holiday_pay_bp: 1020,
            bank_account: None,
            note: None,
            details: Default::default(),
        },
        "Test",
    )
    .await
    .unwrap();
    sqlx::query("update employee set person_id = $2 where id = $1")
        .bind(a)
        .bind(person)
        .execute(&pool)
        .await
        .unwrap();

    // 20 hours in March.
    for dag in [2u32, 3, 4] {
        regnmed_db::timesheet::create_time_entry(
            &pool,
            company,
            person,
            &regnmed_db::timesheet::TimeEntryDraft {
                date: date(2026, 3, dag),
                minutes: 400,
                description: "Arbeid".into(),
                project: None,
                billable: Some(false),
                hourly_rate_ore: None,
            },
            false,
            "Test",
        )
        .await
        .unwrap();
    }

    let g = payroll::hour_basis(&pool, company, a, 2026, 3)
        .await
        .unwrap();
    assert_eq!(g.minutes, 1200, "20 timer");
    assert_eq!(g.amount_ore, 900_000, "20 t x 450 kr = 9 000 kr");
    assert!(!g.locked, "ikke låst ennå");

    let post = || {
        vec![PayrollEntry {
            employee_id: a,
            gross_ore: None,
            holiday_pay_ore: 0,
            holiday_pay_supplement_ore: None,
            from_timesheet: true,
            holiday_pay_exempt: false,
        }]
    };

    // Unlocked: refused.
    let error = payroll::run_payroll(
        &pool,
        company,
        2026,
        3,
        date(2026, 3, 25),
        "I",
        &post(),
        "Test",
    )
    .await
    .unwrap_err();
    assert!(error.to_string().contains("ikke låst"), "{error}");

    // Lock the month, and the run goes through.
    regnmed_db::timesheet::set_timesheet_lock(&pool, company, date(2026, 3, 31), "Test", None)
        .await
        .unwrap();
    let g = payroll::hour_basis(&pool, company, a, 2026, 3)
        .await
        .unwrap();
    assert!(g.locked);

    let run = payroll::run_payroll(
        &pool,
        company,
        2026,
        3,
        date(2026, 3, 25),
        "I",
        &post(),
        "Test",
    )
    .await
    .unwrap();
    assert_eq!(run.sum.gross_ore, 900_000, "timene, ikke månedslønn");
    assert_eq!(run.sum.withholding_ore, 270_000, "30 % av 9 000 kr");
    assert_eq!(voucher_sum(&pool, run.voucher_id).await, 0);
}

/// Without a link to a portal user the timesheet does not know who the
/// employee is — and then we say so, instead of paying zero.
#[tokio::test]
async fn an_employee_without_a_portal_user_fails_clearly() {
    let Some(pool) = pool().await else { return };
    let company = company(&pool).await;
    let a = employee(&pool, company, "26829398612", "Uten kobling", 4_000_000).await;

    let error = payroll::hour_basis(&pool, company, a, 2026, 3)
        .await
        .unwrap_err();
    assert!(error.to_string().contains("portalbruker"), "{error}");
}

// ---------------------------------------------------------------------
// Arbeidsgiveravgift on feriepenger that are accrued but not paid out.
//
// The avgift falls due only when the feriepenger are paid, but the
// obligation arises with the earning. The model is a TARGET, not a stream
// of increments: after every run, account 2780 must be the rate times
// what is actually owed, and the run books the difference.
// ---------------------------------------------------------------------

/// The balance of an account for the WHOLE company, not just one bilag.
async fn account_balance(pool: &PgPool, company: Uuid, account: &str) -> i64 {
    sqlx::query_scalar(
        "select coalesce(sum(e.amount_ore), 0)::bigint from entry e
         join account a on a.id = e.account_id
         where a.company_id = $1 and a.number = $2",
    )
    .bind(company)
    .bind(account)
    .fetch_one(pool)
    .await
    .unwrap()
}

async fn run(
    pool: &PgPool,
    company: Uuid,
    a: Uuid,
    month: u32,
    zone: &str,
    gross: Option<i64>,
    holiday_pay: i64,
) -> payroll::PayrollRun {
    payroll::run_payroll(
        pool,
        company,
        2026,
        month,
        date(2026, month, 20),
        zone,
        &[PayrollEntry {
            employee_id: a,
            gross_ore: gross,
            holiday_pay_ore: holiday_pay,
            holiday_pay_supplement_ore: None,
            from_timesheet: false,
            holiday_pay_exempt: false,
        }],
        "Test",
    )
    .await
    .unwrap()
}

#[tokio::test]
async fn the_accrual_on_unpaid_holiday_pay_is_posted() {
    let Some(pool) = pool().await else { return };
    let company = company(&pool).await;
    let a = employee(&pool, company, "26829398612", "Kari Avsetning", 5_000_000).await;

    let k = run(&pool, company, a, 3, "I", None, 0).await;

    // 10,2 % of 50 000 = 5 100 kr of feriepenger owed; 14,1 % of that is
    // 719,10 kr of avgift accruing now and falling due on payout.
    assert_eq!(k.sum.holiday_pay_accrual_ore, 510_000);
    assert_eq!(k.sum.employer_contribution_holiday_pay_ore, 71_910);
    assert_eq!(voucher_sum(&pool, k.voucher_id).await, 0);
    assert_eq!(account_amount(&pool, k.voucher_id, "5405").await, 71_910);
    assert_eq!(account_amount(&pool, k.voucher_id, "2780").await, -71_910);
    assert!(k.warnings.is_empty(), "{:?}", k.warnings);
}

/// The life cycle: the avgift is accrued on earning and reversed on
/// payout — because then the ordinary aga line is what carries it. Get
/// this wrong and the avgift is either expensed twice or left standing as
/// a liability that never goes away.
#[tokio::test]
async fn the_accrual_is_reversed_when_the_holiday_pay_is_paid_out() {
    let Some(pool) = pool().await else { return };
    let company = company(&pool).await;
    let a = employee(&pool, company, "08888797336", "Ola Feriepenger", 5_000_000).await;

    let opptjening = run(&pool, company, a, 3, "I", None, 0).await;
    assert_eq!(opptjening.sum.employer_contribution_holiday_pay_ore, 71_910);

    // Holiday taken: no ordinary pay, the feriepenger are paid out.
    let utbetaling = run(&pool, company, a, 4, "I", Some(0), 510_000).await;

    assert_eq!(
        utbetaling.sum.employer_contribution_holiday_pay_ore, -71_910,
        "avsetningen føres tilbake i sin helhet"
    );
    assert_eq!(voucher_sum(&pool, utbetaling.voucher_id).await, 0);
    // The avgift on what was paid out now sits in the ordinary aga line,
    // as a claim in whole kroner: 5 100 kr × 14,1 % = 719,10 -> 719. The
    // accrual is a periodisering and keeps its øre.
    assert_eq!(utbetaling.sum.employer_contribution_ore, 71_900);

    // And afterwards both accounts stand at zero: no liability left, no
    // accrual left.
    assert_eq!(account_balance(&pool, company, "2940").await, 0);
    assert_eq!(account_balance(&pool, company, "2780").await, 0);
}

/// A feriepenger liability that carries no accrual — because it was
/// earned before the function existed, or in a zone without avgift — is
/// caught up on the next run. That is the whole point of aiming at a
/// balance instead of adding an amount.
#[tokio::test]
async fn a_liability_without_an_accrual_is_caught_up_on_the_next_run() {
    let Some(pool) = pool().await else { return };
    let company = company(&pool).await;
    let a = employee(&pool, company, "25927898821", "Nils Sonebytte", 5_000_000).await;

    // Sone V is a zero rate: feriepenger are earned, no avgift is accrued.
    let without = run(&pool, company, a, 3, "V", None, 0).await;
    assert_eq!(without.sum.employer_contribution_holiday_pay_ore, 0);
    assert_eq!(account_balance(&pool, company, "2780").await, 0);

    // The business moves to sone I. Now it owes avgift on EVERYTHING
    // outstanding, not only on this month's earning.
    let med = run(&pool, company, a, 4, "I", None, 0).await;
    let owed = 510_000 + 510_000;
    assert_eq!(
        med.sum.employer_contribution_holiday_pay_ore,
        owed * 1410 / 10_000,
        "hele gjelden får avsetning, ikke bare den nye måneden"
    );
    assert_eq!(account_balance(&pool, company, "2780").await, -143_820);
}

/// The invariant that keeps the accrual from drifting: after any run, the
/// balance of 2780 is exactly the rate times the balance of 2940.
#[tokio::test]
async fn the_accrual_is_always_the_rate_times_the_holiday_pay_liability() {
    let Some(pool) = pool().await else { return };
    let company = company(&pool).await;
    let a = employee(&pool, company, "03048810003", "Turid Invariant", 4_321_000).await;
    let b = employee(&pool, company, "03048810194", "Per Invariant", 2_777_700).await;

    for (month, holiday_pay) in [(3u32, 0i64), (4, 0), (5, 130_000), (6, 250_000)] {
        payroll::run_payroll(
            &pool,
            company,
            2026,
            month,
            date(2026, month, 20),
            "I",
            &[
                PayrollEntry {
                    employee_id: a,
                    gross_ore: None,
                    holiday_pay_ore: holiday_pay,
                    holiday_pay_supplement_ore: None,
                    from_timesheet: false,
                    holiday_pay_exempt: false,
                },
                PayrollEntry {
                    employee_id: b,
                    gross_ore: None,
                    holiday_pay_ore: holiday_pay,
                    holiday_pay_supplement_ore: None,
                    from_timesheet: false,
                    holiday_pay_exempt: false,
                },
            ],
            "Test",
        )
        .await
        .unwrap();

        // The rounding happens PER EMPLOYEE, so the expected value must
        // be built per employee — 14,1 % of the total would be off by an
        // øre or two and turn the test into an approximation rather than
        // an invariant.
        let per_employee: Vec<i64> = sqlx::query_scalar(
            "select coalesce(sum(l.feriepengeavsetning_ore - l.feriepenger_ore), 0)::bigint
             from payroll_line l join payroll_run r on r.id = l.run_id
             where r.company_id = $1 group by l.employee_id",
        )
        .bind(company)
        .fetch_all(&pool)
        .await
        .unwrap();
        let ventet: i64 = per_employee
            .iter()
            .map(|s| regnmed_core::payroll::employer_contribution_accrual_target(*s, 1410))
            .sum();

        assert_eq!(
            per_employee.iter().sum::<i64>(),
            -account_balance(&pool, company, "2940").await,
            "etter {month:02}/2026: lønnshistorikken skal forklare hele 2940"
        );
        assert_eq!(
            -account_balance(&pool, company, "2780").await,
            ventet,
            "etter {month:02}/2026: 2780 skal være 14,1 % av hver ansatts gjeld"
        );
    }
}

/// A feriepenger liability that does not come from the payroll runs — an
/// opening balance, a manual accrual — cannot be tied to any employee,
/// and therefore gets no avgift accrual. That is a real limitation, and
/// the run says so instead of pretending otherwise.
#[tokio::test]
async fn an_unallocated_holiday_pay_liability_warns_rather_than_staying_silent() {
    let Some(pool) = pool().await else { return };
    let company = company(&pool).await;
    let a = employee(&pool, company, "03048810275", "Åse Overtatt", 5_000_000).await;

    // The regnskapsfører accrues feriepenger manually, with no employee link.
    use regnmed_core::Ore;
    use regnmed_core::voucher::{EntryDraft, VoucherDraft};
    let line = |account: &str, amount: i64| EntryDraft {
        account_number: account.into(),
        amount: Ore(amount),
        vat_code: None,
        description: Some("Overtatt feriepengegjeld".into()),
        party_no: None,
        department: None,
        project: None,
        currency: None,
    };
    regnmed_db::post_voucher(
        &pool,
        company,
        &VoucherDraft {
            journal_code: "GL".into(),
            voucher_date: date(2026, 1, 1),
            description: "Åpningsbalanse feriepenger".into(),
            reverses: None,
            entries: vec![line("5090", 1_000_000), line("2940", -1_000_000)],
        },
        "Test",
    )
    .await
    .unwrap();

    let k = run(&pool, company, a, 3, "I", None, 0).await;

    // The accrual covers only what the payroll run itself earned.
    assert_eq!(k.sum.employer_contribution_holiday_pay_ore, 71_910);
    let advarsel = k.warnings.join(" ");
    assert!(
        advarsel.contains("10000,00"),
        "differansen skal navngis: {advarsel}"
    );
    assert!(advarsel.contains("2780"), "{advarsel}");
}

fn complete_details(business_unit: &str) -> payroll::EmploymentDetails {
    payroll::EmploymentDetails {
        business_unit_orgnumber: Some(business_unit.into()),
        occupation_code: Some("2512101".into()),
        position_percent_bp: Some(10000),
        full_time_hours_per_week_hundredths: Some(3750),
        working_hours_scheme: Some("ikkeSkift".into()),
        employment_form: Some("fast".into()),
        last_salary_change: Some(date(2025, 1, 1)),
    }
}

/// The arbeidsforhold facts (0073) start missing, are listed by the
/// a-melding's own names, and disappear from the list once set.
#[tokio::test]
async fn employment_details_close_the_amelding_gaps() {
    let Some(pool) = pool().await else { return };
    let company = company(&pool).await;
    let id = employee(&pool, company, "26829398612", "Kari Utvikler", 4_000_000).await;

    let before = &payroll::list_ansatte(&pool, company).await.unwrap()[0];
    assert_eq!(
        before.amelding_missing(),
        vec![
            "virksomhet",
            "antall timer per uke for full stilling",
            "yrkeskode",
            "arbeidstidsordning",
            "stillingsprosent",
            "siste lønnsendringsdato",
            "ansettelsesform",
        ]
    );

    // 974760673 is a valid org number and not the test company's.
    let details = complete_details("974760673");
    payroll::update_employment_details(&pool, company, id, &details)
        .await
        .unwrap();
    let after = &payroll::list_ansatte(&pool, company).await.unwrap()[0];
    assert_eq!(after.details, details);
    assert!(after.amelding_missing().is_empty());

    // A full replacement: clearing a field brings it back on the list.
    let cleared = payroll::EmploymentDetails {
        occupation_code: None,
        ..details
    };
    payroll::update_employment_details(&pool, company, id, &cleared)
        .await
        .unwrap();
    let again = &payroll::list_ansatte(&pool, company).await.unwrap()[0];
    assert_eq!(again.amelding_missing(), vec!["yrkeskode"]);
}

#[tokio::test]
async fn employment_details_are_validated_before_storage() {
    let Some(pool) = pool().await else { return };
    // A MOD11-valid org number for the company itself, or the "own
    // number" case below would be refused by MOD11 and prove nothing.
    let own = valid_orgnumber();
    let company = regnmed_db::create_company(&pool, &own, "Lønnstest AS")
        .await
        .unwrap();
    let id = employee(&pool, company, "26829398612", "Kari Utvikler", 4_000_000).await;

    let refused = [
        ("selskapets eget orgnr", complete_details(&own)),
        ("MOD11", complete_details("974760674")),
        (
            "yrkeskode",
            payroll::EmploymentDetails {
                occupation_code: Some("2512".into()),
                ..complete_details("974760673")
            },
        ),
        (
            "kodeliste",
            payroll::EmploymentDetails {
                working_hours_scheme: Some("turnus".into()),
                ..complete_details("974760673")
            },
        ),
        (
            "kodeliste",
            payroll::EmploymentDetails {
                employment_form: Some("vikar".into()),
                ..complete_details("974760673")
            },
        ),
        (
            "over 100 %",
            payroll::EmploymentDetails {
                position_percent_bp: Some(10001),
                ..complete_details("974760673")
            },
        ),
    ];
    for (why, details) in refused {
        assert!(
            payroll::update_employment_details(&pool, company, id, &details)
                .await
                .is_err(),
            "{why} skulle vært avvist"
        );
    }
    let stored = &payroll::list_ansatte(&pool, company).await.unwrap()[0];
    assert_eq!(stored.details, payroll::EmploymentDetails::default());
}

/// Fastlønn and timelønn are reported under different descriptions, so
/// the run records which it paid — where it is known, never inferred.
#[tokio::test]
async fn a_run_records_whether_it_paid_fixed_salary() {
    let Some(pool) = pool().await else { return };
    let company = company(&pool).await;
    let id = employee(&pool, company, "26829398612", "Kari Utvikler", 4_000_000).await;
    let run = payroll::run_payroll(
        &pool,
        company,
        2026,
        3,
        date(2026, 3, 25),
        "I",
        &[payroll::PayrollEntry {
            employee_id: id,
            gross_ore: None,
            holiday_pay_ore: 0,
            holiday_pay_supplement_ore: None,
            from_timesheet: false,
            holiday_pay_exempt: false,
        }],
        "Test",
    )
    .await
    .unwrap();
    let kind: Option<String> =
        sqlx::query_scalar("select pay_kind from payroll_line where run_id = $1")
            .bind(run.id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(kind.as_deref(), Some("fixed"));
}

fn run_entry(employee_id: Uuid, gross_ore: Option<i64>) -> PayrollEntry {
    PayrollEntry {
        employee_id,
        gross_ore,
        holiday_pay_ore: 0,
        holiday_pay_supplement_ore: None,
        from_timesheet: false,
        holiday_pay_exempt: false,
    }
}

/// A stored run with complete arbeidsforhold renders to an a-melding the
/// renderer accepts — fødselsnummer, fastlønn and the run's own avgift.
#[tokio::test]
async fn a_stored_run_with_complete_employments_renders() {
    let Some(pool) = pool().await else { return };
    let company = company(&pool).await;
    // 40 000 kr at 35 % trekk and 14,1 % aga: whole kroner throughout
    // (14 000 trekk, 5 640 aga), so the kroner rule does not interfere.
    let id = employee(&pool, company, "26829398612", "Kari Utvikler", 4_000_000).await;
    payroll::update_employment_details(&pool, company, id, &complete_details("974760673"))
        .await
        .unwrap();
    let run = payroll::run_payroll(
        &pool,
        company,
        2026,
        3,
        date(2026, 3, 25),
        "I",
        &[run_entry(id, None)],
        "Test",
    )
    .await
    .unwrap();

    let draft = payroll::amelding_for_run(&pool, company, run.id, chrono::Utc::now())
        .await
        .unwrap();
    assert!(draft.hindrances.is_empty(), "{:?}", draft.hindrances);
    let xml = regnmed_core::amelding::render(&draft.melding).unwrap();
    assert!(xml.contains("<norskIdentifikator>26829398612</norskIdentifikator>"));
    assert!(xml.contains("<beskrivelse>fastloenn</beskrivelse>"));
    assert!(xml.contains("<sumArbeidsgiveravgift>5640</sumArbeidsgiveravgift>"));
    assert!(xml.contains(&format!("<meldingsId>{}</meldingsId>", run.id)));
}

/// Salaries in øre give an aga with øre, and the run still reports: the
/// avgift is computed ONCE on the summed basis and rounded down to whole
/// kroner, so it is what the a-melding reports and F185 recomputes.
#[tokio::test]
async fn a_run_with_ore_in_the_avgift_books_whole_kroner_and_renders() {
    let Some(pool) = pool().await else { return };
    let company = company(&pool).await;
    let a = employee(&pool, company, "26829398612", "Kari Utvikler", 4_100_101).await;
    let b = employee(&pool, company, "03048810003", "Ola Kroner", 3_333_333).await;
    for id in [a, b] {
        payroll::update_employment_details(&pool, company, id, &complete_details("974760673"))
            .await
            .unwrap();
    }
    let run = payroll::run_payroll(
        &pool,
        company,
        2026,
        3,
        date(2026, 3, 25),
        "I",
        &[run_entry(a, None), run_entry(b, None)],
        "Test",
    )
    .await
    .unwrap();

    // 74 334,34 kr × 14,1 % = 10 481,14 -> 10 481. Rounded per employee
    // (5 781 + 4 699) it would be 10 480, and F185 would refuse it.
    assert_eq!(run.sum.employer_contribution_ore, 1_048_100);
    assert_eq!(account_balance(&pool, company, "2770").await, -1_048_100);
    let draft = payroll::amelding_for_run(&pool, company, run.id, chrono::Utc::now())
        .await
        .unwrap();
    assert!(draft.hindrances.is_empty(), "{:?}", draft.hindrances);
    let xml = regnmed_core::amelding::render(&draft.melding).unwrap();
    assert!(xml.contains("<sumArbeidsgiveravgift>10481</sumArbeidsgiveravgift>"));
}

/// F108: everyone employed in the melding's kalendermåned is reported,
/// paid or not, until the month the employment ends. The month is the
/// PAYMENT date's -- a March run paid in April is April's melding.
#[tokio::test]
async fn employments_without_pay_are_reported_until_they_end() {
    let Some(pool) = pool().await else { return };
    let company = company(&pool).await;
    let paid = employee(&pool, company, "26829398612", "Kari Betalt", 4_000_000).await;
    let leave = employee(&pool, company, "03048810003", "Per Permisjon", 3_000_000).await;
    let left = employee(&pool, company, "15076500565", "Siv Sluttet", 3_000_000).await;
    let starting = employee(&pool, company, "07045020090", "Nils Ny", 3_000_000).await;
    for id in [paid, leave, left, starting] {
        payroll::update_employment_details(&pool, company, id, &complete_details("974760673"))
            .await
            .unwrap();
    }
    // Siv left in March, Nils starts in April.
    payroll::end_employment(&pool, company, left, Some(date(2026, 3, 31)))
        .await
        .unwrap();
    sqlx::query("update employee set ansatt_fra = $2 where id = $1")
        .bind(starting)
        .bind(date(2026, 4, 1))
        .execute(&pool)
        .await
        .unwrap();

    // The March run, paid on 2 April: April's melding.
    let run = payroll::run_payroll(
        &pool,
        company,
        2026,
        3,
        date(2026, 4, 2),
        "I",
        &[run_entry(paid, None)],
        "Test",
    )
    .await
    .unwrap();
    let draft = payroll::amelding_for_run(&pool, company, run.id, chrono::Utc::now())
        .await
        .unwrap();
    assert!(draft.hindrances.is_empty(), "{:?}", draft.hindrances);
    let names: Vec<&str> = draft
        .melding
        .recipients
        .iter()
        .map(|r| r.name.as_str())
        .collect();
    // The paid first, then the unpaid in the register's (name) order.
    assert_eq!(
        names,
        ["Kari Betalt", "Nils Ny", "Per Permisjon"],
        "{names:?}"
    );
    let unpaid = &draft.melding.recipients[2];
    assert!(unpaid.income.is_empty() && unpaid.withholding_ore == 0);
    let xml = regnmed_core::amelding::render(&draft.melding).unwrap();
    assert!(xml.contains("<norskIdentifikator>03048810003</norskIdentifikator>"));
    assert!(!xml.contains("15076500565"), "Siv sluttet i mars");
    // The unpaid add nothing to the avgift: 40 000 kr × 14,1 %.
    assert!(xml.contains("<sumArbeidsgiveravgift>5640</sumArbeidsgiveravgift>"));
}

/// An end date entered LATE: Siv left in January, February's delivered
/// melding still reported her without a sluttdato, and only then was the
/// end date set. March must report her again WITH it (F108), and April no
/// longer. Ola, whose end was known before February, stays out.
#[tokio::test]
async fn an_end_date_entered_late_is_reported_the_next_month() {
    use regnmed_db::amelding_submission as sub;
    let Some(pool) = pool().await else { return };
    let company = company(&pool).await;
    let kari = employee(&pool, company, "26829398612", "Kari Betalt", 4_000_000).await;
    let siv = employee(&pool, company, "15076500565", "Siv Sluttet", 3_000_000).await;
    let ola = employee(&pool, company, "03048810003", "Ola Tidlig", 3_000_000).await;
    for id in [kari, siv, ola] {
        payroll::update_employment_details(&pool, company, id, &complete_details("974760673"))
            .await
            .unwrap();
    }
    payroll::end_employment(&pool, company, ola, Some(date(2026, 1, 31)))
        .await
        .unwrap();
    let run_in = |month: u32| {
        let pool = pool.clone();
        async move {
            payroll::run_payroll(
                &pool,
                company,
                2026,
                month,
                date(2026, month, 25),
                "I",
                &[run_entry(kari, None)],
                "Test",
            )
            .await
            .unwrap()
            .id
        }
    };
    let deliver = |run: Uuid| {
        let pool = pool.clone();
        async move {
            let s = sub::prepare_submission(&pool, company, sub::Source::Run(run), false, "Test")
                .await
                .unwrap();
            sub::record_receipt(&pool, s.id, "d", "f", "Test")
                .await
                .unwrap();
        }
    };
    let names = |run: Uuid| {
        let pool = pool.clone();
        async move {
            let draft = payroll::amelding_for_run(&pool, company, run, chrono::Utc::now())
                .await
                .unwrap();
            assert!(draft.hindrances.is_empty(), "{:?}", draft.hindrances);
            draft
                .melding
                .recipients
                .iter()
                .map(|r| (r.name.clone(), r.employment.end_date))
                .collect::<Vec<_>>()
        }
    };

    let february = run_in(2).await;
    assert_eq!(
        names(february).await,
        [("Kari Betalt".into(), None), ("Siv Sluttet".into(), None)]
    );
    deliver(february).await;
    // Only now does anyone record that Siv left in January.
    payroll::end_employment(&pool, company, siv, Some(date(2026, 1, 31)))
        .await
        .unwrap();

    let march = run_in(3).await;
    assert_eq!(
        names(march).await,
        [
            ("Kari Betalt".into(), None),
            ("Siv Sluttet".into(), Some(date(2026, 1, 31)))
        ]
    );
    deliver(march).await;

    let april = run_in(4).await;
    assert_eq!(names(april).await, [("Kari Betalt".into(), None)]);
}

/// F108 in a month nobody was paid: the melding carries every employment
/// alone, is stored and retried like a run's, and once the month gets a
/// run, the run's melding REPLACES it -- sent alongside, the employments
/// would be reported twice.
#[tokio::test]
async fn a_month_without_a_run_reports_every_employment_until_a_run_replaces_it() {
    use regnmed_db::amelding_submission as sub;
    let Some(pool) = pool().await else { return };
    let company = company(&pool).await;
    let kari = employee(&pool, company, "26829398612", "Kari Betalt", 4_000_000).await;
    let per = employee(&pool, company, "03048810003", "Per Permisjon", 3_000_000).await;
    for id in [kari, per] {
        payroll::update_employment_details(&pool, company, id, &complete_details("974760673"))
            .await
            .unwrap();
    }
    let may = sub::Source::Month(date(2026, 5, 1));

    let draft = payroll::amelding_for_month(&pool, company, date(2026, 5, 17), chrono::Utc::now())
        .await
        .unwrap();
    assert!(draft.hindrances.is_empty(), "{:?}", draft.hindrances);
    assert_eq!(draft.melding.recipients.len(), 2);
    assert!(
        draft
            .melding
            .recipients
            .iter()
            .all(|r| r.income.is_empty() && r.withholding_ore == 0)
    );
    let xml = regnmed_core::amelding::render(&draft.melding).unwrap();
    assert!(xml.contains("<kalendermaaned>2026-05</kalendermaaned>"));
    assert!(xml.contains("<sumArbeidsgiveravgift>0</sumArbeidsgiveravgift>"));
    assert!(!xml.contains("loennsutbetalingsdato"));

    // Before anyone was employed there is nothing to report.
    let empty = payroll::amelding_for_month(&pool, company, date(2024, 12, 1), chrono::Utc::now())
        .await
        .unwrap();
    assert!(
        empty
            .hindrances
            .iter()
            .any(|h| h.contains("ingen arbeidsforhold å rapportere")),
        "{:?}",
        empty.hindrances
    );

    // Stored, retried unchanged, delivered once.
    let first = sub::prepare_submission(&pool, company, may, false, "Test")
        .await
        .unwrap();
    assert_eq!((first.run_id, first.period.as_str()), (None, "2026-05"));
    let retry = sub::prepare_submission(&pool, company, may, false, "Test")
        .await
        .unwrap();
    assert_eq!(retry.id, first.id);
    sub::record_receipt(&pool, first.id, "d", "f", "Test")
        .await
        .unwrap();
    let err = sub::prepare_submission(&pool, company, may, false, "Test")
        .await
        .unwrap_err();
    assert!(err.to_string().contains("allerede levert"), "{err}");

    // May gets a run after all: the month is now the run's to report.
    let run = payroll::run_payroll(
        &pool,
        company,
        2026,
        5,
        date(2026, 5, 25),
        "I",
        &[run_entry(kari, None)],
        "Test",
    )
    .await
    .unwrap();
    let err = sub::prepare_submission(&pool, company, may, true, "Test")
        .await
        .unwrap_err();
    assert!(
        err.to_string().contains("rapporterer allerede 05/2026"),
        "{err}"
    );
    let from_run = sub::prepare_submission(&pool, company, sub::Source::Run(run.id), false, "Test")
        .await
        .unwrap();
    assert_eq!(from_run.period, "2026-05");
    assert_eq!(
        from_run.replaces_melding_id.as_deref(),
        Some(first.melding_id.as_str())
    );
    // Per, unpaid, is still in the run's melding: nothing is lost by the
    // replacement.
    assert!(from_run.body.contains("03048810003"));
    assert_eq!(
        sub::list_submissions(&pool, company, may)
            .await
            .unwrap()
            .len(),
        1
    );

    // A run in a month WITHOUT a delivered month melding replaces nothing.
    let june = payroll::run_payroll(
        &pool,
        company,
        2026,
        6,
        date(2026, 6, 25),
        "I",
        &[run_entry(kari, None)],
        "Test",
    )
    .await
    .unwrap();
    let june = sub::prepare_submission(&pool, company, sub::Source::Run(june.id), false, "Test")
        .await
        .unwrap();
    assert_eq!(june.replaces_melding_id, None);
}

/// A month melding whose send failed (Skatteetaten may hold it or not) is
/// not stranded when the month then gets a run: it can still be retried
/// under its own key, and the run's melding waits for it and then
/// replaces it -- never goes out beside it.
#[tokio::test]
async fn an_undelivered_month_melding_is_settled_before_the_run_is_sent() {
    use regnmed_db::amelding_submission as sub;
    let Some(pool) = pool().await else { return };
    let company = company(&pool).await;
    let kari = employee(&pool, company, "26829398612", "Kari Betalt", 4_000_000).await;
    payroll::update_employment_details(&pool, company, kari, &complete_details("974760673"))
        .await
        .unwrap();
    let may = sub::Source::Month(date(2026, 5, 1));
    let stored = sub::prepare_submission(&pool, company, may, false, "Test")
        .await
        .unwrap();
    sub::record_failure(&pool, stored.id, "timeout", "Test")
        .await
        .unwrap();

    let run = payroll::run_payroll(
        &pool,
        company,
        2026,
        5,
        date(2026, 5, 25),
        "I",
        &[run_entry(kari, None)],
        "Test",
    )
    .await
    .unwrap();
    let err = sub::prepare_submission(&pool, company, sub::Source::Run(run.id), false, "Test")
        .await
        .unwrap_err();
    assert!(err.to_string().contains("send den ferdig først"), "{err}");

    let retry = sub::prepare_submission(&pool, company, may, false, "Test")
        .await
        .unwrap();
    assert_eq!(
        (retry.id, retry.idempotency_key),
        (stored.id, stored.idempotency_key)
    );
    sub::record_receipt(&pool, retry.id, "d", "f", "Test")
        .await
        .unwrap();
    let from_run = sub::prepare_submission(&pool, company, sub::Source::Run(run.id), false, "Test")
        .await
        .unwrap();
    assert_eq!(
        from_run.replaces_melding_id.as_deref(),
        Some(stored.melding_id.as_str())
    );
}

/// The end date is what stops F108 reporting; it cannot precede the
/// start, and a mistaken one can be cleared.
#[tokio::test]
async fn an_employment_end_date_is_checked_and_can_be_cleared() {
    let Some(pool) = pool().await else { return };
    let other = company(&pool).await;
    let company = company(&pool).await;
    let id = employee(&pool, company, "26829398612", "Kari Utvikler", 4_000_000).await;
    let err = payroll::end_employment(&pool, company, id, Some(date(2024, 12, 31)))
        .await
        .unwrap_err();
    assert!(
        err.to_string().contains("før arbeidsforholdet startet"),
        "{err}"
    );
    let end = |pool: PgPool| async move {
        payroll::list_ansatte(&pool, company)
            .await
            .unwrap()
            .into_iter()
            .find(|e| e.id == id)
            .unwrap()
            .employed_to
    };
    payroll::end_employment(&pool, company, id, Some(date(2026, 5, 31)))
        .await
        .unwrap();
    assert_eq!(end(pool.clone()).await, Some(date(2026, 5, 31)));
    payroll::end_employment(&pool, company, id, None)
        .await
        .unwrap();
    assert_eq!(end(pool.clone()).await, None);
    // Another company's employee is unknown.
    assert!(
        payroll::end_employment(&pool, other, id, None)
            .await
            .is_err()
    );
}

/// Everything that blocks delivery is listed at once, in words — not the
/// first thing the renderer trips over.
#[tokio::test]
async fn every_hindrance_is_listed_before_anything_is_rendered() {
    let Some(pool) = pool().await else { return };
    let company = company(&pool).await;
    let complete = employee(&pool, company, "26829398612", "Kari Utvikler", 4_000_000).await;
    payroll::update_employment_details(&pool, company, complete, &complete_details("974760673"))
        .await
        .unwrap();
    // No arbeidsforhold details at all.
    let incomplete = employee(&pool, company, "03048810003", "Ola Ufullstendig", 3_000_000).await;
    // Employed this month, but not in the run.
    employee(&pool, company, "15076500565", "Per Ulønnet", 3_000_000).await;
    // Both wage forms and an explicit amount: the run cannot tell which
    // it paid, so it stores nothing.
    let both = payroll::create_employee(
        &pool,
        company,
        &NewEmployee {
            nin: "07045020090".into(),
            name: "Eva Begge".into(),
            stilling: None,
            employed_from: date(2025, 1, 1),
            monthly_salary_ore: Some(3_000_000),
            hourly_wage_ore: Some(30_000),
            withholding_type: "prosent".into(),
            withholding_percent_bp: Some(3000),
            withholding_table: None,
            holiday_pay_bp: 1020,
            bank_account: None,
            note: None,
            details: complete_details("974760673"),
        },
        "Test",
    )
    .await
    .unwrap();
    let run = payroll::run_payroll(
        &pool,
        company,
        2026,
        3,
        date(2026, 3, 25),
        "I",
        &[
            run_entry(complete, None),
            run_entry(incomplete, None),
            run_entry(both, Some(1_000_000)),
        ],
        "Test",
    )
    .await
    .unwrap();

    let draft = payroll::amelding_for_run(&pool, company, run.id, chrono::Utc::now())
        .await
        .unwrap();
    let all = draft.hindrances.join("\n");
    assert_eq!(draft.hindrances.len(), 3, "{all}");
    assert!(
        all.contains("Ola Ufullstendig: arbeidsforholdet mangler virksomhet"),
        "{all}"
    );
    assert!(
        all.contains("Eva Begge: det ble ikke lagret om lønnen"),
        "{all}"
    );
    // Not paid, so in the melding with the arbeidsforhold alone -- which
    // has to be complete like anyone else's.
    assert!(
        all.contains("Per Ulønnet: arbeidsforholdet mangler virksomhet"),
        "{all}"
    );
    assert!(all.contains("F108"), "{all}");
    assert!(!all.contains("Kari"), "{all}");
}

#[tokio::test]
async fn another_companys_run_is_unknown() {
    let Some(pool) = pool().await else { return };
    let mine = company(&pool).await;
    let theirs = company(&pool).await;
    let id = employee(&pool, theirs, "26829398612", "Kari Utvikler", 4_000_000).await;
    let run = payroll::run_payroll(
        &pool,
        theirs,
        2026,
        3,
        date(2026, 3, 25),
        "I",
        &[run_entry(id, None)],
        "Test",
    )
    .await
    .unwrap();
    assert!(
        payroll::amelding_for_run(&pool, mine, run.id, chrono::Utc::now())
            .await
            .is_err()
    );
}

/// A run with everything the a-melding needs, ready to submit.
async fn submittable_run(pool: &PgPool) -> (Uuid, Uuid) {
    let company = company(pool).await;
    let id = employee(pool, company, "26829398612", "Kari Utvikler", 4_000_000).await;
    payroll::update_employment_details(pool, company, id, &complete_details("974760673"))
        .await
        .unwrap();
    let run = payroll::run_payroll(
        pool,
        company,
        2026,
        3,
        date(2026, 3, 25),
        "I",
        &[run_entry(id, None)],
        "Test",
    )
    .await
    .unwrap();
    (company, run.id)
}

/// The rule the record exists for: an undelivered submission is retried
/// AS IT IS — same body, same idempotency key — and a delivered one is
/// never followed by a second melding, only by an explicit replacement.
#[tokio::test]
async fn a_submission_is_retried_unchanged_and_replaced_only_on_request() {
    use regnmed_db::amelding_submission as sub;
    let Some(pool) = pool().await else { return };
    let (company, run) = submittable_run(&pool).await;

    let first = sub::prepare_submission(&pool, company, sub::Source::Run(run), false, "Test")
        .await
        .unwrap();
    assert_eq!(first.period, "2026-03");
    assert!(
        first
            .body
            .contains(&format!("\"meldingsId\":\"{}\"", first.melding_id))
    );
    assert!(
        first
            .body
            .contains("\"norskIdentifikator\":\"26829398612\"")
    );

    // The call failed: the retry is the same submission, byte for byte.
    sub::record_failure(&pool, first.id, "connection reset", "Test")
        .await
        .unwrap();
    let retry = sub::prepare_submission(&pool, company, sub::Source::Run(run), false, "Test")
        .await
        .unwrap();
    assert_eq!(retry.id, first.id);
    assert_eq!(retry.idempotency_key, first.idempotency_key);
    assert_eq!(retry.body, first.body);
    let (_, status) = sub::submission(&pool, company, first.id).await.unwrap();
    assert_eq!(
        status,
        sub::SubmissionStatus::NotDelivered {
            last_error: Some("connection reset".into())
        }
    );

    // Delivered: sending again is refused unless it is a replacement.
    sub::record_receipt(&pool, first.id, "d-1", "t-1", "Test")
        .await
        .unwrap();
    let err = sub::prepare_submission(&pool, company, sub::Source::Run(run), false, "Test")
        .await
        .unwrap_err()
        .to_string();
    assert!(err.contains("allerede levert"), "{err}");
    let replacement = sub::prepare_submission(&pool, company, sub::Source::Run(run), true, "Test")
        .await
        .unwrap();
    assert_ne!(replacement.id, first.id);
    assert_ne!(replacement.idempotency_key, first.idempotency_key);
    assert_eq!(
        replacement.replaces_melding_id.as_deref(),
        Some(first.melding_id.as_str())
    );
    assert!(
        replacement
            .body
            .contains(&format!("\"erstatterMeldingsId\":\"{}\"", first.melding_id))
    );

    // A second receipt for the same submission is a delivery the record
    // could not explain: refused by the database.
    assert!(
        sub::record_receipt(&pool, first.id, "d-2", "t-2", "Test")
            .await
            .is_err()
    );
    // The same feedback fetched twice is stored once.
    for _ in 0..2 {
        sub::record_feedback(&pool, first.id, serde_json::json!({"mottak": []}), "Test")
            .await
            .unwrap();
    }
    let list = sub::list_submissions(&pool, company, sub::Source::Run(run))
        .await
        .unwrap();
    assert_eq!(list.len(), 2);
    assert_eq!(list[0].0.id, replacement.id, "newest first");
    assert_eq!(list[1].1.slug(), "tilbakemelding");
}

#[tokio::test]
async fn the_submission_record_is_append_only_and_needs_a_buildable_run() {
    use regnmed_db::amelding_submission as sub;
    let Some(pool) = pool().await else { return };
    let (first_company, run) = submittable_run(&pool).await;
    let s = sub::prepare_submission(&pool, first_company, sub::Source::Run(run), false, "Test")
        .await
        .unwrap();
    for sql in [
        "update amelding_submission set body = 'x' where id = $1",
        "delete from amelding_submission where id = $1",
    ] {
        assert!(
            sqlx::query(sql).bind(s.id).execute(&pool).await.is_err(),
            "{sql} skulle vært avvist"
        );
    }

    // No arbeidsforhold details: the record refuses before storing anything.
    let company2 = company(&pool).await;
    let id = employee(
        &pool,
        company2,
        "26829398612",
        "Ola Ufullstendig",
        4_000_000,
    )
    .await;
    let run2 = payroll::run_payroll(
        &pool,
        company2,
        2026,
        3,
        date(2026, 3, 25),
        "I",
        &[run_entry(id, None)],
        "Test",
    )
    .await
    .unwrap();
    let err = sub::prepare_submission(&pool, company2, sub::Source::Run(run2.id), false, "Test")
        .await
        .unwrap_err()
        .to_string();
    assert!(err.contains("arbeidsforholdet mangler"), "{err}");
    assert!(
        sub::list_submissions(&pool, company2, sub::Source::Run(run2.id))
            .await
            .unwrap()
            .is_empty()
    );
}

/// Two sends at once: the run row is the lock, so there is ONE melding.
#[tokio::test]
async fn concurrent_sends_create_one_submission() {
    use regnmed_db::amelding_submission as sub;
    let Some(pool) = pool().await else { return };
    let (company, run) = submittable_run(&pool).await;
    let (a, b) = tokio::join!(
        sub::prepare_submission(&pool, company, sub::Source::Run(run), false, "Test"),
        sub::prepare_submission(&pool, company, sub::Source::Run(run), false, "Test"),
    );
    assert_eq!(a.unwrap().id, b.unwrap().id);
    assert_eq!(
        sub::list_submissions(&pool, company, sub::Source::Run(run))
            .await
            .unwrap()
            .len(),
        1
    );
}

/// A skattekort that was fetched but NOT applied must not leave the run
/// silent: the run says which employee, and the card's own reason.
#[tokio::test]
async fn an_unapplied_skattekort_is_named_in_the_run() {
    let Some(pool) = pool().await else { return };
    let company = company(&pool).await;
    let new = |nin: &str, name: &str| NewEmployee {
        nin: nin.into(),
        name: name.into(),
        stilling: None,
        employed_from: date(2025, 1, 1),
        monthly_salary_ore: Some(5_000_000),
        hourly_wage_ore: None,
        withholding_type: "prosent".into(),
        withholding_percent_bp: Some(3500),
        withholding_table: None,
        holiday_pay_bp: 1020,
        bank_account: None,
        note: None,
        details: Default::default(),
    };
    let kept = payroll::create_employee(&pool, company, &new("19910298057", "Uten Kort"), "Test")
        .await
        .unwrap();
    let applied = payroll::create_employee(&pool, company, &new("15929998666", "Med Kort"), "Test")
        .await
        .unwrap();

    let order = Uuid::new_v4();
    regnmed_db::tax_card::record_request(&pool, order, company, 2026, 2, "Test")
        .await
        .unwrap();
    regnmed_db::tax_card::record_order(
        &pool,
        order,
        company,
        2026,
        &format!("BR{}", order.as_u128() % 1_000_000_000),
        Some("d-1"),
        2,
        "Test",
    )
    .await
    .unwrap();
    let card = |nin: &str, result: &str, decision| regnmed_db::tax_card::ReceivedCard {
        nin: nin.into(),
        result: result.into(),
        card_id: None,
        issued: None,
        raw: serde_json::json!({}),
        decision,
        table_percent_bp: None,
    };
    regnmed_db::tax_card::record_answer(
        &pool,
        company,
        order,
        &[
            card(
                "19910298057",
                "ikkeSkattekort",
                Err("Uten skattekort skal arbeidsgiver trekke 50 %".into()),
            ),
            card(
                "15929998666",
                "skattekortopplysningerOK",
                Ok(regnmed_core::payroll::Withholding::Percent(3800)),
            ),
        ],
    )
    .await
    .unwrap();

    let entry = |id| PayrollEntry {
        employee_id: id,
        gross_ore: None,
        holiday_pay_ore: 0,
        holiday_pay_supplement_ore: None,
        from_timesheet: false,
        holiday_pay_exempt: false,
    };
    let run = payroll::run_payroll(
        &pool,
        company,
        2026,
        5,
        date(2026, 5, 25),
        "I",
        &[entry(kept), entry(applied)],
        "Test",
    )
    .await
    .unwrap();
    let named: Vec<_> = run
        .warnings
        .iter()
        .filter(|w| w.contains("skattekortet"))
        .collect();
    assert_eq!(named.len(), 1, "{:?}", run.warnings);
    assert!(
        named[0].contains("Uten Kort") && named[0].contains("50 %"),
        "{named:?}"
    );
}

/// Frikort counts what earlier runs paid; a 12-month card withholds on
/// feriepenger. And the payslip says how each trekk was computed.
#[tokio::test]
async fn frikort_counts_the_year_and_twelve_month_cards_withhold_on_holiday_pay() {
    let Some(pool) = pool().await else { return };
    let company = company(&pool).await;
    let new = |nin: &str, name: &str| NewEmployee {
        nin: nin.into(),
        name: name.into(),
        stilling: None,
        employed_from: date(2025, 1, 1),
        monthly_salary_ore: Some(2_000_000),
        hourly_wage_ore: None,
        withholding_type: "prosent".into(),
        withholding_percent_bp: Some(3500),
        withholding_table: None,
        holiday_pay_bp: 1020,
        bank_account: None,
        note: None,
        details: Default::default(),
    };
    let frikort = payroll::create_employee(&pool, company, &new("01810199403", "Fri Kort"), "Test")
        .await
        .unwrap();
    let twelve =
        payroll::create_employee(&pool, company, &new("05850298728", "Kilde Skatt"), "Test")
            .await
            .unwrap();
    let migrated =
        payroll::create_employee(&pool, company, &new("20857597940", "Fra Annet"), "Test")
            .await
            .unwrap();
    let entry = |id, holiday| PayrollEntry {
        employee_id: id,
        gross_ore: None,
        holiday_pay_ore: holiday,
        holiday_pay_supplement_ore: None,
        from_timesheet: false,
        holiday_pay_exempt: false,
    };
    // December 2025 at 35 %: last year's pay, which this year's frikort
    // must not count.
    payroll::run_payroll(
        &pool,
        company,
        2025,
        12,
        date(2025, 12, 20),
        "I",
        &[entry(frikort, 0), entry(twelve, 0)],
        "Test",
    )
    .await
    .unwrap();

    // The 2026 cards, fetched: 30 000 kr frikort for two, and a
    // 22,20 % card over 12 months.
    use regnmed_core::payroll::Withholding;
    let order = Uuid::new_v4();
    regnmed_db::tax_card::record_request(&pool, order, company, 2026, 3, "Test")
        .await
        .unwrap();
    regnmed_db::tax_card::record_order(
        &pool,
        order,
        company,
        2026,
        &format!("BR{}", order.as_u128() % 1_000_000_000),
        Some("d-1"),
        3,
        "Test",
    )
    .await
    .unwrap();
    let card = |nin: &str, w| regnmed_db::tax_card::ReceivedCard {
        nin: nin.into(),
        result: "skattekortopplysningerOK".into(),
        card_id: None,
        issued: None,
        raw: serde_json::json!({}),
        decision: Ok(w),
        table_percent_bp: None,
    };
    let frikort_card = || Withholding::TaxFreeCard {
        limit_ore: 3_000_000,
    };
    regnmed_db::tax_card::record_answer(
        &pool,
        company,
        order,
        &[
            card("01810199403", frikort_card()),
            card("20857597940", frikort_card()),
            card("05850298728", Withholding::PercentTwelveMonths(2220)),
        ],
    )
    .await
    .unwrap();

    let january = payroll::run_payroll(
        &pool,
        company,
        2026,
        1,
        date(2026, 1, 25),
        "I",
        &[entry(frikort, 0), entry(twelve, 0)],
        "Test",
    )
    .await
    .unwrap();
    let slip = payroll::payslip(&pool, company, january.id, frikort)
        .await
        .unwrap();
    assert_eq!(slip.withholding_ore, 0, "20 000 of 30 000 used");
    assert_eq!(slip.withholding_method.as_deref(), Some("frikort"));

    let february = payroll::run_payroll(
        &pool,
        company,
        2026,
        2,
        date(2026, 2, 25),
        "I",
        &[
            entry(frikort, 0),
            entry(twelve, 1_000_000),
            entry(migrated, 0),
        ],
        "Test",
    )
    .await
    .unwrap();
    // Employed since 2025, yet no 2026 pay in regnmed before February:
    // pay from another system is not counted, and the run says so.
    assert!(
        february
            .warnings
            .iter()
            .any(|w| w.starts_with("Fra Annet:") && w.contains("annet system")),
        "{:?}",
        february.warnings
    );
    assert!(!february.warnings.iter().any(|w| w.starts_with("Fri Kort:")));
    // 10 000 kr left of the limit, 20 000 paid: 50 % of 10 000.
    let slip = payroll::payslip(&pool, company, february.id, frikort)
        .await
        .unwrap();
    assert_eq!(slip.withholding_ore, 500_000);
    assert!(
        slip.withholding_method
            .unwrap()
            .contains("over beløpsgrensen")
    );
    // 22,20 % of 20 000 lønn + 10 000 feriepenger.
    let slip = payroll::payslip(&pool, company, february.id, twelve)
        .await
        .unwrap();
    assert_eq!(slip.withholding_ore, 666_000);
    assert_eq!(slip.withholding_basis_ore, 3_000_000);
    assert_eq!(
        slip.withholding_method.as_deref(),
        Some("22,20 % (12 måneder)")
    );

    // December's pay paid in January 2027: the 2026 frikort does not
    // carry over, since the count starts again at zero.
    let refused = payroll::run_payroll(
        &pool,
        company,
        2026,
        12,
        date(2027, 1, 5),
        "I",
        &[entry(frikort, 0)],
        "Test",
    )
    .await
    .unwrap_err()
    .to_string();
    assert!(refused.contains("frikort for 2027"), "{refused}");
}

/// An approved kjøring claim, as the expense flow leaves it: 2910 owes
/// the claimant. `passengers` adds the 2026 supplement (1,00 kr/km, all
/// of it trekkfri).
async fn approved_mileage(
    pool: &PgPool,
    company: Uuid,
    person: Uuid,
    day: u32,
    km: i64,
    passengers: &[&str],
) -> Uuid {
    let n = passengers.len() as i64;
    let allowance = regnmed_core::expenses::trip_allowance(km, (530, 350), (100, 100), n);
    let id = Uuid::now_v7();
    sqlx::query(
        "insert into expense (id, company_id, person_id, kind, dato, beskrivelse, belop_ore, km,
                              sats_ore_per_km, trekkfri_ore, trekkpliktig_ore, route_from,
                              route_to, vehicle, passengers, passenger_rate_ore_per_km,
                              created_by)
         values ($1, $2, $3, 'kjoring', $4, 'Kundemøte', $5, $6, 530, $7, $8, 'Oslo',
                 'Drammen', 'bil', $9, $10, 'Test')",
    )
    .bind(id)
    .bind(company)
    .bind(person)
    .bind(date(2026, 3, day))
    .bind(allowance.amount_ore)
    .bind(km)
    .bind(allowance.tax_free_ore)
    .bind(allowance.taxable_ore)
    .bind(passengers.iter().map(|p| p.to_string()).collect::<Vec<_>>())
    .bind((n > 0).then_some(100i64))
    .execute(pool)
    .await
    .unwrap();
    regnmed_db::approve_expense(
        pool, company, id, "7100", None, "2710", "2910", "Test", person,
    )
    .await
    .unwrap();
    id
}

/// Kjøregodtgjørelse is paid with pay (docs/utlegg.md «Utbetaling»): the
/// run takes trekk on the excess over the trekkfri rate and avgift on it,
/// settles 2910, marks the claims paid, and the a-melding reports each
/// part with its kilometres — and a claim is never paid twice.
#[tokio::test]
async fn mileage_is_paid_with_pay_and_reported_by_part() {
    let Some(pool) = pool().await else { return };
    let company = company(&pool).await;
    for (number, name) in [("7100", "Bilgodtgjørelse"), ("2910", "Gjeld til ansatte")] {
        regnmed_db::ensure_account(&pool, company, number, name)
            .await
            .unwrap();
    }
    let a = employee(&pool, company, "26829398612", "Kari Kjører", 4_000_000).await;
    let person = regnmed_db::ensure_person(
        &pool,
        &format!("test|{}", Uuid::new_v4()),
        Some("Kari Kjører"),
        None,
    )
    .await
    .unwrap();
    sqlx::query("update employee set person_id = $2 where id = $1")
        .bind(a)
        .bind(person)
        .execute(&pool)
        .await
        .unwrap();

    // 120 km alone: 636 kr, 216 kr of it above the trekkfri rate.
    let alone = approved_mileage(&pool, company, person, 10, 120, &[]).await;
    // 100 km with a colleague: 630 kr, 180 kr above.
    let shared = approved_mileage(&pool, company, person, 12, 100, &["Ola"]).await;
    // A claim dated after the payment date waits for the next run.
    let later = approved_mileage(&pool, company, person, 28, 10, &[]).await;

    // A manual payment would skip the trekk and the a-melding.
    let refused = regnmed_db::pay_expense(&pool, company, alone, date(2026, 3, 15), "1920", "Test")
        .await
        .unwrap_err()
        .to_string();
    assert!(refused.contains("lønnskjøring"), "{refused}");

    let r = run(&pool, company, a, 3, "I", None, 0).await;
    assert_eq!(voucher_sum(&pool, r.voucher_id).await, 0);
    assert_eq!(r.sum.allowance_ore, 63_600 + 63_000);
    assert_eq!(
        account_amount(&pool, r.voucher_id, "2910").await,
        126_600,
        "the run settles what the approvals owed"
    );
    // 35 % of 40 000 + 216 + 180 kr = 14 138,60 kr → 14 138 kr.
    let line = &r.lines[0].calculation;
    assert_eq!(line.withholding_basis_ore, 4_039_600);
    assert_eq!(r.sum.withholding_ore, 1_413_800);
    assert_eq!(r.sum.net_ore, 4_000_000 + 126_600 - 1_413_800);
    assert_eq!(
        r.sum.employer_contribution_ore,
        regnmed_core::payroll::employer_contribution(
            4_039_600,
            regnmed_core::payroll::Zone::I,
            1410
        )
        .unwrap(),
        "avgift on pay and the excess, not on the reimbursement"
    );

    let status = |id: Uuid| {
        let pool = pool.clone();
        async move {
            sqlx::query_scalar::<_, String>("select status from expense where id = $1")
                .bind(id)
                .fetch_one(&pool)
                .await
                .unwrap()
        }
    };
    assert_eq!(status(alone).await, "utbetalt");
    assert_eq!(status(shared).await, "utbetalt");
    assert_eq!(status(later).await, "godkjent");

    let draft = payroll::amelding_for_run(&pool, company, r.id, chrono::Utc::now())
        .await
        .unwrap();
    use regnmed_core::amelding::IncomeKind;
    let income: Vec<(IncomeKind, i64)> = draft.melding.recipients[0]
        .income
        .iter()
        .map(|i| (i.kind, i.amount_ore))
        .collect();
    assert_eq!(
        income,
        vec![
            (IncomeKind::FixedSalary, 4_000_000),
            (
                IncomeKind::MileageCar {
                    km: 220,
                    taxable: false
                },
                42_000 + 35_000
            ),
            (
                IncomeKind::MileageCar {
                    km: 220,
                    taxable: true
                },
                21_600 + 18_000
            ),
            (IncomeKind::MileagePassenger { km: 100 }, 10_000),
        ]
    );
    assert_eq!(draft.melding.recipients[0].withholding_ore, 1_413_800);

    // April pays the late claim and nothing from March again.
    let april = run(&pool, company, a, 4, "I", None, 0).await;
    assert_eq!(april.sum.allowance_ore, 5_300);
    assert_eq!(status(later).await, "utbetalt");

    // Once the employment has ended no run can include the employee, so a
    // claim filed afterwards is paid by hand rather than stuck forever.
    sqlx::query("update employee set ansatt_til = '2026-04-30' where id = $1")
        .bind(a)
        .execute(&pool)
        .await
        .unwrap();
    let after = approved_mileage(&pool, company, person, 20, 10, &[]).await;
    regnmed_db::pay_expense(&pool, company, after, date(2026, 5, 2), "1920", "Test")
        .await
        .unwrap();
    assert_eq!(status(after).await, "utbetalt");
}

/// A travel claim through the real path: draft, submit (rates resolved
/// and the døgn stored), approve onto 2910.
async fn approved_trip(
    pool: &PgPool,
    company: Uuid,
    person: Uuid,
    departure: chrono::NaiveDateTime,
    return_at: chrono::NaiveDateTime,
    overnight: bool,
    days: Vec<regnmed_db::travel::TravelDayDraft>,
) -> Uuid {
    let id = regnmed_db::create_draft(
        pool,
        company,
        person,
        person,
        "reise",
        None,
        &regnmed_db::ExpenseDraft {
            description: "Kundebesøk".into(),
            departure_at: Some(departure),
            return_at: Some(return_at),
            destination: Some("Bergen".into()),
            overnight,
            over_15_km: true,
            travel_days: days,
            ..Default::default()
        },
        None,
        "Test",
    )
    .await
    .unwrap();
    regnmed_db::submit_expense(pool, company, id).await.unwrap();
    regnmed_db::approve_expense(
        pool, company, id, "7150", None, "2710", "2910", "Test", person,
    )
    .await
    .unwrap();
    id
}

/// Diett is paid with pay like mileage (docs/utlegg.md «Reise og diett»):
/// on the state's rates the excess over the trekkfri rate gets trekk and
/// avgift, and the a-melding reports the trekkfri part by lodging with its
/// døgn and the excess as `reiseKost`.
#[tokio::test]
async fn diett_is_paid_with_pay_and_reported_by_lodging() {
    use regnmed_core::amelding::IncomeKind;
    use regnmed_core::travel::Report;
    use regnmed_db::travel::TravelDayDraft;
    let Some(pool) = pool().await else { return };
    let company = company(&pool).await;
    for (number, name) in [("7150", "Diettkostnad"), ("2910", "Gjeld til ansatte")] {
        regnmed_db::ensure_account(&pool, company, number, name)
            .await
            .unwrap();
    }
    regnmed_db::travel::set_travel_scheme(&pool, company, "statens")
        .await
        .unwrap();
    let a = employee(&pool, company, "26829398612", "Reidun Reisende", 4_000_000).await;
    let person = regnmed_db::ensure_person(
        &pool,
        &format!("test|{}", Uuid::new_v4()),
        Some("Reidun Reisende"),
        None,
    )
    .await
    .unwrap();
    sqlx::query("update employee set person_id = $2 where id = $1")
        .bind(a)
        .bind(person)
        .execute(&pool)
        .await
        .unwrap();
    let at = |d: u32, h: u32| date(2026, 3, d).and_hms_opt(h, 0, 0).unwrap();

    // A hotel night with breakfast, home 7 hours into the next døgn:
    // the state pays 1 012 − 202 = 810 kr and 397 kr for the part døgn;
    // trekkfri is 693 − 139 = 554 kr and (693 capped at what is paid) 397.
    let night = approved_trip(
        &pool,
        company,
        person,
        at(9, 8),
        at(10, 15),
        true,
        vec![
            TravelDayDraft {
                place: "NO".into(),
                lodging: Some("hotell".into()),
                breakfast: true,
                ..Default::default()
            },
            TravelDayDraft {
                place: "NO".into(),
                ..Default::default()
            },
        ],
    )
    .await;
    // A day trip of 13 hours: 736 kr paid, 400 kr trekkfri.
    approved_trip(
        &pool,
        company,
        person,
        at(12, 7),
        at(12, 20),
        false,
        vec![TravelDayDraft {
            place: "NO".into(),
            ..Default::default()
        }],
    )
    .await;

    // Not by hand: the excess needs trekk.
    let refused = regnmed_db::pay_expense(&pool, company, night, date(2026, 3, 15), "1920", "Test")
        .await
        .unwrap_err()
        .to_string();
    assert!(refused.contains("lønnskjøring"), "{refused}");

    let r = run(&pool, company, a, 3, "I", None, 0).await;
    assert_eq!(voucher_sum(&pool, r.voucher_id).await, 0);
    assert_eq!(r.sum.allowance_ore, 120_700 + 73_600);
    assert_eq!(account_amount(&pool, r.voucher_id, "2910").await, 194_300);
    // 35 % of 40 000 + 256 + 336 kr = 14 207,20 kr → 14 207 kr.
    assert_eq!(r.lines[0].calculation.withholding_basis_ore, 4_059_200);
    assert_eq!(r.sum.withholding_ore, 1_420_700);
    assert_eq!(
        r.sum.employer_contribution_ore,
        regnmed_core::payroll::employer_contribution(
            4_059_200,
            regnmed_core::payroll::Zone::I,
            1410
        )
        .unwrap(),
        "avgift on pay and the excess, not on the trekkfri diett"
    );

    let draft = payroll::amelding_for_run(&pool, company, r.id, chrono::Utc::now())
        .await
        .unwrap();
    let income: Vec<(IncomeKind, i64)> = draft.melding.recipients[0]
        .income
        .iter()
        .map(|i| (i.kind, i.amount_ore))
        .collect();
    let travel = |report, days| IncomeKind::Travel { report, days };
    assert_eq!(
        income,
        vec![
            (IncomeKind::FixedSalary, 4_000_000),
            (travel(Report::KostDayTrip, 1), 40_000),
            (travel(Report::KostHotel, 2), 55_400 + 39_700),
            (travel(Report::KostTaxable, 0), 25_600 + 33_600),
        ]
    );

    // Paid once: April finds nothing left.
    let april = run(&pool, company, a, 4, "I", None, 0).await;
    assert_eq!(april.sum.allowance_ore, 0);
}

/// The preview a person reads before booking must be the run that gets
/// booked, and must itself book nothing: no voucher, no voucher number,
/// no payroll row. Once the month has run, the preview refuses it too.
#[tokio::test]
async fn a_preview_books_nothing_and_matches_the_run() {
    let Some(pool) = pool().await else { return };
    let company = company(&pool).await;
    let a = employee(&pool, company, "26829398612", "Kari Utvikler", 5_000_000).await;
    let entries = [PayrollEntry {
        employee_id: a,
        gross_ore: None,
        holiday_pay_ore: 0,
        holiday_pay_supplement_ore: None,
        from_timesheet: false,
        holiday_pay_exempt: false,
    }];
    let count = |table: &'static str| {
        let pool = pool.clone();
        async move {
            sqlx::query_scalar::<_, i64>(&format!(
                "select count(*) from {table} where company_id = $1"
            ))
            .bind(company)
            .fetch_one(&pool)
            .await
            .unwrap()
        }
    };

    // The portal's unchosen zone is "": never a default rate.
    let err = payroll::preview_payroll(&pool, company, 2026, 4, date(2026, 4, 25), "", &entries)
        .await
        .unwrap_err();
    assert!(
        err.to_string().contains("ukjent arbeidsgiveravgiftssone"),
        "{err}"
    );

    let preview =
        payroll::preview_payroll(&pool, company, 2026, 4, date(2026, 4, 25), "I", &entries)
            .await
            .unwrap();
    assert_eq!(preview.id, Uuid::nil());
    assert_eq!(preview.voucher_id, Uuid::nil());
    assert_eq!(count("voucher").await, 0, "a preview must not post");
    assert_eq!(
        count("payroll_run").await,
        0,
        "a preview must not record a run"
    );

    let run = payroll::run_payroll(
        &pool,
        company,
        2026,
        4,
        date(2026, 4, 25),
        "I",
        &entries,
        "Test",
    )
    .await
    .unwrap();
    assert_eq!(
        preview.sum, run.sum,
        "the preview is the run that gets booked"
    );
    assert_eq!(count("voucher").await, 1);

    let err = payroll::preview_payroll(&pool, company, 2026, 4, date(2026, 4, 25), "I", &entries)
        .await
        .unwrap_err();
    assert!(err.to_string().contains("allerede kjørt"), "{err}");

    // A payout date inside a locked period would fail at posting; the
    // preview says so before anyone reads the figures.
    regnmed_db::set_period_lock(&pool, company, date(2026, 5, 31), "Test", false)
        .await
        .unwrap();
    let err = payroll::preview_payroll(&pool, company, 2026, 5, date(2026, 5, 25), "I", &entries)
        .await
        .unwrap_err();
    assert!(err.to_string().contains("låst"), "{err}");
}

// ---- trekkpålegg registered by hand (docs/trekkpalegg.md §6, #339) -----

mod garnishment {
    use super::*;
    use regnmed_core::garnishment::{Draft, Kind, Size};
    use regnmed_db::garnishment::{self as g, Author, VersionDraft};

    /// Skatteetaten's orgnr, and the API documentation's KID and account.
    const PAYEE: &str = "974761076";
    const KID: &str = "6487719756";
    const ACCOUNT: &str = "70213997155";

    async fn person(pool: &PgPool, name: &str) -> Uuid {
        regnmed_db::tenancy::ensure_person(
            pool,
            &format!("sub-{}", Uuid::new_v4()),
            Some(name),
            None,
        )
        .await
        .unwrap()
    }

    async fn letter(pool: &PgPool, company: Uuid) -> Uuid {
        g::upload_letter(
            pool,
            company,
            "brev.pdf",
            "application/pdf",
            b"%PDF-1.4 brev",
            "Test",
        )
        .await
        .unwrap()
    }

    fn coordinated(amount_ore: i64) -> Draft {
        Draft {
            kind: Kind::Coordinated,
            size: Size::AmountOre(amount_ore),
            valid_from: date(2026, 1, 1),
            valid_to: None,
            kid: Some(KID.into()),
            payee_orgnumber: Some(PAYEE.into()),
            payee_account: Some(ACCOUNT.into()),
            due_days: None,
        }
    }

    fn tax(percent_bp: i64) -> Draft {
        Draft {
            kind: Kind::Tax,
            size: Size::PercentBp(percent_bp),
            kid: None,
            payee_orgnumber: None,
            payee_account: None,
            ..coordinated(1)
        }
    }

    async fn register(
        pool: &PgPool,
        company: Uuid,
        employee: Uuid,
        draft: Draft,
        by: Uuid,
    ) -> Uuid {
        let letter_id = letter(pool, company).await;
        g::register(
            pool,
            company,
            employee,
            VersionDraft {
                draft,
                letter_id: Some(letter_id),
                note: None,
            },
            &Author {
                person_id: by,
                name: "Registrerer",
            },
        )
        .await
        .unwrap()
        .0
    }

    fn entry(employee_id: Uuid) -> PayrollEntry {
        PayrollEntry {
            employee_id,
            gross_ore: None,
            holiday_pay_ore: 0,
            holiday_pay_supplement_ore: None,
            from_timesheet: false,
            holiday_pay_exempt: false,
        }
    }

    async fn preview(
        pool: &PgPool,
        company: Uuid,
        poster: &[PayrollEntry],
    ) -> anyhow::Result<payroll::PayrollRun> {
        payroll::preview_payroll(pool, company, 2026, 3, date(2026, 3, 25), "I", poster).await
    }

    async fn book(
        pool: &PgPool,
        company: Uuid,
        poster: &[PayrollEntry],
        set: Option<&[g::Key]>,
    ) -> anyhow::Result<payroll::PayrollRun> {
        payroll::run_payroll_as_previewed(
            pool,
            company,
            2026,
            3,
            date(2026, 3, 25),
            "I",
            poster,
            "Test",
            set,
        )
        .await
    }

    fn xsd_valid(xml: &str) {
        let xsd = format!(
            "{}/../../docs/amelding/a-meldingen_v2_3.xsd",
            env!("CARGO_MANIFEST_DIR")
        );
        let file = std::env::temp_dir().join(format!("regnmed-trekk-{}.xml", Uuid::new_v4()));
        std::fs::write(&file, xml).unwrap();
        let Ok(out) = std::process::Command::new("xmllint")
            .args(["--noout", "--schema", &xsd])
            .arg(&file)
            .output()
        else {
            eprintln!("xmllint not installed -- skipping schema validation");
            return;
        };
        assert!(
            out.status.success(),
            "{}\n{xml}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    /// The whole §6 path short of Skatteetaten: registered from a letter,
    /// withheld in the run, booked as a supplier post with KID and due
    /// date, listed by the payment list, reported in an XSD-valid
    /// a-melding and shown on the payslip.
    #[tokio::test]
    async fn a_hand_registered_order_is_withheld_booked_paid_and_reported() {
        let Some(pool) = pool().await else { return };
        let company = company(&pool).await;
        let kari = employee(&pool, company, "26829398612", "Kari Utvikler", 5_000_000).await;
        payroll::update_employment_details(&pool, company, kari, &complete_details("974760673"))
            .await
            .unwrap();
        let by = person(&pool, "Lønnsansvarlig").await;
        let order = register(&pool, company, kari, coordinated(400_000), by).await;

        let poster = [entry(kari)];
        let shown = preview(&pool, company, &poster).await.unwrap();
        assert_eq!(shown.garnishment_set, vec![g::Key::Registered(order, 1)]);
        assert_eq!(shown.garnishment_account.as_deref(), Some("2610"));
        let line = &shown.lines[0];
        assert_eq!(line.garnishments[0].withheld_ore, 400_000);
        // 50 000 - 35 % trekk - 4 000 trekkpålegg.
        assert_eq!(line.calculation.net_ore, 5_000_000 - 1_750_000 - 400_000);
        assert!(
            shown
                .warnings
                .iter()
                .any(|w| w.contains("opprettes som leverandør")),
            "{:?}",
            shown.warnings
        );

        // A booking without the set is refused like one with a wrong set.
        let err = book(&pool, company, &poster, None).await.unwrap_err();
        assert!(err.to_string().contains("forhåndsvis"), "{err}");
        let err = book(&pool, company, &poster, Some(&[])).await.unwrap_err();
        assert!(err.to_string().contains("forhåndsvis"), "{err}");

        let run = book(&pool, company, &poster, Some(&shown.garnishment_set))
            .await
            .unwrap();
        assert_eq!(voucher_sum(&pool, run.voucher_id).await, 0);
        assert_eq!(
            account_amount(&pool, run.voucher_id, "2610").await,
            -400_000
        );
        assert_eq!(
            account_amount(&pool, run.voucher_id, "2930").await,
            -(5_000_000 - 1_750_000 - 400_000)
        );
        let (kind, text): (Option<String>, Option<String>) = sqlx::query_as(
            "select a.reskontro_kind, e.description from entry e
             join account a on a.id = e.account_id
             where e.voucher_id = $1 and a.number = '2610'",
        )
        .bind(run.voucher_id)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(kind.as_deref(), Some("leverandor"));
        // Never the employee's name: the ledger is readable by `les`.
        assert_eq!(text.as_deref(), Some("Utleggstrekk 03/2026"));

        // The payment list pays it to the letter's account with its KID,
        // due on the pay date.
        let items = regnmed_db::payments::payable_items(&pool, company)
            .await
            .unwrap();
        let item = items
            .iter()
            .find(|i| i.kid.as_deref() == Some(KID))
            .unwrap();
        assert_eq!(item.amount_ore, 400_000);
        assert_eq!(item.bank_account.as_deref(), Some(ACCOUNT));
        assert_eq!(item.due_date, Some(date(2026, 3, 25)));

        // The account is remembered as the company's choice.
        let settings = g::settings_history(&pool, company).await.unwrap();
        assert_eq!(settings[0].account_number.as_deref(), Some("2610"));
        // Settings saved after the run, dated before its pay date, still
        // govern later months: the remembered row does not shadow them.
        g::set_settings(
            &pool,
            company,
            &g::Settings {
                valid_from: Some(date(2026, 3, 1)),
                account_number: Some("2610".into()),
                tax_kid: Some("12345678903".into()),
                set_by: "Test".into(),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let april = g::settings_on(
            &mut *pool.acquire().await.unwrap(),
            company,
            date(2026, 4, 25),
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(april.tax_kid.as_deref(), Some("12345678903"));

        let draft = payroll::amelding_for_run(&pool, company, run.id, chrono::Utc::now())
            .await
            .unwrap();
        assert!(draft.hindrances.is_empty(), "{:?}", draft.hindrances);
        let xml = regnmed_core::amelding::render(&draft.melding).unwrap();
        xsd_valid(&xml);
        let flat: String = xml.lines().map(str::trim).collect();
        assert!(
            flat.contains("<sumUtleggstrekk>4000</sumUtleggstrekk>"),
            "{flat}"
        );
        assert!(
            flat.contains(
                "<utleggstrekk><beskrivelse>utleggstrekkSamordnet</beskrivelse>\
                 <beloep>-4000</beloep><datoForUtleggstrekk>2026-03-25</datoForUtleggstrekk>"
            ),
            "{flat}"
        );

        let slip = payroll::payslip(&pool, company, run.id, kari)
            .await
            .unwrap();
        assert_eq!(slip.deductions.len(), 1);
        assert_eq!(slip.deductions[0].amount_ore, -400_000);
        assert!(slip.deductions[0].text.contains(KID));
        assert_eq!(slip.net_ore, 5_000_000 - 1_750_000 - 400_000);
    }

    /// §3 point 2: what was shown is what is booked.
    #[tokio::test]
    async fn a_change_after_the_preview_stops_the_run() {
        let Some(pool) = pool().await else { return };
        let company = company(&pool).await;
        let kari = employee(&pool, company, "26829398612", "Kari Utvikler", 5_000_000).await;
        let by = person(&pool, "Lønnsansvarlig").await;
        let order = register(&pool, company, kari, coordinated(400_000), by).await;
        let poster = [entry(kari)];
        let shown = preview(&pool, company, &poster).await.unwrap();

        let v2 = g::change(
            &pool,
            company,
            order,
            VersionDraft {
                draft: coordinated(450_000),
                letter_id: None,
                note: Some("Nytt brev".into()),
            },
            &Author {
                person_id: by,
                name: "Registrerer",
            },
        )
        .await
        .unwrap();
        assert_eq!(v2, 2);
        let err = book(&pool, company, &poster, Some(&shown.garnishment_set))
            .await
            .unwrap_err();
        assert!(err.to_string().contains("forhåndsvis"), "{err}");

        let again = preview(&pool, company, &poster).await.unwrap();
        assert_eq!(again.garnishment_set, vec![g::Key::Registered(order, 2)]);
        let run = book(&pool, company, &poster, Some(&again.garnishment_set))
            .await
            .unwrap();
        assert_eq!(
            account_amount(&pool, run.voucher_id, "2610").await,
            -450_000
        );

        // A stop is a version too, and the next month withholds nothing.
        assert_eq!(
            g::stop(
                &pool,
                company,
                order,
                None,
                &Author {
                    person_id: by,
                    name: "R"
                }
            )
            .await
            .unwrap(),
            3
        );
        let april =
            payroll::preview_payroll(&pool, company, 2026, 4, date(2026, 4, 25), "I", &poster)
                .await
                .unwrap();
        assert!(april.garnishment_set.is_empty());
        assert!(april.lines[0].garnishments.is_empty());
    }

    /// Insert-only in the database itself, whatever the application does.
    #[tokio::test]
    async fn registrations_cannot_be_edited_or_deleted() {
        let Some(pool) = pool().await else { return };
        let company = company(&pool).await;
        let kari = employee(&pool, company, "26829398612", "Kari Utvikler", 5_000_000).await;
        let by = person(&pool, "Lønnsansvarlig").await;
        let order = register(&pool, company, kari, coordinated(400_000), by).await;
        for sql in [
            "update garnishment_version set amount_ore = 4000000 where garnishment_id = $1",
            "delete from garnishment_version where garnishment_id = $1",
            "update garnishment set kind = 'skatt' where id = $1",
            "delete from garnishment where id = $1",
        ] {
            let err = sqlx::query(sql).bind(order).execute(&pool).await;
            assert!(err.is_err(), "{sql} must be refused");
        }
        let letter: Uuid = sqlx::query_scalar(
            "select letter_id from garnishment_version where garnishment_id = $1",
        )
        .bind(order)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert!(
            sqlx::query("update garnishment_letter set filename = 'x' where id = $1")
                .bind(letter)
                .execute(&pool)
                .await
                .is_err()
        );
        // The letter comes back hash-checked.
        let (_, _, bytes) = g::letter(&pool, company, letter).await.unwrap();
        assert_eq!(bytes, b"%PDF-1.4 brev");
    }

    /// With an attestation policy, another person must approve before
    /// anything is withheld; without one, nobody has to.
    #[tokio::test]
    async fn with_a_policy_a_registration_waits_for_another_person() {
        let Some(pool) = pool().await else { return };
        let company = company(&pool).await;
        let kari = employee(&pool, company, "26829398612", "Kari Utvikler", 5_000_000).await;
        let registrar = person(&pool, "Lønnsansvarlig").await;
        let attestant = person(&pool, "Daglig leder").await;
        regnmed_db::approval::set_policy(&pool, company, true, None, None, None, "Test")
            .await
            .unwrap();
        let order = register(&pool, company, kari, coordinated(400_000), registrar).await;
        let poster = [entry(kari)];

        let shown = preview(&pool, company, &poster).await.unwrap();
        assert!(shown.garnishment_set.is_empty());
        assert_eq!(shown.lines[0].calculation.net_ore, 5_000_000 - 1_750_000);
        assert!(
            shown
                .warnings
                .iter()
                .any(|w| w.contains("venter på attestering")),
            "{:?}",
            shown.warnings
        );

        let own = g::attest(
            &pool,
            company,
            order,
            1,
            true,
            None,
            &Author {
                person_id: registrar,
                name: "Lønnsansvarlig",
            },
        )
        .await
        .unwrap_err();
        assert!(own.to_string().contains("selv"), "{own}");
        g::attest(
            &pool,
            company,
            order,
            1,
            true,
            None,
            &Author {
                person_id: attestant,
                name: "Daglig leder",
            },
        )
        .await
        .unwrap();
        let shown = preview(&pool, company, &poster).await.unwrap();
        assert_eq!(shown.garnishment_set, vec![g::Key::Registered(order, 1)]);

        // A change waits again, while the attested version keeps running.
        g::change(
            &pool,
            company,
            order,
            VersionDraft {
                draft: coordinated(900_000),
                letter_id: None,
                note: None,
            },
            &Author {
                person_id: registrar,
                name: "Lønnsansvarlig",
            },
        )
        .await
        .unwrap();
        let shown = preview(&pool, company, &poster).await.unwrap();
        assert_eq!(shown.garnishment_set, vec![g::Key::Registered(order, 1)]);
        assert_eq!(shown.lines[0].garnishments[0].withheld_ore, 400_000);
        let listed = g::list(&pool, company).await.unwrap();
        assert_eq!(listed[0].status(), "aktiv, endringen venter på attestering");
        assert!(
            shown
                .warnings
                .iter()
                .any(|w| w.contains("versjon 1 trekkes i stedet"))
        );
    }

    /// Utleggstrekk skatt: one combined supplier post with the company's
    /// own KID, refused until the settings say where to pay; each
    /// employee's share reported as utleggstrekkSkatt. Øvrige are booked
    /// with the letter's deadline and never reported.
    #[tokio::test]
    async fn tax_orders_are_paid_as_one_post_and_other_claims_are_not_reported() {
        let Some(pool) = pool().await else { return };
        let company = company(&pool).await;
        let kari = employee(&pool, company, "26829398612", "Kari Utvikler", 5_000_000).await;
        let ola = employee(&pool, company, "15076500565", "Ola Tester", 4_000_000).await;
        for e in [kari, ola] {
            payroll::update_employment_details(&pool, company, e, &complete_details("974760673"))
                .await
                .unwrap();
        }
        let by = person(&pool, "Lønnsansvarlig").await;
        let tax_kari = register(&pool, company, kari, tax(1000), by).await;
        let tax_ola = register(&pool, company, ola, tax(1250), by).await;
        let other = register(
            &pool,
            company,
            ola,
            Draft {
                kind: Kind::Other,
                due_days: Some(10),
                ..coordinated(100_000)
            },
            by,
        )
        .await;
        let poster = [entry(kari), entry(ola)];

        let err = preview(&pool, company, &poster).await.unwrap_err();
        let message = err.to_string();
        assert!(
            message.contains("KID")
                && message.contains("kontonummer")
                && message.contains("organisasjonsnummer"),
            "{message}"
        );
        g::set_settings(
            &pool,
            company,
            &g::Settings {
                valid_from: Some(date(2026, 1, 1)),
                tax_kid: Some("12345678903".into()),
                tax_account: Some("6345.06.10110".into()),
                tax_payee_orgnumber: Some(PAYEE.into()),
                set_by: "Test".into(),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        let shown = preview(&pool, company, &poster).await.unwrap();
        let mut expected = vec![
            g::Key::Registered(tax_kari, 1),
            g::Key::Registered(tax_ola, 1),
            g::Key::Registered(other, 1),
        ];
        expected.sort();
        assert_eq!(shown.garnishment_set, expected);
        let run = book(&pool, company, &poster, Some(&shown.garnishment_set))
            .await
            .unwrap();
        // 10 % of 50 000 + 12,5 % of 40 000 = 10 000, one post.
        let posts: Vec<(i64, Option<String>, Option<NaiveDate>)> = sqlx::query_as(
            "select e.amount_ore, t.kid, t.due_date from entry e
             join account a on a.id = e.account_id
             join payable_terms t on t.entry_id = e.id
             where e.voucher_id = $1 and a.number = '2610' order by e.line_no",
        )
        .bind(run.voucher_id)
        .fetch_all(&pool)
        .await
        .unwrap();
        assert_eq!(
            posts,
            vec![
                (
                    -1_000_000,
                    Some("12345678903".into()),
                    Some(date(2026, 3, 25))
                ),
                (-100_000, Some(KID.into()), Some(date(2026, 4, 4))),
            ]
        );

        let draft = payroll::amelding_for_run(&pool, company, run.id, chrono::Utc::now())
            .await
            .unwrap();
        let xml = regnmed_core::amelding::render(&draft.melding).unwrap();
        xsd_valid(&xml);
        let flat: String = xml.lines().map(str::trim).collect();
        assert!(
            flat.contains("<sumUtleggstrekk>10000</sumUtleggstrekk>"),
            "{flat}"
        );
        assert_eq!(flat.matches("utleggstrekkSkatt").count(), 2);
        assert!(
            !flat.contains("<beloep>-1000</beloep>"),
            "øvrige are not reported"
        );
    }

    /// §3: as much as possible, never more, and never from the trekkfri
    /// allowances; an existing 2610 that is not a supplier subledger is
    /// left alone.
    #[tokio::test]
    async fn the_cap_and_the_account_fallback() {
        let Some(pool) = pool().await else { return };
        let company = company(&pool).await;
        regnmed_db::ensure_account(&pool, company, "2610", "Noe annet")
            .await
            .unwrap();
        let kari = employee(&pool, company, "26829398612", "Kari Utvikler", 1_000_000).await;
        let by = person(&pool, "Lønnsansvarlig").await;
        register(&pool, company, kari, coordinated(900_000), by).await;
        let poster = [entry(kari)];
        let shown = preview(&pool, company, &poster).await.unwrap();
        // 10 000 - 35 % = 6 500 left to take.
        assert_eq!(shown.lines[0].garnishments[0].withheld_ore, 650_000);
        assert_eq!(shown.lines[0].calculation.net_ore, 0);
        assert!(
            shown
                .warnings
                .iter()
                .any(|w| w.contains("innkrevingsmyndigheten"))
        );
        assert_eq!(shown.garnishment_account.as_deref(), Some("2611"));
        let run = book(&pool, company, &poster, Some(&shown.garnishment_set))
            .await
            .unwrap();
        assert_eq!(
            account_amount(&pool, run.voucher_id, "2611").await,
            -650_000
        );
        assert_eq!(account_amount(&pool, run.voucher_id, "2610").await, 0);
    }

    // ---- fetched from Skatteetaten (§2, #334) ------------------------------
    //
    // The lists are built in the shape of Skatteetaten's documentation
    // examples (docs/trekkpalegg/liste-fra-dokumentasjonen.json) and stored
    // through the same `store_list` the live fetch uses; only the HTTP call
    // is left out (the API test serves it from a mock).

    async fn orgnumber(pool: &PgPool, company: Uuid) -> String {
        sqlx::query_scalar("select orgnr from company where id = $1")
            .bind(company)
            .fetch_one(pool)
            .await
            .unwrap()
    }

    struct Fetched<'a> {
        trekkid: &'a str,
        version: i32,
        debtor: &'a str,
        kid: &'a str,
        created: &'a str,
        status: &'a str,
        /// The size of the period from March on; January–February is 1 000 kr.
        size: serde_json::Value,
    }

    impl Fetched<'_> {
        fn json(&self, employer: &str) -> serde_json::Value {
            let mut march =
                serde_json::json!({"startdato": "2026-03-01", "sluttdato": "2026-12-31"});
            march
                .as_object_mut()
                .unwrap()
                .extend(self.size.as_object().unwrap().clone());
            serde_json::json!({
                "trekkid": self.trekkid,
                "skyldner": self.debtor,
                "opprettet": self.created,
                "saksnummer": format!("TREKK/2026/{}", self.trekkid),
                "trekkstatus": self.status,
                "trekkpliktig": employer,
                "trekkversjon": self.version,
                "sekvensnummer": 100 + self.version,
                "betalingsinformasjon": {
                    "kidnummer": self.kid,
                    "kontonummer": ACCOUNT,
                    "betalingsmottaker": PAYEE,
                },
                "trekkstoerrelseForPeriode": [
                    { "startdato": "2026-01-01", "sluttdato": "2026-02-28",
                      "trekkbeloep": { "trekkbeloep": 1000 } },
                    march,
                ],
            })
        }
    }

    fn amount(kroner: &str) -> serde_json::Value {
        serde_json::json!({"trekkbeloep": {
            "trekkbeloep": serde_json::from_str::<serde_json::Value>(kroner).unwrap()}})
    }

    fn order<'a>(trekkid: &'a str, version: i32, debtor: &'a str) -> Fetched<'a> {
        Fetched {
            trekkid,
            version,
            debtor,
            kid: KID,
            created: "2026-02-10T15:00:00.00Z",
            status: "aktiv",
            size: amount("4000"),
        }
    }

    async fn store(pool: &PgPool, company: Uuid, orders: &[Fetched<'_>]) -> g::FetchSummary {
        let employer = orgnumber(pool, company).await;
        let list = serde_json::Value::Array(orders.iter().map(|o| o.json(&employer)).collect());
        let fetch = g::record_fetch(pool, company, "Test").await.unwrap();
        g::store_list(pool, company, fetch, &list).await.unwrap()
    }

    const KARI: &str = "26829398612";

    /// §2 steps 1–7 short of the HTTP call: fetched, stored verbatim,
    /// withheld from the period covering the pay date, booked with the
    /// order's KID, reported and shown on the payslip by trekkid.
    #[tokio::test]
    async fn a_fetched_order_is_withheld_booked_and_reported() {
        let Some(pool) = pool().await else { return };
        let company = company(&pool).await;
        let kari = employee(&pool, company, KARI, "Kari Utvikler", 5_000_000).await;
        payroll::update_employment_details(&pool, company, kari, &complete_details("974760673"))
            .await
            .unwrap();
        let summary = store(&pool, company, &[order("20001", 1, KARI)]).await;
        assert_eq!((summary.listed, summary.new_versions), (1, 1));
        // The same list again stores nothing new.
        let again = store(&pool, company, &[order("20001", 1, KARI)]).await;
        assert_eq!((again.listed, again.new_versions), (1, 0));

        let poster = [entry(kari)];
        let shown = preview(&pool, company, &poster).await.unwrap();
        assert_eq!(
            shown.garnishment_set,
            vec![g::Key::Fetched("20001".into(), 1)]
        );
        // The March period says 4 000 kr, not January's 1 000.
        assert_eq!(shown.lines[0].garnishments[0].withheld_ore, 400_000);

        let run = book(&pool, company, &poster, Some(&shown.garnishment_set))
            .await
            .unwrap();
        assert_eq!(
            account_amount(&pool, run.voucher_id, "2610").await,
            -400_000
        );
        let fetched_id: Option<Uuid> =
            sqlx::query_scalar("select fetched_id from payroll_garnishment where run_id = $1")
                .bind(run.id)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert!(fetched_id.is_some());
        let items = regnmed_db::payments::payable_items(&pool, company)
            .await
            .unwrap();
        let item = items
            .iter()
            .find(|i| i.kid.as_deref() == Some(KID))
            .unwrap();
        assert_eq!(item.amount_ore, 400_000);
        assert_eq!(item.due_date, Some(date(2026, 3, 25)));

        let draft = payroll::amelding_for_run(&pool, company, run.id, chrono::Utc::now())
            .await
            .unwrap();
        let xml = regnmed_core::amelding::render(&draft.melding).unwrap();
        xsd_valid(&xml);
        let flat: String = xml.lines().map(str::trim).collect();
        assert!(
            flat.contains("<beskrivelse>utleggstrekkSamordnet</beskrivelse><beloep>-4000</beloep>"),
            "{flat}"
        );
        let slip = payroll::payslip(&pool, company, run.id, kari)
            .await
            .unwrap();
        assert!(slip.deductions[0].text.contains("trekkpålegg 20001"));
    }

    /// §3 point 2 for fetched orders: a new version after the preview
    /// stops the run until it is previewed again.
    #[tokio::test]
    async fn a_new_version_after_the_preview_stops_the_run() {
        let Some(pool) = pool().await else { return };
        let company = company(&pool).await;
        let kari = employee(&pool, company, KARI, "Kari Utvikler", 5_000_000).await;
        store(&pool, company, &[order("20002", 1, KARI)]).await;
        let poster = [entry(kari)];
        let shown = preview(&pool, company, &poster).await.unwrap();

        let mut v2 = order("20002", 2, KARI);
        v2.size = amount("4500.50");
        store(&pool, company, &[v2]).await;
        let err = book(&pool, company, &poster, Some(&shown.garnishment_set))
            .await
            .unwrap_err();
        assert!(err.to_string().contains("forhåndsvis"), "{err}");
        let again = preview(&pool, company, &poster).await.unwrap();
        assert_eq!(
            again.garnishment_set,
            vec![g::Key::Fetched("20002".into(), 2)]
        );
        // Rounded down to whole kroner, like a hand entry.
        assert_eq!(again.lines[0].garnishments[0].withheld_ore, 450_000);
        book(&pool, company, &poster, Some(&again.garnishment_set))
            .await
            .unwrap();
    }

    /// §6: the same KID means the letter and the API describe one order --
    /// the fetched one takes over, derived, with nothing written.
    #[tokio::test]
    async fn a_fetched_order_with_the_same_kid_replaces_the_hand_entry() {
        let Some(pool) = pool().await else { return };
        let company = company(&pool).await;
        let kari = employee(&pool, company, KARI, "Kari Utvikler", 5_000_000).await;
        let by = person(&pool, "Lønnsansvarlig").await;
        let hand = register(&pool, company, kari, coordinated(300_000), by).await;
        let poster = [entry(kari)];
        let before = preview(&pool, company, &poster).await.unwrap();
        assert_eq!(before.garnishment_set, vec![g::Key::Registered(hand, 1)]);

        store(&pool, company, &[order("20003", 1, KARI)]).await;
        let shown = preview(&pool, company, &poster).await.unwrap();
        assert_eq!(
            shown.garnishment_set,
            vec![g::Key::Fetched("20003".into(), 1)]
        );
        assert_eq!(shown.lines[0].garnishments.len(), 1);
        assert!(
            shown.warnings.iter().any(|w| w.contains("avløst")),
            "{:?}",
            shown.warnings
        );
        assert!(!shown.warnings.iter().any(|w| w.starts_with("Avvik")));
        // The hand entry is still registered and active: nothing was written.
        let orders = g::list(&pool, company).await.unwrap();
        assert_eq!(orders[0].status(), "aktiv");
        assert_eq!(orders[0].versions.len(), 1);
        // The set from before the fetch no longer books.
        let err = book(&pool, company, &poster, Some(&before.garnishment_set))
            .await
            .unwrap_err();
        assert!(err.to_string().contains("forhåndsvis"), "{err}");
    }

    /// §6: another KID (an old-scheme trekk next to a new coordinated one)
    /// is an anomaly on both, and only the fetched one is withheld.
    #[tokio::test]
    async fn a_different_kid_is_an_anomaly_and_the_fetched_order_is_withheld() {
        let Some(pool) = pool().await else { return };
        let company = company(&pool).await;
        let kari = employee(&pool, company, KARI, "Kari Utvikler", 5_000_000).await;
        let by = person(&pool, "Lønnsansvarlig").await;
        let mut other_kid = coordinated(300_000);
        other_kid.kid = Some("12345678903".into());
        register(&pool, company, kari, other_kid, by).await;
        register(&pool, company, kari, tax(1000), by).await;

        store(&pool, company, &[order("20004", 1, KARI)]).await;
        let shown = preview(&pool, company, &[entry(kari)]).await.unwrap();
        assert_eq!(
            shown.garnishment_set,
            vec![g::Key::Fetched("20004".into(), 1)]
        );
        let anomalies: Vec<&String> = shown
            .warnings
            .iter()
            .filter(|w| w.starts_with("Avvik") && w.contains("20004"))
            .collect();
        assert_eq!(anomalies.len(), 2, "{:?}", shown.warnings);
        assert!(anomalies.iter().any(|w| w.contains("12345678903")));
        assert!(anomalies.iter().any(|w| w.contains("utleggstrekk skatt")));
    }

    /// §2 steps 1 and 3: what the list cannot back is shown, never
    /// withheld -- an order missing from the newest list, one nobody in
    /// the register owes, an unreadable size, payment details that cannot
    /// be paid, and an ended order. An order that is itself an anomaly
    /// replaces no hand entry: Kari's registration is still withheld.
    #[tokio::test]
    async fn what_cannot_be_withheld_is_shown_as_an_anomaly() {
        let Some(pool) = pool().await else { return };
        let company = company(&pool).await;
        let kari = employee(&pool, company, KARI, "Kari Utvikler", 5_000_000).await;
        let ola = employee(&pool, company, "15929998666", "Ola Testesen", 5_000_000).await;
        let per = employee(&pool, company, "20857597940", "Per Testesen", 5_000_000).await;
        let by = person(&pool, "Lønnsansvarlig").await;
        let hand = register(&pool, company, kari, coordinated(300_000), by).await;
        store(&pool, company, &[order("20005", 1, KARI)]).await;
        let mut unreadable = order("20007", 1, "15929998666");
        unreadable.size = serde_json::json!({"trekkprosent": {"trekkprosent": 12.345}});
        let mut ended = order("20008", 1, "20857597940");
        ended.status = "avsluttet";
        let bad_kid = Fetched {
            kid: "6487719757",
            ..order("20013", 1, "20857597940")
        };
        // The newest list no longer has 20005.
        store(
            &pool,
            company,
            &[order("20006", 1, "01010112345"), unreadable, ended, bad_kid],
        )
        .await;

        let shown = preview(&pool, company, &[entry(kari), entry(ola), entry(per)])
            .await
            .unwrap();
        assert_eq!(
            shown.garnishment_set,
            vec![g::Key::Registered(hand, 1)],
            "{:?}",
            shown.warnings
        );
        let says = |needle: &str| shown.warnings.iter().any(|w| w.contains(needle));
        assert!(
            says("20005 er aktivt i regnmed, men var ikke med i siste liste"),
            "{:?}",
            shown.warnings
        );
        assert!(says("20006 (sak TREKK/2026/20006)"), "{:?}", shown.warnings);
        assert!(
            says("20007 versjon 1 har en trekkstørrelse"),
            "{:?}",
            shown.warnings
        );
        assert!(
            says("20013 har betalingsinformasjon"),
            "{:?}",
            shown.warnings
        );
        assert!(!says("20008"), "{:?}", shown.warnings);
        assert!(!says("avløst"), "{:?}", shown.warnings);
    }

    /// §2 step 3: two active orders on one employee -- the one created last
    /// is withheld, by its EARLIEST stored `opprettet`.
    #[tokio::test]
    async fn of_two_active_orders_the_one_created_last_is_withheld() {
        let Some(pool) = pool().await else { return };
        let company = company(&pool).await;
        let kari = employee(&pool, company, KARI, "Kari Utvikler", 5_000_000).await;
        let older = Fetched {
            created: "2026-01-05T10:00:00Z",
            ..order("20009", 1, KARI)
        };
        let newer = || Fetched {
            created: "2026-02-05T10:00:00Z",
            ..order("20010", 1, KARI)
        };
        store(&pool, company, &[older, newer()]).await;
        // A new version of the older order carries a later `opprettet`, but
        // the order was still created first.
        let older_v2 = Fetched {
            created: "2026-03-01T10:00:00Z",
            ..order("20009", 2, KARI)
        };
        store(&pool, company, &[older_v2, newer()]).await;
        let shown = preview(&pool, company, &[entry(kari)]).await.unwrap();
        assert_eq!(
            shown.garnishment_set,
            vec![g::Key::Fetched("20010".into(), 1)]
        );
        assert!(
            shown
                .warnings
                .iter()
                .any(|w| w.contains("to aktive") && w.contains("20009")),
            "{:?}",
            shown.warnings
        );
    }

    /// §3 point 3: a list older than three days is warned about.
    #[tokio::test]
    async fn a_stale_list_is_warned_about() {
        let Some(pool) = pool().await else { return };
        let company = company(&pool).await;
        let kari = employee(&pool, company, KARI, "Kari Utvikler", 5_000_000).await;
        let fetch = Uuid::now_v7();
        sqlx::query(
            "insert into garnishment_fetch (id, company_id, requested_by, created_at)
             values ($1, $2, 'Test', now() - interval '4 days')",
        )
        .bind(fetch)
        .bind(company)
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "insert into garnishment_fetch_outcome (fetch_id, ok, listed, created_at)
             values ($1, true, '{}', now() - interval '4 days')",
        )
        .bind(fetch)
        .execute(&pool)
        .await
        .unwrap();
        let shown = preview(&pool, company, &[entry(kari)]).await.unwrap();
        assert!(
            shown.warnings.iter().any(|w| w.contains("sist hentet")),
            "{:?}",
            shown.warnings
        );
        store(&pool, company, &[]).await;
        let fresh = preview(&pool, company, &[entry(kari)]).await.unwrap();
        assert!(!fresh.warnings.iter().any(|w| w.contains("sist hentet")));
    }

    /// A list for another employer is refused whole and recorded as a
    /// failure; and what was stored cannot be changed.
    #[tokio::test]
    async fn a_foreign_list_is_refused_and_the_store_is_insert_only() {
        let Some(pool) = pool().await else { return };
        let company = company(&pool).await;
        let fetch = g::record_fetch(&pool, company, "Test").await.unwrap();
        let foreign = serde_json::Value::Array(vec![order("20011", 1, KARI).json("123456785")]);
        let err = g::store_list(&pool, company, fetch, &foreign)
            .await
            .unwrap_err();
        assert!(err.to_string().contains("trekkpliktig"), "{err}");
        // An unreadable list fails whole too. Neither failure shows the
        // orders' content: the detail is read in the portal, and the
        // content carries the employee's fødselsnummer.
        let fetch = g::record_fetch(&pool, company, "Test").await.unwrap();
        let employer = orgnumber(&pool, company).await;
        let mut unreadable = order("20014", 1, KARI).json(&employer);
        unreadable.as_object_mut().unwrap().remove("trekkversjon");
        let err = g::store_list(&pool, company, fetch, &serde_json::json!([unreadable]))
            .await
            .unwrap_err();
        assert!(err.to_string().contains("20014"), "{err}");
        let attempts = g::fetch_attempts(&pool, company).await.unwrap();
        assert_eq!(attempts.len(), 2);
        for a in &attempts {
            assert_eq!(a.ok, Some(false));
            let detail = a.detail.as_deref().unwrap();
            assert!(!detail.contains(KARI), "{detail}");
        }
        let kept: Option<serde_json::Value> =
            sqlx::query_scalar("select answer from garnishment_fetch_outcome where fetch_id = $1")
                .bind(fetch)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert!(
            kept.unwrap().to_string().contains(KARI),
            "the evidence is kept"
        );
        let (orders, last) = g::fetched(&mut pool.acquire().await.unwrap(), company)
            .await
            .unwrap();
        assert!(orders.is_empty() && last.is_none());

        store(&pool, company, &[order("20012", 1, KARI)]).await;
        for sql in [
            "update garnishment_fetched set raw = '{}' where company_id = $1",
            "delete from garnishment_fetched where company_id = $1",
            "delete from garnishment_fetch_outcome where fetch_id in
                 (select id from garnishment_fetch where company_id = $1)",
        ] {
            assert!(
                sqlx::query(sql).bind(company).execute(&pool).await.is_err(),
                "{sql}"
            );
        }
    }
}
