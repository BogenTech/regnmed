//! Meldinger (docs/meldinger.md): one-to-one conversations.
//!
//! - GET  /chat/contacts                          who I can write to
//! - GET  /chat/conversations                     my conversations, newest first
//! - POST /chat/conversations                     open one `{person_id, company_id?}`
//! - GET  /chat/conversations/{id}?since=         its messages
//! - POST /chat/conversations/{id}/messages       write `{text, documents}`
//! - POST /chat/conversations/{id}/read           move my read mark
//! - GET  /chat/unread                            the badge
//! - GET  /chat/stream                            server-sent events: "look now"
//! - GET  /firms/{fid}/contacts                   the byrå's contact per client
//! - PUT  /firms/{fid}/engagements/{eid}/contacts choose them (byrå admin)
//!
//! The guard here is REACH, not a rettighet: who may talk to whom follows
//! company membership, byrå membership, active engagements and agent
//! grants (`regnmed_db::chat::contacts`), recomputed on every call. A
//! conversation whose other side is out of reach is 404 -- for reading as
//! well as writing -- exactly like a company one has lost access to.
//!
//! The stream carries no content. It says "a conversation of yours has
//! something new" and the client fetches through the guarded GET.

use std::convert::Infallible;
use std::sync::{Arc, LazyLock};
use std::time::Duration;

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::response::IntoResponse;
use axum::response::sse::{Event, KeepAlive, Sse};
use chrono::{DateTime, Utc};
use regnmed_db::chat::{self as db, Contact};
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::sync::broadcast;
use uuid::Uuid;

use crate::AppState;
use crate::auth::{ApiError, AuthPerson};

/// The in-process fan-out: the people a new message concerns. One per
/// process because it is fed by one NATS consumer per process
/// ([`follow`]) -- a global is the honest shape for that.
static HUB: LazyLock<broadcast::Sender<Arc<Vec<Uuid>>>> =
    LazyLock::new(|| broadcast::channel(256).0);

/// Feeds the hub from the chat stream until the process stops. Spawned
/// once at startup when NATS is configured; reconnects on its own.
pub async fn follow(js: async_nats::jetstream::Context) {
    use futures_util::StreamExt;
    loop {
        match regnmed_mail::chatbus::follow(&js).await {
            Ok(events) => {
                let mut events = std::pin::pin!(events);
                while let Some(event) = events.next().await {
                    let _ = HUB.send(Arc::new(event.person_ids));
                }
                eprintln!("the chat stream ended, reconnecting");
            }
            Err(error) => eprintln!("could not follow the chat stream: {error:#}"),
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
}

/// Rings the doorbell for one new message. Through NATS when it is
/// there, so every replica's readers hear it; straight into this
/// process's hub when it is not (a single dev process). A failure costs
/// latency only -- the message is committed and the client's backstop
/// poll finds it.
pub async fn announce(state: &AppState, posted: db::Announcement) {
    let event = regnmed_mail::chatbus::ChatEvent {
        conversation_id: posted.conversation_id,
        message_id: posted.message_id,
        person_ids: posted.person_ids,
    };
    if let Some(js) = state.mailq.as_ref() {
        match regnmed_mail::chatbus::announce(js, &event).await {
            Ok(()) => return,
            Err(e) => eprintln!("the chat stream rejected {}: {e:#}", event.message_id),
        }
    }
    let _ = HUB.send(Arc::new(event.person_ids));
}

/// What the portal needs to show a contact, and why they are reachable.
fn contact_json(c: &Contact) -> Value {
    json!({
        "person_id": c.person_id,
        "name": c.name,
        "agent": c.agent,
        "company_id": c.company_id,
        "avatar_color": c.avatar_color,
        "avatar_icon": c.avatar_icon,
        "links": c.links.iter().map(|l| json!({
            "via": l.via,
            "company_id": l.company_id,
            "company": l.company_name,
            "firm": l.firm_name,
        })).collect::<Vec<_>>(),
    })
}

pub async fn contacts(
    State(state): State<AppState>,
    person: AuthPerson,
) -> Result<Json<Value>, ApiError> {
    let contacts = db::contacts(&state.pool, person.person_id).await?;
    Ok(Json(
        json!({ "contacts": contacts.iter().map(contact_json).collect::<Vec<_>>() }),
    ))
}

/// My conversations that are still in reach, with the other side.
async fn reachable(
    state: &AppState,
    me: Uuid,
) -> Result<Vec<(db::Conversation, Contact)>, ApiError> {
    let contacts = db::contacts(&state.pool, me).await?;
    Ok(db::conversations(&state.pool, me)
        .await?
        .into_iter()
        .filter_map(|c| {
            let other = db::find(&contacts, c.other, c.company_id)?.clone();
            Some((c, other))
        })
        .collect())
}

pub async fn conversations(
    State(state): State<AppState>,
    person: AuthPerson,
) -> Result<Json<Value>, ApiError> {
    // An agent the caller has worked with only through the Oppdrag card
    // has history but no conversation row yet; open it, so the history is
    // where the person looks for it. Idempotent (`open` is an upsert on
    // the pair), and only for agents whose queue the caller may read.
    let contacts = db::contacts(&state.pool, person.person_id).await?;
    let mut latest: std::collections::HashMap<(Uuid, Uuid), db::Message> = Default::default();
    for c in contacts.iter().filter(|c| c.agent) {
        let Some(company) = c.company_id else {
            continue;
        };
        if !sees_agent_history(&state, person.person_id, company).await? {
            continue;
        }
        let Some(last) = db::agent_history(&state.pool, company, c.person_id, None, None, 1)
            .await?
            .pop()
        else {
            continue;
        };
        db::open(&state.pool, person.person_id, c.person_id, Some(company))
            .await
            .map_err(|e| ApiError::BadRequest(format!("{e:#}")))?;
        latest.insert((c.person_id, company), last);
    }
    let rows = reachable(&state, person.person_id).await?;
    let mut out: Vec<(DateTime<Utc>, Value)> = rows
        .iter()
        .map(|(c, other)| {
            let mut text = c.last_text.clone();
            let mut from_me = c.last_author == Some(person.person_id);
            let mut at = c.last_at.unwrap_or(c.created_at);
            // The newest line is the history's when it is newer than chat.
            if let Some(h) = other
                .company_id
                .and_then(|co| latest.get(&(other.person_id, co)))
                && c.last_at.is_none_or(|t| h.created_at > t)
            {
                text = Some(h.body.clone());
                from_me =
                    h.author_person_id == person.person_id || h.author_name == person.display();
                at = h.created_at;
            }
            (
                at,
                json!({
                    "conversation_id": c.id,
                    "with": contact_json(other),
                    "last_text": text,
                    "last_from_me": from_me,
                    "last_at": at.to_rfc3339(),
                    "unread": c.unread,
                }),
            )
        })
        .collect();
    out.sort_by(|a, b| b.0.cmp(&a.0));
    Ok(Json(json!({
        "conversations": out.into_iter().map(|(_, v)| v).collect::<Vec<_>>(),
    })))
}

#[derive(Deserialize)]
pub struct OpenRequest {
    person_id: Uuid,
    /// Required for an agent: the company whose agent it is.
    company_id: Option<Uuid>,
}

pub async fn open(
    State(state): State<AppState>,
    person: AuthPerson,
    Json(request): Json<OpenRequest>,
) -> Result<Json<Value>, ApiError> {
    let contacts = db::contacts(&state.pool, person.person_id).await?;
    let Some(other) = db::find(&contacts, request.person_id, request.company_id) else {
        return Err(ApiError::NotFound);
    };
    let id = db::open(
        &state.pool,
        person.person_id,
        other.person_id,
        other.company_id,
    )
    .await
    .map_err(|e| ApiError::BadRequest(format!("{e:#}")))?;
    Ok(Json(json!({ "conversation_id": id })))
}

/// The conversation and its other side, if `me` is in it and still in
/// reach of them. Anything else is 404: not a participant and no longer
/// reachable look the same from outside.
async fn guard(
    state: &AppState,
    me: Uuid,
    conversation: Uuid,
) -> Result<(Option<Uuid>, Contact), ApiError> {
    let (company, other) = db::membership(&state.pool, me, conversation)
        .await?
        .ok_or(ApiError::NotFound)?;
    let contacts = db::contacts(&state.pool, me).await?;
    let contact = db::find(&contacts, other, company)
        .ok_or(ApiError::NotFound)?
        .clone();
    Ok((company, contact))
}

#[derive(Deserialize)]
pub struct MessagesQuery {
    since: Option<DateTime<Utc>>,
    /// Page back from this message: its `created_at` and `message_id`.
    before: Option<DateTime<Utc>>,
    before_id: Option<Uuid>,
    /// Messages per page. A long thread is never sent whole: the portal
    /// shows the newest page and fetches older ones as the reader scrolls.
    limit: Option<i64>,
}

/// The first page and every page back are this long unless asked.
const PAGE: i64 = 30;

/// Whether the caller may see an agent's task history in a company: the
/// queue's own guard (`INTEGRASJON_LES`), so Meldinger never shows a task
/// the Oppdrag card would have refused.
async fn sees_agent_history(state: &AppState, me: Uuid, company: Uuid) -> Result<bool, ApiError> {
    Ok(crate::access::lookup(state, me, company)
        .await?
        .is_some_and(|a| a.has(crate::access::Right::IntegrationRead)))
}

pub async fn messages(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(conversation): Path<Uuid>,
    Query(query): Query<MessagesQuery>,
) -> Result<Json<Value>, ApiError> {
    let (company, other) = guard(&state, person.person_id, conversation).await?;
    let limit = query.limit.unwrap_or(PAGE).clamp(1, 200);
    let before = query.before.zip(query.before_id);
    // One more than asked from each source says whether anything is left.
    let mut messages =
        db::messages(&state.pool, conversation, query.since, before, limit + 1).await?;
    // With an agent, the conversation is the whole history with it: the
    // tasks given from the Oppdrag card are merged in, read from the task
    // trail rather than copied (docs/meldinger.md).
    if other.agent
        && let Some(c) = company
        && sees_agent_history(&state, person.person_id, c).await?
    {
        let mut history = db::agent_history(
            &state.pool,
            c,
            other.person_id,
            query.since,
            before,
            limit + 1,
        )
        .await?;
        // Before 0092 a task recorded only its giver's NAME; the caller's
        // own words should still read as theirs.
        let my_name = person.display().to_string();
        for m in &mut history {
            if m.author_person_id.is_nil() && m.author_name == my_name {
                m.author_person_id = person.person_id;
            }
        }
        messages.extend(history);
    }
    messages.sort_by(|a, b| (a.created_at, a.id).cmp(&(b.created_at, b.id)));
    let more = messages.len() as i64 > limit;
    if query.since.is_some() {
        messages.truncate(limit as usize);
    } else {
        // Paging back keeps the NEWEST `limit` of what both sources gave.
        let skip = messages.len().saturating_sub(limit as usize);
        messages.drain(..skip);
    }
    // An agent regnmed does not run answers only if it runs somewhere
    // else; the thread says so up front rather than leaving the message
    // to wait in silence (docs/meldinger.md §3).
    // Only on the full load: incremental polls keep the first answer.
    let runs_here = match (other.agent, company, query.since) {
        (true, Some(c), None) => Some(db::agent_runs_here(&state.pool, c, other.person_id).await?),
        _ => None,
    };
    // Where the agent was told to start fresh: the thread draws a line.
    let restarts = match other.agent {
        true => db::restarts(&state.pool, conversation).await?,
        false => Vec::new(),
    };
    Ok(Json(json!({
        "me": person.person_id,
        "with": contact_json(&other),
        "runs_here": runs_here,
        "ny_start": restarts.iter().map(|t| t.to_rfc3339()).collect::<Vec<_>>(),
        // Where an attachment can go: the agent's company, or a company
        // both sides work in.
        "attach_to": match company {
            Some(c) => vec![c],
            None => other.companies(),
        },
        // Older messages exist beyond this page (only meaningful when
        // paging back, not on a live poll).
        "older": query.since.is_none() && more,
        "messages": messages.iter().map(|m| json!({
            "message_id": m.id,
            "author_id": m.author_person_id,
            "author": m.author_name,
            "text": m.body,
            "created_at": m.created_at.to_rfc3339(),
            "documents": m.documents,
            "task_id": m.task_id,
            "task_status": m.task_status,
        })).collect::<Vec<_>>(),
    })))
}

/// «Ny samtale» with an agent: from now on, nothing said before reaches
/// it with the next order -- not the earlier answers it would otherwise
/// trust, and not a question it is still waiting on. The messages stay.
pub async fn restart(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(conversation): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    let (_, other) = guard(&state, person.person_id, conversation).await?;
    if !other.agent || person.integration.is_some() {
        return Err(ApiError::BadRequest(
            "en ny samtale startes bare med en agent".into(),
        ));
    }
    let at = db::restart(&state.pool, conversation, person.person_id).await?;
    Ok(Json(json!({ "ny_start": at.to_rfc3339() })))
}

#[derive(Deserialize)]
pub struct NewMessage {
    #[serde(default)]
    text: String,
    /// Inbox document ids -- upload to /companies/{id}/inbox first.
    #[serde(default)]
    documents: Vec<Uuid>,
}

pub async fn post(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(conversation): Path<Uuid>,
    Json(request): Json<NewMessage>,
) -> Result<Json<Value>, ApiError> {
    let (company, other) = guard(&state, person.person_id, conversation).await?;
    // An agent answers through its tasks; it does not start talking.
    if person.integration.is_some() {
        return Err(ApiError::BadRequest(
            "en agent svarer gjennom oppgavene sine, ikke direkte i meldinger".into(),
        ));
    }
    let attachable = match company {
        Some(c) => vec![c],
        None => other.companies(),
    };
    let agent = match (other.agent, company) {
        (true, Some(company_id)) => Some(db::AgentTarget {
            person_id: other.person_id,
            company_id,
        }),
        _ => None,
    };
    let (posted, task) = db::post(
        &state.pool,
        conversation,
        &db::Author {
            person_id: person.person_id,
            name: person.display(),
        },
        &request.text,
        &request.documents,
        &attachable,
        agent.as_ref(),
    )
    .await
    .map_err(|e| ApiError::BadRequest(format!("{e:#}")))?;
    let message_id = posted.message_id;
    announce(&state, posted).await;
    if let (Some(task_id), Some(company_id)) = (task, company) {
        crate::agent_task::nudge(&state, company_id, task_id, message_id).await;
    }
    Ok(Json(json!({ "message_id": message_id, "task_id": task })))
}

#[derive(Deserialize)]
pub struct ReadRequest {
    until: DateTime<Utc>,
}

pub async fn read(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(conversation): Path<Uuid>,
    Json(request): Json<ReadRequest>,
) -> Result<Json<Value>, ApiError> {
    guard(&state, person.person_id, conversation).await?;
    // A mark in the future would hide what is said next.
    let until = request.until.min(Utc::now());
    db::mark_read(&state.pool, conversation, person.person_id, until).await?;
    Ok(Json(json!({ "ok": true })))
}

/// The bell's «Marker alle som lest»: every conversation, in one go.
pub async fn read_all(
    State(state): State<AppState>,
    person: AuthPerson,
) -> Result<Json<Value>, ApiError> {
    db::mark_all_read(&state.pool, person.person_id).await?;
    Ok(Json(json!({ "ok": true })))
}

/// The portal's notification centre: what waits for the caller
/// (docs/meldinger.md §7.2). Today that is unread messages, one item per
/// conversation -- an agent's question is a message from the agent, like
/// a colleague's would be. `type` leaves room for other things that need
/// a person's attention.
///
/// Derived from the read marks, never stored on its own: reading the
/// conversation clears its item, so the bell, the menu badge and the
/// conversation list cannot disagree.
pub async fn notifications(
    State(state): State<AppState>,
    person: AuthPerson,
) -> Result<Json<Value>, ApiError> {
    let rows = reachable(&state, person.person_id).await?;
    Ok(Json(json!({
        "varsler": rows.iter().filter(|(c, _)| c.unread > 0).map(|(c, other)| json!({
            "type": "melding",
            "conversation_id": c.id,
            "fra": other.name,
            "agent": other.agent,
            "tekst": c.last_text,
            "tidspunkt": c.last_at.map(|t| t.to_rfc3339()),
            "uleste": c.unread,
        })).collect::<Vec<_>>(),
    })))
}

pub async fn unread(
    State(state): State<AppState>,
    person: AuthPerson,
) -> Result<Json<Value>, ApiError> {
    let rows = reachable(&state, person.person_id).await?;
    let unread: i64 = rows.iter().map(|(c, _)| c.unread).sum();
    let conversations = rows.iter().filter(|(c, _)| c.unread > 0).count();
    Ok(Json(
        json!({ "unread": unread, "conversations": conversations }),
    ))
}

pub async fn stream(person: AuthPerson) -> impl IntoResponse {
    use futures_util::StreamExt;
    let me = person.person_id;
    // A first event at once: proxies (nginx in front of prod, Vite in
    // dev) hold a response until its first bytes, and the client counts
    // the stream as open -- and fetches what it missed -- only then.
    let hello = futures_util::stream::once(async {
        Ok::<_, Infallible>(Event::default().event("ready").data(""))
    });
    let events = futures_util::stream::unfold(HUB.subscribe(), move |mut rx| async move {
        loop {
            match rx.recv().await {
                Ok(people) if people.contains(&me) => break,
                Ok(_) => continue,
                // Fell behind: something was missed, so "look now" is
                // exactly the right thing to say.
                Err(broadcast::error::RecvError::Lagged(_)) => break,
                Err(broadcast::error::RecvError::Closed) => return None,
            }
        }
        Some((Ok(Event::default().event("message").data("")), rx))
    });
    let sse = Sse::new(hello.chain(events))
        .keep_alive(KeepAlive::new().interval(Duration::from_secs(25)));
    // nginx buffers proxied responses unless told not to, per response.
    ([("x-accel-buffering", "no")], sse)
}

pub async fn firm_contacts(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(firm_id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    if !regnmed_db::is_firm_member(&state.pool, person.person_id, firm_id).await? {
        return Err(ApiError::NotFound);
    }
    let map = db::firm_contacts(&state.pool, firm_id).await?;
    Ok(Json(json!({ "contacts": map })))
}

#[derive(Deserialize)]
pub struct ContactsRequest {
    person_ids: Vec<Uuid>,
}

pub async fn set_firm_contacts(
    State(state): State<AppState>,
    person: AuthPerson,
    Path((firm_id, engagement_id)): Path<(Uuid, Uuid)>,
    Json(request): Json<ContactsRequest>,
) -> Result<Json<Value>, ApiError> {
    crate::firm_members::require_firm_admin(&state, person.person_id, firm_id).await?;
    db::set_firm_contacts(
        &state.pool,
        firm_id,
        engagement_id,
        &request.person_ids,
        person.display(),
    )
    .await
    .map_err(|e| ApiError::BadRequest(format!("{e:#}")))?;
    Ok(Json(json!({ "ok": true })))
}
