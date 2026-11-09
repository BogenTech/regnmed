//! The background-job queue (docs/jobs.md).
//!
//! Same design as the agent queue next door: **a doorbell, not the
//! record.** The `job` row in the database is authoritative and the
//! worker claims it there before doing anything, so a lost message costs
//! latency (the sweep finds the job anyway) and a duplicated one costs
//! nothing. The payload is the job id alone; the input never travels on
//! the stream.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::jetstream;

pub const STREAM: &str = "REGNMED_JOBS";
pub const SUBJECT: &str = "regnmed.jobs";
/// The durable pull consumer every worker replica shares: JetStream hands
/// each message to one of them, so a replica added is throughput added.
pub const CONSUMER: &str = "jobworkers";

#[derive(Debug, Serialize, Deserialize)]
pub struct JobNudge {
    pub job_id: Uuid,
}

pub async fn ensure_stream(js: &jetstream::Context) -> Result<()> {
    js.get_or_create_stream(jetstream::stream::Config {
        name: STREAM.into(),
        subjects: vec![SUBJECT.into()],
        retention: jetstream::stream::RetentionPolicy::WorkQueue,
        duplicate_window: std::time::Duration::from_secs(120),
        ..Default::default()
    })
    .await
    .context("creating/getting the job stream")?;
    Ok(())
}

/// Rings the doorbell. The job id is the Nats-Msg-Id, so a repeat inside
/// the duplicate window collapses into one. Failure is the caller's to
/// shrug off: the job is already in the database.
pub async fn nudge(js: &jetstream::Context, job_id: Uuid) -> Result<()> {
    let headers = async_nats::HeaderMap::from_iter([(
        async_nats::header::NATS_MESSAGE_ID,
        async_nats::HeaderValue::from(job_id.to_string().as_str()),
    )]);
    js.publish_with_headers(
        SUBJECT,
        headers,
        serde_json::to_vec(&JobNudge { job_id })?.into(),
    )
    .await
    .context("publishing the job nudge")?
    .await
    .context("the job stream did not acknowledge the nudge")?;
    Ok(())
}
