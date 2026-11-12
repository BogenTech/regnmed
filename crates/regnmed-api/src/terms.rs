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

use std::sync::LazyLock;

use axum::Json;
use axum::extract::{Path, State};
use include_dir::{Dir, include_dir};
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

/// Every version that has been in force, `YYYY-MM-DD-<sha256>.md` (the
/// date it took effect). A customer bound by an older text must be able to
/// read it after the text has moved on; `the_current_text_is_archived`
/// fails CI on a change to the text that is not archived.
static ARCHIVE: Dir = include_dir!("$CARGO_MANIFEST_DIR/../../docs/kundevilkar");

/// (effective date, version, text), newest first.
fn versions() -> Vec<(&'static str, &'static str, &'static str)> {
    let mut v: Vec<_> = ARCHIVE
        .files()
        .filter_map(|f| {
            let name = f.path().file_name()?.to_str()?.strip_suffix(".md")?;
            let (date, sha) = (name.get(..10)?, name.get(11..)?);
            Some((date, sha, f.contents_utf8()?))
        })
        .collect();
    // The version in force first, then newest date first. Same-day
    // versions are otherwise unordered: the file name has no time.
    v.sort_by_key(|&(date, sha, _)| (sha != VERSION.as_str(), std::cmp::Reverse(date), sha));
    v
}

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

pub async fn current() -> Json<serde_json::Value> {
    Json(json!({ "versjon": *VERSION, "tekst": TEXT }))
}

fn status_json(
    a: Option<regnmed_db::terms::Acceptance>,
    history: &[regnmed_db::terms::Acceptance],
) -> serde_json::Value {
    json!({
        "versjon": *VERSION,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_current_text_is_archived() {
        let all = versions();
        // Every version that was ever in force. A customer may have
        // accepted any of them, so the list only grows, like migrations.
        const KNOWN: &[&str] = &[
            "f2d730ceb33dde33c066d7cc74bfb9d0c725e3958aea3e0711fab11da3a885cd",
            "f376538b16f2bed77e6efabfd80cd7fd247797893c07644a32ea153dcd4329f9",
            "bec6783875729d4f63883b3595bdbf184c0982c92f01958485badc29e0e456b7",
        ];
        for known in KNOWN {
            assert!(
                all.iter().any(|(_, sha, _)| sha == known),
                "archived terms version {known} is gone — a company may have accepted it"
            );
        }
        assert!(
            KNOWN.contains(&VERSION.as_str()),
            "add the new version {} to KNOWN",
            *VERSION
        );
        assert!(
            all.iter()
                .any(|(_, sha, text)| *sha == VERSION.as_str() && *text == TEXT),
            "docs/kundevilkar.md changed: copy it to docs/kundevilkar/<YYYY-MM-DD>-{}.md \
             (the date it takes effect) so customers bound by the old text can still read it",
            *VERSION
        );
        for (date, sha, text) in &all {
            assert!(
                chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d").is_ok(),
                "{date}"
            );
            let actual: String = regnmed_core::hash::sha256(text.as_bytes())
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect();
            assert_eq!(
                &actual, sha,
                "an archived version was edited after the fact"
            );
        }
    }
}
