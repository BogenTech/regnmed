//! Utlegg and kjøregodtgjørelse (docs/utlegg.md, #42):
//!
//! - GET  /companies/{id}/expenses                    list w/ status
//! - POST /companies/{id}/expenses/utlegg?filename=   raw receipt body
//! - POST /companies/{id}/expenses/kjoring            km claim
//! - GET  /companies/{id}/expenses/{eid}/receipt
//! - POST /companies/{id}/expenses/{eid}/approve      posts + attaches
//! - POST /companies/{id}/expenses/{eid}/reject       note required
//! - POST /companies/{id}/expenses/{eid}/pay          mellomregning→bank
//!
//! Everyone with write access registers their OWN claims; approving,
//! rejecting and paying require bokforing or admin (four-eyes comes
//! with attestering, #47 — a one-person company must be able to do
//! everything today).

use axum::Json;
use axum::body::Bytes;
use axum::extract::{Path, Query, State};
use axum::http::header;
use axum::response::Response;
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

use crate::AppState;
use crate::access::{Right, require};
use crate::auth::{ApiError, AuthPerson};
use crate::herding::{Vis, file_response};

#[derive(Deserialize)]
pub struct UploadQuery {
    filename: String,
    #[serde(rename = "dato")]
    date: chrono::NaiveDate,
    #[serde(rename = "belop_ore")]
    amount_ore: i64,
    #[serde(rename = "beskrivelse")]
    description: String,
}

pub async fn create_utlegg(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(company_id): Path<Uuid>,
    Query(query): Query<UploadQuery>,
    headers: axum::http::HeaderMap,
    body: Bytes,
) -> Result<Json<serde_json::Value>, ApiError> {
    require(&state, person.person_id, company_id, Right::ExpenseWriteOwn).await?;
    let content_type = headers
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("application/octet-stream")
        .to_string();
    let created_by = person.name.as_deref().unwrap_or(&person.sub);
    let id = regnmed_db::create_utlegg(
        &state.pool,
        company_id,
        person.person_id,
        query.date,
        &query.description,
        query.amount_ore,
        &query.filename,
        &content_type,
        &body,
        created_by,
    )
    .await
    .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    Ok(Json(json!({ "expense_id": id })))
}

#[derive(Deserialize)]
pub struct KjoringRequest {
    #[serde(rename = "dato")]
    date: chrono::NaiveDate,
    /// Route and purpose, e.g. "Oslo–Drammen t/r, kundemøte".
    #[serde(rename = "beskrivelse")]
    description: String,
    km: i64,
}

pub async fn create_kjoring(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(company_id): Path<Uuid>,
    Json(request): Json<KjoringRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require(&state, person.person_id, company_id, Right::ExpenseWriteOwn).await?;
    let created_by = person.name.as_deref().unwrap_or(&person.sub);
    let (id, amount_ore, trekkpliktig_ore) = regnmed_db::create_kjoring(
        &state.pool,
        company_id,
        person.person_id,
        request.date,
        &request.description,
        request.km,
        created_by,
    )
    .await
    .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    Ok(Json(json!({
        "expense_id": id,
        "belop_ore": amount_ore,
        "trekkpliktig_ore": trekkpliktig_ore,
    })))
}

pub async fn list(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(company_id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ApiError> {
    // En ansatt ser sine egne krav; alle andre ser selskapets. Omfanget
    // decided by the rettighet, not by the role — then it moves by
    // itself when roles are recomposed (#60).
    let role = require(&state, person.person_id, company_id, Right::ExpenseReadOwn).await?;
    let only_own = !role.has(Right::ExpenseReadAll);
    let expenses =
        regnmed_db::list_expenses(&state.pool, company_id, person.person_id, only_own).await?;
    Ok(Json(json!({
        "expenses": expenses.iter().map(|e| json!({
            "expense_id": e.id,
            "person": e.person_name,
            "own": e.own,
            "kind": e.kind,
            "dato": e.date.to_string(),
            "beskrivelse": e.description,
            "belop_ore": e.amount_ore,
            "km": e.km,
            "sats_ore_per_km": e.sats_ore_per_km,
            "trekkpliktig_ore": e.trekkpliktig_ore,
            "receipt_filename": e.receipt_filename,
            "status": e.status,
            "avvist_note": e.avvist_note,
            "voucher": e.voucher,
        })).collect::<Vec<_>>(),
    })))
}

pub async fn receipt(
    State(state): State<AppState>,
    person: AuthPerson,
    Path((company_id, expense_id)): Path<(Uuid, Uuid)>,
) -> Result<Response, ApiError> {
    let role = require(&state, person.person_id, company_id, Right::ExpenseReadOwn).await?;
    if !role.has(Right::ExpenseReadAll) {
        // The receipt is a picture of something private. Without
        // UTLEGG_LES_ALLE you get only your own — and a claim belonging
        // to someone else must answer as though it does not exist.
        let owner = regnmed_db::expense_owner(&state.pool, company_id, expense_id)
            .await
            .map_err(|_| ApiError::NotFound)?;
        if owner != person.person_id {
            return Err(ApiError::NotFound);
        }
    }
    let (filename, content_type, content) =
        regnmed_db::expense_receipt(&state.pool, company_id, expense_id)
            .await
            .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    // A receipt photo is shown in place, but only when we are willing to
    // assert its type — file_response downgrades anything else (#64).
    Ok(file_response(
        Vis::Inline,
        &filename,
        &content_type,
        content,
    ))
}

#[derive(Deserialize, Default)]
pub struct ApproveRequest {
    /// Kostnadskonto; defaults: utlegg 7790, kjøring 7100.
    #[serde(rename = "konto")]
    account: Option<String>,
    /// SAF-T code for inngående mva on utlegg (e.g. "1").
    #[serde(rename = "mva_kode")]
    vat_code: Option<String>,
    /// Inngående mva-konto, default 2710.
    #[serde(rename = "mva_konto")]
    vat_account: Option<String>,
    /// Mellomregning account, default 2910 (owed to employees).
    #[serde(rename = "motkonto")]
    contra_account: Option<String>,
}

pub async fn approve(
    State(state): State<AppState>,
    person: AuthPerson,
    Path((company_id, expense_id)): Path<(Uuid, Uuid)>,
    body: Option<Json<ApproveRequest>>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require(&state, person.person_id, company_id, Right::ExpenseApprove).await?;
    let request = body.map(|Json(r)| r).unwrap_or_default();
    let kind: Option<String> =
        sqlx::query_scalar("select kind from expense where id = $1 and company_id = $2")
            .bind(expense_id)
            .bind(company_id)
            .fetch_optional(&state.pool)
            .await
            .map_err(anyhow::Error::from)?;
    let default_account = match kind.as_deref() {
        Some("kjoring") => "7100",
        _ => "7790",
    };
    let decided_by = person.name.as_deref().unwrap_or(&person.sub);
    let approved = regnmed_db::approve_expense(
        &state.pool,
        company_id,
        expense_id,
        request.account.as_deref().unwrap_or(default_account),
        request.vat_code.as_deref(),
        request.vat_account.as_deref().unwrap_or("2710"),
        request.contra_account.as_deref().unwrap_or("2910"),
        decided_by,
        person.person_id,
    )
    .await
    .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    Ok(Json(json!({
        "voucher": format!("{}-{}", approved.fiscal_year, approved.voucher_number),
        "warning": approved.warning,
    })))
}

#[derive(Deserialize)]
pub struct RejectRequest {
    note: String,
}

pub async fn reject(
    State(state): State<AppState>,
    person: AuthPerson,
    Path((company_id, expense_id)): Path<(Uuid, Uuid)>,
    Json(request): Json<RejectRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require(&state, person.person_id, company_id, Right::ExpenseApprove).await?;
    let decided_by = person.name.as_deref().unwrap_or(&person.sub);
    regnmed_db::reject_expense(
        &state.pool,
        company_id,
        expense_id,
        &request.note,
        decided_by,
    )
    .await
    .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    Ok(Json(json!({ "status": "avvist" })))
}

#[derive(Deserialize, Default)]
pub struct PayRequest {
    #[serde(rename = "dato")]
    date: Option<chrono::NaiveDate>,
    /// Bankkonto, default 1920.
    #[serde(rename = "konto")]
    account: Option<String>,
}

pub async fn pay(
    State(state): State<AppState>,
    person: AuthPerson,
    Path((company_id, expense_id)): Path<(Uuid, Uuid)>,
    body: Option<Json<PayRequest>>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require(&state, person.person_id, company_id, Right::ExpensePay).await?;
    let request = body.map(|Json(r)| r).unwrap_or_default();
    let date = match request.date {
        Some(d) => d,
        None => sqlx::query_scalar("select current_date")
            .fetch_one(&state.pool)
            .await
            .map_err(anyhow::Error::from)?,
    };
    let paid_by = person.name.as_deref().unwrap_or(&person.sub);
    let paid = regnmed_db::pay_expense(
        &state.pool,
        company_id,
        expense_id,
        date,
        request.account.as_deref().unwrap_or("1920"),
        paid_by,
    )
    .await
    .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    Ok(Json(json!({
        "voucher": format!("{}-{}", paid.fiscal_year, paid.voucher_number),
    })))
}
