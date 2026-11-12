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
            from_timesheet: false,
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
            from_timesheet: false,
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
            from_timesheet: false,
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

#[tokio::test]
async fn december_withholds_half() {
    let Some(pool) = pool().await else { return };
    let company = company(&pool).await;
    let a = employee(&pool, company, "25927898821", "Nils Desember", 5_000_000).await;

    let run = payroll::run_payroll(
        &pool,
        company,
        2026,
        12,
        date(2026, 12, 15),
        "I",
        &[PayrollEntry {
            employee_id: a,
            gross_ore: None,
            holiday_pay_ore: 0,
            from_timesheet: false,
        }],
        "Test",
    )
    .await
    .unwrap();
    assert_eq!(run.sum.withholding_ore, 875_000, "halv skatt i desember");
    assert_eq!(voucher_sum(&pool, run.voucher_id).await, 0);
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
            from_timesheet: false,
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
            from_timesheet: false,
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
        from_timesheet: false,
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
            from_timesheet: false,
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
            from_timesheet: false,
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
            from_timesheet: true,
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
            from_timesheet: false,
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
                    from_timesheet: false,
                },
                PayrollEntry {
                    employee_id: b,
                    gross_ore: None,
                    holiday_pay_ore: holiday_pay,
                    from_timesheet: false,
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
            from_timesheet: false,
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
        from_timesheet: false,
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
    assert!(all.contains("Per Ulønnet var ansatt i 03/2026"), "{all}");
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

    let first = sub::prepare_submission(&pool, company, run, false, "Test")
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
    let retry = sub::prepare_submission(&pool, company, run, false, "Test")
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
    let err = sub::prepare_submission(&pool, company, run, false, "Test")
        .await
        .unwrap_err()
        .to_string();
    assert!(err.contains("allerede levert"), "{err}");
    let replacement = sub::prepare_submission(&pool, company, run, true, "Test")
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
    let list = sub::list_submissions(&pool, company, run).await.unwrap();
    assert_eq!(list.len(), 2);
    assert_eq!(list[0].0.id, replacement.id, "newest first");
    assert_eq!(list[1].1.slug(), "tilbakemelding");
}

#[tokio::test]
async fn the_submission_record_is_append_only_and_needs_a_buildable_run() {
    use regnmed_db::amelding_submission as sub;
    let Some(pool) = pool().await else { return };
    let (first_company, run) = submittable_run(&pool).await;
    let s = sub::prepare_submission(&pool, first_company, run, false, "Test")
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
    let err = sub::prepare_submission(&pool, company2, run2.id, false, "Test")
        .await
        .unwrap_err()
        .to_string();
    assert!(err.contains("arbeidsforholdet mangler"), "{err}");
    assert!(
        sub::list_submissions(&pool, company2, run2.id)
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
        sub::prepare_submission(&pool, company, run, false, "Test"),
        sub::prepare_submission(&pool, company, run, false, "Test"),
    );
    assert_eq!(a.unwrap().id, b.unwrap().id);
    assert_eq!(
        sub::list_submissions(&pool, company, run)
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
        "d-1",
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
        from_timesheet: false,
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
        from_timesheet: false,
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
        "d-1",
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
