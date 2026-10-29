//! The hosted agent worker (docs/agent.md §8.3).
//!
//! One task, one loop: read the order, let the customer's own model
//! work with the tools the agent's ROLE allows, and write what happened
//! into `agent_oppgave_hendelse`.
//!
//! What this worker is NOT is a second way into the ledger. It gets a
//! token as the agent itself -- ordinary `client_credentials`, the same
//! identity an agent running on the customer's own machine uses -- so
//! `integration_grant`, `Rett::kan_delegeres` and the attestation rules
//! apply to it unchanged, enforced where they always were. The worker
//! runs the loop and holds the model key. It holds no authority.
//!
//! Tools come from `regnmed_mcp::verktoy` and the system prompt from
//! `regnmed_mcp::rolle`, both unchanged: the two rails must behave the
//! same, and the surest way to make them behave the same is to give
//! them the same code.

pub mod llm;

use anyhow::{Context, Result, bail};
use regnmed_core::hemmelig::Nokkel;
use regnmed_mcp::klient::Klient;
use regnmed_mcp::{Server, rolle, verktoy};
use serde_json::{Value, json};
use sqlx::PgPool;
use uuid::Uuid;

use llm::{Llm, Melding};

/// How many model turns one task may take. A loop that never stops
/// spends the customer's money, so it stops and SAYS it stopped --
/// an agent that quietly gave up is worse than one that reports a wall.
pub const MAKS_TURER: usize = 12;

/// The tool catalogue in the provider's format. Built from the same
/// `VERKTOY` table the MCP server serves, so an agent can never be
/// offered a tool over one rail that does not exist on the other.
pub fn verktoykatalog() -> Value {
    Value::Array(
        verktoy::VERKTOY
            .iter()
            .map(|v| {
                json!({
                    "type": "function",
                    "function": {
                        "name": v.navn,
                        "description": v.beskrivelse,
                        "parameters": serde_json::from_str::<Value>(&verktoy::skjema(v))
                            .unwrap_or_else(|_| json!({"type": "object"})),
                    }
                })
            })
            .collect(),
    )
}

/// What one task run ended as. These are the statuses
/// `agent_oppgave_hendelse` already knows (#103) -- the worker invents
/// none of its own.
#[derive(Debug, PartialEq)]
pub enum Utfall {
    Utfort(String),
    Sporsmal(String),
    Avvist(String),
}

impl Utfall {
    pub fn status(&self) -> &'static str {
        match self {
            Utfall::Utfort(_) => "utfort",
            Utfall::Sporsmal(_) => "sporsmal",
            Utfall::Avvist(_) => "avvist",
        }
    }
    pub fn notat(&self) -> &str {
        match self {
            Utfall::Utfort(t) | Utfall::Sporsmal(t) | Utfall::Avvist(t) => t,
        }
    }
}

/// The instruction that turns a chat model into something that reports
/// back. The role prompt (#103) says how to do the work; this says how
/// to finish, and it is deliberately short: a small model follows two
/// rules better than eight.
fn avslutning() -> String {
    "Når du er ferdig, svar med vanlig tekst som starter med FERDIG: og en kort \
     oppsummering av hva du gjorde.\n\
     Trenger du en avgjørelse et menneske må ta, svar med tekst som starter med \
     SPØRSMÅL: og still ett konkret spørsmål. Ikke gjett.\n\
     Kan oppgaven ikke gjøres, svar med tekst som starter med AVVIST: og si hvorfor.\n\
     Du bokfører aldri noe du ikke har dekning for i et bilag eller i oppgaven."
        .to_string()
}

/// Reads the marker off the model's closing text. Unmarked text is
/// treated as done rather than as failure: a small model that did the
/// work and forgot the prefix should not have its work recorded as a
/// refusal, and the note carries the raw text either way.
fn tolk(tekst: &str) -> Utfall {
    let t = tekst.trim();
    let etter = |p: &str| t[p.len()..].trim().to_string();
    let u = t.to_uppercase();
    if u.starts_with("SPØRSMÅL:") || u.starts_with("SPORSMAL:") {
        Utfall::Sporsmal(etter("SPØRSMÅL:"))
    } else if u.starts_with("AVVIST:") {
        Utfall::Avvist(etter("AVVIST:"))
    } else if u.starts_with("FERDIG:") {
        Utfall::Utfort(etter("FERDIG:"))
    } else {
        Utfall::Utfort(t.to_string())
    }
}

/// Runs one task to a conclusion and returns it. Writing the result is
/// the caller's job ([`kjor_oppgave`]), so this stays testable without
/// a database.
pub async fn kjor_sloyfe(
    modell: &Llm,
    server: &Server,
    rollenavn: &str,
    oppgave: &str,
) -> Result<Utfall> {
    let systemprompt = rolle::prompt(rollenavn)
        .unwrap_or_else(|| rolle::prompt("ansatt").expect("ansatt-prompten finnes"));
    let katalog = verktoykatalog();
    let mut meldinger = vec![
        Melding::system(format!("{systemprompt}\n\n{}", avslutning())),
        Melding::bruker(oppgave.to_string()),
    ];

    for _ in 0..MAKS_TURER {
        let svar = modell.tur(&meldinger, &katalog).await?;

        if svar.kall.is_empty() {
            let tekst = svar.tekst.unwrap_or_default();
            if tekst.trim().is_empty() {
                bail!("modellen svarte uten innhold og uten verktøykall");
            }
            return Ok(tolk(&tekst));
        }

        // Echo the assistant turn back exactly as it came: providers
        // check that each tool result answers a call in it.
        meldinger.push(Melding::assistent_verktoy(
            svar.raa_tool_calls.unwrap_or_else(|| json!([])),
        ));
        for kall in svar.kall {
            let tekst = match verktoy::finn(&kall.navn) {
                Some(v) => server.utfor_verktoy(v, kall.argumenter).await.0,
                // An invented tool is the model's mistake, and it is told
                // so in the same channel as a real result.
                None => format!("ukjent verktøy «{}»", kall.navn),
            };
            meldinger.push(Melding::verktoysvar(kall.id, llm::kutt(&tekst, 8000)));
        }
    }

    Ok(Utfall::Sporsmal(format!(
        "Jeg brukte {MAKS_TURER} steg uten å bli ferdig, og stoppet framfor å fortsette. \
         Si gjerne hva jeg skal prioritere."
    )))
}

/// One task, end to end: build the agent's own API client from its
/// stored credentials, run the loop, and record the outcome.
pub async fn kjor_oppgave(
    pool: &PgPool,
    nokkel: &Nokkel,
    company_id: Uuid,
    oppgave_id: Uuid,
    issuer: &str,
    api_base: &str,
) -> Result<Utfall> {
    let oppgave = regnmed_db::agentoppgave::hent_oppgave(pool, company_id, oppgave_id)
        .await?
        .context("ingen slik oppgave")?;
    let Some(integration_id) = oppgave.integration_id else {
        bail!("oppgaven er ikke stilet til en agent");
    };
    if matches!(oppgave.status.as_str(), "utfort" | "avvist") {
        bail!("oppgaven er avsluttet ({})", oppgave.status);
    }

    let (oppsett, modellnokkel) =
        regnmed_db::agentllm::hent_llm_nokkel(pool, nokkel, company_id, integration_id)
            .await?
            .context("agenten har ingen aktiv modell")?;
    let (client_id, hemmelighet) =
        regnmed_db::agentllm::hent_klientlegitimasjon(pool, nokkel, integration_id)
            .await?
            .context("agenten har ingen lagret klienthemmelighet")?;

    // The role on the grant is the agent's authority, and the prompt it
    // is given must match it: telling a `les` agent it is a bokfører
    // only produces an agent that keeps being refused.
    let rollenavn = regnmed_db::list_integrations(pool, company_id)
        .await?
        .into_iter()
        .find(|g| g.integration_id == integration_id && g.valid_to.is_none())
        .map(|g| g.access)
        .context("agenten har ingen aktiv tilgang til selskapet")?;

    let klient = Klient::med_klientlegitimasjon(
        api_base,
        Some(company_id.to_string()),
        issuer,
        &client_id,
        &hemmelighet,
    )
    .await?;
    let server = Server::new(klient);
    let modell = Llm::ny(&oppsett.base_url, &oppsett.modell, &modellnokkel)?;

    // Say out loud that work started, before it can fail: a task that
    // died mid-run should not look like one nobody ever picked up.
    let navn = oppgave.agent.clone().unwrap_or_else(|| "agenten".into());
    regnmed_db::agentoppgave::svar(pool, company_id, oppgave_id, "pagar", None, &navn).await?;

    let utfall = match kjor_sloyfe(&modell, &server, &rollenavn, &oppgave.melding).await {
        Ok(u) => u,
        // A failure is the agent's own report, not a silent stop: the
        // customer must be able to read why in the same place they read
        // everything else about the task.
        Err(feil) => Utfall::Avvist(llm::kutt(&format!("{feil:#}"), 1000)),
    };
    regnmed_db::agentoppgave::svar(
        pool,
        company_id,
        oppgave_id,
        utfall.status(),
        Some(utfall.notat()),
        &navn,
    )
    .await?;
    Ok(utfall)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_catalogue_matches_the_tools_the_other_rail_serves() {
        let katalog = verktoykatalog();
        let navn: Vec<&str> = katalog
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v["function"]["name"].as_str().unwrap())
            .collect();
        assert_eq!(navn.len(), verktoy::VERKTOY.len());
        // Every tool carries a schema the provider can read; a tool
        // whose parameters did not parse would silently become
        // unusable rather than loudly missing.
        for v in katalog.as_array().unwrap() {
            assert_eq!(
                v["function"]["parameters"]["type"], "object",
                "verktøy uten brukbart skjema: {v}"
            );
        }
        assert!(navn.contains(&"saldobalanse"));
    }

    #[test]
    fn the_closing_marker_decides_the_status() {
        assert_eq!(
            tolk("FERDIG: bokførte 3 bilag"),
            Utfall::Utfort("bokførte 3 bilag".into())
        );
        assert_eq!(
            tolk("SPØRSMÅL: hvilken konto?"),
            Utfall::Sporsmal("hvilken konto?".into())
        );
        assert_eq!(
            tolk("AVVIST: mangler bilag"),
            Utfall::Avvist("mangler bilag".into())
        );
        // A small model that forgot the prefix still gets its work
        // recorded as work, with its own words kept.
        assert_eq!(
            tolk("jeg bokførte alt sammen"),
            Utfall::Utfort("jeg bokførte alt sammen".into())
        );
    }

    #[test]
    fn a_question_is_not_a_terminal_status() {
        // The guard in 0064 locks a task after utfort/avvist only, and
        // the worker depends on that: an agent that asks must be able to
        // continue once somebody answers.
        assert_eq!(Utfall::Sporsmal(String::new()).status(), "sporsmal");
        assert_eq!(Utfall::Utfort(String::new()).status(), "utfort");
        assert_eq!(Utfall::Avvist(String::new()).status(), "avvist");
    }
}
