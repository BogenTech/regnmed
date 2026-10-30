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
//! Tools come from `regnmed_mcp::tools` and the system prompt from
//! `regnmed_mcp::role`, both unchanged: the two rails must behave the
//! same, and the surest way to make them behave the same is to give
//! them the same code.

pub mod llm;

use anyhow::{Context, Result, bail};
use futures_util::{StreamExt, stream};
use regnmed_core::secret::SealingKey;
use regnmed_mcp::client::Client;
use regnmed_mcp::{Server, role, tool};
use serde_json::{Value, json};
use sqlx::PgPool;
use uuid::Uuid;

use llm::{Llm, Message};

/// How many model turns one task may take. A loop that never stops
/// spends the customer's money, so it stops and SAYS it stopped --
/// an agent that quietly gave up is worse than one that reports a wall.
/// Tool calls from one turn run this many at a time. Four is a guess
/// with a reason: the models in use rarely ask for more in a single
/// turn, and the API's per-integration rate limit is what punishes a
/// bigger burst.
pub const CONCURRENT_TOOLS: usize = 4;

pub const MAX_TURNS: usize = 12;

/// The tool catalogue in the provider's format. Built from the same
/// `TOOLS` table the MCP server serves, so an agent can never be
/// offered a tool over one rail that does not exist on the other.
pub fn tool_catalogue() -> Value {
    Value::Array(
        tool::TOOLS
            .iter()
            .map(|v| {
                json!({
                    "type": "function",
                    "function": {
                        "name": v.name,
                        "description": v.description,
                        "parameters": serde_json::from_str::<Value>(&tool::schema(v))
                            .unwrap_or_else(|_| json!({"type": "object"})),
                    }
                })
            })
            .collect(),
    )
}

/// One turn of the recorded conversation, read back out of
/// `agent_oppgave_hendelse`. `from_agent` is decided by who wrote it,
/// which is the only thing the trail actually knows.
#[derive(Debug, Clone)]
pub struct HistoryLine {
    pub from_agent: bool,
    pub text: String,
}

/// What one task run ended as. These are the statuses
/// `agent_oppgave_hendelse` already knows (#103) -- the worker invents
/// none of its own.
#[derive(Debug, PartialEq)]
pub enum Outcome {
    Done(String),
    Question(String),
    Refused(String),
}

impl Outcome {
    pub fn status(&self) -> &'static str {
        match self {
            Outcome::Done(_) => "utfort",
            Outcome::Question(_) => "sporsmal",
            Outcome::Refused(_) => "avvist",
        }
    }
    pub fn note(&self) -> &str {
        match self {
            Outcome::Done(t) | Outcome::Question(t) | Outcome::Refused(t) => t,
        }
    }
}

/// The instruction that turns a chat model into something that reports
/// back. The role prompt (#103) says how to do the work; this says how
/// to finish, and it is deliberately short: a small model follows two
/// rules better than eight.
fn closing_rules() -> String {
    "Når du er ferdig, reply med vanlig text som starter med FERDIG: og en kort \
     oppsummering av hva du gjorde.\n\
     Trenger du en avgjørelse et menneske må ta, eller mangler du en opplysning, \
     reply med text som starter med SPØRSMÅL: og still ett konkret spørsmål. \
     Ikke gjett, og ikke avslutt med FERDIG når du egentlig spør.\n\
     Kan oppgaven ikke gjøres, reply med text som starter med AVVIST: og si hvorfor.\n\
     Du bokfører aldri noe du ikke har dekning for i et bilag eller i oppgaven."
        .to_string()
}

/// Reads the marker off the model's closing text.
///
/// Unmarked text that ENDS IN A QUESTION MARK is a question, whatever
/// prefix the model forgot. Found with a real provider
/// (gemini-2.5-flash-lite, 2026-10-29): it asked which date in 2026 was
/// meant, and without this it was recorded `utfort` -- which is
/// TERMINAL in the 0064 guard, so the task closed and the question was
/// lost with it. The asymmetry decides the rule: being wrongly
/// non-terminal is recoverable, because a human reads the note and
/// closes it; being wrongly terminal is not.
///
/// Other unmarked text is still treated as done. A small model that did
/// the work and forgot the prefix should not have its work recorded as
/// a refusal, and the note carries the raw text either way.
fn interpret(text: &str) -> Outcome {
    let t = text.trim();
    let etter = |p: &str| t[p.len()..].trim().to_string();
    let u = t.to_uppercase();
    if u.starts_with("SPØRSMÅL:") || u.starts_with("SPORSMAL:") {
        Outcome::Question(etter("SPØRSMÅL:"))
    } else if u.starts_with("AVVIST:") {
        Outcome::Refused(etter("AVVIST:"))
    } else if u.starts_with("FERDIG:") {
        Outcome::Done(etter("FERDIG:"))
    } else if t.ends_with('?') {
        Outcome::Question(t.to_string())
    } else {
        Outcome::Done(t.to_string())
    }
}

/// The agent's whole system message: the role prompt, then this
/// agent's own instructions, then the protocol.
///
/// Order matters, and only in one direction. The extra text comes AFTER
/// the role so it can specialise — this company's account numbers, who
/// to ask, which suppliers are routine — and the protocol comes last so
/// the closing markers [`interpret`] reads cannot be talked away.
///
/// What the order does NOT do is decide access. Whatever an owner
/// writes here, the role on the grant is what `access::krev` enforces
/// on every call the agent makes, so the worst a bad instruction can do
/// is make the agent attempt things it will be refused.
fn system_prompt(role_name: &str, ekstra: Option<&str>) -> String {
    let role_prompt = role::prompt(role_name)
        .unwrap_or_else(|| role::prompt("ansatt").expect("ansatt-prompten finnes"));
    let mut ut = role_prompt.to_string();
    if let Some(ekstra) = ekstra.map(str::trim).filter(|t| !t.is_empty()) {
        ut.push_str("\n\nSÆRSKILTE INSTRUKSER FOR DEG:\n");
        ut.push_str(ekstra);
    }
    ut.push_str("\n\n");
    ut.push_str(&closing_rules());
    ut
}

/// Runs one task to a conclusion and returns it. Writing the result is
/// the caller's job ([`run_task`]), so this stays testable without
/// a database.
pub async fn run_loop(
    model: &Llm,
    server: &Server,
    role_name: &str,
    ekstra: Option<&str>,
    task: &str,
    history: &[HistoryLine],
) -> Result<Outcome> {
    let katalog = tool_catalogue();
    let mut messages = vec![
        Message::system(system_prompt(role_name, ekstra)),
        Message::bruker(task.to_string()),
    ];
    // The trail IS the conversation (docs/agent.md §8.4): an agent that
    // asked a question and is picked up again must see its own question
    // and the answer, or it asks the same thing forever.
    for replikk in history {
        messages.push(match replikk.from_agent {
            true => Message::assistant(replikk.text.clone()),
            false => Message::bruker(replikk.text.clone()),
        });
    }

    for _ in 0..MAX_TURNS {
        let reply = model.tur(&messages, &katalog).await?;

        if reply.calls.is_empty() {
            let text = reply.text.unwrap_or_default();
            if text.trim().is_empty() {
                bail!("modellen svarte uten innhold og uten verktøykall");
            }
            return Ok(interpret(&text));
        }

        // Echo the assistant turn back exactly as it came: providers
        // check that each tool result answers a call in it.
        messages.push(Message::assistant_tools(
            reply.raw_tool_calls.unwrap_or_else(|| json!([])),
        ));
        // Tool calls in ONE turn are independent of each other -- the
        // model asked for all of them before seeing any answer -- so
        // they run together. This is the only parallelism a single task
        // can have: turns themselves cannot overlap, because a turn's
        // input is the previous turn's output. Concurrency ACROSS tasks
        // is what the work queue is for (docs/agent.md §8.7).
        //
        // Bounded, because the API rate-limits per integration
        // (docs/integrations.md): an unbounded burst would earn the
        // agent a 429 and spend a turn being told off.
        let tool_results = stream::iter(reply.calls.into_iter().map(|calls| async move {
            let text = match tool::find(&calls.name) {
                Some(v) => server.execute_tool(v, calls.arguments).await.0,
                // An invented tool is the model's mistake, and it is told
                // so in the same channel as a real result.
                None => format!("ukjent verktøy «{}»", calls.name),
            };
            (calls.id, text)
        }))
        .buffered(CONCURRENT_TOOLS)
        .collect::<Vec<_>>()
        .await;
        // `buffered` preserves the order they were asked in, which some
        // providers check when matching results to calls.
        for (id, text) in tool_results {
            messages.push(Message::tool_result(id, llm::truncate(&text, 8000)));
        }
    }

    Ok(Outcome::Question(format!(
        "Jeg brukte {MAX_TURNS} steg uten å bli ferdig, og stoppet framfor å fortsette. \
         Si gjerne hva jeg skal prioritere."
    )))
}

/// One task, end to end: build the agent's own API client from its
/// stored credentials, run the loop, and record the outcome.
pub async fn run_task(
    pool: &PgPool,
    key: &SealingKey,
    company_id: Uuid,
    task_id: Uuid,
    issuer: &str,
    api_base: &str,
) -> Result<Outcome> {
    let task = regnmed_db::agent_task::fetch_task(pool, company_id, task_id)
        .await?
        .context("no such task")?;
    let Some(integration_id) = task.integration_id else {
        bail!("the task is not addressed to an agent");
    };
    if matches!(task.status.as_str(), "utfort" | "avvist") {
        bail!("the task is closed ({})", task.status);
    }

    let (settings, model_key) =
        regnmed_db::agentllm::fetch_llm_key(pool, key, company_id, integration_id)
            .await?
            .context("the agent has no active model")?;
    let (client_id, secret) =
        regnmed_db::agentllm::hent_klientlegitimasjon(pool, key, integration_id)
            .await?
            .context("the agent has no stored client secret")?;

    // The role on the grant is the agent's authority, and the prompt it
    // is given must match it: telling a `les` agent it is a bokfører
    // only produces an agent that keeps being refused.
    let grant = regnmed_db::list_integrations(pool, company_id)
        .await?
        .into_iter()
        .find(|g| g.integration_id == integration_id && g.valid_to.is_none())
        .context("the agent has no active access to the company")?;

    let client = Client::with_client_credentials(
        api_base,
        Some(company_id.to_string()),
        issuer,
        &client_id,
        &secret,
    )
    .await?;
    let server = Server::new(client);
    let model = Llm::new(&settings.base_url, &settings.model, &model_key)?;

    // Say out loud that work started, before it can fail: a task that
    // died mid-run should not look like one nobody ever picked up.
    let name = task.agent.clone().unwrap_or_else(|| "agenten".into());
    regnmed_db::agent_task::answer(pool, company_id, task_id, "pagar", None, &name).await?;

    let history: Vec<HistoryLine> = regnmed_db::agent_task::history(pool, company_id, task_id)
        .await?
        .into_iter()
        .filter_map(|h| {
            // A bare `pagar` with no note is "started working", not
            // something anybody said.
            let text = h.note.filter(|t| !t.trim().is_empty())?;
            Some(HistoryLine {
                from_agent: h.by == name,
                text,
            })
        })
        .collect();

    let outcome = match run_loop(
        &model,
        &server,
        &grant.access,
        grant.system_prompt.as_deref(),
        &task.message,
        &history,
    )
    .await
    {
        Ok(u) => u,
        // A failure is the agent's own report, not a silent stop: the
        // customer must be able to read why in the same place they read
        // everything else about the task.
        Err(error) => Outcome::Refused(llm::truncate(&format!("{error:#}"), 1000)),
    };
    regnmed_db::agent_task::answer(
        pool,
        company_id,
        task_id,
        outcome.status(),
        Some(outcome.note()),
        &name,
    )
    .await?;
    Ok(outcome)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_catalogue_matches_the_tools_the_other_rail_serves() {
        let katalog = tool_catalogue();
        let name: Vec<&str> = katalog
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v["function"]["name"].as_str().unwrap())
            .collect();
        assert_eq!(name.len(), tool::TOOLS.len());
        // Every tool carries a schema the provider can read; a tool
        // whose parameters did not parse would silently become
        // unusable rather than loudly missing.
        for v in katalog.as_array().unwrap() {
            assert_eq!(
                v["function"]["parameters"]["type"], "object",
                "verktøy uten brukbart skjema: {v}"
            );
        }
        assert!(name.contains(&"saldobalanse"));
    }

    #[test]
    fn the_extra_instructions_sit_between_the_role_and_the_protocol() {
        let without = system_prompt("bokforer", None);
        let med = system_prompt("bokforer", Some("Strøm bokføres på 6340."));
        // The role still opens the message, so nothing an owner writes
        // displaces the rules the role carries.
        let role_prompt = role::prompt("bokforer").unwrap();
        assert!(med.starts_with(&role_prompt));
        assert!(med.contains("Strøm bokføres på 6340."));
        // And the protocol still closes it: the markers `interpret` reads
        // must be the last thing the model is told.
        let protokoll = closing_rules();
        assert!(med.trim_end().ends_with(protokoll.trim_end()));
        assert!(without.trim_end().ends_with(protokoll.trim_end()));
        // Empty instructions add nothing at all -- no stray heading.
        assert_eq!(system_prompt("bokforer", Some("   ")), without);
    }

    #[test]
    fn the_closing_marker_decides_the_status() {
        assert_eq!(
            interpret("FERDIG: bokførte 3 bilag"),
            Outcome::Done("bokførte 3 bilag".into())
        );
        assert_eq!(
            interpret("SPØRSMÅL: hvilken konto?"),
            Outcome::Question("hvilken konto?".into())
        );
        assert_eq!(
            interpret("AVVIST: mangler bilag"),
            Outcome::Refused("mangler bilag".into())
        );
        // A small model that forgot the prefix still gets its work
        // recorded as work, with its own words kept.
        assert_eq!(
            interpret("jeg bokførte alt sammen"),
            Outcome::Done("jeg bokførte alt sammen".into())
        );
    }

    #[test]
    fn an_unmarked_question_does_not_close_the_task() {
        // Verbatim from gemini-2.5-flash-lite through OpenRouter,
        // 2026-10-29. Recorded as `utfort` it would have CLOSED the
        // task — terminal in the 0064 guard — and taken the question
        // with it.
        let ekte = "Før jeg kan hente saldobalansen, må jeg vite hvilken dato i 2026 \
                    du er interessert i. Kan du vennligst oppgi en spesifikk dato?";
        assert_eq!(interpret(ekte).status(), "sporsmal");
        // A statement that merely mentions a question is still done.
        assert_eq!(
            interpret("Jeg svarte på spørsmålet ditt og bokførte bilaget.").status(),
            "utfort"
        );
    }

    #[test]
    fn a_question_is_not_a_terminal_status() {
        // The guard in 0064 locks a task after utfort/avvist only, and
        // the worker depends on that: an agent that asks must be able to
        // continue once somebody answers.
        assert_eq!(Outcome::Question(String::new()).status(), "sporsmal");
        assert_eq!(Outcome::Done(String::new()).status(), "utfort");
        assert_eq!(Outcome::Refused(String::new()).status(), "avvist");
    }
}
