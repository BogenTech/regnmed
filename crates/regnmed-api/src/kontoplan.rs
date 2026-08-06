//! Hovedbok endpoints (docs/hovedbok.md):
//!
//! - GET  /companies/{id}/accounts        kontoplan + standard catalog (BilagLes)
//! - POST /companies/{id}/accounts        add account, standard or custom (BilagBokfor)
//! - PUT  /companies/{id}/accounts/{nr}   rename / (de)activate (BilagBokfor)
//! - POST /companies/{id}/vouchers        manual bilag → posted voucher (BilagBokfor)
//!
//! The standard catalog is Skatteetaten's account list vendored in
//! regnmed-core (the same names the SAF-T wizard matches against) — the
//! portal shows every code a regnskapsfører knows without the company
//! carrying 254 unused rows.

use axum::Json;
use axum::extract::{Path, State};
use regnmed_core::Ore;
use regnmed_core::voucher::{EntryDraft, VoucherDraft};
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

use crate::AppState;
use crate::auth::{ApiError, AuthPerson};
use crate::tilgang::{Rett, krev};

pub async fn list_accounts(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(company_id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ApiError> {
    krev(&state, person.person_id, company_id, Rett::BilagLes).await?;
    let kontoer = regnmed_db::list_accounts(&state.pool, company_id)
        .await
        .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    let standard: Vec<_> = regnmed_core::saft::standard_accounts()
        .iter()
        .map(|(number, name)| json!({ "number": format!("{number}"), "name": name }))
        .collect();
    Ok(Json(json!({
        "kontoer": kontoer.iter().map(|k| json!({
            "number": k.number,
            "name": k.name,
            "vat_code": k.vat_code,
            "active": k.active,
            "reskontro_kind": k.reskontro_kind,
            "saldo_ore": k.saldo_ore,
            "posteringer": k.posteringer,
        })).collect::<Vec<_>>(),
        "standard": standard,
    })))
}

#[derive(Deserialize)]
pub struct CreateAccountRequest {
    number: String,
    /// Omitted or empty = use the standard catalog's name; custom
    /// accounts (numbers outside the catalog) must bring their own.
    name: Option<String>,
}

pub async fn create_account(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(company_id): Path<Uuid>,
    Json(request): Json<CreateAccountRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    krev(&state, person.person_id, company_id, Rett::BilagBokfor).await?;
    let number = request.number.trim();
    let name = match request.name.as_deref().map(str::trim) {
        Some(n) if !n.is_empty() => n.to_string(),
        _ => standard_name(number)
            .ok_or_else(|| {
                ApiError::BadRequest(format!(
                    "konto {number} finnes ikke i standardkontoplanen — oppgi et navn"
                ))
            })?
            .to_string(),
    };
    regnmed_db::create_account(&state.pool, company_id, number, &name)
        .await
        .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    Ok(Json(json!({ "number": number, "name": name })))
}

#[derive(Deserialize)]
pub struct UpdateAccountRequest {
    name: Option<String>,
    active: Option<bool>,
}

pub async fn update_account(
    State(state): State<AppState>,
    person: AuthPerson,
    Path((company_id, number)): Path<(Uuid, String)>,
    Json(request): Json<UpdateAccountRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    krev(&state, person.person_id, company_id, Rett::BilagBokfor).await?;
    regnmed_db::update_account(
        &state.pool,
        company_id,
        &number,
        request.name.as_deref(),
        request.active,
    )
    .await
    .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    Ok(Json(json!({ "number": number })))
}

fn standard_name(number: &str) -> Option<&'static str> {
    let n: u32 = number.parse().ok()?;
    regnmed_core::saft::standard_accounts()
        .iter()
        .find(|(num, _)| *num == n)
        .map(|(_, name)| *name)
}

#[derive(Deserialize)]
pub struct ManualLine {
    account: String,
    amount_ore: i64,
    vat_code: Option<String>,
    party_no: Option<String>,
    description: Option<String>,
    avdeling: Option<String>,
    prosjekt: Option<String>,
}

/// Same request shape as innboks-bokføring, minus the document — so the
/// portal's posting form serves both.
#[derive(Deserialize)]
pub struct ManualVoucherRequest {
    journal_code: String,
    date: chrono::NaiveDate,
    description: String,
    lines: Vec<ManualLine>,
}

pub async fn post_manual_voucher(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(company_id): Path<Uuid>,
    Json(request): Json<ManualVoucherRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    krev(&state, person.person_id, company_id, Rett::BilagBokfor).await?;
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
                valuta: None,
            })
            .collect(),
    };
    let created_by = person.name.as_deref().unwrap_or(&person.sub);
    let posted = regnmed_db::post_manual_voucher(&state.pool, company_id, &draft, created_by)
        .await
        .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    Ok(Json(json!({
        "voucher_id": posted.id,
        "voucher": format!("{}-{}", posted.fiscal_year, posted.voucher_number),
        "chain_seq": posted.chain_seq,
    })))
}
