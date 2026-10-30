//! Balansedokumentasjon over the web (#88, docs/balansedokumentasjon.md).
//!
//! Reading the status is `RAPPORT_LES` — it IS a report, and the revisor
//! must be able to read it through a read-only engagement. Recording an
//! avstemming is `BILAG_BOKFOR`: it is the accountant's assertion about
//! what a balance post consists of, not something a reader does.

use axum::Json;
use axum::body::Bytes;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, header};
use axum::response::IntoResponse;
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

use crate::AppState;
use crate::access::{Right, require};
use crate::auth::{ApiError, AuthPerson};

#[derive(Deserialize)]
pub struct PeriodeQuery {
    /// Period end. Defaults to the latest closed period — the one the
    /// revisjonsrapport measures, so the two never disagree by accident.
    periode: Option<chrono::NaiveDate>,
}

async fn periode_or_lock(
    state: &AppState,
    company_id: Uuid,
    valgt: Option<chrono::NaiveDate>,
) -> Result<chrono::NaiveDate, ApiError> {
    if let Some(p) = valgt {
        return Ok(p);
    }
    sqlx::query_scalar::<_, Option<chrono::NaiveDate>>("select current_period_lock($1)")
        .bind(company_id)
        .fetch_one(&state.pool)
        .await
        .map_err(anyhow::Error::from)?
        .ok_or_else(|| {
            ApiError::BadRequest("ingen periode er låst ennå — oppgi ?periode=YYYY-MM-DD".into())
        })
}

pub async fn status(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(company_id): Path<Uuid>,
    Query(query): Query<PeriodeQuery>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require(&state, person.person_id, company_id, Right::ReportRead).await?;
    let periode = periode_or_lock(&state, company_id, query.periode).await?;
    let linjer =
        regnmed_db::balance_documentation::balance_status(&state.pool, company_id, periode).await?;
    Ok(Json(json!({
        "periode": periode.to_string(),
        "kontoer": linjer.iter().map(|l| json!({
            "konto": l.account,
            "kontonavn": l.account_name,
            "saldo_ore": l.balance_ore,
            // Computed, never stored — a difference means the account was
            // posted to AFTER it was reconciled, which is worth saying
            // out loud rather than quietly re-baselining.
            "avvik_ore": l.avvik_ore(),
            "avstemt": l.avstemt.as_ref().map(|a| json!({
                "id": a.id,
                "saldo_ore": a.balance_ore,
                "forklaring": a.forklaring,
                "avstemt_dato": a.reconciled_date.to_string(),
                "avstemt_av": a.avstemt_av,
                "har_vedlegg": a.har_vedlegg,
                "vedlegg_navn": a.attachment_name,
            })),
        })).collect::<Vec<_>>(),
    })))
}

#[derive(Deserialize)]
pub struct AvstemRequest {
    #[serde(rename = "konto")]
    account: String,
    periode: chrono::NaiveDate,
    forklaring: String,
}

pub async fn avstem(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(company_id): Path<Uuid>,
    Json(request): Json<AvstemRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require(&state, person.person_id, company_id, Right::VoucherPost).await?;
    let idag: chrono::NaiveDate = sqlx::query_scalar("select current_date")
        .fetch_one(&state.pool)
        .await
        .map_err(anyhow::Error::from)?;
    let id = regnmed_db::balance_documentation::avstem(
        &state.pool,
        company_id,
        &request.account,
        request.periode,
        &request.forklaring,
        None,
        person.person_id,
        idag,
    )
    .await
    .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    Ok(Json(json!({ "id": id })))
}

#[derive(Deserialize)]
pub struct VedleggQuery {
    #[serde(rename = "konto")]
    account: String,
    periode: chrono::NaiveDate,
    forklaring: String,
    filename: String,
}

/// Avstemming WITH its documentation in one call: the kontoutskrift or
/// the signed varetellingsliste IS the documentation, so uploading it
/// and asserting the saldo are one act, not two.
pub async fn avstem_med_vedlegg(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(company_id): Path<Uuid>,
    Query(query): Query<VedleggQuery>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Json<serde_json::Value>, ApiError> {
    require(&state, person.person_id, company_id, Right::VoucherPost).await?;
    let content_type = headers
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("application/octet-stream")
        .to_string();
    let idag: chrono::NaiveDate = sqlx::query_scalar("select current_date")
        .fetch_one(&state.pool)
        .await
        .map_err(anyhow::Error::from)?;
    let id = regnmed_db::balance_documentation::avstem(
        &state.pool,
        company_id,
        &query.account,
        query.periode,
        &query.forklaring,
        Some((&query.filename, &content_type, &body)),
        person.person_id,
        idag,
    )
    .await
    .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    Ok(Json(json!({ "id": id })))
}

pub async fn vedlegg(
    State(state): State<AppState>,
    person: AuthPerson,
    Path((company_id, id)): Path<(Uuid, Uuid)>,
) -> Result<impl IntoResponse, ApiError> {
    require(&state, person.person_id, company_id, Right::ReportRead).await?;
    let (name, content_type, bytes) =
        regnmed_db::balance_documentation::hent_vedlegg(&state.pool, company_id, id)
            .await
            .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    Ok((
        [
            (header::CONTENT_TYPE, content_type),
            (
                header::CONTENT_DISPOSITION,
                format!("attachment; filename=\"{name}\""),
            ),
        ],
        bytes,
    ))
}

#[derive(Deserialize)]
pub struct HistorikkQuery {
    #[serde(rename = "konto")]
    account: String,
    periode: chrono::NaiveDate,
}

pub async fn historikk(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(company_id): Path<Uuid>,
    Query(query): Query<HistorikkQuery>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require(&state, person.person_id, company_id, Right::ReportRead).await?;
    let rader = regnmed_db::balance_documentation::historikk(
        &state.pool,
        company_id,
        &query.account,
        query.periode,
    )
    .await?;
    Ok(Json(json!({
        "avstemminger": rader.iter().map(|a| json!({
            "id": a.id,
            "saldo_ore": a.balance_ore,
            "forklaring": a.forklaring,
            "avstemt_dato": a.reconciled_date.to_string(),
            "avstemt_av": a.avstemt_av,
            "har_vedlegg": a.har_vedlegg,
            "vedlegg_navn": a.attachment_name,
        })).collect::<Vec<_>>(),
    })))
}
