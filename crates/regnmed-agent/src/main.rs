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
    /// Run every open task that is addressed to an agent with a model.
    Kjor {
        /// Only this company's tasks.
        #[arg(long)]
        selskap: Option<uuid::Uuid>,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    dotenvy::dotenv().ok();
    let cli = Cli::parse();
    let pool = regnmed_db::connect(&std::env::var("DATABASE_URL").context("DATABASE_URL mangler")?)
        .await?;
    // Without the sealing key nothing stored can be opened, so there is
    // no useful work to do: say that rather than looping over tasks and
    // failing one by one.
    let nokkel = Nokkel::fra_hex(
        &std::env::var("AGENT_SEALING_KEY")
            .context("AGENT_SEALING_KEY mangler — vertsagenter er av")?,
    )?;
    let issuer = std::env::var("REGNMED_ISSUER").unwrap_or_else(|_| "https://id.regnmed.no".into());
    let api = std::env::var("REGNMED_API").unwrap_or_else(|_| "https://api.regnmed.no".into());

    match cli.command {
        Command::Kjor { selskap } => {
            let oppgaver = regnmed_db::agentllm::apne_agentoppgaver(&pool, selskap).await?;
            if oppgaver.is_empty() {
                println!("ingen åpne agentoppgaver");
                return Ok(());
            }
            for (company_id, oppgave_id) in oppgaver {
                // One task's failure must not stop the rest: the outcome
                // is written into that task's own trail either way.
                match regnmed_agent::kjor_oppgave(
                    &pool, &nokkel, company_id, oppgave_id, &issuer, &api,
                )
                .await
                {
                    Ok(utfall) => println!("{oppgave_id} {} {}", utfall.status(), utfall.notat()),
                    Err(feil) => eprintln!("{oppgave_id} feilet: {feil:#}"),
                }
            }
        }
    }
    Ok(())
}
