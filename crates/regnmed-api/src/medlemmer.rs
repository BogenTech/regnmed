//! Membership administration (#53, docs/auth.md):
//!
//! - GET    /companies/{id}/access                 who has access, and how
//! - PUT    /companies/{id}/access/{person_id}     change role
//! - DELETE /companies/{id}/access/{person_id}     remove access
//! - GET    /companies/{id}/access/history         who granted whom access
//! - GET    /companies/{id}/invitations            open invitations
//! - POST   /companies/{id}/invitations            invite an e-mail address
//! - DELETE /companies/{id}/invitations/{id}       revoke
//!
//! Everything requires `MEDLEM_ADMIN`. The endpoints live under `/access`
//! rather than `/members`, because `/companies/{id}/members` is already
//! attestering's candidate list and should stay that.

use axum::Json;
use axum::extract::{Path, State};
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

use crate::AppState;
use crate::auth::{ApiError, AuthPerson};
use crate::tilgang::{Rett, krev};

pub async fn list_access(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(company_id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ApiError> {
    krev(&state, person.person_id, company_id, Rett::MedlemAdmin).await?;
    let medlemmer = regnmed_db::medlemmer::list_medlemmer(&state.pool, company_id).await?;
    Ok(Json(json!({
        "medlemmer": medlemmer.iter().map(|m| json!({
            "person_id": m.person_id,
            "navn": m.navn,
            "epost": m.epost,
            "rolle": m.rolle,
            "aktiv": m.aktiv,
            "via": m.via,
            // Access through an oppdrag is governed by the engagement.
            // The portal should show it, not offer a button that does
            // virker.
            "kan_endres": m.kan_endres,
        })).collect::<Vec<_>>(),
    })))
}

#[derive(Deserialize)]
pub struct RolleRequest {
    rolle: String,
}

/// The role must be either built-in or an active custom role in this
/// company. Otherwise the membership would be created
/// without any rettigheter — safe, but completely baffling to the person
/// concerned.
async fn krev_kjent_rolle(state: &AppState, company_id: Uuid, rolle: &str) -> Result<(), ApiError> {
    if regnmed_db::medlemmer::ROLLER.contains(&rolle) {
        return Ok(());
    }
    if regnmed_db::roller::finnes(&state.pool, company_id, rolle).await? {
        return Ok(());
    }
    Err(ApiError::BadRequest(format!(
        "«{rolle}» er verken en innebygd rolle eller en aktiv rolle i dette selskapet"
    )))
}

pub async fn set_role(
    State(state): State<AppState>,
    person: AuthPerson,
    Path((company_id, person_id)): Path<(Uuid, Uuid)>,
    Json(request): Json<RolleRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    krev(&state, person.person_id, company_id, Rett::MedlemAdmin).await?;
    krev_kjent_rolle(&state, company_id, &request.rolle).await?;
    regnmed_db::medlemmer::sett_rolle(
        &state.pool,
        company_id,
        person_id,
        &request.rolle,
        person.person_id,
    )
    .await
    .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    Ok(Json(json!({ "ok": true })))
}

pub async fn revoke_access(
    State(state): State<AppState>,
    person: AuthPerson,
    Path((company_id, person_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<serde_json::Value>, ApiError> {
    krev(&state, person.person_id, company_id, Rett::MedlemAdmin).await?;
    regnmed_db::medlemmer::sett_aktiv(
        &state.pool,
        company_id,
        person_id,
        false,
        person.person_id,
        "admin",
    )
    .await
    .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    Ok(Json(json!({ "ok": true })))
}

pub async fn restore_access(
    State(state): State<AppState>,
    person: AuthPerson,
    Path((company_id, person_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<serde_json::Value>, ApiError> {
    krev(&state, person.person_id, company_id, Rett::MedlemAdmin).await?;
    regnmed_db::medlemmer::sett_aktiv(
        &state.pool,
        company_id,
        person_id,
        true,
        person.person_id,
        "admin",
    )
    .await
    .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    Ok(Json(json!({ "ok": true })))
}

pub async fn access_history(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(company_id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ApiError> {
    krev(&state, person.person_id, company_id, Rett::MedlemAdmin).await?;
    let rader = regnmed_db::medlemmer::tilgangshistorikk(&state.pool, company_id).await?;
    Ok(Json(json!({
        "endringer": rader.iter().map(|e| json!({
            "navn": e.navn,
            "endring": e.endring,
            "fra_rolle": e.fra_rolle,
            "til_rolle": e.til_rolle,
            "utfort_av": e.utfort_av,
            "kilde": e.kilde,
            "notat": e.notat,
            "tidspunkt": e.created_at.to_rfc3339(),
        })).collect::<Vec<_>>(),
    })))
}

pub async fn list_invitations(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(company_id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ApiError> {
    krev(&state, person.person_id, company_id, Rett::MedlemAdmin).await?;
    let rader = regnmed_db::medlemmer::list_invitasjoner(&state.pool, company_id).await?;
    Ok(Json(json!({
        "invitasjoner": rader.iter().map(|i| json!({
            "id": i.id,
            "epost": i.epost,
            "rolle": i.rolle,
            "invitert_av": i.invitert_av,
            "tidspunkt": i.created_at.to_rfc3339(),
            "sist_sendt": i.sist_sendt.map(|t| t.to_rfc3339()),
            "employee_id": i.employee_id,
        })).collect::<Vec<_>>(),
    })))
}

#[derive(Deserialize)]
pub struct InviteRequest {
    epost: String,
    rolle: String,
    /// Lønnsmottaker to link on redemption (docs/lonn.md, 0050).
    /// Requires LONN_SKRIV in addition to MEDLEM_ADMIN — the link
    /// decides who may read a payslip.
    employee_id: Option<Uuid>,
}

/// Inviterer en e-postadresse.
///
/// The response does **not** say whether the address already has a user
/// with us. That would let any company admin look up who is a user on the
/// platform, one attempt at a time (migration 0037).
pub async fn invite(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(company_id): Path<Uuid>,
    Json(request): Json<InviteRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    krev(&state, person.person_id, company_id, Rett::MedlemAdmin).await?;
    krev_kjent_rolle(&state, company_id, &request.rolle).await?;
    if request.employee_id.is_some() {
        krev(&state, person.person_id, company_id, Rett::LonnSkriv).await?;
    }
    let id = regnmed_db::medlemmer::inviter(
        &state.pool,
        company_id,
        &request.epost,
        &request.rolle,
        person.person_id,
        request.employee_id,
    )
    .await
    .map_err(|e| ApiError::BadRequest(e.to_string()))?;

    // The invitation stands whether or not the e-mail went out: the
    // invitation IS the grant, the mail only announces it. A queue that
    // is down must not take membership administration down with it — so
    // the failure is reported, never raised (#66).
    let feil = crate::utsendelse::try_send_invitation(&state, &person, company_id, id)
        .await
        .err();
    Ok(Json(json!({
        "invitasjon_id": id,
        "epost_sendt": feil.is_none(),
        "epost_grunn": feil,
    })))
}

pub async fn revoke_invitation(
    State(state): State<AppState>,
    person: AuthPerson,
    Path((company_id, invitasjon_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<serde_json::Value>, ApiError> {
    krev(&state, person.person_id, company_id, Rett::MedlemAdmin).await?;
    regnmed_db::medlemmer::tilbakekall_invitasjon(
        &state.pool,
        company_id,
        invitasjon_id,
        person.person_id,
    )
    .await
    .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    Ok(Json(json!({ "ok": true })))
}
