//! Customer terms (docs/kundevilkar.md, #185).
//!
//! - GET  /vilkar                             the text, as plain text (open)
//! - GET  /terms                              the current text and its version (open)
//! - GET  /companies/{id}/terms               has the company accepted it?
//! - POST /companies/{id}/terms/accept        accept it (SELSKAP_ADMIN)
//!
//! The text is served from the binary, so the version a customer accepts
//! is exactly the text this release shows: its SHA-256. Every endpoint
//! that files with an authority on a customer's behalf calls
//! [`require_accepted`] first — Digdir's and Skatteetaten's terms allow no
//! filing without an agreement with the customer.

use std::sync::LazyLock;

use axum::Json;
use axum::extract::{Path, State};
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

use crate::AppState;
use crate::access::{Right, require};
use crate::auth::{ApiError, AuthPerson};

pub const TEXT: &str = include_str!("../../../docs/kundevilkar.md");

/// The version is the hash of the text: a changed text is a new version
/// and needs a new acceptance, with no number for anyone to forget.
pub static VERSION: LazyLock<String> = LazyLock::new(|| {
    regnmed_core::hash::sha256(TEXT.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
});

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

pub async fn current() -> Json<serde_json::Value> {
    Json(json!({ "versjon": *VERSION, "tekst": TEXT }))
}

fn status_json(a: Option<regnmed_db::terms::Acceptance>) -> serde_json::Value {
    json!({
        "versjon": *VERSION,
        "godtatt": a.is_some(),
        "godtatt_av": a.as_ref().map(|a| a.accepted_by.clone()),
        "godtatt_tidspunkt": a.as_ref().map(|a| a.accepted_at),
    })
}

pub async fn status(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(company_id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require(&state, person.person_id, company_id, Right::CompanyRead).await?;
    let a = regnmed_db::terms::acceptance(&state.pool, company_id, &VERSION).await?;
    Ok(Json(status_json(a)))
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
    Ok(Json(status_json(Some(a))))
}

/// Refuses unless the company has accepted the current terms. Called by
/// every endpoint that files with an authority on the company's behalf,
/// before anything is stored or sent.
pub async fn require_accepted(state: &AppState, company_id: Uuid) -> Result<(), ApiError> {
    if regnmed_db::terms::acceptance(&state.pool, company_id, &VERSION)
        .await?
        .is_none()
    {
        return Err(ApiError::BadRequest(
            "selskapet har ikke godtatt kundevilkårene for innsending til myndighetene. En \
             administrator godtar dem under Administrasjon → Firmaopplysninger."
                .into(),
        ));
    }
    Ok(())
}
