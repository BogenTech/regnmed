//! Live delivery for Meldinger (docs/meldinger.md).
//!
//! Same principle as the agent queue: **a doorbell, not the record.**
//! `chat_message` in Postgres is the conversation; an event here only
//! says "conversation X has something new, for these people", and every
//! reader fetches it
//! through the guarded API. So the payload carries ids only -- no text,
//! no names -- and a lost event costs latency, never a message.
//!
//! JetStream rather than core NATS because the writers are several
//! processes (API replicas, agent workers) and the readers are every API
//! replica holding open streams: each replica runs one ordered consumer
//! from "now" and fans out in memory. A short `max_age` keeps the stream
//! a bus, not a second archive.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::jetstream;

pub const STREAM: &str = "REGNMED_CHAT";
pub const SUBJECTS: &str = "regnmed.chat.>";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatEvent {
    pub conversation_id: Uuid,
    pub message_id: Uuid,
    /// Who is in the conversation: each API replica wakes only the open
    /// streams of these people.
    pub person_ids: Vec<Uuid>,
}

pub async fn ensure_stream(js: &jetstream::Context) -> Result<()> {
    js.get_or_create_stream(jetstream::stream::Config {
        name: STREAM.into(),
        subjects: vec![SUBJECTS.into()],
        retention: jetstream::stream::RetentionPolicy::Limits,
        storage: jetstream::stream::StorageType::Memory,
        max_age: std::time::Duration::from_secs(300),
        // A bus, not a store: ~100-byte events, oldest dropped first, and
        // the NATS pod's memory is not ours to grow.
        max_bytes: 1 << 20,
        duplicate_window: std::time::Duration::from_secs(60),
        ..Default::default()
    })
    .await
    .context("creating/getting the chat stream")?;
    Ok(())
}

/// Announces one new message. The message id is the Nats-Msg-Id, so a
/// retried publish is one event.
pub async fn announce(js: &jetstream::Context, event: &ChatEvent) -> Result<()> {
    let headers = async_nats::HeaderMap::from_iter([(
        async_nats::header::NATS_MESSAGE_ID,
        async_nats::HeaderValue::from(event.message_id.to_string().as_str()),
    )]);
    js.publish_with_headers(
        format!("regnmed.chat.{}", event.conversation_id),
        headers,
        serde_json::to_vec(event)?.into(),
    )
    .await
    .context("publishing the chat event")?
    .await
    .context("the chat stream did not acknowledge the event")?;
    Ok(())
}

/// Every event from now on, until the stream ends. One per process;
/// the API fans it out to its open connections.
pub async fn follow(
    js: &jetstream::Context,
) -> Result<impl futures_util::Stream<Item = ChatEvent>> {
    use futures_util::StreamExt;
    let consumer = js
        .get_stream(STREAM)
        .await?
        .create_consumer(jetstream::consumer::push::OrderedConfig {
            deliver_subject: js_inbox(),
            deliver_policy: jetstream::consumer::DeliverPolicy::New,
            ..Default::default()
        })
        .await
        .context("creating the chat consumer")?;
    let messages = consumer.messages().await?;
    Ok(messages.filter_map(|m| async move {
        let m = m.ok()?;
        serde_json::from_slice(&m.payload).ok()
    }))
}

fn js_inbox() -> String {
    format!("_INBOX.regnmed-chat.{}", Uuid::new_v4().simple())
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures_util::StreamExt;

    /// The round trip the API relies on: a follower started first sees
    /// an announcement made after, and only ids cross the wire. Runs
    /// against a real nats-server on PATH (skips without).
    #[tokio::test]
    async fn a_follower_hears_an_announcement() {
        let port = std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        // Its own store: nats-servers started by parallel tests would
        // otherwise share the default JetStream directory and fight over it.
        let dir = std::env::temp_dir().join(format!("regnmed-chatbus-{}", Uuid::new_v4().simple()));
        let Ok(mut server) = std::process::Command::new("nats-server")
            .args(["-js", "-p", &port.to_string(), "-sd"])
            .arg(&dir)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
        else {
            eprintln!("skipping: nats-server not found on PATH");
            return;
        };
        let url = format!("nats://127.0.0.1:{port}");
        let mut js = None;
        for _ in 0..50 {
            if let Ok(c) = crate::connect(&url, None, None).await {
                js = Some(c);
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
        let js = js.expect("nats-server did not come up");
        ensure_stream(&js).await.unwrap();

        let events = follow(&js).await.unwrap();
        let mut events = std::pin::pin!(events);
        let event = |message| ChatEvent {
            conversation_id: Uuid::nil(),
            message_id: message,
            person_ids: vec![Uuid::nil()],
        };
        let (message, other) = (Uuid::now_v7(), Uuid::now_v7());
        announce(&js, &event(message)).await.unwrap();
        // The same id again is one event (Nats-Msg-Id dedup).
        announce(&js, &event(message)).await.unwrap();
        announce(&js, &event(other)).await.unwrap();

        let wait = std::time::Duration::from_secs(5);
        let first = tokio::time::timeout(wait, events.next())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            (first.message_id, first.person_ids),
            (message, vec![Uuid::nil()])
        );
        let second = tokio::time::timeout(wait, events.next())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            second.message_id, other,
            "the duplicate was delivered twice"
        );
        let _ = server.kill();
        let _ = std::fs::remove_dir_all(&dir);
        let _ = server.wait();
    }
}
