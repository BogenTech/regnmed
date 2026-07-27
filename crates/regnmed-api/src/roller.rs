//! Egendefinerte roller (#60, docs/auth.md):
//!
//! - GET  /companies/{id}/roles            innebygde + selskapets egne
//! - POST /companies/{id}/roles            lag en rolle
//! - PUT  /companies/{id}/roles/{role_id}  sett rettighetene
//! - POST /companies/{id}/roles/{role_id}/deactivate|restore
//! - GET  /companies/{id}/roles/history
//!
//! Alt krever `MEDLEM_ADMIN`: å sette sammen en rolle er å bestemme hvem
//! som får gjøre hva, og hører til samme myndighet som å tildele den.

use axum::Json;
use axum::extract::{Path, State};
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

use crate::AppState;
use crate::auth::{ApiError, AuthPerson};
use crate::tilgang::{Rett, Rolle, krev};

/// Rettighetene, med det navnet portalen viser dem under.
fn rett_json(r: Rett) -> serde_json::Value {
    json!({ "rett": r.slug(), "kan_delegeres": r.kan_delegeres() })
}

pub async fn list_roles(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(company_id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ApiError> {
    krev(&state, person.person_id, company_id, Rett::MedlemAdmin).await?;
    let egne = regnmed_db::roller::list_roller(&state.pool, company_id).await?;
    let innebygde = [
        Rolle::Ansatt,
        Rolle::Les,
        Rolle::Revisor,
        Rolle::Bokforing,
        Rolle::Admin,
    ];
    Ok(Json(json!({
        // Innebygde roller kan ikke endres. De vises som de er, så en
        // admin ser hva de faktisk betyr uten å måtte lese koden.
        "innebygde": innebygde.iter().map(|r| json!({
            "navn": r.slug(),
            "rettigheter": r.rettigheter().iter().map(|x| x.slug()).collect::<Vec<_>>(),
        })).collect::<Vec<_>>(),
        "egne": egne.iter().map(|r| json!({
            "id": r.id,
            "navn": r.navn,
            "aktiv": r.aktiv,
            "rettigheter": r.rettigheter,
            "i_bruk": r.i_bruk,
        })).collect::<Vec<_>>(),
        // Vokabularet, så portalen kan bygge avkrysningsrutenettet uten
        // å ha en egen kopi av listen.
        "vokabular": Rett::ALLE.iter().map(|r| rett_json(*r)).collect::<Vec<_>>(),
    })))
}

#[derive(Deserialize)]
pub struct RolleRequest {
    navn: Option<String>,
    #[serde(default)]
    rettigheter: Vec<String>,
}

/// Oversetter navnene til rettigheter og nekter det som ikke kan
/// delegeres.
///
/// Feiler HØYLYTT på et ukjent navn i stedet for å ignorere det: her
/// skriver et menneske, og en rolle som stilltiende mangler halve
/// innholdet er verre enn en feilmelding. (Ved oppslag ignoreres ukjente
/// navn — der er de en gammel database, ikke en skrivefeil.)
fn godkjenn(navn: &[String]) -> Result<Vec<String>, ApiError> {
    let mut ut = Vec::new();
    for n in navn {
        let rett = Rett::fra_slug(n)
            .ok_or_else(|| ApiError::BadRequest(format!("ukjent rettighet «{n}»")))?;
        if !rett.kan_delegeres() {
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
    krev(&state, person.person_id, company_id, Rett::MedlemAdmin).await?;
    let navn = request
        .navn
        .ok_or_else(|| ApiError::BadRequest("rollen må ha et navn".into()))?;
    let godkjente = godkjenn(&request.rettigheter)?;
    let id = regnmed_db::roller::opprett(
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
    krev(&state, person.person_id, company_id, Rett::MedlemAdmin).await?;
    let godkjente = godkjenn(&request.rettigheter)?;
    regnmed_db::roller::sett_rettigheter(
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
    krev(&state, person.person_id, company_id, Rett::MedlemAdmin).await?;
    regnmed_db::roller::sett_aktiv(&state.pool, company_id, role_id, false, person.person_id)
        .await
        .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    Ok(Json(json!({ "ok": true })))
}

pub async fn restore(
    State(state): State<AppState>,
    person: AuthPerson,
    Path((company_id, role_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<serde_json::Value>, ApiError> {
    krev(&state, person.person_id, company_id, Rett::MedlemAdmin).await?;
    regnmed_db::roller::sett_aktiv(&state.pool, company_id, role_id, true, person.person_id)
        .await
        .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    Ok(Json(json!({ "ok": true })))
}

pub async fn history(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(company_id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ApiError> {
    krev(&state, person.person_id, company_id, Rett::MedlemAdmin).await?;
    let rader = regnmed_db::roller::historikk(&state.pool, company_id).await?;
    Ok(Json(json!({
        "endringer": rader.iter().map(|e| json!({
            "navn": e.navn,
            "endring": e.endring,
            "rettigheter": e.rettigheter,
            "utfort_av": e.utfort_av,
            "tidspunkt": e.created_at.to_rfc3339(),
        })).collect::<Vec<_>>(),
    })))
}
