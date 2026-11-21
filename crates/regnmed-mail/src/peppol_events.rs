//! Signed Peppol webhooks on their way to a worker (docs/efaktura.md §3.7).
//!
//! Unlike the job queue this is not a doorbell: until a worker has
//! committed an event, this stream holds the only copy.

use anyhow::{Context, Result};

use crate::jetstream;

pub const STREAM: &str = "REGNMED_PEPPOL_EVENTS";
pub const SUBJECT: &str = "regnmed.peppol.events";
/// Shared by all worker replicas, so each event goes to one of them.
pub const CONSUMER: &str = "peppolworkers";

pub async fn ensure_stream(js: &jetstream::Context) -> Result<()> {
    js.get_or_create_stream(jetstream::stream::Config {
        name: STREAM.into(),
        subjects: vec![SUBJECT.into()],
        retention: jetstream::stream::RetentionPolicy::WorkQueue,
        storage: jetstream::stream::StorageType::File,
        duplicate_window: std::time::Duration::from_secs(120),
        ..Default::default()
    })
    .await
    .context("creating/getting the Peppol event stream")?;
    Ok(())
}

/// Returns only after JetStream has stored the event.
pub async fn publish(js: &jetstream::Context, message_id: &str, body: Vec<u8>) -> Result<()> {
    let headers = async_nats::HeaderMap::from_iter([(
        async_nats::header::NATS_MESSAGE_ID,
        async_nats::HeaderValue::from(message_id),
    )]);
    js.publish_with_headers(SUBJECT, headers, body.into())
        .await
        .context("publishing the Peppol event")?
        .await
        .context("the Peppol event stream did not acknowledge")?;
    Ok(())
}
