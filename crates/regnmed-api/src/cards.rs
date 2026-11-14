//! Company cards (docs/utlegg.md «Firmakort»):
//!
//! - GET  /companies/{id}/cards              the register (BANK_AVSTEM), or the
//!                                           cards the caller may file for
//! - POST /companies/{id}/cards              register a card (BANK_AVSTEM)
//! - PUT  /companies/{id}/cards/{card_id}    rename, hand over, retire
//! - GET  /companies/{id}/expenses/card-lines  statement lines without a receipt

use axum::Json;
use axum::extract::{Path, Query, State};
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

use crate::AppState;
use crate::access::{Right, require};
use crate::auth::{ApiError, AuthPerson};

fn card_json(c: &regnmed_db::company_card::CompanyCard) -> serde_json::Value {
    json!({
        "card_id": c.id,
        "name": c.name,
        "last4": c.last4,
        "label": c.label(),
        "holder_person_id": c.holder_person_id,
        "holder": c.holder_name,
        "account": c.account_number,
        "account_name": c.account_name,
        "active": c.active,
    })
}

#[derive(Deserialize)]
pub struct ListQuery {
    /// Only the cards the caller may file a purchase for, also for whoever
    /// may see the whole register (the claim form asks this).
    #[serde(default)]
    usable: bool,
}

/// Whoever reconciles the bank sees the whole register; everyone else the
/// active cards they may file a purchase for — their own, and the shared
/// ones if they handle claims.
pub async fn list(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(company_id): Path<Uuid>,
    Query(query): Query<ListQuery>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let access = require(&state, person.person_id, company_id, Right::ExpenseWriteOwn).await?;
    let usable_by = if access.has(Right::BankReconcile) && !query.usable {
        None
    } else {
        Some((person.person_id, access.has(Right::ExpenseApprove)))
    };
    let cards = regnmed_db::company_card::list_cards(&state.pool, company_id, usable_by).await?;
    Ok(Json(json!({
        "cards": cards.iter().map(card_json).collect::<Vec<_>>(),
    })))
}

#[derive(Deserialize)]
pub struct CreateCardRequest {
    name: String,
    last4: String,
    holder_person_id: Option<Uuid>,
    #[serde(rename = "konto")]
    account: String,
}

pub async fn create(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(company_id): Path<Uuid>,
    Json(request): Json<CreateCardRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require(&state, person.person_id, company_id, Right::BankReconcile).await?;
    let created_by = person.name.as_deref().unwrap_or(&person.sub);
    let id = regnmed_db::company_card::create_card(
        &state.pool,
        company_id,
        &request.name,
        request.last4.trim(),
        request.holder_person_id,
        request.account.trim(),
        created_by,
    )
    .await
    .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    Ok(Json(json!({ "card_id": id })))
}

#[derive(Deserialize)]
pub struct UpdateCardRequest {
    name: String,
    holder_person_id: Option<Uuid>,
    active: bool,
}

pub async fn update(
    State(state): State<AppState>,
    person: AuthPerson,
    Path((company_id, card_id)): Path<(Uuid, Uuid)>,
    Json(request): Json<UpdateCardRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require(&state, person.person_id, company_id, Right::BankReconcile).await?;
    regnmed_db::company_card::update_card(
        &state.pool,
        company_id,
        card_id,
        &request.name,
        request.holder_person_id,
        request.active,
    )
    .await
    .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    Ok(Json(json!({ "ok": true })))
}

/// Card statement lines still waiting for a receipt: the holder's own, or
/// every card's for whoever handles claims.
pub async fn missing_receipts(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(company_id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let access = require(&state, person.person_id, company_id, Right::ExpenseWriteOwn).await?;
    let lines = regnmed_db::company_card::card_lines(
        &state.pool,
        company_id,
        person.person_id,
        access.has(Right::ExpenseApprove),
    )
    .await?;
    Ok(Json(json!({
        "lines": lines.iter().map(|l| json!({
            "bank_transaction_id": l.bank_transaction_id,
            "card_id": l.card_id,
            "card": l.card_label,
            "dato": l.date,
            "belop_ore": l.amount_ore,
            "beskrivelse": l.description,
            "own": l.holder_person_id == Some(person.person_id),
        })).collect::<Vec<_>>(),
    })))
}
