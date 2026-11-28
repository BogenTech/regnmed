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
use regnmed_db::peppol_worker::{self, EventAction};
use regnmed_mail::{jetstream, jobqueue, peppol_events};
use sqlx::PgPool;
use tokio::sync::Semaphore;

/// How often each replica looks for jobs the queue did not deliver. Short
/// when there is no queue at all, since then the sweep is the only way in.
const SWEEP_WITH_QUEUE: Duration = Duration::from_secs(30);
const SWEEP_WITHOUT_QUEUE: Duration = Duration::from_secs(3);
/// Longest a shutdown waits for running jobs (the pod's grace period
/// should be a little longer).
const DRAIN: Duration = Duration::from_secs(110);
/// How often replicas check open Peppol dispatches against the provider.
const RECONCILE_EVERY: Duration = Duration::from_secs(5 * 60);
const PEPPOL_RETRY: Duration = Duration::from_secs(5);
/// How often replicas ask Altinn for Skatteetaten's answer to a sent mva-melding.
/// The person's Altinn token lived two minutes in TT02, so a minute is too coarse.
const VAT_FEEDBACK_EVERY: Duration = Duration::from_secs(10);

pub async fn run(pool: PgPool, concurrency: usize, once: bool) -> Result<()> {
    let name = std::env::var("HOSTNAME").unwrap_or_else(|_| "regnmed-work".into());
    let slots = Arc::new(Semaphore::new(concurrency.max(1)));
    // What jobs need beyond the database: the bank providers, from the same
    // environment the API reads (docs/bank.md). None is fine: a scheduled
    // fetch then reports that no provider is set up.
    let js = regnmed_mail::connect_from_env().await?;
    let runtime = regnmed_db::job::Runtime {
        bank: regnmed_gov::bankfeed::Providers::from_env()?,
        peppol: regnmed_gov::recommand::Recommand::from_env()?.map(Arc::new),
        mail: js
            .clone()
            .map(|js| regnmed_mail::follow_up::job_mail(pool.clone(), js)),
        directory_answer: None,
        vat_filing: regnmed_db::vat_filing::Rail::from_env()?.map(Arc::new),
        // Trekkpålegg from Skatteetaten (docs/trekkpalegg.md): the
        // Maskinporten client the API uses. None: the daily fetch fails
        // with a message saying the worker is not set up for it.
        garnishment: regnmed_db::garnishment::Rail::from_env()?.map(Arc::new),
    };

    if once {
        // Drain what is waiting, then exit (an operator's or a test's tool):
        // sweep, wait for what was started, and again until a sweep starts nothing.
        while sweep(&pool, &name, &slots, &runtime).await > 0 {
            drop(slots.acquire_many(concurrency.max(1) as u32).await?);
        }
        return Ok(());
    }

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
        let (pool, name, slots, runtime) =
            (pool.clone(), name.clone(), slots.clone(), runtime.clone());
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(every);
            loop {
                tick.tick().await;
                sweep(&pool, &name, &slots, &runtime).await;
            }
        })
    };

    let reconciler = runtime.peppol.clone().map(|provider| {
        let pool = pool.clone();
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(RECONCILE_EVERY);
            loop {
                tick.tick().await;
                if let Err(e) = peppol_worker::reconcile(&pool, &provider).await {
                    eprintln!("peppol reconciliation failed: {e:#}");
                }
            }
        })
    });
    let vat_feedback = runtime.vat_filing.clone().map(|rail| {
        let pool = pool.clone();
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(VAT_FEEDBACK_EVERY);
            // A slow Altinn must not be answered with a burst of catch-up sweeps.
            tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            loop {
                tick.tick().await;
                if let Err(e) = regnmed_db::vat_filing::reconcile(&pool, &rail).await {
                    eprintln!("vat filing reconciliation failed: {e:#}");
                }
            }
        })
    });
    let events = js.clone().map(|js| {
        let pool = pool.clone();
        tokio::spawn(async move {
            if let Err(e) = listen_peppol_events(&pool, &js).await {
                eprintln!("the Peppol event listener stopped: {e:#}");
            }
        })
    });

    let listener = js.map(|js| {
        let (pool, name, slots, runtime) =
            (pool.clone(), name.clone(), slots.clone(), runtime.clone());
        tokio::spawn(async move {
            if let Err(e) = listen(&pool, &js, &name, &slots, &runtime).await {
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
    for task in [reconciler, vat_feedback, events].into_iter().flatten() {
        task.abort();
    }
    match tokio::time::timeout(DRAIN, slots.acquire_many(concurrency.max(1) as u32)).await {
        Ok(_) => println!("{name} stopped cleanly"),
        Err(_) => {
            eprintln!("{name} stopped with jobs still running; another worker takes them over")
        }
    }
    Ok(())
}

/// Turns due schedules into jobs (docs/jobs.md, «Tidsplaner»), then claims
/// and starts jobs while there are free slots and claimable jobs. Every
/// replica does this; `skip locked` keeps two of them from taking the same
/// schedule or the same job. Returns how many jobs it started.
async fn sweep(
    pool: &PgPool,
    name: &str,
    slots: &Arc<Semaphore>,
    runtime: &regnmed_db::job::Runtime,
) -> usize {
    let mut started = 0;
    match regnmed_db::schedule::enqueue_due(pool).await {
        Ok(jobs) if jobs.is_empty() => {}
        Ok(jobs) => println!("{} scheduled job(s) created", jobs.len()),
        Err(e) => eprintln!("could not check the schedules: {e:#}"),
    }
    match regnmed_db::job::fail_abandoned(pool).await {
        Ok(0) => {}
        Ok(n) => eprintln!("{n} abandoned job(s) marked failed"),
        Err(e) => eprintln!("could not check for abandoned jobs: {e:#}"),
    }
    loop {
        let Ok(permit) = slots.clone().try_acquire_owned() else {
            return started;
        };
        match regnmed_db::job::claim_next(pool, name).await {
            Ok(Some(job)) => {
                spawn_job(pool, name, job, permit, runtime);
                started += 1;
            }
            Ok(None) => return started,
            Err(e) => {
                eprintln!("the sweep could not claim: {e:#}");
                return started;
            }
        }
    }
}

async fn listen(
    pool: &PgPool,
    js: &jetstream::Context,
    name: &str,
    slots: &Arc<Semaphore>,
    runtime: &regnmed_db::job::Runtime,
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
            Ok(Some(job)) => spawn_job(pool, name, job, permit, runtime),
            Ok(None) => {} // someone else has it, or it is finished
            Err(e) => eprintln!("could not claim job {}: {e:#}", nudge.job_id),
        }
        // Acked once claimed (or found taken): from here the database row
        // and its heartbeat are what recover a crash, not a redelivery.
        let _ = message.ack().await;
    }
    Ok(())
}

/// Webhook events (docs/efaktura.md §3.7). Each is committed before its
/// ack, so a crash means redelivery and the event key absorbs it.
async fn listen_peppol_events(pool: &PgPool, js: &jetstream::Context) -> Result<()> {
    peppol_events::ensure_stream(js).await?;
    let team = std::env::var("RECOMMAND_TEAM_ID").ok();
    let consumer = js
        .get_stream(peppol_events::STREAM)
        .await?
        .get_or_create_consumer(
            peppol_events::CONSUMER,
            jetstream::consumer::pull::Config {
                durable_name: Some(peppol_events::CONSUMER.into()),
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
                eprintln!("the Peppol event queue returned an error: {e:#}");
                continue;
            }
        };
        let delivered = message.info().map(|i| i.delivered).unwrap_or(1);
        let action = peppol_worker::handle_provider_event(
            pool,
            team.as_deref(),
            &message.payload,
            delivered,
        )
        .await;
        let ack = match action {
            EventAction::Ack => message.ack().await,
            EventAction::Retry => {
                message
                    .ack_with(jetstream::AckKind::Nak(Some(PEPPOL_RETRY)))
                    .await
            }
            EventAction::Drop => message.ack_with(jetstream::AckKind::Term).await,
        };
        if let Err(e) = ack {
            eprintln!("could not acknowledge a Peppol event: {e:#}");
        }
    }
    Ok(())
}

fn spawn_job(
    pool: &PgPool,
    name: &str,
    job: regnmed_db::job::Claimed,
    permit: tokio::sync::OwnedSemaphorePermit,
    runtime: &regnmed_db::job::Runtime,
) {
    let (pool, name, runtime) = (pool.clone(), name.to_owned(), runtime.clone());
    tokio::spawn(async move {
        let started = std::time::Instant::now();
        match regnmed_db::job::process(&pool, &job, &name, &runtime).await {
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
