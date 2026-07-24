//! Tilbud → ordre → faktura (docs/faktura.md, #31):
//!
//! - GET/POST /companies/{id}/quotes            list / create tilbud
//! - PUT      /companies/{id}/quotes/{qid}      edit (utkast/sendt only)
//! - POST     /companies/{id}/quotes/{qid}/status    sendt|akseptert|avslatt
//! - POST     /companies/{id}/quotes/{qid}/order     akseptert tilbud → ordre
//! - GET/POST /companies/{id}/orders            list / create direct ordre
//! - POST     /companies/{id}/orders/{oid}/invoice   ordre → faktura
//! - GET      /companies/{id}/quotes/{qid}/pdf, /orders/{oid}/pdf
//!
//! Reading is open to every access level; everything else requires
//! bokforing or admin.

use axum::Json;
use axum::extract::{Path, State};
use axum::http::header;
use axum::response::{IntoResponse, Response};
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

use crate::AppState;
use crate::auth::{ApiError, AuthPerson};

async fn require_access(
    state: &AppState,
    person_id: Uuid,
    company_id: Uuid,
    write: bool,
) -> Result<(), ApiError> {
    let access = regnmed_db::company_access(&state.pool, person_id, company_id)
        .await?
        .ok_or(ApiError::NotFound)?;
    if write && access == "les" {
        return Err(ApiError::Forbidden(
            "read-only access — salgsdokumenter require bokforing",
        ));
    }
    Ok(())
}

#[derive(Deserialize)]
pub struct LineRequest {
    description: String,
    account: Option<String>,
    quantity_milli: Option<i64>,
    unit_price_ore: i64,
    vat_code: Option<String>,
    avdeling: Option<String>,
    prosjekt: Option<String>,
}

fn line_drafts(lines: Vec<LineRequest>) -> Vec<regnmed_db::SalgsLineDraft> {
    lines
        .into_iter()
        .map(|line| regnmed_db::SalgsLineDraft {
            description: line.description,
            account_number: line.account.unwrap_or_else(|| "3000".into()),
            quantity_milli: line.quantity_milli.unwrap_or(1000),
            unit_price_ore: line.unit_price_ore,
            vat_code: line.vat_code,
            avdeling: line.avdeling,
            prosjekt: line.prosjekt,
        })
        .collect()
}

#[derive(Deserialize)]
pub struct CreateRequest {
    party_no: String,
    doc_date: Option<chrono::NaiveDate>,
    lines: Vec<LineRequest>,
}

async fn create_kind(
    state: &AppState,
    person: &AuthPerson,
    company_id: Uuid,
    kind: &str,
    request: CreateRequest,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_access(state, person.person_id, company_id, true).await?;
    let doc_date = match request.doc_date {
        Some(d) => d,
        None => sqlx::query_scalar("select current_date")
            .fetch_one(&state.pool)
            .await
            .map_err(anyhow::Error::from)?,
    };
    let created_by = person.name.as_deref().unwrap_or(&person.sub);
    let (id, doc_no) = regnmed_db::create_salgsdokument(
        &state.pool,
        company_id,
        kind,
        &request.party_no,
        doc_date,
        &line_drafts(request.lines),
        created_by,
    )
    .await
    .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    Ok(Json(json!({ "id": id, "doc_no": doc_no })))
}

pub async fn create_quote(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(company_id): Path<Uuid>,
    Json(request): Json<CreateRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    create_kind(&state, &person, company_id, "tilbud", request).await
}

pub async fn create_order(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(company_id): Path<Uuid>,
    Json(request): Json<CreateRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    create_kind(&state, &person, company_id, "ordre", request).await
}

async fn list_kind(
    state: &AppState,
    person: &AuthPerson,
    company_id: Uuid,
    kind: &str,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_access(state, person.person_id, company_id, false).await?;
    let rows = regnmed_db::list_salgsdokumenter(&state.pool, company_id, Some(kind)).await?;
    Ok(Json(json!({
        "documents": rows.iter().map(|d| json!({
            "id": d.id,
            "doc_no": d.doc_no,
            "party_no": d.party_no,
            "party_name": d.party_name,
            "doc_date": d.doc_date.to_string(),
            "status": d.status,
            "netto_ore": d.netto_ore,
            "tilbud_no": d.tilbud_no,
            "invoice_no": d.invoice_no,
        })).collect::<Vec<_>>(),
    })))
}

pub async fn list_quotes(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(company_id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ApiError> {
    list_kind(&state, &person, company_id, "tilbud").await
}

pub async fn list_orders(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(company_id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ApiError> {
    list_kind(&state, &person, company_id, "ordre").await
}

#[derive(Deserialize)]
pub struct UpdateQuoteRequest {
    doc_date: Option<chrono::NaiveDate>,
    lines: Option<Vec<LineRequest>>,
}

pub async fn update_quote(
    State(state): State<AppState>,
    person: AuthPerson,
    Path((company_id, quote_id)): Path<(Uuid, Uuid)>,
    Json(request): Json<UpdateQuoteRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_access(&state, person.person_id, company_id, true).await?;
    let lines = request.lines.map(line_drafts);
    regnmed_db::update_tilbud(
        &state.pool,
        company_id,
        quote_id,
        request.doc_date,
        lines.as_deref(),
    )
    .await
    .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    Ok(Json(json!({ "updated": true })))
}

#[derive(Deserialize)]
pub struct StatusRequest {
    status: String,
}

pub async fn quote_status(
    State(state): State<AppState>,
    person: AuthPerson,
    Path((company_id, quote_id)): Path<(Uuid, Uuid)>,
    Json(request): Json<StatusRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_access(&state, person.person_id, company_id, true).await?;
    regnmed_db::set_tilbud_status(&state.pool, company_id, quote_id, &request.status)
        .await
        .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    Ok(Json(json!({ "status": request.status })))
}

#[derive(Deserialize, Default)]
pub struct ToOrderRequest {
    doc_date: Option<chrono::NaiveDate>,
}

pub async fn quote_to_order(
    State(state): State<AppState>,
    person: AuthPerson,
    Path((company_id, quote_id)): Path<(Uuid, Uuid)>,
    body: Option<Json<ToOrderRequest>>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_access(&state, person.person_id, company_id, true).await?;
    let request = body.map(|Json(r)| r).unwrap_or_default();
    let doc_date = match request.doc_date {
        Some(d) => d,
        None => sqlx::query_scalar("select current_date")
            .fetch_one(&state.pool)
            .await
            .map_err(anyhow::Error::from)?,
    };
    let created_by = person.name.as_deref().unwrap_or(&person.sub);
    let (ordre_id, doc_no) =
        regnmed_db::tilbud_to_ordre(&state.pool, company_id, quote_id, doc_date, created_by)
            .await
            .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    Ok(Json(json!({ "order_id": ordre_id, "doc_no": doc_no })))
}

#[derive(Deserialize, Default)]
pub struct ToInvoiceRequest {
    invoice_date: Option<chrono::NaiveDate>,
    due_date: Option<chrono::NaiveDate>,
}

pub async fn order_to_invoice(
    State(state): State<AppState>,
    person: AuthPerson,
    Path((company_id, order_id)): Path<(Uuid, Uuid)>,
    body: Option<Json<ToInvoiceRequest>>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_access(&state, person.person_id, company_id, true).await?;
    let request = body.map(|Json(r)| r).unwrap_or_default();
    let today: chrono::NaiveDate = sqlx::query_scalar("select current_date")
        .fetch_one(&state.pool)
        .await
        .map_err(anyhow::Error::from)?;
    let invoice_date = request.invoice_date.unwrap_or(today);
    let due_date = request
        .due_date
        .unwrap_or(invoice_date + chrono::Days::new(14));
    let created_by = person.name.as_deref().unwrap_or(&person.sub);
    let issued = regnmed_db::ordre_to_invoice(
        &state.pool,
        company_id,
        order_id,
        invoice_date,
        due_date,
        created_by,
    )
    .await
    .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    Ok(Json(json!({
        "invoice_id": issued.invoice_id,
        "invoice_no": issued.invoice_no,
        "kid": issued.kid,
        "gross_ore": issued.gross_ore,
        "voucher": format!("{}-{}", issued.fiscal_year, issued.voucher_number),
    })))
}

/// Tilbud/ordrebekreftelse as PDF — rendered on demand from current
/// state (the stored, hash-verified document arrives with the invoice).
pub async fn pdf(
    State(state): State<AppState>,
    person: AuthPerson,
    Path((company_id, dokument_id)): Path<(Uuid, Uuid)>,
) -> Result<Response, ApiError> {
    require_access(&state, person.person_id, company_id, false).await?;
    let (filename, pdf) = regnmed_db::salgsdokument_pdf(&state.pool, company_id, dokument_id)
        .await
        .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    Ok((
        [
            (header::CONTENT_TYPE, "application/pdf".to_string()),
            (
                header::CONTENT_DISPOSITION,
                format!("inline; filename=\"{filename}\""),
            ),
        ],
        pdf,
    )
        .into_response())
}
