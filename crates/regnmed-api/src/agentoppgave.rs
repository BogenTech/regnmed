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
//! - ANSWERING requires `INTEGRASJON_LES` **and** being the agent the
//!   task was addressed to (or a human who may administer integrations,
//!   so an order can be withdrawn). An agent must not answer for
//!   another agent.

use axum::Json;
use axum::extract::{Path, Query, State};
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

use crate::AppState;
use crate::auth::{ApiError, AuthPerson};
use crate::tilgang::{Rett, krev};

fn oppgave_json(o: &regnmed_db::agentoppgave::Oppgave) -> serde_json::Value {
    json!({
        "oppgave_id": o.id,
        "integration_id": o.integration_id,
        "agent": o.agent,
        "melding": o.melding,
        "frist": o.frist.map(|d| d.to_string()),
        "opprettet_av": o.opprettet_av,
        "opprettet": o.created_at.to_rfc3339(),
        "status": o.status,
        "avsluttet": regnmed_db::agentoppgave::AVSLUTTET.contains(&o.status.as_str()),
        "siste_notat": o.siste_notat,
        "siste_av": o.siste_av,
        "siste_tidspunkt": o.siste_tidspunkt.map(|t| t.to_rfc3339()),
    })
}

#[derive(Deserialize)]
pub struct OppgaveQuery {
    /// Default true: an agent asking "what should I do" wants the open
    /// ones. The history is one query parameter away.
    apne: Option<bool>,
}

pub async fn list(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(company_id): Path<Uuid>,
    Query(query): Query<OppgaveQuery>,
) -> Result<Json<serde_json::Value>, ApiError> {
    krev(&state, person.person_id, company_id, Rett::IntegrasjonLes).await?;
    // A machine sees its own queue; a human sees everything, which is
    // the point of having given the orders.
    let for_integration = person.integration.as_ref().map(|i| i.id);
    let rader = regnmed_db::agentoppgave::list_oppgaver(
        &state.pool,
        company_id,
        for_integration,
        query.apne.unwrap_or(true),
    )
    .await?;
    Ok(Json(json!({
        "oppgaver": rader.iter().map(oppgave_json).collect::<Vec<_>>(),
    })))
}

#[derive(Deserialize)]
pub struct NyOppgave {
    /// Addressed to one agent; omitted = any agent with access.
    integration_id: Option<Uuid>,
    melding: String,
    frist: Option<chrono::NaiveDate>,
}

pub async fn opprett(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(company_id): Path<Uuid>,
    Json(request): Json<NyOppgave>,
) -> Result<Json<serde_json::Value>, ApiError> {
    krev(&state, person.person_id, company_id, Rett::IntegrasjonAdmin).await?;
    let id = regnmed_db::agentoppgave::opprett_oppgave(
        &state.pool,
        company_id,
        request.integration_id,
        &request.melding,
        request.frist,
        person.display(),
    )
    .await
    .map_err(|e| ApiError::BadRequest(format!("{e:#}")))?;
    Ok(Json(json!({ "oppgave_id": id })))
}

pub async fn hent(
    State(state): State<AppState>,
    person: AuthPerson,
    Path((company_id, oppgave_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<serde_json::Value>, ApiError> {
    krev(&state, person.person_id, company_id, Rett::IntegrasjonLes).await?;
    let oppgave = regnmed_db::agentoppgave::hent_oppgave(&state.pool, company_id, oppgave_id)
        .await?
        .ok_or(ApiError::NotFound)?;
    // Another agent's order is not this agent's business.
    if let Some(caller) = person.integration.as_ref()
        && oppgave
            .integration_id
            .is_some_and(|addressed| addressed != caller.id)
    {
        return Err(ApiError::NotFound);
    }
    let trail = regnmed_db::agentoppgave::historikk(&state.pool, company_id, oppgave_id).await?;
    Ok(Json(json!({
        "oppgave": oppgave_json(&oppgave),
        "historikk": trail.iter().map(|h| json!({
            "status": h.status,
            "notat": h.notat,
            "av": h.av,
            "tidspunkt": h.created_at.to_rfc3339(),
        })).collect::<Vec<_>>(),
    })))
}

#[derive(Deserialize)]
pub struct Svar {
    /// `pagar`, `sporsmal`, `utfort` or `avvist`. The last two are
    /// final — see migration 0064.
    status: String,
    notat: Option<String>,
}

pub async fn svar(
    State(state): State<AppState>,
    person: AuthPerson,
    Path((company_id, oppgave_id)): Path<(Uuid, Uuid)>,
    Json(request): Json<Svar>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let tilgang = krev(&state, person.person_id, company_id, Rett::IntegrasjonLes).await?;
    let oppgave = regnmed_db::agentoppgave::hent_oppgave(&state.pool, company_id, oppgave_id)
        .await?
        .ok_or(ApiError::NotFound)?;
    match person.integration.as_ref() {
        // A machine answers its own tasks and the unaddressed ones.
        Some(caller) => {
            if oppgave
                .integration_id
                .is_some_and(|addressed| addressed != caller.id)
            {
                return Err(ApiError::NotFound);
            }
        }
        // A human closing or withdrawing an order is administering a
        // machine's access to the books, so it takes that rettighet.
        None => {
            if !tilgang.har(Rett::IntegrasjonAdmin) {
                return Err(ApiError::Forbidden("INTEGRASJON_ADMIN"));
            }
        }
    }
    regnmed_db::agentoppgave::svar(
        &state.pool,
        company_id,
        oppgave_id,
        &request.status,
        request.notat.as_deref(),
        person.display(),
    )
    .await
    .map_err(|e| ApiError::BadRequest(format!("{e:#}")))?;
    Ok(Json(json!({ "status": request.status })))
}
