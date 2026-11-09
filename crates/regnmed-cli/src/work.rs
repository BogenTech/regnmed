//! `regnmed work` -- the background-job worker (docs/jobs.md).
//!
//! Built to run as many replicas as the load needs, each running up to
//! `concurrency` jobs at once:
//!
//! - every replica joins the SAME durable pull consumer, so JetStream
//!   hands each doorbell to one of them;
//! - a job is claimed in the database before anything else happens
//!   (`regnmed_db::job::claim`), which is what actually guarantees one
//!   worker per job;
//! - every replica also sweeps for jobs no doorbell reached -- a NATS
//!   outage, a worker that died mid-job -- with `skip locked`, so sweeping
//!   replicas never collide;
//! - SIGTERM stops taking work and waits for running jobs, so a rolling
//!   update does not abandon an import halfway (and if it has to, the
//!   heartbeat lapses and another replica takes the job over).

use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use futures_util::StreamExt;
use regnmed_mail::{jetstream, jobqueue};
use sqlx::PgPool;
use tokio::sync::Semaphore;

/// How often each replica looks for jobs the queue did not deliver. Short
/// when there is no queue at all, since then the sweep is the only way in.
const SWEEP_WITH_QUEUE: Duration = Duration::from_secs(30);
const SWEEP_WITHOUT_QUEUE: Duration = Duration::from_secs(3);
/// Longest a shutdown waits for running jobs (the pod's grace period
/// should be a little longer).
const DRAIN: Duration = Duration::from_secs(110);

pub async fn run(pool: PgPool, concurrency: usize, once: bool) -> Result<()> {
    let name = std::env::var("HOSTNAME").unwrap_or_else(|_| "regnmed-work".into());
    let slots = Arc::new(Semaphore::new(concurrency.max(1)));

    if once {
        // Drain what is waiting, then exit (an operator's or a test's tool).
        sweep(&pool, &name, &slots).await;
        let _ = slots.acquire_many(concurrency.max(1) as u32).await?;
        return Ok(());
    }

    let js = regnmed_mail::connect_from_env().await?;
    let every = if js.is_some() {
        SWEEP_WITH_QUEUE
    } else {
        SWEEP_WITHOUT_QUEUE
    };
    println!(
        "{name} working, {concurrency} at a time, {}",
        if js.is_some() {
            "listening on the job queue"
        } else {
            "no NATS_URL: sweeping only"
        }
    );

    let sweeper = {
        let (pool, name, slots) = (pool.clone(), name.clone(), slots.clone());
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(every);
            loop {
                tick.tick().await;
                sweep(&pool, &name, &slots).await;
            }
        })
    };

    let listener = js.map(|js| {
        let (pool, name, slots) = (pool.clone(), name.clone(), slots.clone());
        tokio::spawn(async move {
            if let Err(e) = listen(&pool, &js, &name, &slots).await {
                eprintln!("the job queue listener stopped: {e:#}");
            }
        })
    });

    shutdown_signal().await;
    println!("{name} stopping: finishing running jobs");
    sweeper.abort();
    if let Some(l) = listener {
        l.abort();
    }
    match tokio::time::timeout(DRAIN, slots.acquire_many(concurrency.max(1) as u32)).await {
        Ok(_) => println!("{name} stopped cleanly"),
        Err(_) => {
            eprintln!("{name} stopped with jobs still running; another worker takes them over")
        }
    }
    Ok(())
}

/// Claims and starts jobs while there are free slots and claimable jobs.
async fn sweep(pool: &PgPool, name: &str, slots: &Arc<Semaphore>) {
    match regnmed_db::job::fail_abandoned(pool).await {
        Ok(0) => {}
        Ok(n) => eprintln!("{n} abandoned job(s) marked failed"),
        Err(e) => eprintln!("could not check for abandoned jobs: {e:#}"),
    }
    loop {
        let Ok(permit) = slots.clone().try_acquire_owned() else {
            return;
        };
        match regnmed_db::job::claim_next(pool, name).await {
            Ok(Some(job)) => spawn_job(pool, name, job, permit),
            Ok(None) => return,
            Err(e) => {
                eprintln!("the sweep could not claim: {e:#}");
                return;
            }
        }
    }
}

async fn listen(
    pool: &PgPool,
    js: &jetstream::Context,
    name: &str,
    slots: &Arc<Semaphore>,
) -> Result<()> {
    jobqueue::ensure_stream(js).await?;
    let consumer = js
        .get_stream(jobqueue::STREAM)
        .await?
        .get_or_create_consumer(
            jobqueue::CONSUMER,
            jetstream::consumer::pull::Config {
                durable_name: Some(jobqueue::CONSUMER.into()),
                // A message waits here only until this replica has a free
                // slot; past ack_wait it goes to another replica, and the
                // claim keeps that from doubling any work.
                ack_wait: Duration::from_secs(60),
                ..Default::default()
            },
        )
        .await?;
    let mut messages = consumer.messages().await?;
    while let Some(message) = messages.next().await {
        let message = match message {
            Ok(m) => m,
            Err(e) => {
                eprintln!("the job queue returned an error: {e:#}");
                continue;
            }
        };
        let nudge: jobqueue::JobNudge = match serde_json::from_slice(&message.payload) {
            Ok(n) => n,
            Err(e) => {
                eprintln!("unusable job nudge, skipping: {e:#}");
                let _ = message.ack().await;
                continue;
            }
        };
        // Wait for a slot BEFORE claiming, so a busy replica never holds a
        // job it cannot start.
        let permit = slots.clone().acquire_owned().await?;
        match regnmed_db::job::claim(pool, nudge.job_id, name).await {
            Ok(Some(job)) => spawn_job(pool, name, job, permit),
            Ok(None) => {} // someone else has it, or it is finished
            Err(e) => eprintln!("could not claim job {}: {e:#}", nudge.job_id),
        }
        // Acked once claimed (or found taken): from here the database row
        // and its heartbeat are what recover a crash, not a redelivery.
        let _ = message.ack().await;
    }
    Ok(())
}

fn spawn_job(
    pool: &PgPool,
    name: &str,
    job: regnmed_db::job::Claimed,
    permit: tokio::sync::OwnedSemaphorePermit,
) {
    let (pool, name) = (pool.clone(), name.to_owned());
    tokio::spawn(async move {
        let started = std::time::Instant::now();
        match regnmed_db::job::process(&pool, &job, &name).await {
            Ok(()) => println!(
                "job {} ({}) finished in {:?}",
                job.id,
                job.kind,
                started.elapsed()
            ),
            Err(e) => eprintln!(
                "job {} ({}): could not record the outcome: {e:#}",
                job.id, job.kind
            ),
        }
        drop(permit);
    });
}

async fn shutdown_signal() {
    let ctrl_c = tokio::signal::ctrl_c();
    #[cfg(unix)]
    {
        let mut term = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("installing the SIGTERM handler");
        tokio::select! {
            _ = ctrl_c => {}
            _ = term.recv() => {}
        }
    }
    #[cfg(not(unix))]
    let _ = ctrl_c.await;
}
