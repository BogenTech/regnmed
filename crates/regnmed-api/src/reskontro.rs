//! Reskontro endpoints (web-first, engagement-guarded):
//!
//! - GET    /companies/{id}/parties?kind=kunde       spesifikasjon (saldo per party)
//! - POST   /companies/{id}/parties                  {kind, name, orgnr?, party_no?}
//! - GET    /companies/{id}/parties/{party_id}/items?open=true
//! - POST   /companies/{id}/reskontro/matches        {entry_a, entry_b, amount_ore}
//! - DELETE /companies/{id}/reskontro/matches/{match_id}
//! - PUT    /companies/{id}/accounts/{number}/reskontro {kind}  (flag account)
//!
//! Reading is open to every access level; mutations require bokforing or
//! admin.

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
            "read-only access — reskontro changes require bokforing",
        ));
    }
    Ok(())
}

#[derive(Deserialize)]
pub struct KindQuery {
    kind: Option<String>,
}

pub async fn list_parties(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(company_id): Path<Uuid>,
    Query(query): Query<KindQuery>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_access(&state, person.person_id, company_id, false).await?;
    let parties = regnmed_db::list_parties(&state.pool, company_id, query.kind.as_deref()).await?;
    Ok(Json(json!({
        "parties": parties.iter().map(|p| json!({
            "party_id": p.id,
            "party_no": p.party_no,
            "kind": p.kind,
            "name": p.name,
            "orgnr": p.orgnr,
            "address": p.address,
            "email": p.email,
            "saldo_ore": p.saldo_ore,
        })).collect::<Vec<_>>(),
    })))
}

#[derive(Deserialize)]
pub struct CreatePartyRequest {
    kind: String,
    name: String,
    orgnr: Option<String>,
    party_no: Option<String>,
}

pub async fn create_party(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(company_id): Path<Uuid>,
    Json(request): Json<CreatePartyRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_access(&state, person.person_id, company_id, true).await?;
    let (party_id, party_no) = regnmed_db::create_party(
        &state.pool,
        company_id,
        &request.kind,
        &request.name,
        request.orgnr.as_deref(),
        request.party_no.as_deref(),
    )
    .await
    .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    Ok(Json(json!({ "party_id": party_id, "party_no": party_no })))
}

#[derive(Deserialize)]
pub struct ItemsQuery {
    #[serde(default)]
    open: bool,
}

pub async fn party_items(
    State(state): State<AppState>,
    person: AuthPerson,
    Path((company_id, party_id)): Path<(Uuid, Uuid)>,
    Query(query): Query<ItemsQuery>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_access(&state, person.person_id, company_id, false).await?;
    let items = regnmed_db::party_items(&state.pool, company_id, party_id, query.open).await?;
    Ok(Json(json!({
        "items": items.iter().map(|i| json!({
            "entry_id": i.entry_id,
            "voucher": i.voucher_label,
            "date": i.date.to_string(),
            "description": i.description,
            "amount_ore": i.amount_ore,
            "matched_ore": i.matched_ore,
            "remaining_ore": i.remaining_ore,
        })).collect::<Vec<_>>(),
    })))
}

#[derive(Deserialize)]
pub struct MatchRequest {
    entry_a: Uuid,
    entry_b: Uuid,
    amount_ore: i64,
}

pub async fn create_match(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(company_id): Path<Uuid>,
    Json(request): Json<MatchRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_access(&state, person.person_id, company_id, true).await?;
    let matched_by = person.name.as_deref().unwrap_or(&person.sub);
    let match_id = regnmed_db::match_items(
        &state.pool,
        company_id,
        request.entry_a,
        request.entry_b,
        request.amount_ore,
        matched_by,
    )
    .await
    .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    Ok(Json(json!({ "match_id": match_id })))
}

pub async fn delete_match(
    State(state): State<AppState>,
    person: AuthPerson,
    Path((company_id, match_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_access(&state, person.person_id, company_id, true).await?;
    regnmed_db::unmatch_items(&state.pool, company_id, match_id)
        .await
        .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    Ok(Json(json!({ "matched": false })))
}

#[derive(Deserialize)]
pub struct ReskontroFlagRequest {
    /// "kunde", "leverandor" or null to clear.
    kind: Option<String>,
}

pub async fn set_account_reskontro(
    State(state): State<AppState>,
    person: AuthPerson,
    Path((company_id, account_number)): Path<(Uuid, String)>,
    Json(request): Json<ReskontroFlagRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_access(&state, person.person_id, company_id, true).await?;
    regnmed_db::set_account_reskontro(
        &state.pool,
        company_id,
        &account_number,
        request.kind.as_deref(),
    )
    .await
    .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    Ok(Json(
        json!({ "account": account_number, "reskontro_kind": request.kind }),
    ))
}
