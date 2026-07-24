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
            "read-only access — invoicing requires bokforing",
        ));
    }
    Ok(())
}

#[derive(Deserialize)]
pub struct InvoiceLineRequest {
    description: String,
    /// Revenue account; defaults to 3000.
    account: Option<String>,
    /// Thousandths (2,5 stk = 2500); defaults to 1000.
    quantity_milli: Option<i64>,
    unit_price_ore: i64,
    vat_code: Option<String>,
    /// Dimension codes (docs/dimensjoner.md) for the revenue line.
    avdeling: Option<String>,
    prosjekt: Option<String>,
}

#[derive(Deserialize)]
pub struct CreateInvoiceRequest {
    party_no: String,
    invoice_date: chrono::NaiveDate,
    due_date: chrono::NaiveDate,
    /// Defaults: journal GL, receivable 1500, VAT account 2700.
    journal: Option<String>,
    receivable_account: Option<String>,
    vat_account: Option<String>,
    lines: Vec<InvoiceLineRequest>,
}

fn issued_json(issued: &regnmed_db::IssuedInvoice) -> serde_json::Value {
    json!({
        "invoice_id": issued.invoice_id,
        "invoice_no": issued.invoice_no,
        "kid": issued.kid,
        "net_ore": issued.net_ore,
        "vat_ore": issued.vat_ore,
        "gross_ore": issued.gross_ore,
        "voucher": format!("{}-{}", issued.fiscal_year, issued.voucher_number),
    })
}

pub async fn create_invoice(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(company_id): Path<Uuid>,
    Json(request): Json<CreateInvoiceRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_access(&state, person.person_id, company_id, true).await?;

    let draft = regnmed_db::InvoiceDraft {
        party_no: request.party_no,
        invoice_date: request.invoice_date,
        due_date: request.due_date,
        journal_code: request.journal.unwrap_or_else(|| "GL".into()),
        receivable_account: request.receivable_account.unwrap_or_else(|| "1500".into()),
        vat_account: request.vat_account.unwrap_or_else(|| "2700".into()),
        lines: request
            .lines
            .into_iter()
            .map(|line| regnmed_db::InvoiceLineDraft {
                description: line.description,
                account_number: line.account.unwrap_or_else(|| "3000".into()),
                quantity_milli: line.quantity_milli.unwrap_or(1000),
                unit_price_ore: line.unit_price_ore,
                vat_code: line.vat_code,
                avdeling: line.avdeling,
                prosjekt: line.prosjekt,
            })
            .collect(),
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
    require_access(&state, person.person_id, company_id, false).await?;
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
    require_access(&state, person.person_id, company_id, true).await?;
    let created_by = person.name.as_deref().unwrap_or(&person.sub);
    let credit = regnmed_db::credit_invoice(&state.pool, company_id, invoice_id, created_by)
        .await
        .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    Ok(Json(issued_json(&credit)))
}
