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
//!
//! Since #101 the grant carries a ROLE NAME, the same vocabulary a
//! member gets, `admin` included when the owner decides it. The ceiling
//! is `Rett::kan_delegeres`, enforced where access is resolved
//! (`tilgang::slaa_opp`) rather than by a shorter list of roles here: a
//! machine does the work, but never widens anyone's access.

use axum::Json;
use axum::extract::{Path, State};
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

use crate::AppState;
use crate::auth::{ApiError, AuthPerson};
use crate::tilgang::{Rett, Rolle, krev};

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
            "avatar_farge": g.avatar_farge,
            "avatar_ikon": g.avatar_ikon,
        })).collect::<Vec<_>>(),
        // The face's colours are served from the code that validates
        // them, so the picker cannot offer one the server rejects.
        "farger": regnmed_db::AVATAR_FARGER,
    })))
}

#[derive(Deserialize)]
pub struct GrantRequest {
    /// The client_id the IdP issues tokens for — the token's `sub`.
    client_id: String,
    navn: String,
    kontakt: Option<String>,
    /// A role name, exactly as a member gets one: a built-in role or one
    /// of the company's own (#101). `revisor` is not assignable here any
    /// more than it is to a person — it follows a revisjonsoppdrag.
    access: String,
}

/// The roles an owner may hand a machine.
///
/// `revisor` is absent for the same reason it is absent for people: it
/// comes from an oppdrag, never from an assignment. `ukjent` is not a
/// role. Everything else is on the table — including `admin`, which is
/// the revision #101 made explicitly: the ceiling is
/// [`Rett::kan_delegeres`], enforced in the access lookup, not a
/// shorter list of roles here.
const TILDELBARE: [Rolle; 4] = [Rolle::Ansatt, Rolle::Les, Rolle::Bokforing, Rolle::Admin];

/// Refuses a role name that is neither built-in nor one of the
/// company's own ACTIVE roles. Without this a typo would be stored and
/// then resolve to `Rolle::Ukjent` — an integration with a grant and no
/// access, which looks like a bug in the API rather than in the request.
async fn krev_gyldig_rolle(state: &AppState, company_id: Uuid, navn: &str) -> Result<(), ApiError> {
    let navn = navn.trim();
    if TILDELBARE.iter().any(|r| r.slug() == navn) {
        return Ok(());
    }
    let egne = regnmed_db::roller::list_roller(&state.pool, company_id).await?;
    if egne.iter().any(|r| r.aktiv && r.navn == navn) {
        return Ok(());
    }
    let valg: Vec<&str> = TILDELBARE
        .iter()
        .map(|r| r.slug())
        .chain(egne.iter().filter(|r| r.aktiv).map(|r| r.navn.as_str()))
        .collect();
    Err(ApiError::BadRequest(format!(
        "«{navn}» er ingen rolle i dette selskapet — velg en av: {}",
        valg.join(", ")
    )))
}

pub async fn grant(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(company_id): Path<Uuid>,
    Json(request): Json<GrantRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    krev(&state, person.person_id, company_id, Rett::IntegrasjonAdmin).await?;
    krev_gyldig_rolle(&state, company_id, &request.access).await?;
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

#[derive(Deserialize)]
pub struct EndreRequest {
    navn: String,
    kontakt: Option<String>,
    avatar_farge: Option<String>,
    avatar_ikon: Option<String>,
}

/// Rename an agent and give it a face (docs/agent.md §8.1). Takes
/// `IntegrasjonAdmin` like the grant itself: a name is what the trail
/// shows for everything the robot does, so changing it is administering
/// machine access, not decoration.
pub async fn endre(
    State(state): State<AppState>,
    person: AuthPerson,
    Path((company_id, integration_id)): Path<(Uuid, Uuid)>,
    Json(request): Json<EndreRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    krev(&state, person.person_id, company_id, Rett::IntegrasjonAdmin).await?;
    regnmed_db::update_integration(
        &state.pool,
        company_id,
        integration_id,
        &request.navn,
        request.kontakt.as_deref(),
        request.avatar_farge.as_deref(),
        request.avatar_ikon.as_deref(),
    )
    .await
    .map_err(|e| ApiError::BadRequest(format!("{e:#}")))?;
    Ok(Json(json!({ "navn": request.navn.trim() })))
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
