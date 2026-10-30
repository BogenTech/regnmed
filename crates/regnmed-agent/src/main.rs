//! `regnmed-agent` — runs hosted agents' open tasks (docs/agent.md §8.3).
//!
//! Its own binary, not a thread inside regnmed-api: one model turn can
//! take minutes, and the API lives inside a 64 MB container whose budget
//! is measured under request load (docs/frugality.md). Same reasoning
//! that gave the mail worker its own deployment.

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use regnmed_core::hemmelig::Nokkel;

#[derive(Parser)]
#[command(name = "regnmed-agent", version, about)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Run every open task that is addressed to an agent with a model,
    /// then exit. The backstop sweep: it catches tasks whose nudge was
    /// never published or was lost, so a queue outage costs latency
    /// rather than work.
    Run {
        /// Only this company's tasks.
        #[arg(long)]
        company: Option<uuid::Uuid>,
    },
    /// Consume the work queue until stopped (docs/agent.md §8.7).
    ///
    /// This is the normal way agents run. Every replica joins the same
    /// durable pull consumer, so adding a replica adds throughput --
    /// which the CronJob could not do: `concurrencyPolicy: Forbid` held
    /// the whole platform to one task at a time.
    Listen,
}

#[tokio::main]
async fn main() -> Result<()> {
    dotenvy::dotenv().ok();
    let cli = Cli::parse();
    let pool =
        regnmed_db::connect(&std::env::var("DATABASE_URL").context("DATABASE_URL is not set")?)
            .await?;
    // Without the sealing key nothing stored can be opened, so there is
    // no useful work to do: say that rather than looping over tasks and
    // failing one by one.
    let key = Nokkel::fra_hex(
        &std::env::var("AGENT_SEALING_KEY")
            .context("AGENT_SEALING_KEY is not set — hosted agents are off")?,
    )?;
    let issuer = std::env::var("REGNMED_ISSUER").unwrap_or_else(|_| "https://id.regnmed.no".into());
    let api = std::env::var("REGNMED_API").unwrap_or_else(|_| "https://api.regnmed.no".into());

    match cli.command {
        Command::Run { company } => {
            let tasks = regnmed_db::agentllm::apne_agentoppgaver(&pool, company).await?;
            if tasks.is_empty() {
                println!("no open agent tasks");
                return Ok(());
            }
            let name = std::env::var("HOSTNAME").unwrap_or_else(|_| "regnmed-agent".into());
            for (company_id, task_id) in tasks {
                // The sweep takes the same lease the queue workers take.
                // Without it this would be the one place two processes
                // could run a task at once -- precisely when a backlog
                // makes that most likely.
                match regnmed_db::agentoppgave::claim_task(&pool, task_id, &name).await {
                    Ok(true) => {}
                    Ok(false) => {
                        println!("{task_id} is already being worked on, skipping");
                        continue;
                    }
                    Err(error) => {
                        eprintln!("could not claim {task_id}: {error:#}");
                        continue;
                    }
                }
                // One task's failure must not stop the rest: the outcome
                // is written into that task's own trail either way.
                let outcome =
                    regnmed_agent::run_task(&pool, &key, company_id, task_id, &issuer, &api).await;
                if let Err(error) = regnmed_db::agentoppgave::release_task(&pool, task_id).await {
                    eprintln!("could not release {task_id}: {error:#}");
                }
                match outcome {
                    Ok(outcome) => println!("{task_id} {} {}", outcome.status(), outcome.note()),
                    Err(error) => eprintln!("{task_id} failed: {error:#}"),
                }
            }
        }
        Command::Listen => {
            let url = std::env::var("NATS_URL")
                .context("NATS_URL is not set — without a queue there is nothing to listen to")?;
            let js = regnmed_mail::connect(
                &url,
                std::env::var("NATS_USER").ok(),
                std::env::var("NATS_PASSWORD").ok(),
            )
            .await?;
            regnmed_mail::taskqueue::ensure_stream(&js).await?;
            listen(&pool, &key, &js, &issuer, &api).await?;
        }
    }
    Ok(())
}

/// Consumes the work queue until the process is stopped.
///
/// Every replica joins the SAME durable pull consumer, which is what
/// makes a replica worth adding: JetStream hands each message to one of
/// them. Tasks therefore run in parallel across workers while each task
/// stays sequential within itself -- which is the only shape that works,
/// since a turn's input is the previous turn's output.
async fn listen(
    pool: &sqlx::PgPool,
    key: &Nokkel,
    js: &regnmed_mail::jetstream::Context,
    issuer: &str,
    api: &str,
) -> Result<()> {
    use futures_util::StreamExt;
    use regnmed_mail::jetstream;

    // Named after the pod so a stuck claim in the database points at a
    // process an operator can actually go and look at.
    let name = std::env::var("HOSTNAME").unwrap_or_else(|_| "regnmed-agent".into());

    let consumer = js
        .get_stream(regnmed_mail::taskqueue::STREAM)
        .await?
        .get_or_create_consumer(
            regnmed_mail::taskqueue::CONSUMER,
            jetstream::consumer::pull::Config {
                durable_name: Some(regnmed_mail::taskqueue::CONSUMER.into()),
                // A model turn takes minutes and a task runs up to
                // twelve of them, so a short ack_wait would redeliver
                // work that is still running. The database claim is
                // what makes that safe anyway; this just keeps it rare.
                ack_wait: std::time::Duration::from_secs(20 * 60),
                // Two attempts, not endless: a task that kills its
                // worker twice is a task that needs a human, and the
                // trail already says so.
                max_deliver: 2,
                ..Default::default()
            },
        )
        .await?;

    println!("{name} listening on {}", regnmed_mail::taskqueue::SUBJECT);
    let mut messages = consumer.messages().await?;
    while let Some(message) = messages.next().await {
        let message = match message {
            Ok(m) => m,
            Err(error) => {
                eprintln!("the queue returned an error: {error:#}");
                continue;
            }
        };
        let nudge: regnmed_mail::taskqueue::TaskNudge =
            match serde_json::from_slice(&message.payload) {
                Ok(v) => v,
                // Unreadable payloads are acked: redelivering something we
                // cannot parse only burns the queue.
                Err(error) => {
                    eprintln!("unusable nudge, skipping: {error:#}");
                    let _ = message.ack().await;
                    continue;
                }
            };

        // The claim, not the queue, is what guarantees one worker per
        // task (docs/agent.md §8.7). A redelivery after ack_wait, or a
        // backstop sweep running at the same moment, stops here.
        match regnmed_db::agentoppgave::claim_task(pool, nudge.task_id, &name).await {
            Ok(true) => {}
            Ok(false) => {
                println!("{} is already being worked on, skipping", nudge.task_id);
                let _ = message.ack().await;
                continue;
            }
            Err(error) => {
                eprintln!("could not claim {}: {error:#}", nudge.task_id);
                let _ = message.ack().await;
                continue;
            }
        }

        let outcome =
            regnmed_agent::run_task(pool, key, nudge.company_id, nudge.task_id, issuer, api).await;
        // Released whatever happened: the lease is not evidence, and a
        // failed run must not lock the task out of a later retry.
        if let Err(error) = regnmed_db::agentoppgave::release_task(pool, nudge.task_id).await {
            eprintln!("could not release {}: {error:#}", nudge.task_id);
        }
        match outcome {
            Ok(u) => println!("{} {} {}", nudge.task_id, u.status(), u.note()),
            Err(error) => eprintln!("{} failed: {error:#}", nudge.task_id),
        }
        // Acked either way: the outcome is written into the task's own
        // trail, so a redelivery would only repeat work somebody can
        // already read about.
        let _ = message.ack().await;
    }
    Ok(())
}
