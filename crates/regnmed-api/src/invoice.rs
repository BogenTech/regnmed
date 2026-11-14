//! Invoice endpoints (web-first, engagement-guarded):
//!
//! - POST /companies/{id}/invoices                    issue an invoice
//! - GET  /companies/{id}/invoices?open=true          list with remaining
//! - POST /companies/{id}/invoices/{invoice_id}/credit-note
//! - POST /companies/{id}/invoices/{invoice_id}/payments  record a payment
//!
//! Reading is open to every access level; issuing and crediting require
//! bokforing or admin.

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::response::Response;
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

use crate::AppState;
use crate::access::{Right, require};
use crate::auth::{ApiError, AuthPerson};
use crate::herding::{Vis, file_response};

use crate::product::{DocLineRequest, resolve_lines};

#[derive(Deserialize)]
pub struct CreateInvoiceRequest {
    party_no: String,
    invoice_date: chrono::NaiveDate,
    due_date: chrono::NaiveDate,
    /// Leveringstidspunkt (bokføringsforskriften §5-1-1 nr. 4).
    /// Omitted = the invoice date, which is the ordinary case: billed
    /// on delivery. The default is stated HERE, at the boundary, so it
    /// is one documented decision rather than an assumption buried in
    /// the posting — the portal always sends it explicitly.
    #[serde(rename = "leveringsdato")]
    delivery_date: Option<chrono::NaiveDate>,
    /// Leveringssted, required "der det er relevant" — typically a
    /// vareleveranse to an address other than the buyer's.
    #[serde(rename = "leveringssted")]
    delivery_place: Option<String>,
    /// Defaults: journal GL, receivable 1500, VAT account 2700.
    journal: Option<String>,
    receivable_account: Option<String>,
    vat_account: Option<String>,
    /// Document currency (docs/valuta.md); line amounts in its minor
    /// unit. None = NOK. Requires a kurs in the valutakurs table.
    #[serde(rename = "valuta")]
    currency: Option<String>,
    /// Kontantsalg (#89, §5-3): what settled the ytelse on delivery
    /// ("Kort", "Vipps", "Kontant"). Present = kontantfaktura: the
    /// receivable is raised and settled in one transaction, and the
    /// document carries no KID and no forfall.
    #[serde(rename = "kontant_betalingsmiddel")]
    cash_payment_method: Option<String>,
    /// Where the money landed — 1900 kontanter, 1920 bank, or the card
    /// acquirer's clearing account. Required with the above; we never
    /// guess how somebody was paid.
    #[serde(rename = "oppgjorskonto")]
    settlement_account: Option<String>,
    lines: Vec<DocLineRequest>,
    /// The buyer's own reference ("Deres referanse", EN 16931 BT-10).
    /// Omitted = the customer's standing reference, if any (#221).
    buyer_reference: Option<String>,
    /// Selected unbilled hours (docs/timer.md): appended as hour lines
    /// per (prosjekt, sats) group and marked fakturert in the SAME
    /// transaction as the invoice — one invoice can carry products and
    /// hours. Requires TIMER_FAKTURER in addition to FAKTURA_SKRIV.
    timer_entry_ids: Option<Vec<Uuid>>,
    /// Billable expense claims to rebill (docs/utlegg.md
    /// «Viderefakturering»), each as `resale` (cost + output VAT) or
    /// `disbursement` (outside the VAT base). Requires UTLEGG_LES_ALLE in
    /// addition to FAKTURA_SKRIV.
    expenses: Option<Vec<RebillRequest>>,
}

#[derive(Deserialize)]
pub struct RebillRequest {
    expense_id: Uuid,
    treatment: RebillTreatment,
}

#[derive(Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RebillTreatment {
    Resale,
    Disbursement,
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
    require(&state, person.person_id, company_id, Right::InvoiceWrite).await?;
    let timer = request.timer_entry_ids.unwrap_or_default();
    if !timer.is_empty() {
        // Hours on the invoice lock other people's entries — that is the
        // biller's action, not any invoice writer's.
        require(&state, person.person_id, company_id, Right::TimesheetBill).await?;
    }
    let rebill: Vec<regnmed_db::Rebill> = request
        .expenses
        .unwrap_or_default()
        .into_iter()
        .map(|e| regnmed_db::Rebill {
            expense_id: e.expense_id,
            treatment: match e.treatment {
                RebillTreatment::Resale => regnmed_db::RebillAs::Resale,
                RebillTreatment::Disbursement => regnmed_db::RebillAs::Disbursement,
            },
        })
        .collect();
    if !rebill.is_empty() {
        // Someone else's claims: whoever bills them must be able to see them.
        require(&state, person.person_id, company_id, Right::ExpenseReadAll).await?;
    }

    let cash = request.cash_payment_method.filter(|s| !s.trim().is_empty());
    let settlement_account = request.settlement_account;
    let draft = regnmed_db::InvoiceDraft {
        // create_kontantfaktura sets this itself; the ordinary route
        // never issues one, so a caller cannot turn a credit sale into a
        // "paid" document by passing a flag.
        cash_payment_method: None,
        party_no: request.party_no,
        buyer_reference: request.buyer_reference,
        invoice_date: request.invoice_date,
        due_date: request.due_date,
        delivery_date: request.delivery_date.unwrap_or(request.invoice_date),
        delivery_place: request.delivery_place,
        journal_code: request.journal.unwrap_or_else(|| "GL".into()),
        receivable_account: request.receivable_account.unwrap_or_else(|| "1500".into()),
        vat_account: request.vat_account.unwrap_or_else(|| "2700".into()),
        currency: request.currency.map(|v| v.to_uppercase()),
        exchange_rate_micro: None,
        lines: resolve_lines(&state, company_id, request.lines).await?,
    };
    let created_by = person.name.as_deref().unwrap_or(&person.sub);

    // Kontantsalg takes its own route: the settlement has to be in the
    // same transaction as the issue, so it cannot be an extra step a
    // caller might skip and leave a "paid" document with an open item
    // behind it.
    if let Some(middel) = cash {
        if !timer.is_empty() || !rebill.is_empty() {
            return Err(ApiError::BadRequest(
                "timer og utlegg faktureres på en vanlig faktura, ikke på kontantsalg".into(),
            ));
        }
        let account = settlement_account.ok_or_else(|| {
            ApiError::BadRequest(
                "kontantsalg må si hvor pengene havnet (oppgjorskonto, f.eks. 1900 eller 1920)"
                    .into(),
            )
        })?;
        let issued = regnmed_db::invoice::create_cash_invoice(
            &state.pool,
            company_id,
            &draft,
            &account,
            &middel,
            created_by,
        )
        .await
        .map_err(|e| ApiError::BadRequest(e.to_string()))?;
        return Ok(Json(issued_json(&issued)));
    }

    let issued = if timer.is_empty() && rebill.is_empty() {
        regnmed_db::create_invoice(&state.pool, company_id, &draft, created_by, None).await
    } else {
        regnmed_db::create_invoice_with_billables(
            &state.pool,
            company_id,
            &draft,
            &timer,
            &rebill,
            None,
            created_by,
        )
        .await
    }
    .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    Ok(Json(issued_json(&issued)))
}

#[derive(Deserialize)]
pub struct ListQuery {
    /// Legacy: every invoice with something remaining, credit notes
    /// included. Oversikt sums it; `status=open` is the portal's filter.
    #[serde(default)]
    open: bool,
    /// `open`, `overdue`, `paid` or `credit`.
    status: Option<String>,
    /// Invoice number, KID, or part of the customer's name.
    q: Option<String>,
    /// Newest first, at most this many (1..=200). Absent = all, oldest
    /// first, as before.
    limit: Option<i64>,
    #[serde(default)]
    offset: i64,
    /// Only invoices numbered below this (keyset paging; see
    /// regnmed_db::InvoiceFilter::before).
    before: Option<i64>,
    /// One customer's invoices (party number).
    party_no: Option<String>,
}

fn invoice_row_json(i: &regnmed_db::InvoiceRow) -> serde_json::Value {
    json!({
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
        "status": i.status,
    })
}

pub async fn list_invoices(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(company_id): Path<Uuid>,
    Query(query): Query<ListQuery>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require(&state, person.person_id, company_id, Right::InvoiceRead).await?;
    let filter = regnmed_db::InvoiceFilter {
        status: query.status.as_deref(),
        search: query.q.as_deref(),
        limit: query.limit.map(|n| n.clamp(1, 200)),
        offset: query.offset,
        before: query.before,
        party_no: query.party_no.as_deref(),
    };
    let page = regnmed_db::list_invoices(&state.pool, company_id, &filter)
        .await
        .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    let rows = page
        .rows
        .iter()
        .filter(|i| !query.open || i.remaining_ore != 0);
    Ok(Json(json!({
        "invoices": rows.map(invoice_row_json).collect::<Vec<_>>(),
        "total": page.total,
        "counts": page.counts,
    })))
}

/// One invoice with its lines -- the portal's invoice page.
pub async fn get_invoice(
    State(state): State<AppState>,
    person: AuthPerson,
    Path((company_id, invoice_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require(&state, person.person_id, company_id, Right::InvoiceRead).await?;
    let d = regnmed_db::get_invoice(&state.pool, company_id, invoice_id)
        .await?
        .ok_or(ApiError::NotFound)?;
    let pair =
        |p: Option<(Uuid, i64)>| p.map(|(id, no)| json!({ "invoice_id": id, "invoice_no": no }));
    let mut out = invoice_row_json(&d.row);
    out["delivery_date"] = json!(d.delivery_date.map(|x| x.to_string()));
    out["delivery_place"] = json!(d.delivery_place);
    out["buyer_reference"] = json!(d.buyer_reference);
    out["valuta"] = json!(d.currency);
    out["party_email"] = json!(d.party_email);
    out["credits"] = json!(pair(d.credits));
    out["credited_by"] = json!(pair(d.credited_by));
    out["created_by"] = json!(d.created_by);
    out["voucher_id"] = json!(d.voucher_id);
    out["voucher"] = json!(d.voucher_label);
    out["settlements"] = json!(
        d.settlements
            .iter()
            .map(|m| json!({
                "voucher_id": m.voucher_id,
                "voucher": m.voucher_label,
                "date": m.date.to_string(),
                "description": m.description,
                "amount_ore": m.amount_ore,
                "matched_by": m.matched_by,
            }))
            .collect::<Vec<_>>()
    );
    out["net_ore"] = json!(d.lines.iter().map(|l| l.net_ore).sum::<i64>());
    out["vat_ore"] = json!(d.lines.iter().map(|l| l.vat_ore).sum::<i64>());
    out["lines"] = json!(
        d.lines
            .iter()
            .map(|l| json!({
                "line_no": l.line_no,
                "description": l.description,
                "quantity_milli": l.quantity_milli,
                "unit_price_ore": l.unit_price_ore,
                "net_ore": l.net_ore,
                "vat_code": l.vat_code,
                "vat_ore": l.vat_ore,
                "avdeling": l.department,
                "prosjekt": l.project,
            }))
            .collect::<Vec<_>>()
    );
    Ok(Json(out))
}

#[derive(Deserialize)]
pub struct PaymentRequest {
    #[serde(rename = "dato")]
    date: chrono::NaiveDate,
    #[serde(rename = "belop_ore")]
    amount_ore: i64,
    /// The money account (19xx); 1920 bank when absent.
    #[serde(rename = "konto")]
    account: Option<String>,
}

/// Records a payment on the invoice: posts it and matches it against the
/// receivable in one transaction. Issuing rights plus the right to post
/// vouchers, since it is both.
pub async fn register_payment(
    State(state): State<AppState>,
    person: AuthPerson,
    Path((company_id, invoice_id)): Path<(Uuid, Uuid)>,
    Json(request): Json<PaymentRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let access = require(&state, person.person_id, company_id, Right::InvoiceWrite).await?;
    if !access.has(Right::VoucherPost) {
        return Err(ApiError::Forbidden(Right::VoucherPost.slug()));
    }
    let created_by = person.name.as_deref().unwrap_or(&person.sub);
    let posted = regnmed_db::register_invoice_payment(
        &state.pool,
        company_id,
        invoice_id,
        request.date,
        request.amount_ore,
        request.account.as_deref().unwrap_or("1920"),
        created_by,
    )
    .await
    .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    Ok(Json(json!({
        "voucher_id": posted.id,
        "voucher": format!("{}-{}", posted.fiscal_year, posted.voucher_number),
    })))
}

pub async fn credit_note(
    State(state): State<AppState>,
    person: AuthPerson,
    Path((company_id, invoice_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require(&state, person.person_id, company_id, Right::InvoiceWrite).await?;
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
    require(&state, person.person_id, company_id, Right::InvoiceRead).await?;
    let attachment_id = regnmed_db::invoice_pdf_attachment_id(&state.pool, company_id, invoice_id)
        .await?
        .ok_or(ApiError::NotFound)?;
    let (meta, content) = regnmed_db::get_attachment(&state.pool, company_id, attachment_id)
        .await
        .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    Ok(file_response(
        Vis::Inline,
        &meta.filename,
        "application/pdf",
        content,
    ))
}

/// EHF (PEPPOL BIS Billing 3.0) for an issued invoice: the document
/// stored with the PDF at issue, hash-checked on read (#222). Invoices
/// issued before that are rendered from their rows on request
/// (docs/ehf.md).
pub async fn invoice_ehf(
    State(state): State<AppState>,
    person: AuthPerson,
    Path((company_id, invoice_id)): Path<(Uuid, Uuid)>,
) -> Result<Response, ApiError> {
    require(&state, person.person_id, company_id, Right::InvoiceRead).await?;
    let xml = regnmed_db::invoice_ehf(&state.pool, company_id, invoice_id)
        .await
        .map_err(|e| ApiError::BadRequest(format!("{e:#}")))?;
    Ok(file_response(
        Vis::Attachment,
        &format!("ehf-{invoice_id}.xml"),
        "application/xml",
        xml,
    ))
}
