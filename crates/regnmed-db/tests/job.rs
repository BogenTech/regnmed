//! Background jobs under parallel workers (docs/jobs.md, migration 0071).
//! Requires DATABASE_URL (skips otherwise).

use regnmed_db::job::{self, Kind};
use std::collections::HashSet;
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
        regnmed_db::create_company(&pool, &format!("{:09}", n % 1_000_000_000), "Jobb AS")
            .await
            .unwrap();
    Some((pool, company))
}

async fn new_job(pool: &sqlx::PgPool, company: Uuid) -> Uuid {
    job::create(pool, company, Kind::AuditReport, None, "test", None)
        .await
        .unwrap()
}

#[tokio::test]
async fn parallel_workers_claim_each_job_exactly_once() {
    let Some((pool, company)) = setup().await else {
        return;
    };
    let mut ids = HashSet::new();
    for _ in 0..150 {
        ids.insert(new_job(&pool, company).await);
    }

    // Sixteen sweeping workers at once, each taking jobs until none are left.
    let workers = (0..16).map(|w| {
        let pool = pool.clone();
        tokio::spawn(async move {
            let mut mine = Vec::new();
            while let Some(j) = job::claim_next(&pool, &format!("w{w}")).await.unwrap() {
                mine.push(j.id);
            }
            mine
        })
    });
    let mut claimed = Vec::new();
    for w in workers {
        claimed.extend(w.await.unwrap());
    }
    let ours: Vec<Uuid> = claimed.into_iter().filter(|id| ids.contains(id)).collect();
    let unique: HashSet<Uuid> = ours.iter().copied().collect();
    assert_eq!(ours.len(), unique.len(), "a job was claimed twice");
    assert_eq!(unique, ids, "every job was claimed");

    // Ten deliveries of one doorbell: one claim wins.
    let one = new_job(&pool, company).await;
    let racers = (0..10).map(|w| {
        let pool = pool.clone();
        tokio::spawn(async move { job::claim(&pool, one, &format!("r{w}")).await.unwrap() })
    });
    let mut winners = 0;
    for r in racers {
        winners += r.await.unwrap().is_some() as usize;
    }
    assert_eq!(winners, 1);
}

#[tokio::test]
async fn a_dead_workers_job_is_taken_over_and_it_cannot_overwrite_the_result() {
    let Some((pool, company)) = setup().await else {
        return;
    };
    let id = new_job(&pool, company).await;
    let a = job::claim(&pool, id, "a").await.unwrap().unwrap();
    // A live claim is not up for grabs.
    assert!(job::claim(&pool, id, "b").await.unwrap().is_none());

    // a stops heartbeating.
    sqlx::query("update job set heartbeat_at = now() - interval '5 minutes' where id = $1")
        .bind(id)
        .execute(&pool)
        .await
        .unwrap();
    let b = job::claim(&pool, id, "b")
        .await
        .unwrap()
        .expect("b takes over");

    // a wakes up and finishes: its outcome is NOT recorded.
    job::process(&pool, &a, "a").await.unwrap();
    let status = job::get(&pool, company, id).await.unwrap().unwrap();
    assert_eq!(status.status, "running");
    assert_eq!(status.attempts, 2);

    job::process(&pool, &b, "b").await.unwrap();
    let status = job::get(&pool, company, id).await.unwrap().unwrap();
    assert_eq!(status.status, "done", "{status:?}");
    assert_eq!(status.result.unwrap()["alle_ok"], true);
}

#[tokio::test]
async fn a_job_that_keeps_killing_its_worker_is_failed_after_three_attempts() {
    let Some((pool, company)) = setup().await else {
        return;
    };
    let id = new_job(&pool, company).await;
    for worker in ["a", "b", "c"] {
        sqlx::query("update job set heartbeat_at = now() - interval '5 minutes' where id = $1")
            .bind(id)
            .execute(&pool)
            .await
            .unwrap();
        assert!(
            job::claim(&pool, id, worker).await.unwrap().is_some(),
            "{worker}"
        );
    }
    sqlx::query("update job set heartbeat_at = now() - interval '5 minutes' where id = $1")
        .bind(id)
        .execute(&pool)
        .await
        .unwrap();
    assert!(
        job::claim(&pool, id, "d").await.unwrap().is_none(),
        "no fourth attempt"
    );
    job::fail_abandoned(&pool).await.unwrap();
    let status = job::get(&pool, company, id).await.unwrap().unwrap();
    assert_eq!(status.status, "failed");
    assert!(status.error.is_some());
}
