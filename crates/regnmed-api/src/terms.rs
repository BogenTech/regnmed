//! Customer terms (docs/kundevilkar.md, #185).
//!
//! - GET  /vilkar                             the text, as plain text (open)
//! - GET  /vilkar/{versjon}                   any earlier version, as plain text (open)
//! - GET  /terms                              the current text and its version (open)
//! - GET  /terms/versions                     every version, newest first (open)
//! - GET  /companies/{id}/terms               has the company accepted it, and what before?
//! - POST /companies/{id}/terms/accept        accept it (SELSKAP_ADMIN)
//! - GET  /companies/{id}/authority-calls     the §7.1 register of calls to the authorities
//!
//! The text is served from the binary, so the version a customer accepts
//! is exactly the text this release shows: its SHA-256. Every endpoint
//! that files with an authority on a customer's behalf calls
//! [`require_accepted`] first — Digdir's and Skatteetaten's terms allow no
//! filing without an agreement with the customer.
//!
//! A version takes effect on the date in its archive file name, so a
//! changed text can be ANNOUNCED (terms §14.1, 30 days' notice): until its
//! date the version before it still governs, and a company that accepted
//! that one can keep filing.

use axum::Json;
use axum::extract::{Path, State};
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

use crate::AppState;
use crate::access::{Right, require};
use crate::auth::{ApiError, AuthPerson};

pub use regnmed_db::terms::{TEXT, VERSION};
use regnmed_db::terms::{covered, effective_date, versions};

/// The text itself, readable at a stable address (regnmed.no/vilkar).
pub async fn page() -> impl axum::response::IntoResponse {
    (
        [(
            axum::http::header::CONTENT_TYPE,
            "text/plain; charset=utf-8",
        )],
        TEXT,
    )
}

pub async fn version_page(
    Path(version): Path<String>,
) -> Result<impl axum::response::IntoResponse, ApiError> {
    let (_, _, text) = versions()
        .into_iter()
        .find(|(_, sha, _)| *sha == version)
        .ok_or(ApiError::NotFound)?;
    Ok((
        [(
            axum::http::header::CONTENT_TYPE,
            "text/plain; charset=utf-8",
        )],
        text,
    ))
}

pub async fn list_versions() -> Json<serde_json::Value> {
    Json(json!(
        versions()
            .into_iter()
            .map(|(date, sha, _)| json!({
                "versjon": sha,
                "gjelder_fra": date,
                "gjeldende": sha == VERSION.as_str(),
            }))
            .collect::<Vec<_>>()
    ))
}

/// Whether the company may file today under the terms it has accepted.
fn may_file(history: &[regnmed_db::terms::Acceptance]) -> bool {
    let accepted: Vec<String> = history.iter().map(|a| a.terms_sha256.clone()).collect();
    // shortcut: the day turns at midnight UTC, not Oslo, so a new version
    // takes effect up to two hours late -- the old text governs meanwhile.
    let today = chrono::Utc::now().date_naive().to_string();
    covered(&versions(), &accepted, &today)
}

pub async fn current() -> Json<serde_json::Value> {
    Json(json!({ "versjon": *VERSION, "gjelder_fra": effective_date(), "tekst": TEXT }))
}

fn status_json(
    a: Option<regnmed_db::terms::Acceptance>,
    history: &[regnmed_db::terms::Acceptance],
) -> serde_json::Value {
    json!({
        "versjon": *VERSION,
        "gjelder_fra": effective_date(),
        // May the company file today? True also while a newer, announced
        // version waits for acceptance -- until its date.
        "kan_sende_inn": may_file(history),
        "godtatt": a.is_some(),
        "godtatt_av": a.as_ref().map(|a| a.accepted_by.clone()),
        "godtatt_tidspunkt": a.as_ref().map(|a| a.accepted_at),
        "historikk": history.iter().map(|a| json!({
            "versjon": a.terms_sha256,
            "godtatt_av": a.accepted_by,
            "godtatt_tidspunkt": a.accepted_at,
        })).collect::<Vec<_>>(),
    })
}

pub async fn status(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(company_id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require(&state, person.person_id, company_id, Right::CompanyRead).await?;
    let a = regnmed_db::terms::acceptance(&state.pool, company_id, &VERSION).await?;
    let history = regnmed_db::terms::acceptances(&state.pool, company_id).await?;
    Ok(Json(status_json(a, &history)))
}

#[derive(Deserialize)]
pub struct AcceptRequest {
    /// The version the person read. Must be the current one, so nobody
    /// accepts a text they did not see.
    #[serde(rename = "versjon")]
    version: String,
}

pub async fn accept(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(company_id): Path<Uuid>,
    Json(request): Json<AcceptRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    // Binding the company is an admin's act, and never a machine's:
    // SELSKAP_ADMIN cannot be delegated to an integration.
    require(&state, person.person_id, company_id, Right::CompanyAdmin).await?;
    if request.version != *VERSION {
        return Err(ApiError::BadRequest(
            "vilkårene er endret siden du åpnet dem — les den gjeldende versjonen og godta den"
                .into(),
        ));
    }
    let a = regnmed_db::terms::accept(
        &state.pool,
        company_id,
        person.person_id,
        &VERSION,
        person.display(),
    )
    .await?;
    let history = regnmed_db::terms::acceptances(&state.pool, company_id).await?;
    Ok(Json(status_json(Some(a), &history)))
}

/// Terms §7.1: every call regnmed made to Altinn or the authorities on the
/// company's behalf. Metadata only -- what was sent and what the authority
/// answered stay behind each form's own rights (payroll, share register).
pub async fn authority_calls(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(company_id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require(&state, person.person_id, company_id, Right::CompanyRead).await?;
    let calls = regnmed_db::terms::authority_calls(&state.pool, company_id).await?;
    Ok(Json(json!(
        calls
            .into_iter()
            .map(|c| json!({
                "skjema": c.form,
                "id": c.id,
                "gjelder": c.reference,
                "bestilt_av": c.requested_by,
                "bestilt_tidspunkt": c.requested_at,
                "hendelser": c.events,
            }))
            .collect::<Vec<_>>()
    )))
}

/// Refuses unless the company has accepted the terms in force today (or
/// a newer version already announced). Called by every endpoint that files
/// with an authority on the company's behalf, before anything is stored or
/// sent.
pub async fn require_accepted(state: &AppState, company_id: Uuid) -> Result<(), ApiError> {
    if !may_file(&regnmed_db::terms::acceptances(&state.pool, company_id).await?) {
        return Err(ApiError::BadRequest(
            "selskapet har ikke godtatt kundevilkårene for innsending til myndighetene. En \
             administrator godtar dem under Administrasjon → Firmaopplysninger."
                .into(),
        ));
    }
    Ok(())
}
