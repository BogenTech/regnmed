//! Bilagsinnboks endpoints — the client/accountant daily loop:
//!
//! - POST /companies/{id}/inbox?filename=…            upload (bytes; admin/bokforing)
//! - GET  /companies/{id}/inbox[?status=ny]           list (any access)
//! - GET  /companies/{id}/inbox/{doc}/content         download, hash-checked
//! - POST /companies/{id}/inbox/{doc}/bokfor          voucher draft → posted +
//!                                                    attached + marked, one tx
//! - POST /companies/{id}/inbox/{doc}/avvis           {note} (admin/bokforing)

use axum::Json;
use axum::body::Bytes;
use axum::extract::{Path, Query, State};
use axum::http::header;
use axum::response::Response;
use regnmed_core::Ore;
use regnmed_core::voucher::{EntryDraft, VoucherDraft};
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
    /// What the client hashed before sending (the mobile offline queue
    /// does — docs/portal.md, #48).
    sha256: Option<String>,
}

pub async fn upload(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(company_id): Path<Uuid>,
    Query(query): Query<UploadQuery>,
    headers: axum::http::HeaderMap,
    body: Bytes,
) -> Result<Json<serde_json::Value>, ApiError> {
    require(&state, person.person_id, company_id, Right::VoucherUpload).await?;
    let content_type = headers
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("application/octet-stream")
        .to_string();
    let uploaded_by = person.name.as_deref().unwrap_or(&person.sub);
    let id = regnmed_db::upload_inbox_document(
        &state.pool,
        company_id,
        &query.filename,
        &content_type,
        &body,
        uploaded_by,
        query.sha256.as_deref(),
    )
    .await
    .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    Ok(Json(json!({ "document_id": id })))
}

#[derive(Deserialize)]
pub struct ListQuery {
    status: Option<String>,
}

pub async fn list(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(company_id): Path<Uuid>,
    Query(query): Query<ListQuery>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require(&state, person.person_id, company_id, Right::VoucherRead).await?;
    let rows = regnmed_db::list_inbox(&state.pool, company_id, query.status.as_deref()).await?;
    Ok(Json(json!({
        "documents": rows.iter().map(|d| json!({
            "document_id": d.id,
            "filename": d.filename,
            "content_type": d.content_type,
            "byte_size": d.byte_size,
            "sha256": d.sha256_hex,
            "uploaded_by": d.uploaded_by,
            "uploaded_at": d.created_at.to_rfc3339(),
            "status": d.status,
            "voucher_id": d.voucher_id,
            "decided_by": d.decided_by,
            "note": d.note,
            "attestering": d.attestering,
            "attestert_av": d.attestert_av,
        })).collect::<Vec<_>>(),
    })))
}

pub async fn download(
    State(state): State<AppState>,
    person: AuthPerson,
    Path((company_id, document_id)): Path<(Uuid, Uuid)>,
) -> Result<Response, ApiError> {
    require(&state, person.person_id, company_id, Right::VoucherRead).await?;
    let (filename, content_type, content) =
        regnmed_db::get_inbox_document(&state.pool, company_id, document_id)
            .await
            .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    // The uploader does not decide what we say the bytes are (#64).
    Ok(file_response(
        Vis::Attachment,
        &filename,
        &content_type,
        content,
    ))
}

#[derive(Deserialize)]
pub struct BokforLine {
    account: String,
    amount_ore: i64,
    vat_code: Option<String>,
    party_no: Option<String>,
    description: Option<String>,
    /// Dimension codes (docs/dimensjoner.md).
    #[serde(rename = "avdeling")]
    department: Option<String>,
    #[serde(rename = "prosjekt")]
    project: Option<String>,
}

#[derive(Deserialize)]
pub struct BokforRequest {
    journal_code: String,
    date: chrono::NaiveDate,
    description: String,
    lines: Vec<BokforLine>,
}

pub async fn bokfor(
    State(state): State<AppState>,
    person: AuthPerson,
    Path((company_id, document_id)): Path<(Uuid, Uuid)>,
    Json(request): Json<BokforRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require(&state, person.person_id, company_id, Right::VoucherPost).await?;
    let draft = VoucherDraft {
        journal_code: request.journal_code,
        voucher_date: request.date,
        description: request.description,
        reverses: None,
        entries: request
            .lines
            .iter()
            .map(|l| EntryDraft {
                account_number: l.account.clone(),
                amount: Ore(l.amount_ore),
                vat_code: l.vat_code.clone(),
                description: l.description.clone(),
                party_no: l.party_no.clone(),
                department: l.department.clone(),
                project: l.project.clone(),
                currency: None,
            })
            .collect(),
    };
    let decided_by = person.name.as_deref().unwrap_or(&person.sub);
    let posted = regnmed_db::bokfor_inbox_document(
        &state.pool,
        company_id,
        document_id,
        &draft,
        decided_by,
        person.person_id,
    )
    .await
    .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    Ok(Json(json!({
        "voucher_id": posted.id,
        "voucher": format!("{}-{}", posted.fiscal_year, posted.voucher_number),
        "chain_seq": posted.chain_seq,
    })))
}

#[derive(Deserialize)]
pub struct AvvisRequest {
    note: String,
}

pub async fn avvis(
    State(state): State<AppState>,
    person: AuthPerson,
    Path((company_id, document_id)): Path<(Uuid, Uuid)>,
    Json(request): Json<AvvisRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require(&state, person.person_id, company_id, Right::VoucherPost).await?;
    let decided_by = person.name.as_deref().unwrap_or(&person.sub);
    regnmed_db::avvis_inbox_document(
        &state.pool,
        company_id,
        document_id,
        &request.note,
        decided_by,
    )
    .await
    .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    Ok(Json(json!({ "avvist": true })))
}

/// The posting suggestion derived from a stored EHF document — computed
/// on every read, never stored (docs/ehf.md). When the document is not
/// EHF the endpoint says so; the human then posts it by hand as before.
pub async fn ehf_forslag(
    State(state): State<AppState>,
    person: AuthPerson,
    Path((company_id, document_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require(&state, person.person_id, company_id, Right::VoucherRead).await?;
    let forslag = regnmed_db::inbox_ehf_forslag(&state.pool, company_id, document_id)
        .await
        .map_err(|e| ApiError::BadRequest(format!("{e:#}")))?;
    let m = &forslag.received;
    Ok(Json(json!({
        "er_kreditnota": m.er_kreditnota,
        "fakturanr": m.fakturanr,
        "fakturadato": m.invoice_date.map(|d| d.to_string()),
        "forfallsdato": m.due_date.map(|d| d.to_string()),
        "valuta": m.currency,
        "selger_navn": m.seller_name,
        "selger_orgnr": m.seller_orgnumber,
        "kid": m.kid,
        "kontonummer": m.account_number,
        "netto_ore": m.net_ore,
        "mva_ore": m.vat_ore,
        "brutto_ore": m.gross_ore,
        "linjer": m.linjer.iter().map(|l| json!({
            "beskrivelse": l.description,
            "netto_ore": l.net_ore,
            "mva_sats_bp": l.vat_rate_bp,
        })).collect::<Vec<_>>(),
        "leverandor_no": forslag.supplier_no,
        "leverandor_navn": forslag.supplier_name,
        "warnings": forslag.advarsler,
    })))
}

/// "What should this bilag become?" for ANY document
/// (docs/bilagstolkning.md, #34): EHF is read exactly, a PDF with a text
/// layer is interpreted heuristically, a scanned image yields nothing —
/// and says so. Every field carries a justification, and nothing is
/// posted: the suggestion fills the form, the human decides.
pub async fn forslag(
    State(state): State<AppState>,
    person: AuthPerson,
    Path((company_id, document_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require(&state, person.person_id, company_id, Right::VoucherRead).await?;
    let f = regnmed_db::inbox_forslag(&state.pool, company_id, document_id)
        .await
        .map_err(|e| ApiError::BadRequest(format!("{e:#}")))?;
    Ok(Json(json!({
        "kilde": f.source,
        "selger_navn": f.seller_name,
        "orgnr": f.orgnumber,
        "fakturanr": f.fakturanr,
        "dato": f.date.map(|d| d.to_string()),
        "forfall": f.due_date.map(|d| d.to_string()),
        "kid": f.kid,
        "kontonummer": f.account_number,
        "netto_ore": f.net_ore,
        "mva_ore": f.vat_ore,
        "brutto_ore": f.gross_ore,
        "leverandor_no": f.supplier_no,
        "leverandor_navn": f.supplier_name,
        "konto": f.account,
        "konto_begrunnelse": f.account_reason,
        "begrunnelser": f.begrunnelser.iter().map(|(felt, hvorfor)| json!({
            "felt": felt, "hvorfor": hvorfor,
        })).collect::<Vec<_>>(),
        "warnings": f.advarsler,
    })))
}
