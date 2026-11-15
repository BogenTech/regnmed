//! Årsavslutning over the web (#84, docs/arsavslutning.md).
//!
//! Closing a year posts a voucher AND locks the year, so it needs both
//! rights: `BILAG_BOKFOR` to post and `PERIODE_LAAS` to lock. Requiring
//! both is not belt-and-braces — the lock is half the act, and someone
//! who may not lock a period should not close a year either.

use axum::Json;
use axum::extract::{Path, Query, State};
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

use crate::AppState;
use crate::access::{Right, require};
use crate::auth::{ApiError, AuthPerson};

#[derive(Deserialize)]
pub struct CloseRequest {
    #[serde(rename = "ar")]
    year: i32,
    /// Skattekostnaden for året, i øre. Kalleren regner den ut —
    /// skattemessig resultat er ikke regnskapsmessig resultat, og å
    /// utlede den her ville vært å dikte opp en skattemelding. 0 er et
    /// gyldig svar som må oppgis.
    #[serde(rename = "skattekostnad_ore")]
    tax_expense_ore: i64,
    /// The result the person saw in the preview. A ledger that has moved
    /// since is refused, not closed on a number nobody looked at.
    #[serde(rename = "forventet_resultat_ore")]
    expected_result_ore: Option<i64>,
}

#[derive(Deserialize)]
pub struct CloseQuery {
    /// Post and roll back: the voucher's lines without keeping them.
    #[serde(default)]
    preview: bool,
}

pub async fn list(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(company_id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require(&state, person.person_id, company_id, Right::ReportRead).await?;
    let rader = regnmed_db::year_end::list_arsavslutninger(&state.pool, company_id).await?;
    Ok(Json(json!({
        "arsavslutninger": rader.iter().map(|a| json!({
            "ar": a.year,
            "bilag": format!("{}-{}", a.voucher.0, a.voucher.1),
            "resultat_for_skatt_ore": a.result_before_tax_ore,
            "skattekostnad_ore": a.tax_expense_ore,
            "disponert_ore": a.allocated_ore,
            "created_by": a.created_by,
        })).collect::<Vec<_>>(),
    })))
}

/// The year's result as the ledger has it — what the closing WOULD
/// disponere. Offered so the form can show the number before anyone
/// commits to it.
pub async fn suggestion(
    State(state): State<AppState>,
    person: AuthPerson,
    Path((company_id, year)): Path<(Uuid, i32)>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require(&state, person.person_id, company_id, Right::ReportRead).await?;
    let result = regnmed_db::year_end::result_for_the_year(&state.pool, company_id, year)
        .await
        .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    Ok(Json(json!({
        "ar": year,
        "resultat_for_skatt_ore": result,
    })))
}

pub async fn close(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(company_id): Path<Uuid>,
    Query(query): Query<CloseQuery>,
    Json(request): Json<CloseRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require(&state, person.person_id, company_id, Right::VoucherPost).await?;
    // Reopening a lock that reaches past 31.12 is decided inside
    // close_year, so the role comes along like for the period lock.
    let role = require(&state, person.person_id, company_id, Right::PeriodLock).await?;
    let c = regnmed_db::year_end::close_year(
        &state.pool,
        company_id,
        request.year,
        request.tax_expense_ore,
        person.name.as_deref().unwrap_or(&person.sub),
        role.is_admin(),
        query.preview,
        request.expected_result_ore,
    )
    .await
    .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    let a = &c.closing;
    Ok(Json(json!({
        "ar": a.year,
        "bilag": format!("{}-{}", a.voucher.0, a.voucher.1),
        "resultat_for_skatt_ore": a.result_before_tax_ore,
        "skattekostnad_ore": a.tax_expense_ore,
        "disponert_ore": a.allocated_ore,
        "linjer": c.lines.iter().map(|(konto, belop, tekst)| json!({
            "konto": konto, "belop_ore": belop, "tekst": tekst,
        })).collect::<Vec<_>>(),
        "relast_til": c.relocked_through.map(|d| d.to_string()),
    })))
}
