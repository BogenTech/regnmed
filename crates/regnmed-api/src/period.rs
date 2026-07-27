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
use axum::response::{IntoResponse, Response};
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

use crate::AppState;
use crate::auth::{ApiError, AuthPerson};
use crate::tilgang::{Rett, krev};

pub async fn get_period_lock(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(company_id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ApiError> {
    krev(&state, person.person_id, company_id, Rett::BilagLes).await?;
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
    // Å låse krever bokføring; å ÅPNE IGJEN krever admin, og det
    // avgjøres inne i set_period_lock — derfor følger rollen med.
    let rolle = krev(&state, person.person_id, company_id, Rett::PeriodeLaas).await?;
    let set_by = person.name.as_deref().unwrap_or(&person.sub);
    regnmed_db::set_period_lock(
        &state.pool,
        company_id,
        request.locked_through,
        set_by,
        rolle.er_admin(),
    )
    .await
    .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    Ok(Json(
        json!({ "locked_through": request.locked_through.to_string() }),
    ))
}

pub async fn list_vouchers(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(company_id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ApiError> {
    krev(&state, person.person_id, company_id, Rett::BilagLes).await?;
    let rows = sqlx::query_as::<_, (Uuid, i32, i64, chrono::NaiveDate, String)>(
        "select v.id, v.fiscal_year, v.voucher_number, v.voucher_date, v.description
         from voucher v join journal j on j.id = v.journal_id
         where v.company_id = $1
         order by v.chain_seq desc limit 500",
    )
    .bind(company_id)
    .fetch_all(&state.pool)
    .await
    .map_err(anyhow::Error::from)?;
    Ok(Json(json!({
        "vouchers": rows.iter().map(|(id, year, number, date, description)| json!({
            "voucher_id": id,
            "voucher": format!("{year}-{number}"),
            "date": date.to_string(),
            "description": description,
        })).collect::<Vec<_>>(),
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
    krev(&state, person.person_id, company_id, Rett::VedleggSkriv).await?;
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
    krev(&state, person.person_id, company_id, Rett::BilagLes).await?;
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
    krev(&state, person.person_id, company_id, Rett::BilagLes).await?;
    let (meta, content) = regnmed_db::get_attachment(&state.pool, company_id, attachment_id)
        .await
        .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    Ok((
        [
            (header::CONTENT_TYPE, meta.content_type.clone()),
            (
                header::CONTENT_DISPOSITION,
                format!("attachment; filename=\"{}\"", meta.filename),
            ),
        ],
        content,
    )
        .into_response())
}
