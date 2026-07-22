//! OCR giro endpoints (web-first, engagement-guarded):
//!
//! - POST /companies/{id}/ocr/files?account=1920   OCR file body
//! - GET  /companies/{id}/ocr/payments?from=&to=
//!
//! Reading is open to every access level; importing requires bokforing
//! or admin — same rules as bank reconciliation.

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
            "read-only access — importing requires bokforing",
        ));
    }
    Ok(())
}

#[derive(Deserialize)]
pub struct AccountQuery {
    account: String,
}

pub async fn import_file(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(company_id): Path<Uuid>,
    Query(query): Query<AccountQuery>,
    body: String,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_access(&state, person.person_id, company_id, true).await?;

    let file = regnmed_core::ocr::parse(&body)
        .map_err(|e| ApiError::BadRequest(format!("OCR file: {e}")))?;
    let imported_by = person.name.as_deref().unwrap_or(&person.sub);
    let summary =
        regnmed_db::import_ocr_file(&state.pool, company_id, &query.account, &file, imported_by)
            .await
            .map_err(|e| ApiError::BadRequest(e.to_string()))?;

    Ok(Json(json!({
        "transmission_number": file.transmission_number,
        "batches": summary.batches,
        "payments": summary.payments,
        "sum_ore": summary.sum_ore,
        "kid_invalid": summary.kid_invalid,
    })))
}

#[derive(Deserialize)]
pub struct RangeQuery {
    from: Option<chrono::NaiveDate>,
    to: Option<chrono::NaiveDate>,
}

pub async fn list_payments(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(company_id): Path<Uuid>,
    Query(query): Query<RangeQuery>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_access(&state, person.person_id, company_id, false).await?;

    let payments =
        regnmed_db::list_ocr_payments(&state.pool, company_id, query.from, query.to).await?;
    Ok(Json(json!({
        "payments": payments.iter().map(|p| json!({
            "id": p.id,
            "date": p.payment_date.to_string(),
            "amount_ore": p.amount_ore,
            "kid": p.kid,
            "kid_valid": p.kid_valid,
            "transaction_type": p.transaction_type,
            "bank_reference": p.bank_reference,
            "account": p.account_number,
            "invoice_no": p.invoice_no,
        })).collect::<Vec<_>>(),
        "sum_ore": payments.iter().map(|p| p.amount_ore).sum::<i64>(),
    })))
}
