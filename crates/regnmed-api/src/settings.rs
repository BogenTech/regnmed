//! Company kontaktinfo (docs/faktura.md, #32):
//!
//! - GET /companies/{id}/settings        address, kontonummer, orgform
//! - PUT /companies/{id}/settings        update (admin only)
//! - PUT /companies/{id}/parties/{pid}/contact   party address/e-mail
//!
//! Master data the salgsdokument-PDF and e-postutsendelsen read;
//! nothing here touches the ledger or any hash.

use axum::Json;
use axum::extract::{Path, State};
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

use crate::AppState;
use crate::auth::{ApiError, AuthPerson};

async fn require_level(
    state: &AppState,
    person_id: Uuid,
    company_id: Uuid,
    admin: bool,
) -> Result<(), ApiError> {
    let access = regnmed_db::company_access(&state.pool, person_id, company_id)
        .await?
        .ok_or(ApiError::NotFound)?;
    if admin && access != "admin" {
        return Err(ApiError::Forbidden("company settings require admin"));
    }
    if !admin && access == "les" {
        return Err(ApiError::Forbidden("read-only access"));
    }
    Ok(())
}

pub async fn get_settings(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(company_id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let access = regnmed_db::company_access(&state.pool, person.person_id, company_id)
        .await?
        .ok_or(ApiError::NotFound)?;
    let _ = access;
    let s = regnmed_db::company_settings(&state.pool, company_id).await?;
    Ok(Json(json!({
        "name": s.name,
        "orgnr": s.orgnr,
        "address": s.address,
        "bank_account": s.bank_account,
        "orgform": s.orgform,
        "email": s.email,
    })))
}

#[derive(Deserialize)]
pub struct UpdateSettingsRequest {
    address: Option<String>,
    bank_account: Option<String>,
    orgform: Option<String>,
    email: Option<String>,
}

pub async fn update_settings(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(company_id): Path<Uuid>,
    Json(request): Json<UpdateSettingsRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_level(&state, person.person_id, company_id, true).await?;
    regnmed_db::update_company_settings(
        &state.pool,
        company_id,
        request.address.as_deref(),
        request.bank_account.as_deref(),
        request.orgform.as_deref(),
        request.email.as_deref(),
    )
    .await
    .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    Ok(Json(json!({ "updated": true })))
}

#[derive(Deserialize)]
pub struct PartyContactRequest {
    address: Option<String>,
    email: Option<String>,
}

pub async fn update_party_contact(
    State(state): State<AppState>,
    person: AuthPerson,
    Path((company_id, party_id)): Path<(Uuid, Uuid)>,
    Json(request): Json<PartyContactRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_level(&state, person.person_id, company_id, false).await?;
    regnmed_db::update_party_contact(
        &state.pool,
        company_id,
        party_id,
        request.address.as_deref(),
        request.email.as_deref(),
    )
    .await
    .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    Ok(Json(json!({ "updated": true })))
}
