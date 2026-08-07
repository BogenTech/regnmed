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
use axum::response::Response;
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

use crate::AppState;
use crate::auth::{ApiError, AuthPerson};
use crate::herding::{Vis, file_response};
use crate::tilgang::{Rett, krev};

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
    leveringsdato: Option<chrono::NaiveDate>,
    /// Leveringssted, required "der det er relevant" — typically a
    /// vareleveranse to an address other than the buyer's.
    leveringssted: Option<String>,
    /// Defaults: journal GL, receivable 1500, VAT account 2700.
    journal: Option<String>,
    receivable_account: Option<String>,
    vat_account: Option<String>,
    /// Document currency (docs/valuta.md); line amounts in its minor
    /// unit. None = NOK. Requires a kurs in the valutakurs table.
    valuta: Option<String>,
    /// Kontantsalg (#89, §5-3): what settled the ytelse on delivery
    /// ("Kort", "Vipps", "Kontant"). Present = kontantfaktura: the
    /// receivable is raised and settled in one transaction, and the
    /// document carries no KID and no forfall.
    kontant_betalingsmiddel: Option<String>,
    /// Where the money landed — 1900 kontanter, 1920 bank, or the card
    /// acquirer's clearing account. Required with the above; we never
    /// guess how somebody was paid.
    oppgjorskonto: Option<String>,
    lines: Vec<DocLineRequest>,
    /// Selected unbilled hours (docs/timer.md): appended as hour lines
    /// per (prosjekt, sats) group and marked fakturert in the SAME
    /// transaction as the invoice — one invoice can carry products and
    /// hours. Requires TIMER_FAKTURER in addition to FAKTURA_SKRIV.
    timer_entry_ids: Option<Vec<Uuid>>,
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
    krev(&state, person.person_id, company_id, Rett::FakturaSkriv).await?;
    let timer = request.timer_entry_ids.filter(|ids| !ids.is_empty());
    if timer.is_some() {
        // Hours on the invoice lock other people's entries — that is the
        // biller's action, not any invoice writer's.
        krev(&state, person.person_id, company_id, Rett::TimerFakturer).await?;
    }

    let kontant = request
        .kontant_betalingsmiddel
        .filter(|s| !s.trim().is_empty());
    let oppgjorskonto = request.oppgjorskonto;
    let draft = regnmed_db::InvoiceDraft {
        // create_kontantfaktura sets this itself; the ordinary route
        // never issues one, so a caller cannot turn a credit sale into a
        // "paid" document by passing a flag.
        kontant_betalingsmiddel: None,
        party_no: request.party_no,
        invoice_date: request.invoice_date,
        due_date: request.due_date,
        delivery_date: request.leveringsdato.unwrap_or(request.invoice_date),
        delivery_place: request.leveringssted,
        journal_code: request.journal.unwrap_or_else(|| "GL".into()),
        receivable_account: request.receivable_account.unwrap_or_else(|| "1500".into()),
        vat_account: request.vat_account.unwrap_or_else(|| "2700".into()),
        valuta: request.valuta.map(|v| v.to_uppercase()),
        valuta_kurs_micro: None,
        lines: resolve_lines(&state, company_id, request.lines).await?,
    };
    let created_by = person.name.as_deref().unwrap_or(&person.sub);

    // Kontantsalg takes its own route: the settlement has to be in the
    // same transaction as the issue, so it cannot be an extra step a
    // caller might skip and leave a "paid" document with an open item
    // behind it.
    if let Some(middel) = kontant {
        let konto = oppgjorskonto.ok_or_else(|| {
            ApiError::BadRequest(
                "kontantsalg må si hvor pengene havnet (oppgjorskonto, f.eks. 1900 eller 1920)"
                    .into(),
            )
        })?;
        let issued = regnmed_db::invoice::create_kontantfaktura(
            &state.pool,
            company_id,
            &draft,
            &konto,
            &middel,
            created_by,
        )
        .await
        .map_err(|e| ApiError::BadRequest(e.to_string()))?;
        return Ok(Json(issued_json(&issued)));
    }

    let issued = match timer {
        Some(entry_ids) => {
            regnmed_db::create_invoice_with_hours(
                &state.pool,
                company_id,
                &draft,
                &entry_ids,
                None,
                created_by,
            )
            .await
        }
        None => regnmed_db::create_invoice(&state.pool, company_id, &draft, created_by, None).await,
    }
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
    krev(&state, person.person_id, company_id, Rett::FakturaLes).await?;
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
    krev(&state, person.person_id, company_id, Rett::FakturaSkriv).await?;
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
    krev(&state, person.person_id, company_id, Rett::FakturaLes).await?;
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
    krev(&state, person.person_id, company_id, Rett::FakturaLes).await?;
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
