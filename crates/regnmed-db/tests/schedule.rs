//! The general scheduler (docs/jobs.md «Tidsplaner», migration 0097).
//! Requires DATABASE_URL (skips otherwise).
//!
//! What must not break: a due schedule becomes exactly one job however many
//! workers look at once; nothing runs early, paused, or on top of a run that
//! has not finished; time that was missed is not made up for; and a schedule
//! with nothing to do, or for a blocked company, writes no job at all.

use chrono::{DateTime, NaiveTime, Utc};
use regnmed_db::job::Kind;
use regnmed_db::schedule;
use uuid::Uuid;

async fn setup() -> Option<(sqlx::PgPool, Uuid)> {
    dotenvy::dotenv().ok();
    let Ok(url) = std::env::var("DATABASE_URL") else {
        eprintln!("skipping: DATABASE_URL not set");
        return None;
    };
    let pool = regnmed_db::connect(&url).await.unwrap();
    regnmed_db::MIGRATOR.run(&pool).await.unwrap();
    let n = u32::from_be_bytes(Uuid::new_v4().as_bytes()[..4].try_into().unwrap());
    let company =
        regnmed_db::create_company(&pool, &format!("{:09}", n % 1_000_000_000), "Tidsplan AS")
            .await
            .unwrap();
    Some((pool, company))
}

/// What `Rules::applies` asks of a bank schedule: a live connection and the add-on.
async fn connect_bank(pool: &sqlx::PgPool, company: Uuid) {
    let account = Uuid::new_v4();
    sqlx::query(
        "insert into account (id, company_id, number, name) values ($1, $2, '1920', 'Bank')",
    )
    .bind(account)
    .bind(company)
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(
        "insert into bank_connection (id, company_id, account_id, provider, bank_name, bban,
                                      session_id, external_account_id, valid_until, created_by)
         values ($1, $2, $3, 'mock', 'Testbanken', '12345678903', 's', 'a',
                 now() + interval '90 days', 'test')",
    )
    .bind(Uuid::new_v4())
    .bind(company)
    .bind(account)
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(
        "insert into company_addon (company_id, addon, note, created_by)
         values ($1, 'bank', 'test', 'test')",
    )
    .bind(company)
    .execute(pool)
    .await
    .unwrap();
}

async fn jobs(pool: &sqlx::PgPool, company: Uuid) -> i64 {
    sqlx::query_scalar("select count(*) from job where company_id = $1 and kind = 'bank_sync'")
        .bind(company)
        .fetch_one(pool)
        .await
        .unwrap()
}

async fn make_due(pool: &sqlx::PgPool, company: Uuid, ago: &str) {
    sqlx::query(&format!(
        "update schedule set next_run_at = now() - interval '{ago}' where company_id = $1"
    ))
    .bind(company)
    .execute(pool)
    .await
    .unwrap();
}

/// Runs what is due and waits until this company's schedule has moved on. The
/// database is shared with tests running in parallel, and one of THEIR calls
/// may be the one that takes this schedule — its job is then visible only when
/// that transaction commits, together with the new `next_run_at`.
async fn run_due(pool: &sqlx::PgPool, company: Uuid) {
    for _ in 0..250 {
        schedule::enqueue_due(pool).await.unwrap();
        let due: bool = sqlx::query_scalar(
            "select exists (select 1 from schedule
                            where company_id = $1 and active and next_run_at <= now())",
        )
        .bind(company)
        .fetch_one(pool)
        .await
        .unwrap();
        if !due {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    panic!("the schedule never moved on");
}

#[tokio::test]
async fn the_next_run_is_the_next_of_the_times_in_norwegian_time() {
    let Some((pool, _)) = setup().await else {
        return;
    };
    let next = |times: &'static str, after: &'static str| {
        let pool = pool.clone();
        async move {
            sqlx::query_scalar::<_, DateTime<Utc>>(&format!(
                "select schedule_next('{times}'::time[], '{after}'::timestamptz)"
            ))
            .fetch_one(&pool)
            .await
            .unwrap()
            .to_rfc3339()
        }
    };
    // Winter: Norway is UTC+1, so 03:15 local is 02:15 UTC.
    assert_eq!(
        next("{03:15}", "2026-01-10T00:00:00Z").await,
        "2026-01-10T02:15:00+00:00"
    );
    // Strictly after: at 02:15 UTC the next one is tomorrow's.
    assert_eq!(
        next("{03:15}", "2026-01-10T02:15:00Z").await,
        "2026-01-11T02:15:00+00:00"
    );
    // Several times a day: the nearest one that is still ahead.
    assert_eq!(
        next("{03:15,12:00}", "2026-01-10T05:00:00Z").await,
        "2026-01-10T11:00:00+00:00"
    );
    // Summer: UTC+2, so the same wall-clock time is an hour earlier in UTC.
    assert_eq!(
        next("{03:15}", "2026-07-10T00:00:00Z").await,
        "2026-07-10T01:15:00+00:00"
    );
}

#[tokio::test]
async fn a_due_schedule_becomes_exactly_one_job() {
    let Some((pool, company)) = setup().await else {
        return;
    };
    connect_bank(&pool, company).await;
    schedule::ensure(&pool, company, Kind::BankSync, "test")
        .await
        .unwrap();
    let row = &schedule::for_company(&pool, company).await.unwrap()[0];
    assert_eq!(row.times, vec![schedule::default_time(company)]);
    assert!(
        row.next_run_at > Utc::now(),
        "a new schedule is never due at once"
    );

    // Not due: nothing happens.
    schedule::enqueue_due(&pool).await.unwrap();
    assert_eq!(jobs(&pool, company).await, 0);

    // Due, and sixteen workers look at the same moment: one job.
    make_due(&pool, company, "1 minute").await;
    let lookers = (0..16).map(|_| {
        let pool = pool.clone();
        tokio::spawn(async move { schedule::enqueue_due(&pool).await.unwrap() })
    });
    for l in lookers {
        l.await.unwrap();
    }
    run_due(&pool, company).await;
    assert_eq!(
        jobs(&pool, company).await,
        1,
        "exactly one job for one due run"
    );
    let row = &schedule::for_company(&pool, company).await.unwrap()[0];
    assert!(
        row.next_run_at > Utc::now(),
        "the schedule moved on in the same transaction"
    );
    // Not its status: another test's worker may already have picked the job up.
    assert!(row.last.is_some(), "the schedule shows its latest run");

    // Due again while that job has not finished: no second one on top of it.
    make_due(&pool, company, "1 minute").await;
    run_due(&pool, company).await;
    assert_eq!(jobs(&pool, company).await, 1);

    // The job finishes; three days pass with no worker. One run, not three.
    sqlx::query("update job set status = 'done', finished_at = now() where company_id = $1")
        .bind(company)
        .execute(&pool)
        .await
        .unwrap();
    make_due(&pool, company, "3 days").await;
    run_due(&pool, company).await;
    schedule::enqueue_due(&pool).await.unwrap();
    assert_eq!(
        jobs(&pool, company).await,
        2,
        "missed time is not made up for"
    );
}

#[tokio::test]
async fn nothing_to_do_or_a_blocked_company_writes_no_job() {
    let Some((pool, company)) = setup().await else {
        return;
    };
    schedule::ensure(&pool, company, Kind::BankSync, "test")
        .await
        .unwrap();

    // No bank connection and no add-on: the schedule moves on, and no job is written.
    make_due(&pool, company, "1 minute").await;
    run_due(&pool, company).await;
    assert_eq!(jobs(&pool, company).await, 0, "nothing to fetch from");
    let row = &schedule::for_company(&pool, company).await.unwrap()[0];
    assert!(row.next_run_at > Utc::now(), "it moved on all the same");

    // A blocked subscription: the connection is there, but nothing is changed for the company.
    connect_bank(&pool, company).await;
    sqlx::query("update company set created_at = now() - interval '120 days' where id = $1")
        .bind(company)
        .execute(&pool)
        .await
        .unwrap();
    assert!(
        regnmed_db::subscription::blocked(&pool, company)
            .await
            .unwrap()
    );
    make_due(&pool, company, "1 minute").await;
    run_due(&pool, company).await;
    assert_eq!(
        jobs(&pool, company).await,
        0,
        "a blocked company is left alone"
    );

    // Covered again: the same schedule starts by itself.
    sqlx::query("update company set created_at = now() where id = $1")
        .bind(company)
        .execute(&pool)
        .await
        .unwrap();
    make_due(&pool, company, "1 minute").await;
    run_due(&pool, company).await;
    assert_eq!(jobs(&pool, company).await, 1);

    // The consent runs out: back to no job, without anyone pausing anything.
    sqlx::query("update job set status = 'done', finished_at = now() where company_id = $1")
        .bind(company)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query(
        "update bank_connection set valid_until = now() - interval '1 hour' where company_id = $1",
    )
    .bind(company)
    .execute(&pool)
    .await
    .unwrap();
    make_due(&pool, company, "1 minute").await;
    run_due(&pool, company).await;
    assert_eq!(
        jobs(&pool, company).await,
        1,
        "an expired consent fetches nothing"
    );
}

#[tokio::test]
async fn a_paused_schedule_does_not_run_and_ensure_leaves_it_alone() {
    let Some((pool, company)) = setup().await else {
        return;
    };
    connect_bank(&pool, company).await;
    schedule::ensure(&pool, company, Kind::BankSync, "test")
        .await
        .unwrap();
    let noon = vec![NaiveTime::from_hms_opt(12, 0, 0).unwrap()];
    assert!(
        schedule::set(&pool, company, Kind::BankSync, &noon, false, "admin")
            .await
            .unwrap()
    );
    make_due(&pool, company, "1 minute").await;
    run_due(&pool, company).await;
    assert_eq!(jobs(&pool, company).await, 0, "paused means paused");

    // A renewed bank connection calls ensure again: the company's choice stands.
    schedule::ensure(&pool, company, Kind::BankSync, "someone else")
        .await
        .unwrap();
    let row = &schedule::for_company(&pool, company).await.unwrap()[0];
    assert!(!row.active);
    assert_eq!(row.times, noon);

    // A kind that cannot be scheduled gets no schedule, and a company without one is told so.
    assert!(
        schedule::ensure(&pool, company, Kind::AuditReport, "test")
            .await
            .is_err()
    );
    let other = Uuid::new_v4();
    assert!(
        !schedule::set(&pool, other, Kind::BankSync, &noon, true, "x")
            .await
            .unwrap()
    );
}
