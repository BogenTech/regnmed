//! Machine access: who gets in, and what they have done
//! (docs/integrations.md, #45).
//!
//! - GET    /companies/{id}/integrations              grants + usage today
//! - POST   /companies/{id}/integrations              grant access (admin)
//! - POST   /companies/{id}/integrations/{iid}/revoke revoke (admin)
//! - GET    /companies/{id}/integrations/log          changing calls
//!
//! Granting and revoking require admin — that is letting a robot into the
//! company's books. Reading the list and the log requires only access: a
//! revisor must be able to see which machines have written.

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
    krev(&state, person.person_id, company_id, Rett::IntegrasjonLes).await?;
    let rows = regnmed_db::list_integrations(&state.pool, company_id).await?;
    Ok(Json(json!({
        "integrasjoner": rows.iter().map(|g| json!({
            "integration_id": g.integration_id,
            "client_id": g.client_id,
            "navn": g.navn,
            "kontakt": g.kontakt,
            "access": g.access,
            "valid_from": g.valid_from.to_string(),
            "valid_to": g.valid_to.map(|d| d.to_string()),
            "aktiv": g.valid_to.is_none(),
            "created_by": g.created_by,
            "revoked_by": g.revoked_by,
            "rate_limit_min": g.rate_limit_min,
            "kall_i_dag": g.kall_i_dag,
        })).collect::<Vec<_>>(),
    })))
}

#[derive(Deserialize)]
pub struct GrantRequest {
    /// The client_id the IdP issues tokens for — the token's `sub`.
    client_id: String,
    navn: String,
    kontakt: Option<String>,
    /// "les" or "bokforing"; admin is deliberately not grantable to a
    /// machine — changing who has access stays a human decision.
    access: String,
}

pub async fn grant(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(company_id): Path<Uuid>,
    Json(request): Json<GrantRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    krev(&state, person.person_id, company_id, Rett::IntegrasjonAdmin).await?;
    let by = person.display().to_string();
    let id = regnmed_db::grant_integration(
        &state.pool,
        company_id,
        &request.client_id,
        &request.navn,
        request.kontakt.as_deref(),
        &request.access,
        &by,
    )
    .await
    .map_err(|e| ApiError::BadRequest(format!("{e:#}")))?;
    Ok(Json(json!({ "integration_id": id })))
}

pub async fn revoke(
    State(state): State<AppState>,
    person: AuthPerson,
    Path((company_id, integration_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<serde_json::Value>, ApiError> {
    krev(&state, person.person_id, company_id, Rett::IntegrasjonAdmin).await?;
    let by = person.display().to_string();
    regnmed_db::revoke_integration(&state.pool, company_id, integration_id, &by)
        .await
        .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    Ok(Json(json!({ "revoked": true })))
}

pub async fn log(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(company_id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ApiError> {
    krev(&state, person.person_id, company_id, Rett::IntegrasjonLes).await?;
    let rows = regnmed_db::integration_calls(&state.pool, company_id).await?;
    Ok(Json(json!({
        "kall": rows.iter().map(|c| json!({
            "navn": c.navn,
            "method": c.method,
            "path": c.path,
            "status": c.status,
            "tidspunkt": c.created_at.to_rfc3339(),
        })).collect::<Vec<_>>(),
    })))
}
