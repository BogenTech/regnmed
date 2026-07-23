//! Bank reconciliation endpoints (web-first, engagement-guarded):
//!
//! - POST   /companies/{id}/bank/statements?account=1920   camt.053 XML or bank CSV body
//! - GET    /companies/{id}/bank/reconciliation?account=1920
//! - POST   /companies/{id}/bank/matches                    {bank_transaction_id, entry_id}
//! - DELETE /companies/{id}/bank/matches/{bank_transaction_id}
//!
//! Reading is open to every access level (revisor included); importing
//! and matching require bokforing or admin.

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
            "read-only access — importing and matching require bokforing",
        ));
    }
    Ok(())
}

#[derive(Deserialize)]
pub struct AccountQuery {
    account: String,
}

pub async fn import_statement(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(company_id): Path<Uuid>,
    Query(query): Query<AccountQuery>,
    body: String,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_access(&state, person.person_id, company_id, true).await?;

    // Both file tiers land here: XML is camt.053, anything else goes
    // through the header-detecting CSV parser — same statement shape,
    // same engine (docs/bank.md).
    let statement = if body
        .trim_start_matches('\u{feff}')
        .trim_start()
        .starts_with('<')
    {
        regnmed_core::camt053::parse(&body)
            .map_err(|e| ApiError::BadRequest(format!("camt.053: {e}")))?
    } else {
        regnmed_core::bankcsv::parse(&body)
            .map_err(|e| ApiError::BadRequest(format!("CSV: {e}")))?
    };
    let imported_by = person.name.as_deref().unwrap_or(&person.sub);
    let summary = regnmed_db::import_statement(
        &state.pool,
        company_id,
        &query.account,
        &statement,
        imported_by,
    )
    .await
    .map_err(|e| ApiError::BadRequest(e.to_string()))?;

    Ok(Json(json!({
        "statement_id": summary.statement_id,
        "statement_ref": statement.statement_ref,
        "transactions": summary.transactions,
        "auto_matched": summary.auto_matched,
    })))
}

pub async fn reconciliation(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(company_id): Path<Uuid>,
    Query(query): Query<AccountQuery>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_access(&state, person.person_id, company_id, false).await?;

    let status = regnmed_db::reconciliation_status(&state.pool, company_id, &query.account)
        .await
        .map_err(|e| ApiError::BadRequest(e.to_string()))?;

    Ok(Json(json!({
        "account": status.account_number,
        "ledger_balance_ore": status.ledger_balance_ore,
        "statement_closing_ore": status.statement_closing_ore,
        "statement_to_date": status.statement_to_date.map(|d| d.to_string()),
        "matched_count": status.matched_count,
        "unmatched_bank": status.unmatched_bank.iter().map(|t| json!({
            "bank_transaction_id": t.id,
            "booking_date": t.booking_date.to_string(),
            "amount_ore": t.amount_ore,
            "description": t.description,
            "reference": t.reference,
        })).collect::<Vec<_>>(),
        "unmatched_entries": status.unmatched_entries.iter().map(|e| json!({
            "entry_id": e.entry_id,
            "voucher": e.voucher_label,
            "date": e.voucher_date.to_string(),
            "amount_ore": e.amount_ore,
            "description": e.description,
        })).collect::<Vec<_>>(),
    })))
}

#[derive(Deserialize)]
pub struct MatchRequest {
    bank_transaction_id: Uuid,
    entry_id: Uuid,
}

pub async fn create_match(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(company_id): Path<Uuid>,
    Json(request): Json<MatchRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_access(&state, person.person_id, company_id, true).await?;
    let matched_by = person.name.as_deref().unwrap_or(&person.sub);
    regnmed_db::manual_match(
        &state.pool,
        company_id,
        request.bank_transaction_id,
        request.entry_id,
        matched_by,
    )
    .await
    .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    Ok(Json(json!({ "matched": true })))
}

pub async fn delete_match(
    State(state): State<AppState>,
    person: AuthPerson,
    Path((company_id, bank_transaction_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_access(&state, person.person_id, company_id, true).await?;
    regnmed_db::unmatch(&state.pool, company_id, bank_transaction_id)
        .await
        .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    Ok(Json(json!({ "matched": false })))
}
