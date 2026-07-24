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
use axum::response::{IntoResponse, Response};
use regnmed_core::Ore;
use regnmed_core::voucher::{EntryDraft, VoucherDraft};
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
        return Err(ApiError::Forbidden("krever bokføringstilgang"));
    }
    Ok(())
}

#[derive(Deserialize)]
pub struct UploadQuery {
    filename: String,
}

pub async fn upload(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(company_id): Path<Uuid>,
    Query(query): Query<UploadQuery>,
    headers: axum::http::HeaderMap,
    body: Bytes,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_access(&state, person.person_id, company_id, true).await?;
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
    require_access(&state, person.person_id, company_id, false).await?;
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
        })).collect::<Vec<_>>(),
    })))
}

pub async fn download(
    State(state): State<AppState>,
    person: AuthPerson,
    Path((company_id, document_id)): Path<(Uuid, Uuid)>,
) -> Result<Response, ApiError> {
    require_access(&state, person.person_id, company_id, false).await?;
    let (filename, content_type, content) =
        regnmed_db::get_inbox_document(&state.pool, company_id, document_id)
            .await
            .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    Ok((
        [
            (header::CONTENT_TYPE, content_type),
            (
                header::CONTENT_DISPOSITION,
                format!("attachment; filename=\"{filename}\""),
            ),
        ],
        content,
    )
        .into_response())
}

#[derive(Deserialize)]
pub struct BokforLine {
    account: String,
    amount_ore: i64,
    vat_code: Option<String>,
    party_no: Option<String>,
    description: Option<String>,
    /// Dimension codes (docs/dimensjoner.md).
    avdeling: Option<String>,
    prosjekt: Option<String>,
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
    require_access(&state, person.person_id, company_id, true).await?;
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
                avdeling: l.avdeling.clone(),
                prosjekt: l.prosjekt.clone(),
            })
            .collect(),
    };
    let decided_by = person.name.as_deref().unwrap_or(&person.sub);
    let posted =
        regnmed_db::bokfor_inbox_document(&state.pool, company_id, document_id, &draft, decided_by)
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
    require_access(&state, person.person_id, company_id, true).await?;
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
