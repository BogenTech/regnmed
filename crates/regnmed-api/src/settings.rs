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
use crate::tilgang::{Rett, krev};

pub async fn get_settings(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(company_id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ApiError> {
    krev(&state, person.person_id, company_id, Rett::SelskapLes).await?;
    let s = regnmed_db::company_settings(&state.pool, company_id).await?;
    let today: chrono::NaiveDate = sqlx::query_scalar("select current_date")
        .fetch_one(&state.pool)
        .await
        .map_err(anyhow::Error::from)?;
    let reg = regnmed_db::registrering_on(&state.pool, company_id, today).await?;
    let historikk = regnmed_db::registrering_history(&state.pool, company_id).await?;
    Ok(Json(json!({
        "name": s.name,
        "orgnr": s.orgnr,
        "address": s.address,
        "bank_account": s.bank_account,
        "orgform": s.orgform,
        "email": s.email,
        // Registreringsstatus (§5-1-2, #81): lagret og datert, aldri
        // utledet av dokumentet som rendres.
        "mva_registrert": reg.mva_registrert,
        "foretaksregistrert": reg.foretaksregistrert,
        "registrering_historikk": historikk.iter().map(|(dato, r, kilde, notat)| json!({
            "valid_from": dato.to_string(),
            "mva_registrert": r.mva_registrert,
            "foretaksregistrert": r.foretaksregistrert,
            "kilde": kilde,
            "notat": notat,
        })).collect::<Vec<_>>(),
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
    krev(&state, person.person_id, company_id, Rett::SelskapAdmin).await?;
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
pub struct RegistreringRequest {
    mva_registrert: bool,
    foretaksregistrert: bool,
    /// The date the status took effect. Omitted = today; a registration
    /// that happened earlier should say so, since documents dated
    /// before it must not carry the påtegning.
    valid_from: Option<chrono::NaiveDate>,
    notat: Option<String>,
}

/// Records the company's registration status (§5-1-2, #81). A new dated
/// row — the previous one stays as the record of what applied then, so
/// old salgsdokumenter keep rendering with the status they were issued
/// under.
pub async fn set_registrering(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(company_id): Path<Uuid>,
    Json(request): Json<RegistreringRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    krev(&state, person.person_id, company_id, Rett::SelskapAdmin).await?;
    let valid_from = match request.valid_from {
        Some(dato) => dato,
        None => sqlx::query_scalar("select current_date")
            .fetch_one(&state.pool)
            .await
            .map_err(anyhow::Error::from)?,
    };
    regnmed_db::record_registrering(
        &state.pool,
        company_id,
        valid_from,
        regnmed_db::Registrering {
            mva_registrert: request.mva_registrert,
            foretaksregistrert: request.foretaksregistrert,
        },
        "manuell",
        request.notat.as_deref(),
        person.name.as_deref().unwrap_or(&person.sub),
    )
    .await
    .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    Ok(Json(json!({ "valid_from": valid_from.to_string() })))
}

#[derive(Deserialize)]
pub struct PartyContactRequest {
    address: Option<String>,
    email: Option<String>,
    /// Kontonummer for remittering (11 siffer, MOD11-validert).
    bank_account: Option<String>,
}

pub async fn update_party_contact(
    State(state): State<AppState>,
    person: AuthPerson,
    Path((company_id, party_id)): Path<(Uuid, Uuid)>,
    Json(request): Json<PartyContactRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    krev(&state, person.person_id, company_id, Rett::KontaktSkriv).await?;
    regnmed_db::update_party_contact(
        &state.pool,
        company_id,
        party_id,
        request.address.as_deref(),
        request.email.as_deref(),
        request.bank_account.as_deref(),
    )
    .await
    .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    Ok(Json(json!({ "updated": true })))
}
