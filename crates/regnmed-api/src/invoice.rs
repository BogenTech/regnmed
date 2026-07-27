//! Invoice endpoints (web-first, engagement-guarded):
//!
//! - POST /companies/{id}/invoices                    issue an invoice
//! - GET  /companies/{id}/invoices?open=true          list with remaining
//! - POST /companies/{id}/invoices/{invoice_id}/credit-note
//!
//! Reading is open to every access level; issuing and crediting require
//! bokforing or admin.

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::header;
use axum::response::{IntoResponse, Response};
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

use crate::AppState;
use crate::auth::{ApiError, AuthPerson};
use crate::tilgang::{Krav, krev};

use crate::product::{DocLineRequest, resolve_lines};

#[derive(Deserialize)]
pub struct CreateInvoiceRequest {
    party_no: String,
    invoice_date: chrono::NaiveDate,
    due_date: chrono::NaiveDate,
    /// Defaults: journal GL, receivable 1500, VAT account 2700.
    journal: Option<String>,
    receivable_account: Option<String>,
    vat_account: Option<String>,
    /// Document currency (docs/valuta.md); line amounts in its minor
    /// unit. None = NOK. Requires a kurs in the valutakurs table.
    valuta: Option<String>,
    lines: Vec<DocLineRequest>,
}

fn issued_json(issued: &regnmed_db::IssuedInvoice) -> serde_json::Value {
    json!({
        "invoice_id": issued.invoice_id,
        "invoice_no": issued.invoice_no,
        "kid": issued.kid,
        "net_ore": issued.net_ore,
        "vat_ore": issued.vat_ore,
        "gross_ore": issued.gross_ore,
        "gross_nok_ore": issued.gross_nok_ore,
        "voucher": format!("{}-{}", issued.fiscal_year, issued.voucher_number),
    })
}

pub async fn create_invoice(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(company_id): Path<Uuid>,
    Json(request): Json<CreateInvoiceRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    krev(&state, person.person_id, company_id, Krav::Bokfor).await?;

    let draft = regnmed_db::InvoiceDraft {
        party_no: request.party_no,
        invoice_date: request.invoice_date,
        due_date: request.due_date,
        journal_code: request.journal.unwrap_or_else(|| "GL".into()),
        receivable_account: request.receivable_account.unwrap_or_else(|| "1500".into()),
        vat_account: request.vat_account.unwrap_or_else(|| "2700".into()),
        valuta: request.valuta.map(|v| v.to_uppercase()),
        valuta_kurs_micro: None,
        lines: resolve_lines(&state, company_id, request.lines).await?,
    };
    let created_by = person.name.as_deref().unwrap_or(&person.sub);
    let issued = regnmed_db::create_invoice(&state.pool, company_id, &draft, created_by, None)
        .await
        .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    Ok(Json(issued_json(&issued)))
}

#[derive(Deserialize)]
pub struct ListQuery {
    #[serde(default)]
    open: bool,
}

pub async fn list_invoices(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(company_id): Path<Uuid>,
    Query(query): Query<ListQuery>,
) -> Result<Json<serde_json::Value>, ApiError> {
    krev(&state, person.person_id, company_id, Krav::Les).await?;
    let invoices = regnmed_db::list_invoices(&state.pool, company_id, query.open).await?;
    Ok(Json(json!({
        "invoices": invoices.iter().map(|i| json!({
            "invoice_id": i.invoice_id,
            "invoice_no": i.invoice_no,
            "party_no": i.party_no,
            "party_name": i.party_name,
            "invoice_date": i.invoice_date.to_string(),
            "due_date": i.due_date.to_string(),
            "kid": i.kid,
            "gross_ore": i.gross_ore,
            "remaining_ore": i.remaining_ore,
            "is_credit_note": i.is_credit_note,
        })).collect::<Vec<_>>(),
    })))
}

pub async fn credit_note(
    State(state): State<AppState>,
    person: AuthPerson,
    Path((company_id, invoice_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<serde_json::Value>, ApiError> {
    krev(&state, person.person_id, company_id, Krav::Bokfor).await?;
    let created_by = person.name.as_deref().unwrap_or(&person.sub);
    let credit = regnmed_db::credit_invoice(&state.pool, company_id, invoice_id, created_by)
        .await
        .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    Ok(Json(issued_json(&credit)))
}

/// The salgsdokument stored at issue time — served hash-checked, so the
/// customer-facing document can never silently diverge from the
/// oppbevarte original.
pub async fn invoice_pdf(
    State(state): State<AppState>,
    person: AuthPerson,
    Path((company_id, invoice_id)): Path<(Uuid, Uuid)>,
) -> Result<Response, ApiError> {
    krev(&state, person.person_id, company_id, Krav::Les).await?;
    let attachment_id = regnmed_db::invoice_pdf_attachment_id(&state.pool, company_id, invoice_id)
        .await?
        .ok_or(ApiError::NotFound)?;
    let (meta, content) = regnmed_db::get_attachment(&state.pool, company_id, attachment_id)
        .await
        .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    Ok((
        [
            (header::CONTENT_TYPE, "application/pdf".to_string()),
            (
                header::CONTENT_DISPOSITION,
                format!("inline; filename=\"{}\"", meta.filename),
            ),
        ],
        content,
    )
        .into_response())
}

/// EHF (PEPPOL BIS Billing 3.0) for an issued invoice — rendered from
/// the invoice's own locked rows on request. Unlike the PDF this is not
/// stored: the PDF *is* the salgsdokument (oppbevaringsplikt), while the
/// EHF is a transport envelope derived from the same immutable numbers.
/// What an access point actually transmits is what gets logged, when
/// that tier arrives (docs/ehf.md).
pub async fn invoice_ehf(
    State(state): State<AppState>,
    person: AuthPerson,
    Path((company_id, invoice_id)): Path<(Uuid, Uuid)>,
) -> Result<Response, ApiError> {
    krev(&state, person.person_id, company_id, Krav::Les).await?;
    let xml = regnmed_db::invoice_ehf(&state.pool, company_id, invoice_id)
        .await
        .map_err(|e| ApiError::BadRequest(format!("{e:#}")))?;
    Ok((
        [
            (header::CONTENT_TYPE, "application/xml".to_string()),
            (
                header::CONTENT_DISPOSITION,
                format!("attachment; filename=\"ehf-{invoice_id}.xml\""),
            ),
        ],
        xml,
    )
        .into_response())
}
