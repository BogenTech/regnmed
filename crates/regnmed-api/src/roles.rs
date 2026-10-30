//! Custom roles (#60, docs/auth.md):
//!
//! - GET  /companies/{id}/roles            built-in + the company's own
//! - POST /companies/{id}/roles            create a role
//! - PUT  /companies/{id}/roles/{role_id}  set its rettigheter
//! - POST /companies/{id}/roles/{role_id}/deactivate|restore
//! - GET  /companies/{id}/roles/history
//!
//! Everything requires `MEDLEM_ADMIN`: composing a role is deciding who
//! may do what, and belongs to the same authority as assigning one.

use axum::Json;
use axum::extract::{Path, State};
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

use crate::AppState;
use crate::access::{Right, Role, require};
use crate::auth::{ApiError, AuthPerson};

/// The rettigheter, with the name the portal displays them under.
fn rett_json(r: Right) -> serde_json::Value {
    json!({
        "rett": r.slug(),
        "gruppe": r.group(),
        "beskrivelse": r.description(),
        "kan_delegeres": r.can_be_delegated(),
    })
}

pub async fn list_roles(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(company_id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require(&state, person.person_id, company_id, Right::MemberAdmin).await?;
    let egne = regnmed_db::roles::list_roller(&state.pool, company_id).await?;
    let innebygde = [
        Role::Employee,
        Role::Read,
        Role::Auditor,
        Role::Bookkeeping,
        Role::Admin,
    ];
    Ok(Json(json!({
        // Built-in roles cannot be changed. They are shown as they are,
        // so an admin sees what they actually mean without reading code.
        "innebygde": innebygde.iter().map(|r| json!({
            "navn": r.slug(),
            "beskrivelse": r.description(),
            "rettigheter": r.rights().iter().map(|x| x.slug()).collect::<Vec<_>>(),
        })).collect::<Vec<_>>(),
        "egne": egne.iter().map(|r| json!({
            "id": r.id,
            "navn": r.navn,
            "aktiv": r.aktiv,
            "rettigheter": r.rights,
            "i_bruk": r.i_bruk,
        })).collect::<Vec<_>>(),
        // The vocabulary, so the portal can build the checkbox grid
        // without keeping its own copy of the list.
        "vokabular": Right::ALLE.iter().map(|r| rett_json(*r)).collect::<Vec<_>>(),
    })))
}

#[derive(Deserialize)]
pub struct RolleRequest {
    navn: Option<String>,
    #[serde(default, rename = "rettigheter")]
    rights: Vec<String>,
}

/// Translates the names into rettigheter and refuses what cannot be
/// delegated.
///
/// Fails LOUDLY on an unknown name rather than ignoring it: a human is
/// writing here, and a role that silently lacks half its contents is
/// worse than an error message. (On lookup, unknown names ARE ignored —
/// there they mean an old database, not a typo.)
fn godkjenn(navn: &[String]) -> Result<Vec<String>, ApiError> {
    let mut ut = Vec::new();
    for n in navn {
        let rett = Right::from_slug(n)
            .ok_or_else(|| ApiError::BadRequest(format!("ukjent rettighet «{n}»")))?;
        if !rett.can_be_delegated() {
            return Err(ApiError::BadRequest(format!(
                "«{}» kan ikke legges i en egendefinert rolle: den styrer hvem som har \
                 tilgang, og en rolle som kan endre tilganger kan gi seg selv alt annet",
                rett.slug()
            )));
        }
        ut.push(rett.slug().to_string());
    }
    ut.sort();
    ut.dedup();
    Ok(ut)
}

pub async fn create_role(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(company_id): Path<Uuid>,
    Json(request): Json<RolleRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require(&state, person.person_id, company_id, Right::MemberAdmin).await?;
    let navn = request
        .navn
        .ok_or_else(|| ApiError::BadRequest("rollen må ha et navn".into()))?;
    let godkjente = godkjenn(&request.rights)?;
    let id = regnmed_db::roles::opprett(
        &state.pool,
        company_id,
        &navn,
        &godkjente,
        person.person_id,
        person.display(),
    )
    .await
    .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    Ok(Json(json!({ "role_id": id })))
}

pub async fn set_rights(
    State(state): State<AppState>,
    person: AuthPerson,
    Path((company_id, role_id)): Path<(Uuid, Uuid)>,
    Json(request): Json<RolleRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require(&state, person.person_id, company_id, Right::MemberAdmin).await?;
    let godkjente = godkjenn(&request.rights)?;
    regnmed_db::roles::sett_rettigheter(
        &state.pool,
        company_id,
        role_id,
        &godkjente,
        person.person_id,
    )
    .await
    .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    Ok(Json(json!({ "ok": true })))
}

pub async fn deactivate(
    State(state): State<AppState>,
    person: AuthPerson,
    Path((company_id, role_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require(&state, person.person_id, company_id, Right::MemberAdmin).await?;
    regnmed_db::roles::sett_aktiv(&state.pool, company_id, role_id, false, person.person_id)
        .await
        .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    Ok(Json(json!({ "ok": true })))
}

pub async fn restore(
    State(state): State<AppState>,
    person: AuthPerson,
    Path((company_id, role_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require(&state, person.person_id, company_id, Right::MemberAdmin).await?;
    regnmed_db::roles::sett_aktiv(&state.pool, company_id, role_id, true, person.person_id)
        .await
        .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    Ok(Json(json!({ "ok": true })))
}

pub async fn history(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(company_id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require(&state, person.person_id, company_id, Right::MemberAdmin).await?;
    let rader = regnmed_db::roles::historikk(&state.pool, company_id).await?;
    Ok(Json(json!({
        "endringer": rader.iter().map(|e| json!({
            "navn": e.navn,
            "endring": e.endring,
            "rettigheter": e.rights,
            "utfort_av": e.utfort_av,
            "tidspunkt": e.created_at.to_rfc3339(),
        })).collect::<Vec<_>>(),
    })))
}
