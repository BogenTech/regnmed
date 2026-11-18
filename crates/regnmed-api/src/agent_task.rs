//! Oppgaver til en agent (docs/agent.md, #103).
//!
//! - GET  /companies/{id}/agent/oppgaver              the queue
//! - POST /companies/{id}/agent/oppgaver              give one (admin)
//! - GET  /companies/{id}/agent/oppgaver/{oid}        one task + its trail
//! - POST /companies/{id}/agent/oppgaver/{oid}/svar   report back
//!
//! The authorization here is more specific than any single rettighet
//! could be, so it is written out instead of invented as a new one:
//!
//! - GIVING a task requires `INTEGRASJON_ADMIN`, which
//!   [`Rett::kan_delegeres`] refuses to a machine (#101). An agent
//!   therefore cannot write its own orders — the one property that
//!   makes the trail worth reading.
//! - An AGENT reaches its own queue -- reads it and answers it -- on
//!   access to the company alone, whatever its role: the task was
//!   addressed to it, so needing a further rettighet to read its own
//!   orders only stranded the narrow roles (an `ansatt` agent could not
//!   see what it was asked to do). It still sees and answers only tasks
//!   addressed to it or to no one; another agent's are 404.
//! - A HUMAN reads the queue with `INTEGRASJON_LES` and answers
//!   (closes, withdraws) with `INTEGRASJON_ADMIN`.

use axum::Json;
use axum::extract::{Path, Query, State};
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

use crate::AppState;
use crate::access::{Access, Right, lookup, require};
use crate::auth::{ApiError, AuthPerson};

/// The guard for the queue: an agent needs only to be in the company
/// (see the module comment), a human needs `human`.
async fn require_queue(
    state: &AppState,
    person: &AuthPerson,
    company_id: Uuid,
    human: Right,
) -> Result<Access, ApiError> {
    if person.integration.is_some() {
        // A grant whose role grants NOTHING (a deactivated or unknown
        // custom role) is how an owner cuts an agent off; it must not
        // keep the queue open. Fail-closed, as in docs/auth.md §7.
        match lookup(state, person.person_id, company_id).await? {
            Some(access) if !access.right_slugs().is_empty() => Ok(access),
            _ => Err(ApiError::NotFound),
        }
    } else {
        require(state, person.person_id, company_id, human).await
    }
}

fn task_json(o: &regnmed_db::agent_task::Task) -> serde_json::Value {
    json!({
        "oppgave_id": o.id,
        "integration_id": o.integration_id,
        "agent": o.agent,
        "melding": o.message,
        "frist": o.deadline.map(|d| d.to_string()),
        "opprettet_av": o.created_by,
        "opprettet": o.created_at.to_rfc3339(),
        "status": o.status,
        "avsluttet": regnmed_db::agent_task::AVSLUTTET.contains(&o.status.as_str()),
        "siste_notat": o.last_note,
        "siste_av": o.last_by,
        "siste_tidspunkt": o.last_timestamp.map(|t| t.to_rfc3339()),
    })
}

#[derive(Deserialize)]
pub struct TaskQuery {
    /// Default true: an agent asking "what should I do" wants the open
    /// ones. The history is one query parameter away.
    #[serde(rename = "apne")]
    open: Option<bool>,
}

pub async fn list(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(company_id): Path<Uuid>,
    Query(query): Query<TaskQuery>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_queue(&state, &person, company_id, Right::IntegrationRead).await?;
    // A machine sees its own queue; a human sees everything, which is
    // the point of having given the orders.
    let for_integration = person.integration.as_ref().map(|i| i.id);
    let rader = regnmed_db::agent_task::list_oppgaver(
        &state.pool,
        company_id,
        for_integration,
        query.open.unwrap_or(true),
    )
    .await?;
    Ok(Json(json!({
        "oppgaver": rader.iter().map(task_json).collect::<Vec<_>>(),
    })))
}

#[derive(Deserialize)]
pub struct NewTask {
    /// Addressed to one agent; omitted = any agent with access.
    integration_id: Option<Uuid>,
    #[serde(rename = "melding")]
    message: String,
    #[serde(rename = "frist")]
    deadline: Option<chrono::NaiveDate>,
}

pub async fn create(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(company_id): Path<Uuid>,
    Json(request): Json<NewTask>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require(
        &state,
        person.person_id,
        company_id,
        Right::IntegrationAdmin,
    )
    .await?;
    let id = regnmed_db::agent_task::create_task(
        &state.pool,
        company_id,
        request.integration_id,
        &request.message,
        request.deadline,
        person.display(),
        Some(person.person_id),
    )
    .await
    .map_err(|e| ApiError::BadRequest(format!("{e:#}")))?;
    nudge(&state, company_id, id, id).await;
    Ok(Json(json!({ "oppgave_id": id })))
}

pub async fn hent(
    State(state): State<AppState>,
    person: AuthPerson,
    Path((company_id, task_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_queue(&state, &person, company_id, Right::IntegrationRead).await?;
    let task = regnmed_db::agent_task::fetch_task(&state.pool, company_id, task_id)
        .await?
        .ok_or(ApiError::NotFound)?;
    // Another agent's order is not this agent's business.
    if let Some(caller) = person.integration.as_ref()
        && task
            .integration_id
            .is_some_and(|addressed| addressed != caller.id)
    {
        return Err(ApiError::NotFound);
    }
    let trail = regnmed_db::agent_task::history(&state.pool, company_id, task_id).await?;
    Ok(Json(json!({
        "oppgave": task_json(&task),
        "historikk": trail.iter().map(|h| json!({
            "status": h.status,
            "notat": h.note,
            "av": h.by,
            "tidspunkt": h.created_at.to_rfc3339(),
        })).collect::<Vec<_>>(),
    })))
}

#[derive(Deserialize)]
pub struct Answer {
    /// `pagar`, `sporsmal`, `utfort` or `avvist`. The last two are
    /// final — see migration 0064.
    status: String,
    #[serde(rename = "notat")]
    note: Option<String>,
}

pub async fn answer(
    State(state): State<AppState>,
    person: AuthPerson,
    Path((company_id, task_id)): Path<(Uuid, Uuid)>,
    Json(request): Json<Answer>,
) -> Result<Json<serde_json::Value>, ApiError> {
    // A human closing or withdrawing an order is administering a
    // machine's access to the books, so it takes that rettighet.
    require_queue(&state, &person, company_id, Right::IntegrationAdmin).await?;
    let task = regnmed_db::agent_task::fetch_task(&state.pool, company_id, task_id)
        .await?
        .ok_or(ApiError::NotFound)?;
    // A machine answers its own tasks and the unaddressed ones.
    if let Some(caller) = person.integration.as_ref()
        && task
            .integration_id
            .is_some_and(|addressed| addressed != caller.id)
    {
        return Err(ApiError::NotFound);
    }
    let mirrored = regnmed_db::agent_task::answer(
        &state.pool,
        company_id,
        task_id,
        &request.status,
        request.note.as_deref(),
        person.display(),
        person.integration.as_ref().map(|i| i.id),
    )
    .await
    .map_err(|e| ApiError::BadRequest(format!("{e:#}")))?;
    match mirrored {
        Some(posted) => crate::chat::announce(&state, posted).await,
        // An agent on the customer's own machine asked: tell the person
        // who gave the task, as the hosted worker does. A queue outage
        // must not fail the agent's answer -- it is already recorded.
        None if person.integration.is_some() && request.status == "sporsmal" => {
            if let Some(js) = state.mailq.as_ref()
                && let Err(e) = regnmed_mail::notify_question(
                    js,
                    &state.pool,
                    company_id,
                    task_id,
                    request.note.as_deref().unwrap_or_default(),
                    state.portal_base.as_deref(),
                )
                .await
            {
                eprintln!("could not mail the question on {task_id}: {e:#}");
            }
        }
        None => {}
    }
    // A human answering a question puts the task back in play, so it
    // deserves the same nudge as a new one -- otherwise the reply waits
    // for the backstop sweep, which is the slow path we are leaving.
    // An agent's own answer never does: it is reporting, not asking.
    if person.integration.is_none() && !matches!(request.status.as_str(), "utfort" | "avvist") {
        // Each answer is its own cause: a second reply right after the
        // first must wake the worker again.
        nudge(&state, company_id, task_id, Uuid::now_v7()).await;
    }
    Ok(Json(json!({ "status": request.status })))
}

/// What is waiting for the caller across every company they reach --
/// the portal's notification centre (docs/agent.md §8.5).
///
/// Derived, never stored: an agent question is listed while its task's
/// newest status is `sporsmal` and disappears when someone answers, so
/// the centre cannot disagree with the queue and needs no read-state.
/// Each company is checked with the SAME guard as the queue itself
/// (`INTEGRASJON_LES`), so the centre shows nothing a person could not
/// open. A machine gets an empty list: its questions are its own output.
pub async fn notifications(
    State(state): State<AppState>,
    person: AuthPerson,
) -> Result<Json<serde_json::Value>, ApiError> {
    let mut items = Vec::new();
    if person.integration.is_none() {
        let companies = regnmed_db::one_per_company(
            regnmed_db::company_access_for_person(&state.pool, person.person_id).await?,
        );
        for c in companies {
            let allowed = lookup(&state, person.person_id, c.company_id)
                .await?
                .is_some_and(|a| a.has(Right::IntegrationRead));
            if !allowed {
                continue;
            }
            for t in regnmed_db::agent_task::list_oppgaver(&state.pool, c.company_id, None, true)
                .await?
                .into_iter()
                .filter(|t| t.status == "sporsmal")
            {
                items.push(json!({
                    "type": "agent_sporsmal",
                    "company_id": c.company_id,
                    "selskap": c.name,
                    "oppgave_id": t.id,
                    "agent": t.agent,
                    "melding": t.message,
                    "sporsmal": t.last_note,
                    "tidspunkt": t.last_timestamp.map(|ts| ts.to_rfc3339()),
                }));
            }
        }
    }
    Ok(Json(json!({ "varsler": items })))
}

/// Rings the doorbell, and shrugs if the queue is down.
///
/// The task is already in the database, which is what makes this safe
/// to ignore: the backstop sweep finds it regardless. A queue outage
/// must never fail the request that created the work -- that would turn
/// a slow agent into a broken portal.
pub(crate) async fn nudge(state: &AppState, company_id: Uuid, task_id: Uuid, cause: Uuid) {
    let Some(js) = state.mailq.as_ref() else {
        return;
    };
    if let Err(e) = regnmed_mail::taskqueue::nudge(js, company_id, task_id, cause).await {
        eprintln!("the agent queue rejected the nudge for {task_id}: {e:#}");
    }
}
