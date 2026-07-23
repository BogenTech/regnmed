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

/// SAF-T migration import: XML body, admin only, empty ledger only —
/// the whole file lands in one transaction or not at all.
async fn require_admin(
    state: &AppState,
    person_id: uuid::Uuid,
    company_id: uuid::Uuid,
    what: &'static str,
) -> Result<(), ApiError> {
    let access = regnmed_db::company_access(&state.pool, person_id, company_id)
        .await?
        .ok_or(ApiError::NotFound)?;
    if access != "admin" {
        return Err(ApiError::Forbidden(what));
    }
    Ok(())
}

/// The body is either raw SAF-T XML, or (content-type application/json)
/// a wizard envelope `{"file": "<xml>", "mapping": {"15000": "1500"}}`
/// from the kontoplan mapping step.
fn parse_import_body(
    headers: &axum::http::HeaderMap,
    body: &str,
) -> Result<regnmed_core::saft_import::SaftFile, ApiError> {
    let is_json = headers
        .get(axum::http::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|ct| ct.contains("application/json"));
    if !is_json {
        return regnmed_core::saft_import::parse(body)
            .map_err(|e| ApiError::BadRequest(format!("SAF-T: {e}")));
    }
    #[derive(serde::Deserialize)]
    struct Envelope {
        file: String,
        #[serde(default)]
        mapping: std::collections::HashMap<String, String>,
    }
    let envelope: Envelope = serde_json::from_str(body)
        .map_err(|e| ApiError::BadRequest(format!("ugyldig konvolutt: {e}")))?;
    let mut file = regnmed_core::saft_import::parse(&envelope.file)
        .map_err(|e| ApiError::BadRequest(format!("SAF-T: {e}")))?;
    regnmed_core::kontoplan::apply_mapping(&mut file, &envelope.mapping)
        .map_err(ApiError::BadRequest)?;
    Ok(file)
}

pub async fn import_saft(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(company_id): Path<uuid::Uuid>,
    headers: axum::http::HeaderMap,
    body: String,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_admin(
        &state,
        person.person_id,
        company_id,
        "SAF-T-import krever admin-tilgang",
    )
    .await?;
    let file = parse_import_body(&headers, &body)?;
    let created_by = person.name.as_deref().unwrap_or(&person.sub);
    let report = regnmed_db::import_saft(&state.pool, company_id, &file, created_by)
        .await
        .map_err(|e| ApiError::BadRequest(format!("{e:#}")))?;
    Ok(Json(json!({
        "accounts": report.accounts,
        "customers": report.customers,
        "suppliers": report.suppliers,
        "vouchers": report.vouchers,
        "opening_posted": report.opening_posted,
        "warnings": report.warnings,
    })))
}

/// Kontoplan wizard, step 1: parse the file and suggest a mapping to
/// NS 4102 for every account. Nothing is written; the administrator
/// reviews, adjusts, and re-posts to the import endpoint with the
/// mapping envelope.
pub async fn analyze_saft(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(company_id): Path<uuid::Uuid>,
    body: String,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_admin(
        &state,
        person.person_id,
        company_id,
        "SAF-T-import krever admin-tilgang",
    )
    .await?;
    let file = regnmed_core::saft_import::parse(&body)
        .map_err(|e| ApiError::BadRequest(format!("SAF-T: {e}")))?;
    let accounts: Vec<(String, String)> = file
        .accounts
        .iter()
        .map(|a| (a.account_id.clone(), a.name.clone()))
        .collect();
    let suggestions = regnmed_core::kontoplan::suggest(&accounts);
    let needs_mapping = suggestions.iter().any(|s| s.reason != "allerede NS 4102");
    Ok(Json(json!({
        "needs_mapping": needs_mapping,
        "transactions": file.transactions.len(),
        "customers": file.customers.len(),
        "suppliers": file.suppliers.len(),
        "accounts": suggestions.iter().map(|s| json!({
            "account_id": s.account_id,
            "name": s.name,
            "suggested": s.suggested,
            "standard_name": s.standard_name,
            "reason": s.reason,
        })).collect::<Vec<_>>(),
    })))
}

#[derive(serde::Deserialize)]
pub struct OpeningLine {
    account: String,
    amount_ore: i64,
}

#[derive(serde::Deserialize)]
pub struct OpeningRequest {
    date: chrono::NaiveDate,
    lines: Vec<OpeningLine>,
}

/// Manual åpningsbalanse for companies without a SAF-T export.
pub async fn opening_balance(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(company_id): Path<uuid::Uuid>,
    Json(request): Json<OpeningRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_admin(
        &state,
        person.person_id,
        company_id,
        "åpningsbalanse krever admin-tilgang",
    )
    .await?;
    let lines: Vec<(String, i64)> = request
        .lines
        .iter()
        .map(|l| (l.account.clone(), l.amount_ore))
        .collect();
    let created_by = person.name.as_deref().unwrap_or(&person.sub);
    let report =
        regnmed_db::post_opening_balance(&state.pool, company_id, request.date, &lines, created_by)
            .await
            .map_err(|e| ApiError::BadRequest(format!("{e:#}")))?;
    Ok(Json(json!({
        "voucher": format!("{}-{}", report.posted.fiscal_year, report.posted.voucher_number),
        "warnings": report.warnings,
    })))
}
