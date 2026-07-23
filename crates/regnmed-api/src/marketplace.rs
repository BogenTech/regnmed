//! Marketplace endpoints — onboarding from the official registries:
//!
//! - GET  /registry/enheter/{orgnr}   preview: BRREG facts + autorisasjoner
//! - POST /companies                  {orgnr} — onboard from Enhetsregisteret
//! - POST /firms                      {orgnr, kind} — requires verified
//!                                    autorisasjon in Finanstilsynets register
//!
//! Any authenticated person may onboard: the creator becomes the
//! company's/firm's admin. Names always come from the registry, never
//! from user input — the marketplace's trust starts here.

use axum::Json;
use axum::extract::{Path, State};
use serde::Deserialize;
use serde_json::json;

use crate::AppState;
use crate::auth::{ApiError, AuthPerson};

fn validate_orgnr(orgnr: &str) -> Result<(), ApiError> {
    if !regnmed_core::orgnr::is_valid(orgnr) {
        return Err(ApiError::BadRequest(format!(
            "{orgnr} er ikke et gyldig organisasjonsnummer"
        )));
    }
    Ok(())
}

pub async fn registry_preview(
    _person: AuthPerson,
    Path(orgnr): Path<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    validate_orgnr(&orgnr)?;
    let enhet = regnmed_gov::brreg::BrregClient::from_env()
        .enhet(&orgnr)
        .await
        .map_err(|e| ApiError::BadRequest(e.to_string()))?
        .ok_or(ApiError::NotFound)?;

    let finanstilsynet = regnmed_gov::finanstilsynet::FinanstilsynetClient::from_env();
    let regnskap = finanstilsynet
        .has_autorisasjon(&orgnr, "regnskap")
        .await
        .unwrap_or(false);
    let revisjon = finanstilsynet
        .has_autorisasjon(&orgnr, "revisjon")
        .await
        .unwrap_or(false);

    Ok(Json(json!({
        "orgnr": enhet.organisasjonsnummer,
        "navn": enhet.navn,
        "organisasjonsform": enhet.organisasjonsform.as_ref().map(|k| k.kode.clone()),
        "naeringskode": enhet.naeringskode1.as_ref().map(|k| format!("{} {}", k.kode, k.beskrivelse)),
        "mva_registrert": enhet.registrert_i_mvaregisteret,
        "konkurs": enhet.konkurs,
        "slettet": enhet.slettedato.is_some(),
        "autorisasjon": { "regnskap": regnskap, "revisjon": revisjon },
    })))
}

#[derive(Deserialize)]
pub struct OnboardRequest {
    orgnr: String,
}

pub async fn onboard_company(
    State(state): State<AppState>,
    person: AuthPerson,
    Json(request): Json<OnboardRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    validate_orgnr(&request.orgnr)?;
    let enhet = regnmed_gov::brreg::BrregClient::from_env()
        .enhet(&request.orgnr)
        .await
        .map_err(|e| ApiError::BadRequest(e.to_string()))?
        .ok_or_else(|| ApiError::BadRequest("orgnr finnes ikke i Enhetsregisteret".into()))?;
    if enhet.slettedato.is_some() {
        return Err(ApiError::BadRequest(
            "enheten er slettet i Enhetsregisteret".into(),
        ));
    }
    if enhet.konkurs {
        return Err(ApiError::BadRequest(
            "enheten er registrert som konkurs".into(),
        ));
    }

    let onboarded =
        regnmed_db::onboard_company(&state.pool, &request.orgnr, &enhet.navn, person.person_id)
            .await
            .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    Ok(Json(json!({
        "company_id": onboarded.company_id,
        "orgnr": request.orgnr,
        "navn": onboarded.name,
        "seeded_accounts": onboarded.seeded_accounts,
    })))
}

#[derive(Deserialize)]
pub struct FirmRequest {
    orgnr: String,
    /// 'regnskap' or 'revisjon'.
    kind: String,
}

pub async fn create_firm(
    State(state): State<AppState>,
    person: AuthPerson,
    Json(request): Json<FirmRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    validate_orgnr(&request.orgnr)?;
    let enhet = regnmed_gov::brreg::BrregClient::from_env()
        .enhet(&request.orgnr)
        .await
        .map_err(|e| ApiError::BadRequest(e.to_string()))?
        .ok_or_else(|| ApiError::BadRequest("orgnr finnes ikke i Enhetsregisteret".into()))?;

    let verified = regnmed_gov::finanstilsynet::FinanstilsynetClient::from_env()
        .has_autorisasjon(&request.orgnr, &request.kind)
        .await
        .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    if !verified {
        return Err(ApiError::Forbidden(
            "orgnr har ingen aktiv autorisasjon i Finanstilsynets register",
        ));
    }

    let firm_id = regnmed_db::create_verified_firm(
        &state.pool,
        &request.orgnr,
        &enhet.navn,
        &request.kind,
        "finanstilsynet-virksomhetsregister",
        person.person_id,
    )
    .await
    .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    Ok(Json(json!({
        "firm_id": firm_id,
        "orgnr": request.orgnr,
        "navn": enhet.navn,
        "kind": request.kind,
        "autorisasjon_verified": true,
    })))
}
