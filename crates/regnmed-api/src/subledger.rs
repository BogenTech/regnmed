//! Reskontro endpoints (web-first, engagement-guarded):
//!
//! - GET    /companies/{id}/parties?kind=kunde       spesifikasjon (saldo per party)
//! - POST   /companies/{id}/parties                  {kind, name, orgnr?, party_no?}
//! - GET    /companies/{id}/parties/{party_id}/items?open=true
//! - POST   /companies/{id}/reskontro/matches        {entry_a, entry_b, amount_ore}
//! - DELETE /companies/{id}/reskontro/matches/{match_id}
//! - GET    /companies/{id}/reskontro/overview       tie-out + parties to match
//! - GET    /companies/{id}/parties/{party_id}/matches  match history
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
use crate::access::{Right, require};
use crate::auth::{ApiError, AuthPerson};

#[derive(Deserialize)]
pub struct KindQuery {
    kind: Option<String>,
    /// Exact party number or orgnr, or part of the name or e-mail.
    q: Option<String>,
    /// `number` (default), `name`, `balance` or `overdue`.
    sort: Option<String>,
    #[serde(default)]
    desc: bool,
    /// At most this many (1..=200). Absent = all, as before.
    limit: Option<i64>,
    #[serde(default)]
    offset: i64,
}

fn party_json(p: &regnmed_db::PartyRow) -> serde_json::Value {
    json!({
        "party_id": p.id,
        "party_no": p.party_no,
        "kind": p.kind,
        "name": p.name,
        "orgnr": p.orgnumber,
        "address": p.address,
        "email": p.email,
        "bank_account": p.bank_account,
        "buyer_reference": p.buyer_reference,
        "saldo_ore": p.balance_ore,
        "overdue_ore": p.overdue_ore,
    })
}

pub async fn list_parties(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(company_id): Path<Uuid>,
    Query(query): Query<KindQuery>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require(&state, person.person_id, company_id, Right::SubledgerRead).await?;
    let filter = regnmed_db::PartyFilter {
        kind: query.kind.as_deref(),
        id: None,
        search: query.q.as_deref(),
        sort: query.sort.as_deref(),
        descending: query.desc,
        limit: query.limit.map(|n| n.clamp(1, 200)),
        offset: query.offset,
    };
    let page = regnmed_db::list_parties(&state.pool, company_id, &filter)
        .await
        .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    Ok(Json(json!({
        "parties": page.rows.iter().map(party_json).collect::<Vec<_>>(),
        "total": page.total,
    })))
}

/// One party with saldo and overdue amount -- the portal's customer page,
/// without loading the whole register.
pub async fn get_party(
    State(state): State<AppState>,
    person: AuthPerson,
    Path((company_id, party_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require(&state, person.person_id, company_id, Right::SubledgerRead).await?;
    let filter = regnmed_db::PartyFilter {
        id: Some(party_id),
        ..Default::default()
    };
    let page = regnmed_db::list_parties(&state.pool, company_id, &filter).await?;
    let party = page.rows.first().ok_or(ApiError::NotFound)?;
    Ok(Json(party_json(party)))
}

#[derive(Deserialize)]
pub struct CreatePartyRequest {
    kind: String,
    name: String,
    #[serde(rename = "orgnr")]
    orgnumber: Option<String>,
    party_no: Option<String>,
    /// Contact info in the same request, so a new customer can be
    /// invoiced and e-mailed at once (the portal's Ny kunde form).
    address: Option<String>,
    email: Option<String>,
    bank_account: Option<String>,
}

#[derive(Deserialize)]
pub struct TermsRequest {
    due_date: Option<chrono::NaiveDate>,
    kid: Option<String>,
}

/// Sets or corrects due date and KID on a supplier posting (docs/betaling.md).
/// Insert-only: the newest applies, the earlier ones remain.
pub async fn record_terms(
    State(state): State<AppState>,
    person: AuthPerson,
    Path((company_id, entry_id)): Path<(Uuid, Uuid)>,
    Json(request): Json<TermsRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require(&state, person.person_id, company_id, Right::SubledgerWrite).await?;
    let terms = regnmed_db::PayableTermsDraft {
        due_date: request.due_date,
        kid: request.kid,
    };
    let by = person.name.as_deref().unwrap_or(&person.sub);
    regnmed_db::record_payable_terms(&state.pool, company_id, entry_id, &terms, by)
        .await
        .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    Ok(Json(json!({ "ok": true })))
}

pub async fn create_party(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(company_id): Path<Uuid>,
    Json(request): Json<CreatePartyRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require(&state, person.person_id, company_id, Right::SubledgerWrite).await?;
    let contact =
        request.address.is_some() || request.email.is_some() || request.bank_account.is_some();
    if contact {
        // Contact info has its own right; the same rule as PUT .../contact.
        require(&state, person.person_id, company_id, Right::ContactWrite).await?;
        regnmed_db::validate_party_contact(
            request.email.as_deref(),
            request.bank_account.as_deref(),
        )
        .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    }
    let (party_id, party_no) = regnmed_db::create_party(
        &state.pool,
        company_id,
        &request.kind,
        &request.name,
        request.orgnumber.as_deref(),
        request.party_no.as_deref(),
    )
    .await
    .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    if contact {
        regnmed_db::update_party_contact(
            &state.pool,
            company_id,
            party_id,
            request.address.as_deref(),
            request.email.as_deref(),
            request.bank_account.as_deref(),
        )
        .await
        .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    }
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
    require(&state, person.person_id, company_id, Right::SubledgerRead).await?;
    let items = regnmed_db::party_items(&state.pool, company_id, party_id, query.open).await?;
    Ok(Json(json!({
        "items": items.iter().map(|i| json!({
            "entry_id": i.entry_id,
            "voucher_id": i.voucher_id,
            "voucher": i.voucher_label,
            "date": i.date.to_string(),
            "description": i.description,
            "due_date": i.due_date.map(|d| d.to_string()),
            "kid": i.kid,
            "in_run": i.in_run,
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
    /// NOK matching. Ignored when valuta_cent is given.
    amount_ore: Option<i64>,
    /// Valuta matching (docs/valuta.md): both entries must carry the
    /// same valuta; realized agio posts in the same transaction.
    valuta_cent: Option<i64>,
    /// Agio accounts; defaults 8060 (gevinst) / 8160 (tap).
    #[serde(rename = "gevinstkonto")]
    gain_account: Option<String>,
    #[serde(rename = "tapskonto")]
    loss_account: Option<String>,
}

pub async fn create_match(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(company_id): Path<Uuid>,
    Json(request): Json<MatchRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require(&state, person.person_id, company_id, Right::SubledgerWrite).await?;
    let matched_by = person.name.as_deref().unwrap_or(&person.sub);
    if let Some(valuta_cent) = request.valuta_cent {
        let agio_ore = regnmed_db::match_valuta(
            &state.pool,
            company_id,
            request.entry_a,
            request.entry_b,
            valuta_cent,
            request.gain_account.as_deref().unwrap_or("8060"),
            request.loss_account.as_deref().unwrap_or("8160"),
            matched_by,
        )
        .await
        .map_err(|e| ApiError::BadRequest(e.to_string()))?;
        return Ok(Json(json!({ "matched": true, "agio_ore": agio_ore })));
    }
    let amount_ore = request
        .amount_ore
        .ok_or_else(|| ApiError::BadRequest("amount_ore or valuta_cent required".into()))?;
    let match_id = regnmed_db::match_items(
        &state.pool,
        company_id,
        request.entry_a,
        request.entry_b,
        amount_ore,
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
    require(&state, person.person_id, company_id, Right::SubledgerWrite).await?;
    regnmed_db::unmatch_items(&state.pool, company_id, match_id)
        .await
        .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    Ok(Json(json!({ "matched": false })))
}

#[derive(Deserialize)]
pub struct SubledgerFlagRequest {
    /// "kunde", "leverandor" or null to clear.
    kind: Option<String>,
}

pub async fn set_account_subledger(
    State(state): State<AppState>,
    person: AuthPerson,
    Path((company_id, account_number)): Path<(Uuid, String)>,
    Json(request): Json<SubledgerFlagRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require(&state, person.person_id, company_id, Right::SubledgerWrite).await?;
    regnmed_db::set_account_subledger(
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

/// The workbench's front page: per reskontro account the hovedbok against
/// the parties' sum (the revisjonsrapport's check 4, same numbers), and
/// the parties that have open items on both sides to match.
pub async fn overview(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(company_id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require(&state, person.person_id, company_id, Right::SubledgerRead).await?;
    let (accounts, (parties, total), names) = tokio::try_join!(
        regnmed_db::subledger_accounts(&state.pool, company_id),
        regnmed_db::parties_to_match(&state.pool, company_id, 100),
        async {
            let rows: Vec<(String, String)> =
                sqlx::query_as("select number, name from account where company_id = $1")
                    .bind(company_id)
                    .fetch_all(&state.pool)
                    .await?;
            anyhow::Ok(rows)
        },
    )?;
    let name_of = |n: &str| {
        names
            .iter()
            .find(|(no, _)| no == n)
            .map(|(_, name)| name.clone())
    };
    Ok(Json(json!({
        "accounts": accounts.iter().map(|a| json!({
            "account": a.account,
            "name": name_of(&a.account),
            "reskontro_kind": a.flagg,
            "ledger_ore": a.general_ledger_ore,
            "subledger_ore": a.subledger_ore,
            "without_party_count": a.without_party_count,
            "wrong_kind_count": a.wrong_kind_count,
        })).collect::<Vec<_>>(),
        "to_match": parties.iter().map(|p| json!({
            "party_id": p.party_id,
            "party_no": p.party_no,
            "name": p.name,
            "kind": p.kind,
            "open_positive_ore": p.open_positive_ore,
            "open_negative_ore": p.open_negative_ore,
        })).collect::<Vec<_>>(),
        "to_match_total": total,
    })))
}

pub async fn party_matches(
    State(state): State<AppState>,
    person: AuthPerson,
    Path((company_id, party_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require(&state, person.person_id, company_id, Right::SubledgerRead).await?;
    let rows = regnmed_db::party_matches(&state.pool, company_id, party_id).await?;
    Ok(Json(json!({
        "matches": rows.iter().map(|m| json!({
            "match_id": m.id,
            "entry_a": m.entry_a,
            "entry_b": m.entry_b,
            "voucher_a": m.voucher_a,
            "date_a": m.date_a.to_string(),
            "voucher_b": m.voucher_b,
            "date_b": m.date_b.to_string(),
            "amount_ore": m.amount_ore,
            "matched_by": m.matched_by,
            "created_at": m.created_at,
            "currency_settlement": m.currency_settlement,
        })).collect::<Vec<_>>(),
    })))
}
