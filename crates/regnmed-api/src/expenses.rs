//! Utlegg and kjøregodtgjørelse (docs/utlegg.md, #42):
//!
//! - GET  /companies/{id}/expenses?status=&limit=&offset=
//!                                                    list + tab counts
//! - GET  /companies/{id}/expenses/{eid}              one claim
//! - POST /companies/{id}/expenses/utlegg?filename=   raw receipt body
//! - POST /companies/{id}/expenses/kjoring            km claim
//! - GET  /companies/{id}/expenses/kjoring?dato=      the km-sats that day
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
    amount_ore: Option<i64>,
    #[serde(rename = "beskrivelse", default)]
    description: String,
}

pub async fn create_expense(
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
    let id = regnmed_db::create_expense(
        &state.pool,
        company_id,
        person.person_id,
        query.date,
        &query.description,
        query
            .amount_ore
            .ok_or_else(|| ApiError::BadRequest("oppgi beløpet".into()))?,
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
pub struct RunRequest {
    #[serde(rename = "dato")]
    date: chrono::NaiveDate,
    /// Route and purpose, e.g. "Oslo–Drammen t/r, kundemøte".
    #[serde(rename = "beskrivelse", default)]
    description: String,
    km: Option<i64>,
}

#[derive(Deserialize)]
pub struct RateQuery {
    #[serde(rename = "dato")]
    date: chrono::NaiveDate,
}

/// The satser a kjøring on `dato` would be paid with — so the form can
/// show the amount, and the trekkpliktig part, before anyone submits.
pub async fn mileage_rate(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(company_id): Path<Uuid>,
    Query(query): Query<RateQuery>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require(&state, person.person_id, company_id, Right::ExpenseWriteOwn).await?;
    let (rate, trekkfri) = regnmed_db::mileage_rates(&state.pool, query.date)
        .await
        .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    Ok(Json(json!({
        "sats_ore_per_km": rate,
        "trekkfri_ore_per_km": trekkfri,
    })))
}

pub async fn create_expense_run(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(company_id): Path<Uuid>,
    Json(request): Json<RunRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require(&state, person.person_id, company_id, Right::ExpenseWriteOwn).await?;
    let created_by = person.name.as_deref().unwrap_or(&person.sub);
    let (id, amount_ore, trekkpliktig_ore) = regnmed_db::create_expense_run(
        &state.pool,
        company_id,
        person.person_id,
        request.date,
        &request.description,
        request
            .km
            .ok_or_else(|| ApiError::BadRequest("oppgi antall kilometer".into()))?,
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

#[derive(Deserialize, Default)]
pub struct ListQuery {
    /// `apne` (innsendt + godkjent), `utbetalt`, `avvist`, … — empty = all.
    status: Option<String>,
    limit: Option<i64>,
    offset: Option<i64>,
}

fn expense_json(e: &regnmed_db::ExpenseRow) -> serde_json::Value {
    json!({
        "expense_id": e.id,
        "person": e.person_name,
        "own": e.own,
        "kind": e.kind,
        "dato": e.date.to_string(),
        "beskrivelse": e.description,
        "belop_ore": e.amount_ore,
        "km": e.km,
        "sats_ore_per_km": e.sats_ore_per_km,
        "trekkfri_ore": e.trekkfri_ore,
        "trekkpliktig_ore": e.trekkpliktig_ore,
        "receipt_filename": e.receipt_filename,
        "status": e.status,
        "avvist_note": e.avvist_note,
        "decided_by": e.decided_by,
        "decided_at": e.decided_at,
        "voucher": e.voucher,
        "utbetalt_voucher": e.paid_voucher,
        "utbetalt_at": e.utbetalt_at,
        "created_at": e.created_at,
    })
}

/// A viewer sees their own claims; with UTLEGG_LES_ALLE, the company's.
/// The scope is decided by the right, not by the role — then it moves
/// by itself when roles are recomposed (#60).
async fn scope(state: &AppState, person: Uuid, company_id: Uuid) -> Result<bool, ApiError> {
    let role = require(state, person, company_id, Right::ExpenseReadOwn).await?;
    Ok(!role.has(Right::ExpenseReadAll))
}

pub async fn list(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(company_id): Path<Uuid>,
    Query(query): Query<ListQuery>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let only_own = scope(&state, person.person_id, company_id).await?;
    let page = regnmed_db::list_expenses(
        &state.pool,
        company_id,
        person.person_id,
        only_own,
        &regnmed_db::ExpenseFilter {
            status: query.status,
            expense_id: None,
            limit: query.limit.unwrap_or(200),
            offset: query.offset.unwrap_or(0),
        },
    )
    .await
    .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    Ok(Json(json!({
        "expenses": page.rows.iter().map(expense_json).collect::<Vec<_>>(),
        "total": page.total,
        "counts": {
            "innsendt": page.counts.innsendt,
            "godkjent": page.counts.godkjent,
            "utbetalt": page.counts.utbetalt,
            "avvist": page.counts.avvist,
        },
    })))
}

pub async fn get(
    State(state): State<AppState>,
    person: AuthPerson,
    Path((company_id, expense_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<serde_json::Value>, ApiError> {
    // Someone else's claim, seen with only UTLEGG_LES_EGNE, answers as
    // though it does not exist — the same rule as the receipt.
    let only_own = scope(&state, person.person_id, company_id).await?;
    let page = regnmed_db::list_expenses(
        &state.pool,
        company_id,
        person.person_id,
        only_own,
        &regnmed_db::ExpenseFilter {
            expense_id: Some(expense_id),
            limit: 1,
            ..Default::default()
        },
    )
    .await?;
    let expense = page.rows.first().ok_or(ApiError::NotFound)?;
    Ok(Json(expense_json(expense)))
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
