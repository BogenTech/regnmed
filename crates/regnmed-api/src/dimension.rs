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
    let rows = regnmed_db::list_dimensions(&state.pool, company_id).await?;
    Ok(Json(json!({
        "dimensions": rows.iter().map(|d| json!({
            "kind": d.kind,
            "code": d.code,
            "name": d.name,
            "active": d.active,
            "kunde": d.kunde,
            "kunde_navn": d.kunde_navn,
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
    )
    .await
    .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    Ok(Json(json!({ "kind": kind, "code": code })))
}
