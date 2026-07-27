//! Betalingsliste og remittering (docs/betaling.md, #33):
//!
//! - GET  /companies/{id}/payments/payable       åpne leverandørposter
//! - GET  /companies/{id}/payments/runs          kjøringer m/ status
//! - POST /companies/{id}/payments/runs          lag liste (utkast)
//! - POST /companies/{id}/payments/runs/{rid}/approve   → pain.001
//! - GET  /companies/{id}/payments/runs/{rid}/file      hash-checked
//! - POST /companies/{id}/payments/runs/{rid}/settle    bilag + matcher
//! - POST /companies/{id}/payments/runs/{rid}/cancel    utkast only
//!
//! Creating and approving are separate audited actions (four-eyes
//! friendly; enforcement arrives with attestering, #47). Reading is
//! open to every access level; everything else requires bokforing.

use axum::Json;
use axum::extract::{Path, State};
use axum::http::header;
use axum::response::{IntoResponse, Response};
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

use crate::AppState;
use crate::auth::{ApiError, AuthPerson};
use crate::tilgang::{Krav, krev};

pub async fn payable(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(company_id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ApiError> {
    krev(&state, person.person_id, company_id, Krav::Les).await?;
    let items = regnmed_db::payable_items(&state.pool, company_id).await?;
    Ok(Json(json!({
        "items": items.iter().map(|i| json!({
            "entry_id": i.entry_id,
            "voucher": i.voucher_label,
            "date": i.date.to_string(),
            "description": i.description,
            "party_no": i.party_no,
            "party_name": i.party_name,
            "bank_account": i.bank_account,
            "belop_ore": i.belop_ore,
            "i_kjoring": i.i_kjoring,
        })).collect::<Vec<_>>(),
    })))
}

#[derive(Deserialize)]
pub struct RunItemRequest {
    entry_id: Uuid,
    belop_ore: Option<i64>,
    kid: Option<String>,
    melding: Option<String>,
}

#[derive(Deserialize)]
pub struct CreateRunRequest {
    items: Vec<RunItemRequest>,
    /// 11-digit kontonummer; defaults to the company's bank_account.
    debitor_konto: Option<String>,
    /// Defaults to today.
    execution_date: Option<chrono::NaiveDate>,
}

pub async fn create_run(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(company_id): Path<Uuid>,
    Json(request): Json<CreateRunRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    krev(&state, person.person_id, company_id, Krav::Bokfor).await?;
    let execution_date = match request.execution_date {
        Some(d) => d,
        None => sqlx::query_scalar("select current_date")
            .fetch_one(&state.pool)
            .await
            .map_err(anyhow::Error::from)?,
    };
    let items: Vec<regnmed_db::PaymentItemDraft> = request
        .items
        .into_iter()
        .map(|i| regnmed_db::PaymentItemDraft {
            entry_id: i.entry_id,
            belop_ore: i.belop_ore,
            kid: i.kid.filter(|k| !k.trim().is_empty()),
            melding: i.melding.filter(|m| !m.trim().is_empty()),
        })
        .collect();
    let created_by = person.name.as_deref().unwrap_or(&person.sub);
    let run_id = regnmed_db::create_run(
        &state.pool,
        company_id,
        &items,
        request.debitor_konto.as_deref(),
        execution_date,
        created_by,
        person.person_id,
    )
    .await
    .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    Ok(Json(json!({ "run_id": run_id })))
}

pub async fn list_runs(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(company_id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ApiError> {
    krev(&state, person.person_id, company_id, Krav::Les).await?;
    let runs = regnmed_db::list_payment_runs(&state.pool, company_id).await?;
    Ok(Json(json!({
        "runs": runs.iter().map(|r| json!({
            "run_id": r.id,
            "status": r.status,
            "execution_date": r.execution_date.to_string(),
            "sum_ore": r.sum_ore,
            "antall": r.antall,
            "created_by": r.created_by,
            "approved_by": r.approved_by,
            "settled_voucher": r.settled_voucher,
        })).collect::<Vec<_>>(),
    })))
}

pub async fn approve(
    State(state): State<AppState>,
    person: AuthPerson,
    Path((company_id, run_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<serde_json::Value>, ApiError> {
    krev(&state, person.person_id, company_id, Krav::Bokfor).await?;
    let approved_by = person.name.as_deref().unwrap_or(&person.sub);
    let digest = regnmed_db::approve_run(
        &state.pool,
        company_id,
        run_id,
        approved_by,
        person.person_id,
    )
    .await
    .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    Ok(Json(json!({
        "status": "godkjent",
        "file_sha256": hex::encode(digest),
    })))
}

pub async fn file(
    State(state): State<AppState>,
    person: AuthPerson,
    Path((company_id, run_id)): Path<(Uuid, Uuid)>,
) -> Result<Response, ApiError> {
    krev(&state, person.person_id, company_id, Krav::Les).await?;
    let (filename, content) = regnmed_db::run_file(&state.pool, company_id, run_id)
        .await
        .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    Ok((
        [
            (header::CONTENT_TYPE, "application/xml".to_string()),
            (
                header::CONTENT_DISPOSITION,
                format!("attachment; filename=\"{filename}\""),
            ),
        ],
        content,
    )
        .into_response())
}

#[derive(Deserialize, Default)]
pub struct SettleRequest {
    dato: Option<chrono::NaiveDate>,
    /// Ledger bank account, default 1920.
    bank_konto: Option<String>,
}

pub async fn settle(
    State(state): State<AppState>,
    person: AuthPerson,
    Path((company_id, run_id)): Path<(Uuid, Uuid)>,
    body: Option<Json<SettleRequest>>,
) -> Result<Json<serde_json::Value>, ApiError> {
    krev(&state, person.person_id, company_id, Krav::Bokfor).await?;
    let request = body.map(|Json(r)| r).unwrap_or_default();
    let dato = match request.dato {
        Some(d) => d,
        None => sqlx::query_scalar("select current_date")
            .fetch_one(&state.pool)
            .await
            .map_err(anyhow::Error::from)?,
    };
    let settled_by = person.name.as_deref().unwrap_or(&person.sub);
    let settled = regnmed_db::settle_run(
        &state.pool,
        company_id,
        run_id,
        dato,
        request.bank_konto.as_deref().unwrap_or("1920"),
        settled_by,
    )
    .await
    .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    Ok(Json(json!({
        "voucher": format!("{}-{}", settled.fiscal_year, settled.voucher_number),
    })))
}

pub async fn cancel(
    State(state): State<AppState>,
    person: AuthPerson,
    Path((company_id, run_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<serde_json::Value>, ApiError> {
    krev(&state, person.person_id, company_id, Krav::Bokfor).await?;
    let cancelled_by = person.name.as_deref().unwrap_or(&person.sub);
    regnmed_db::cancel_run(&state.pool, company_id, run_id, cancelled_by)
        .await
        .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    Ok(Json(json!({ "status": "annullert" })))
}
