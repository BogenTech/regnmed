//! Attestering (docs/attestering.md, #47) — internal control as a flow:
//!
//! - GET  /companies/{id}/attestering/policy       current policy + history
//! - POST /companies/{id}/attestering/policy       new policy row (admin)
//! - GET  /companies/{id}/members                  attestant candidates (admin)
//! - POST /companies/{id}/inbox/{doc}/attester     {godkjent, note?}
//! - GET  /companies/{id}/inbox/{doc}/attestering  the decision trail
//!
//! Enforcement lives in regnmed-db (the post/approve transactions) — the
//! endpoints here record decisions and show the trail. Reading is open to
//! every access level: the attestering history is exactly what a revisor
//! asks for.

use axum::Json;
use axum::extract::{Path, State};
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

use crate::AppState;
use crate::auth::{ApiError, AuthPerson};
use crate::tilgang::{Rett, krev};

fn policy_json(p: &regnmed_db::AttestationPolicy) -> serde_json::Value {
    json!({
        "aktiv": p.aktiv,
        "belopsgrense_ore": p.belopsgrense_ore,
        "attestant_person_id": p.attestant_person_id,
        "created_by": p.created_by,
        "created_at": p.created_at.to_rfc3339(),
    })
}

pub async fn get_policy(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(company_id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ApiError> {
    krev(&state, person.person_id, company_id, Rett::AttesteringLes).await?;
    let history = regnmed_db::policy_history(&state.pool, company_id).await?;
    Ok(Json(json!({
        "policy": history.first().map(policy_json),
        "history": history.iter().map(policy_json).collect::<Vec<_>>(),
    })))
}

#[derive(Deserialize)]
pub struct PolicyRequest {
    aktiv: bool,
    belopsgrense_ore: Option<i64>,
    attestant_person_id: Option<Uuid>,
}

pub async fn set_policy(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(company_id): Path<Uuid>,
    Json(request): Json<PolicyRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    krev(&state, person.person_id, company_id, Rett::AttesteringAdmin).await?;
    let created_by = person.name.as_deref().unwrap_or(&person.sub);
    regnmed_db::set_policy(
        &state.pool,
        company_id,
        request.aktiv,
        request.belopsgrense_ore,
        request.attestant_person_id,
        created_by,
    )
    .await
    .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    Ok(Json(json!({ "ok": true })))
}

pub async fn members(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(company_id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ApiError> {
    krev(&state, person.person_id, company_id, Rett::AttesteringAdmin).await?;
    let members = regnmed_db::company_members(&state.pool, company_id).await?;
    Ok(Json(json!({
        "members": members.iter().map(|m| json!({
            "person_id": m.person_id,
            "name": m.name,
            "role": m.role,
        })).collect::<Vec<_>>(),
    })))
}

#[derive(Deserialize)]
pub struct AttesterRequest {
    godkjent: bool,
    note: Option<String>,
}

pub async fn attester(
    State(state): State<AppState>,
    person: AuthPerson,
    Path((company_id, document_id)): Path<(Uuid, Uuid)>,
    Json(request): Json<AttesterRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    krev(&state, person.person_id, company_id, Rett::AttesteringUtfor).await?;
    let display = person.name.as_deref().unwrap_or(&person.sub);
    regnmed_db::attester_inbox_document(
        &state.pool,
        company_id,
        document_id,
        request.godkjent,
        request.note.as_deref(),
        person.person_id,
        display,
    )
    .await
    .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    Ok(Json(json!({
        "decision": if request.godkjent { "godkjent" } else { "avvist" },
    })))
}

pub async fn trail(
    State(state): State<AppState>,
    person: AuthPerson,
    Path((company_id, document_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<serde_json::Value>, ApiError> {
    krev(&state, person.person_id, company_id, Rett::AttesteringLes).await?;
    let rows =
        regnmed_db::attestation_trail(&state.pool, company_id, "inbox_document", document_id)
            .await?;
    Ok(Json(json!({
        "trail": rows.iter().map(|a| json!({
            "decision": a.decision,
            "note": a.note,
            "decided_by": a.decided_by,
            "decided_at": a.created_at.to_rfc3339(),
        })).collect::<Vec<_>>(),
    })))
}
