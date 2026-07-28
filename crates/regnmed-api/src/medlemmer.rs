//! Medlemsadministrasjon (#53, docs/auth.md):
//!
//! - GET    /companies/{id}/access                 hvem har tilgang, og hvordan
//! - PUT    /companies/{id}/access/{person_id}     endre rolle
//! - DELETE /companies/{id}/access/{person_id}     ta bort tilgangen
//! - GET    /companies/{id}/access/history         hvem ga hvem tilgang
//! - GET    /companies/{id}/invitations            åpne invitasjoner
//! - POST   /companies/{id}/invitations            inviter en e-postadresse
//! - DELETE /companies/{id}/invitations/{id}       tilbakekall
//!
//! Alt krever `MEDLEM_ADMIN`. Endepunktene ligger på `/access` og ikke
//! på `/members`, fordi `/companies/{id}/members` alt er attesteringens
//! kandidatliste og bør fortsette å være det.

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
            // Tilgang gjennom et oppdrag styres av engasjementet.
            // Portalen skal vise det, ikke tilby en knapp som ikke
            // virker.
            "kan_endres": m.kan_endres,
        })).collect::<Vec<_>>(),
    })))
}

#[derive(Deserialize)]
pub struct RolleRequest {
    rolle: String,
}

/// Rollen må enten være innebygd eller en aktiv egendefinert rolle i
/// DETTE selskapet. Uten sjekken ville en skrivefeil gitt et medlemskap
/// uten rettigheter — trygt, men helt uforståelig for den det gjelder.
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
    regnmed_db::medlemmer::sett_aktiv(&state.pool, company_id, person_id, false, person.person_id)
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
    regnmed_db::medlemmer::sett_aktiv(&state.pool, company_id, person_id, true, person.person_id)
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
        })).collect::<Vec<_>>(),
    })))
}

#[derive(Deserialize)]
pub struct InviteRequest {
    epost: String,
    rolle: String,
}

/// Inviterer en e-postadresse.
///
/// Svaret sier **ikke** om adressen alt har en bruker hos oss. Det ville
/// gjort enhver selskapsadmin i stand til å slå opp hvem som er bruker
/// på plattformen, ett forsøk om gangen (migrasjon 0037).
pub async fn invite(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(company_id): Path<Uuid>,
    Json(request): Json<InviteRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    krev(&state, person.person_id, company_id, Rett::MedlemAdmin).await?;
    krev_kjent_rolle(&state, company_id, &request.rolle).await?;
    let id = regnmed_db::medlemmer::inviter(
        &state.pool,
        company_id,
        &request.epost,
        &request.rolle,
        person.person_id,
    )
    .await
    .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    Ok(Json(json!({
        "invitasjon_id": id,
        // Invitasjonen gjelder uansett om en e-post gikk ut; den løses
        // inn når adressen logger inn. Utsending er en egen sak — å
        // late som en e-post ble sendt ville vært verre enn å si det.
        "epost_sendt": false,
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
