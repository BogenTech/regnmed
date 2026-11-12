//! Bank reconciliation endpoints (web-first, engagement-guarded):
//!
//! - POST   /companies/{id}/bank/statements?account=1920   camt.053 XML or bank CSV body
//! - GET    /companies/{id}/bank/accounts                   the 19xx accounts
//! - GET    /companies/{id}/bank/reconciliation?account=1920
//! - GET    /companies/{id}/bank/entries?account=1920&q=    search open entries
//! - POST   /companies/{id}/bank/matches                    {bank_transaction_id, entry_id}
//! - DELETE /companies/{id}/bank/matches/{bank_transaction_id}
//! - POST   /companies/{id}/bank/transactions/{tid}/post    {konto, tekst}
//!
//! Reading is open to every access level (revisor included); importing
//! and matching require bokforing or admin.

use axum::Json;
use axum::extract::{Path, Query, State};
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

use crate::AppState;
use crate::access::{Right, require};
use crate::auth::{ApiError, AuthPerson};

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
    require(&state, person.person_id, company_id, Right::BankReconcile).await?;

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

fn entry_json(e: &regnmed_db::UnmatchedEntryRow) -> serde_json::Value {
    json!({
        "entry_id": e.entry_id,
        "voucher": e.voucher_label,
        "date": e.voucher_date.to_string(),
        "amount_ore": e.amount_ore,
        "description": e.description,
    })
}

pub async fn accounts(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(company_id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require(&state, person.person_id, company_id, Right::BankRead).await?;
    let rows = regnmed_db::bank_accounts(&state.pool, company_id).await?;
    Ok(Json(json!({
        "accounts": rows.iter().map(|a| json!({
            "number": a.number,
            "name": a.name,
            "balance_ore": a.balance_ore,
            "last_statement_to": a.last_statement_to.map(|d| d.to_string()),
            "unmatched_bank": a.unmatched_bank,
        })).collect::<Vec<_>>(),
    })))
}

pub async fn reconciliation(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(company_id): Path<Uuid>,
    Query(query): Query<AccountQuery>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require(&state, person.person_id, company_id, Right::BankRead).await?;

    let status = regnmed_db::reconciliation_status(&state.pool, company_id, &query.account)
        .await
        .map_err(|e| ApiError::BadRequest(e.to_string()))?;

    Ok(Json(json!({
        "account": status.account_number,
        "ledger_balance_ore": status.ledger_balance_ore,
        "ledger_at_statement_ore": status.ledger_at_statement_ore,
        "statement_closing_ore": status.statement_closing_ore,
        "statement_to_date": status.statement_to_date.map(|d| d.to_string()),
        "matched_count": status.matched_count,
        "unmatched_bank": status.unmatched_bank.iter().zip(&status.suggestions).map(|(t, s)| json!({
            "bank_transaction_id": t.id,
            "booking_date": t.booking_date.to_string(),
            "amount_ore": t.amount_ore,
            "description": t.description,
            "reference": t.reference,
            "suggestions": s.iter().map(entry_json).collect::<Vec<_>>(),
        })).collect::<Vec<_>>(),
        "unmatched_entries": status.unmatched_entries.iter().map(entry_json).collect::<Vec<_>>(),
        "statements": status.statements.iter().map(|s| json!({
            "statement_id": s.id,
            "statement_ref": s.statement_ref,
            "from_date": s.from_date.map(|d| d.to_string()),
            "to_date": s.to_date.map(|d| d.to_string()),
            "closing_ore": s.closing_ore,
            "transactions": s.transactions,
            "imported_by": s.imported_by,
            "imported_at": s.imported_at,
        })).collect::<Vec<_>>(),
        "recent_matches": status.recent_matches.iter().map(|m| json!({
            "bank_transaction_id": m.bank_transaction_id,
            "booking_date": m.booking_date.to_string(),
            "amount_ore": m.amount_ore,
            "description": m.description,
            "voucher": m.voucher_label,
            "method": m.method,
        })).collect::<Vec<_>>(),
    })))
}

#[derive(Deserialize)]
pub struct SearchQuery {
    account: String,
    #[serde(default)]
    q: String,
}

pub async fn search_entries(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(company_id): Path<Uuid>,
    Query(query): Query<SearchQuery>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require(&state, person.person_id, company_id, Right::BankRead).await?;
    let rows = regnmed_db::search_open_entries(&state.pool, company_id, &query.account, &query.q)
        .await
        .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    Ok(Json(json!({
        "entries": rows.iter().map(entry_json).collect::<Vec<_>>(),
    })))
}

#[derive(Deserialize)]
pub struct PostLineRequest {
    #[serde(rename = "konto")]
    account: String,
    #[serde(rename = "tekst", default)]
    text: String,
}

/// Posts a bank line that has no voucher behind it (fee, interest) and
/// matches it in the same transaction — reconciling AND posting, so it
/// takes both rights.
pub async fn post_line(
    State(state): State<AppState>,
    person: AuthPerson,
    Path((company_id, bank_transaction_id)): Path<(Uuid, Uuid)>,
    Json(request): Json<PostLineRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let access = require(&state, person.person_id, company_id, Right::BankReconcile).await?;
    if !access.has(Right::VoucherPost) {
        return Err(ApiError::Forbidden(Right::VoucherPost.slug()));
    }
    let created_by = person.name.as_deref().unwrap_or(&person.sub);
    let posted = regnmed_db::post_from_bank_line(
        &state.pool,
        company_id,
        bank_transaction_id,
        request.account.trim(),
        &request.text,
        created_by,
    )
    .await
    .map_err(|e| ApiError::BadRequest(format!("{e:#}")))?;
    Ok(Json(json!({
        "voucher": format!("{}-{}", posted.fiscal_year, posted.voucher_number),
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
    require(&state, person.person_id, company_id, Right::BankReconcile).await?;
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
    require(&state, person.person_id, company_id, Right::BankReconcile).await?;
    regnmed_db::unmatch(&state.pool, company_id, bank_transaction_id)
        .await
        .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    Ok(Json(json!({ "matched": false })))
}
