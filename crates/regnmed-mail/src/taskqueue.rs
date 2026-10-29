//! The agent work queue (docs/agent.md §8.7).
//!
//! It lives in this crate because this is where the house's NATS
//! plumbing already is — not because it has anything to do with mail.
//! Both the API (which publishes) and the worker (which consumes) depend
//! on it, and a third crate for one stream would be ceremony.
//!
//! **The queue is a doorbell, not the record.** `agent_oppgave` in the
//! database is authoritative: it says which tasks are open and what
//! happened to them. A message here only says "look now" instead of
//! "look within five minutes". That is the whole design, and it has two
//! consequences worth stating plainly:
//!
//! 1. A lost message costs LATENCY, not work. The backstop sweep finds
//!    the task anyway, so a NATS outage degrades agents to how they
//!    behaved before this existed rather than stopping them.
//! 2. A duplicated message costs NOTHING, because the worker claims the
//!    task in the database before it does anything
//!    (`agentoppgave::claim_task`). At-least-once delivery is fine
//!    when the thing being delivered is a nudge.
//!
//! The payload carries ids and no ledger content, deliberately. What the
//! agent reads and writes stays between the worker and the API; putting
//! accounting data on a shared stream would widen where it sits at rest
//! for no gain, since the worker has to query the database regardless.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::jetstream;

pub const STREAM: &str = "REGNMED_AGENT";
pub const SUBJECT: &str = "regnmed.agent.oppgave";
/// The durable pull consumer every worker shares. One consumer, many
/// subscribers: that is what makes adding a replica add throughput.
pub const CONSUMER: &str = "agentarbeidere";

/// "This task wants looking at." Ids only — see the module comment.
#[derive(Debug, Serialize, Deserialize)]
pub struct TaskNudge {
    pub company_id: Uuid,
    pub oppgave_id: Uuid,
}

/// Makes sure the stream exists. Same idempotent shape as the mail
/// stream, so a misconfigured queue fails at startup rather than at the
/// first publish.
pub async fn ensure_stream(js: &jetstream::Context) -> Result<()> {
    js.get_or_create_stream(jetstream::stream::Config {
        name: STREAM.into(),
        subjects: vec![SUBJECT.into()],
        // WorkQueue: a message is removed once a worker acks it, so two
        // workers never see the same nudge in the normal case.
        retention: jetstream::stream::RetentionPolicy::WorkQueue,
        // Re-publishing the same task inside the window is a no-op, so
        // the API may nudge freely -- on create, on an answer, on a
        // retry -- without piling up work.
        duplicate_window: std::time::Duration::from_secs(120),
        ..Default::default()
    })
    .await
    .context("creating/getting the agent stream")?;
    Ok(())
}

/// Rings the doorbell for one task.
///
/// The task id is the Nats-Msg-Id, so two nudges for the same task
/// inside the duplicate window collapse into one — the same trick the
/// utsendelse log uses, where the log row id IS the message id.
///
/// Failure is the CALLER's to shrug off: the task is already safely in
/// the database, and the backstop sweep will pick it up. Never let a
/// queue that is down fail the request that created the work.
pub async fn nudge(js: &jetstream::Context, company_id: Uuid, oppgave_id: Uuid) -> Result<()> {
    let payload = serde_json::to_vec(&TaskNudge {
        company_id,
        oppgave_id,
    })?;
    let melding = async_nats::HeaderMap::from_iter([(
        async_nats::header::NATS_MESSAGE_ID,
        async_nats::HeaderValue::from(oppgave_id.to_string().as_str()),
    )]);
    js.publish_with_headers(SUBJECT, melding, payload.into())
        .await
        .context("publishing the agent nudge")?
        .await
        .context("the agent stream did not acknowledge the nudge")?;
    Ok(())
}
