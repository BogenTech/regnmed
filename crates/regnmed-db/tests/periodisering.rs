//! Periodisering end to end (#87): a prepaid cost is spread over the
//! months it belongs to, the parts sum exactly to the total, a month
//! cannot be posted twice, and stopping leaves what was posted alone.
//! Requires DATABASE_URL (skips otherwise).

use chrono::NaiveDate;
use regnmed_core::Ore;
use regnmed_core::voucher::{EntryDraft, VoucherDraft};
use regnmed_db::accrual::{AccrualDraft, accrue_plan, create_accrual};
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

#[tokio::test]
async fn a_prepaid_cost_is_spread_over_its_months_and_sums_exactly() {
    let Some(pool) = pool().await else { return };
    let company = regnmed_db::create_company(&pool, &unique_orgnumber(), "Periodisering AS")
        .await
        .unwrap();
    regnmed_db::ensure_journal(&pool, company, "GL", "Hovedbok")
        .await
        .unwrap();
    for (number, name) in [
        ("1700", "Forskuddsbetalt kostnad"),
        ("1920", "Bank"),
        ("6300", "Leie lokale"),
    ] {
        regnmed_db::ensure_account(&pool, company, number, name)
            .await
            .unwrap();
    }

    // The source bilag: a year of rent paid in January, parked on 1700.
    // 10 000,01 kr so the split cannot come out even.
    let total = 1_000_001;
    let source = regnmed_db::post_voucher(
        &pool,
        company,
        &VoucherDraft {
            journal_code: "GL".into(),
            voucher_date: d(2026, 1, 15),
            description: "Husleie 2026, forskuddsbetalt".into(),
            reverses: None,
            entries: vec![
                EntryDraft {
                    account_number: "1700".into(),
                    amount: Ore(total),
                    vat_code: None,
                    description: None,
                    party_no: None,
                    department: None,
                    project: None,
                    currency: None,
                },
                EntryDraft {
                    account_number: "1920".into(),
                    amount: Ore(-total),
                    vat_code: None,
                    description: None,
                    party_no: None,
                    department: None,
                    project: None,
                    currency: None,
                },
            ],
        },
        "test",
    )
    .await
    .unwrap();

    let plan = create_accrual(
        &pool,
        company,
        &AccrualDraft {
            kilde_voucher: Some(source.id),
            description: "Husleie 2026".into(),
            income_account: "6300".into(),
            balance_account: "1700".into(),
            total_ore: total,
            from: (2026, 1),
            to: (2026, 12),
            department: None,
            project: None,
            note: None,
        },
        "test",
    )
    .await
    .unwrap();

    // Run as if it were 31 March: three months are due, nine are not.
    let outcome = accrue_plan(&pool, company, plan, d(2026, 3, 31))
        .await
        .unwrap();
    assert_eq!(outcome.len(), 3, "{outcome:?}");
    assert!(outcome.iter().all(|u| u.voucher.is_some()), "{outcome:?}");
    assert_eq!(balance(&pool, company, "6300").await, 249_999, "3 × 833,33");
    assert_eq!(
        balance(&pool, company, "1700").await,
        total - 249_999,
        "resten står fortsatt parkert i balansen"
    );

    // Running again changes nothing — the partial unique index decides,
    // not a flag we remembered to set.
    let igjen = accrue_plan(&pool, company, plan, d(2026, 3, 31))
        .await
        .unwrap();
    assert!(igjen.is_empty(), "{igjen:?}");
    assert_eq!(balance(&pool, company, "6300").await, 249_999);

    // The whole year: the parts sum EXACTLY to the total, and 1700 is
    // emptied to the øre — the property the feature stands on.
    accrue_plan(&pool, company, plan, d(2026, 12, 31))
        .await
        .unwrap();
    assert_eq!(balance(&pool, company, "6300").await, total);
    assert_eq!(balance(&pool, company, "1700").await, 0, "på øret");

    regnmed_db::verify_chain(&pool, company).await.unwrap();
}

#[tokio::test]
async fn stopping_leaves_posted_months_alone_and_posts_no_more() {
    let Some(pool) = pool().await else { return };
    let company = regnmed_db::create_company(&pool, &unique_orgnumber(), "Stopp AS")
        .await
        .unwrap();
    regnmed_db::ensure_journal(&pool, company, "GL", "Hovedbok")
        .await
        .unwrap();
    for (number, name) in [("1700", "Forskuddsbetalt"), ("6300", "Leie")] {
        regnmed_db::ensure_account(&pool, company, number, name)
            .await
            .unwrap();
    }
    let plan = create_accrual(
        &pool,
        company,
        &AccrualDraft {
            kilde_voucher: None,
            description: "Forsikring".into(),
            income_account: "6300".into(),
            balance_account: "1700".into(),
            total_ore: 120_000,
            from: (2026, 1),
            to: (2026, 12),
            department: None,
            project: None,
            note: None,
        },
        "test",
    )
    .await
    .unwrap();
    accrue_plan(&pool, company, plan, d(2026, 2, 28))
        .await
        .unwrap();
    assert_eq!(balance(&pool, company, "6300").await, 20_000);

    regnmed_db::accrual::stop_accrual(&pool, company, plan, d(2026, 3, 1))
        .await
        .unwrap();
    // Even asked for the whole year, nothing more is posted.
    let after = accrue_plan(&pool, company, plan, d(2026, 12, 31))
        .await
        .unwrap();
    assert!(after.is_empty(), "{after:?}");
    assert_eq!(
        balance(&pool, company, "6300").await,
        20_000,
        "det som er ført står"
    );
}

/// A started plan is history: the amounts already posted refer to it, so
/// changing the total afterwards would break the sum. Enforced by the
/// trigger, not only by the code.
#[tokio::test]
async fn a_started_plan_cannot_be_rewritten() {
    let Some(pool) = pool().await else { return };
    let company = regnmed_db::create_company(&pool, &unique_orgnumber(), "Frys AS")
        .await
        .unwrap();
    regnmed_db::ensure_journal(&pool, company, "GL", "Hovedbok")
        .await
        .unwrap();
    for (number, name) in [("1700", "Forskuddsbetalt"), ("6300", "Leie")] {
        regnmed_db::ensure_account(&pool, company, number, name)
            .await
            .unwrap();
    }
    let plan = create_accrual(
        &pool,
        company,
        &AccrualDraft {
            kilde_voucher: None,
            description: "Abonnement".into(),
            income_account: "6300".into(),
            balance_account: "1700".into(),
            total_ore: 60_000,
            from: (2026, 1),
            to: (2026, 6),
            department: None,
            project: None,
            note: None,
        },
        "test",
    )
    .await
    .unwrap();
    accrue_plan(&pool, company, plan, d(2026, 1, 31))
        .await
        .unwrap();

    let error = sqlx::query("update periodisering set total_ore = 99_000 where id = $1")
        .bind(plan)
        .execute(&pool)
        .await
        .expect_err("en påbegynt plan skal ikke kunne endres");
    assert!(
        error.to_string().contains("påbegynt"),
        "feilmeldingen skal si hvorfor: {error}"
    );

    // …but stopping it is still allowed, or a plan could never be ended.
    regnmed_db::accrual::stop_accrual(&pool, company, plan, d(2026, 2, 1))
        .await
        .unwrap();
}

/// A plan is a standing instruction, so a bad account must fail when it
/// is created — not once a month for a year, where only the run log
/// would see it. Found in browser verification (#87).
#[tokio::test]
async fn a_plan_refuses_an_account_it_could_never_post_to() {
    let Some(pool) = pool().await else { return };
    let company = regnmed_db::create_company(&pool, &unique_orgnumber(), "Kontosjekk AS")
        .await
        .unwrap();
    regnmed_db::ensure_journal(&pool, company, "GL", "Hovedbok")
        .await
        .unwrap();
    for (number, name) in [
        ("1500", "Kundefordringer"),
        ("1700", "Forskuddsbetalt"),
        ("6300", "Leie"),
    ] {
        regnmed_db::ensure_account(&pool, company, number, name)
            .await
            .unwrap();
    }
    regnmed_db::set_account_subledger(&pool, company, "1500", Some("kunde"))
        .await
        .unwrap();

    let plan = |result: &str, balance_sheet: &str| AccrualDraft {
        kilde_voucher: None,
        description: "Husleie".into(),
        income_account: result.into(),
        balance_account: balance_sheet.into(),
        total_ore: 30_000,
        from: (2026, 1),
        to: (2026, 3),
        department: None,
        project: None,
        note: None,
    };

    let error = create_accrual(&pool, company, &plan("6300", "1500"), "test")
        .await
        .expect_err("reskontrokonto kan aldri lykkes uten part");
    assert!(error.to_string().contains("reskontrokonto"), "{error}");

    let error = create_accrual(&pool, company, &plan("6300", "9999"), "test")
        .await
        .expect_err("ukjent konto");
    assert!(error.to_string().contains("finnes ikke"), "{error}");

    // The good one still works.
    create_accrual(&pool, company, &plan("6300", "1700"), "test")
        .await
        .unwrap();
}
