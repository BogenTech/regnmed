//! Dimension registry endpoints (docs/dimensjoner.md):
//!
//! - GET  /companies/{id}/dimensions                  the registry
//! - POST /companies/{id}/dimensions                  create (kind, code, name)
//! - PUT  /companies/{id}/dimensions/{kind}/{code}    rename and/or open/close
//!
//! Reading is open to every access level; writing requires bokforing or
//! admin. The code itself is immutable — it is referenced by posted
//! entries and covered by their v3 hashes.

use axum::Json;
use axum::extract::{Path, State};
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

use crate::AppState;
use crate::auth::{ApiError, AuthPerson};
use crate::tilgang::{Rett, krev};

pub async fn list(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(company_id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ApiError> {
    krev(&state, person.person_id, company_id, Rett::DimensjonLes).await?;
    let rows = regnmed_db::list_dimensions(&state.pool, company_id, person.person_id).await?;
    Ok(Json(json!({
        "dimensions": rows.iter().map(|d| json!({
            "kind": d.kind,
            "code": d.code,
            "name": d.name,
            "active": d.active,
            "kunde": d.kunde,
            "kunde_navn": d.kunde_navn,
            "fakturerbar_default": d.fakturerbar_default,
            // The CALLER's effective rate today — what their grid rows
            // will bill at. Not other people's rates.
            "min_timesats_ore": d.min_timesats_ore,
        })).collect::<Vec<_>>(),
    })))
}

#[derive(Deserialize)]
pub struct CreateRequest {
    kind: String,
    code: String,
    name: String,
    /// Customer party_no for a prosjekt (#80). Optional.
    kunde: Option<String>,
}

pub async fn create(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(company_id): Path<Uuid>,
    Json(request): Json<CreateRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    krev(&state, person.person_id, company_id, Rett::DimensjonSkriv).await?;
    regnmed_db::create_dimension(
        &state.pool,
        company_id,
        &request.kind,
        &request.code,
        &request.name,
        request.kunde.as_deref(),
    )
    .await
    .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    Ok(Json(json!({ "kind": request.kind, "code": request.code })))
}

#[derive(Deserialize)]
pub struct UpdateRequest {
    name: Option<String>,
    active: Option<bool>,
    /// Customer link (#80): absent = unchanged, "" = clear,
    /// party_no = point the prosjekt at that customer.
    kunde: Option<String>,
    /// Whether hours on the prosjekt are billable unless the entry says
    /// otherwise (migration 0052).
    fakturerbar_default: Option<bool>,
}

pub async fn update(
    State(state): State<AppState>,
    person: AuthPerson,
    Path((company_id, kind, code)): Path<(Uuid, String, String)>,
    Json(request): Json<UpdateRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    krev(&state, person.person_id, company_id, Rett::DimensjonSkriv).await?;
    regnmed_db::update_dimension(
        &state.pool,
        company_id,
        &kind,
        &code,
        request.name.as_deref(),
        request.active,
        request.kunde.as_deref(),
        request.fakturerbar_default,
    )
    .await
    .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    Ok(Json(json!({ "kind": kind, "code": code })))
}

/// The dated sats history for one prosjekt. Restricted to
/// `TIMER_SATS_SKRIV`: the register carries every person's rates, which
/// is the editor's business — an individual sees their own effective
/// rate through the dimensions list instead.
pub async fn list_satser(
    State(state): State<AppState>,
    person: AuthPerson,
    Path((company_id, code)): Path<(Uuid, String)>,
) -> Result<Json<serde_json::Value>, ApiError> {
    krev(&state, person.person_id, company_id, Rett::TimerSatsSkriv).await?;
    let rows = regnmed_db::list_prosjekt_satser(&state.pool, company_id, &code).await?;
    Ok(Json(json!({
        "satser": rows.iter().map(|r| json!({
            "person_id": r.person_id,
            "person_navn": r.person_navn,
            "timesats_ore": r.timesats_ore,
            "valid_from": r.valid_from.to_string(),
            "created_by": r.created_by,
        })).collect::<Vec<_>>(),
    })))
}

#[derive(Deserialize)]
pub struct SatsRequest {
    /// None sets the project's default rate.
    person_id: Option<Uuid>,
    timesats_ore: i64,
    /// Defaults to today. A rate change is one INSERT — history stays,
    /// and already-recorded hours keep the rate they were logged at.
    valid_from: Option<chrono::NaiveDate>,
}

pub async fn set_sats(
    State(state): State<AppState>,
    person: AuthPerson,
    Path((company_id, code)): Path<(Uuid, String)>,
    Json(request): Json<SatsRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    krev(&state, person.person_id, company_id, Rett::TimerSatsSkriv).await?;
    let valid_from = request
        .valid_from
        .unwrap_or_else(|| chrono::Utc::now().date_naive());
    let created_by = person.name.as_deref().unwrap_or(&person.sub);
    regnmed_db::set_prosjekt_sats(
        &state.pool,
        company_id,
        &code,
        request.person_id,
        request.timesats_ore,
        valid_from,
        created_by,
    )
    .await
    .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    Ok(Json(
        json!({ "code": code, "valid_from": valid_from.to_string() }),
    ))
}
