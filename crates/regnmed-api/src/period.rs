//! Periodelåsing and bilagsvedlegg endpoints (web-first,
//! engagement-guarded):
//!
//! - GET  /companies/{id}/period-lock                current + history
//! - PUT  /companies/{id}/period-lock                {locked_through}
//!        advancing needs bokforing; reopening (moving back) needs admin
//! - GET  /companies/{id}/vouchers                   minimal listing
//! - POST /companies/{id}/vouchers/{vid}/attachments?filename=…  (bytes)
//! - GET  /companies/{id}/vouchers/{vid}/attachments
//! - GET  /companies/{id}/attachments/{aid}          download (hash-checked)

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

pub async fn get_period_lock(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(company_id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require(&state, person.person_id, company_id, Right::VoucherRead).await?;
    let current = regnmed_db::current_period_lock(&state.pool, company_id).await?;
    let history = regnmed_db::period_lock_history(&state.pool, company_id).await?;
    Ok(Json(json!({
        "locked_through": current.map(|d| d.to_string()),
        "history": history.iter().map(|h| json!({
            "locked_through": h.locked_through.to_string(),
            "set_by": h.set_by,
            "at": h.created_at.to_rfc3339(),
        })).collect::<Vec<_>>(),
    })))
}

#[derive(Deserialize)]
pub struct SetLockRequest {
    locked_through: chrono::NaiveDate,
}

pub async fn set_period_lock(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(company_id): Path<Uuid>,
    Json(request): Json<SetLockRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    // Locking requires bokføring; REOPENING requires admin, and that is
    // decided inside set_period_lock — hence the role comes along.
    let role = require(&state, person.person_id, company_id, Right::PeriodLock).await?;
    let set_by = person.name.as_deref().unwrap_or(&person.sub);
    regnmed_db::set_period_lock(
        &state.pool,
        company_id,
        request.locked_through,
        set_by,
        role.is_admin(),
    )
    .await
    .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    Ok(Json(
        json!({ "locked_through": request.locked_through.to_string() }),
    ))
}

#[derive(Deserialize)]
pub struct VoucherListQuery {
    from: Option<chrono::NaiveDate>,
    to: Option<chrono::NaiveDate>,
    /// Free text: bilagsnr, dato, tekst, kontonummer/-navn.
    sok: Option<String>,
    limit: Option<i64>,
    offset: Option<i64>,
    /// Include each voucher's lines (the hovedbok browsing view).
    lines: Option<bool>,
    /// Only bilag without an attachment (#85) — the working list behind
    /// the audit report's Dokumentasjon check, so whoever is
    /// tidying can see exactly which ones. Importjournalen is left out:
    /// its documentation is the source file, hashed in check 8.
    #[serde(rename = "uten_vedlegg")]
    without_attachment: Option<bool>,
}

/// Vouchers newest-first, paged and filtered server-side. Without
/// parameters this answers exactly what it always has (500 newest,
/// headers only) — the parameters exist for the hovedbok view, where
/// client-side paging stopped scaling with the ledger.
pub async fn list_vouchers(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(company_id): Path<Uuid>,
    Query(query): Query<VoucherListQuery>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require(&state, person.person_id, company_id, Right::VoucherRead).await?;
    let limit = query.limit.unwrap_or(500).clamp(1, 500);
    let offset = query.offset.unwrap_or(0).max(0);
    let med_linjer = query.lines.unwrap_or(false);
    let side = regnmed_db::list_vouchers_paged(
        &state.pool,
        company_id,
        query.from,
        query.to,
        query.sok.as_deref(),
        limit,
        offset,
        med_linjer,
        query.without_attachment.unwrap_or(false),
    )
    .await
    .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    Ok(Json(json!({
        "total": side.total,
        "vouchers": side.vouchers.iter().map(|v| {
            let mut o = json!({
                "voucher_id": v.voucher_id,
                "voucher": format!("{}-{}", v.fiscal_year, v.voucher_number),
                "journal": v.journal_code,
                "date": v.voucher_date.to_string(),
                "description": v.description,
            });
            if med_linjer {
                o["lines"] = json!(v.lines.iter().map(|l| json!({
                    "account": l.account,
                    "account_name": l.account_name,
                    "amount_ore": l.amount_ore,
                    "vat_code": l.vat_code,
                    "description": l.description,
                    "party_no": l.party_no,
                    "avdeling": l.department,
                    "prosjekt": l.project,
                })).collect::<Vec<_>>());
            }
            o
        }).collect::<Vec<_>>(),
    })))
}

#[derive(Deserialize)]
pub struct UploadQuery {
    filename: String,
}

pub async fn upload_attachment(
    State(state): State<AppState>,
    person: AuthPerson,
    Path((company_id, voucher_id)): Path<(Uuid, Uuid)>,
    Query(query): Query<UploadQuery>,
    headers: axum::http::HeaderMap,
    body: Bytes,
) -> Result<Json<serde_json::Value>, ApiError> {
    require(&state, person.person_id, company_id, Right::AttachmentWrite).await?;
    let content_type = headers
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("application/octet-stream")
        .to_string();
    let uploaded_by = person.name.as_deref().unwrap_or(&person.sub);
    let meta = regnmed_db::add_attachment(
        &state.pool,
        company_id,
        voucher_id,
        &query.filename,
        &content_type,
        &body,
        uploaded_by,
    )
    .await
    .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    Ok(Json(json!({
        "attachment_id": meta.id,
        "sha256": meta.sha256_hex,
        "byte_size": meta.byte_size,
    })))
}

pub async fn list_voucher_attachments(
    State(state): State<AppState>,
    person: AuthPerson,
    Path((company_id, voucher_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require(&state, person.person_id, company_id, Right::VoucherRead).await?;
    let attachments = regnmed_db::list_attachments(&state.pool, company_id, voucher_id).await?;
    Ok(Json(json!({
        "attachments": attachments.iter().map(|a| json!({
            "attachment_id": a.id,
            "filename": a.filename,
            "content_type": a.content_type,
            "byte_size": a.byte_size,
            "sha256": a.sha256_hex,
            "uploaded_by": a.uploaded_by,
        })).collect::<Vec<_>>(),
    })))
}

pub async fn download_attachment(
    State(state): State<AppState>,
    person: AuthPerson,
    Path((company_id, attachment_id)): Path<(Uuid, Uuid)>,
) -> Result<Response, ApiError> {
    require(&state, person.person_id, company_id, Right::VoucherRead).await?;
    let (meta, content) = regnmed_db::get_attachment(&state.pool, company_id, attachment_id)
        .await
        .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    // The uploader does not decide what we say the bytes are (#64).
    Ok(file_response(
        Vis::Attachment,
        &meta.filename,
        &meta.content_type,
        content,
    ))
}

/// One voucher with its lines, attachments count, reversal links and —
/// when another flow owns it — where it is corrected instead.
pub async fn get_voucher(
    State(state): State<AppState>,
    person: AuthPerson,
    Path((company_id, voucher_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require(&state, person.person_id, company_id, Right::VoucherRead).await?;
    let d = regnmed_db::voucher_detail(&state.pool, company_id, voucher_id)
        .await?
        .ok_or(ApiError::NotFound)?;
    let v = &d.voucher;
    let link = |p: &Option<(Uuid, String)>| {
        p.as_ref()
            .map(|(id, label)| json!({ "voucher_id": id, "voucher": label }))
    };
    Ok(Json(json!({
        "voucher_id": v.voucher_id,
        "voucher": format!("{}-{}", v.fiscal_year, v.voucher_number),
        "journal": v.journal_code,
        "date": v.voucher_date.to_string(),
        "description": v.description,
        "created_by": d.created_by,
        "created_at": d.created_at,
        "reverses": link(&d.reverses),
        "reversed_by": link(&d.reversed_by),
        "owner": d.owner,
        "attachments": d.attachments,
        "lines": v.lines.iter().zip(&d.currency).map(|(l, c)| json!({
            "account": l.account,
            "account_name": l.account_name,
            "amount_ore": l.amount_ore,
            "vat_code": l.vat_code,
            "description": l.description,
            "party_no": l.party_no,
            "avdeling": l.department,
            "prosjekt": l.project,
            "valuta": c.as_ref().map(|(code, _)| code),
            "valutabelop_cent": c.as_ref().map(|(_, cent)| cent),
        })).collect::<Vec<_>>(),
    })))
}

#[derive(Deserialize, Default)]
pub struct ReverseRequest {
    /// The reversal's date; today when left out. It must lie in an open
    /// period — a locked one is refused by the posting itself.
    #[serde(rename = "dato")]
    date: Option<chrono::NaiveDate>,
}

pub async fn reverse_voucher(
    State(state): State<AppState>,
    person: AuthPerson,
    Path((company_id, voucher_id)): Path<(Uuid, Uuid)>,
    body: Option<Json<ReverseRequest>>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require(&state, person.person_id, company_id, Right::VoucherPost).await?;
    let request = body.map(|Json(r)| r).unwrap_or_default();
    let date = match request.date {
        Some(d) => d,
        None => sqlx::query_scalar("select current_date")
            .fetch_one(&state.pool)
            .await
            .map_err(anyhow::Error::from)?,
    };
    let created_by = person.name.as_deref().unwrap_or(&person.sub);
    let reversal =
        regnmed_db::reverse_voucher(&state.pool, company_id, voucher_id, date, created_by)
            .await
            .map_err(|e| ApiError::BadRequest(format!("{e:#}")))?;
    let posted = &reversal.posted;
    Ok(Json(json!({
        "voucher_id": posted.id,
        "voucher": format!("{}-{}", posted.fiscal_year, posted.voucher_number),
        "bank_unmatched": reversal.bank_unmatched,
    })))
}
