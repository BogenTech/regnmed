//! Exchange rates and revaluation (docs/valuta.md, #44):
//!
//! - GET  /companies/{id}/currency/rates          newest rate per valuta
//! - POST /companies/{id}/currency/rates          manual rate, kilde recorded
//! - POST /companies/{id}/currency/rates/fetch    Norges Banks åpne API
//! - POST /companies/{id}/currency/regulate       urealisert year-end
//!
//! The rate table is global market data; endpoints are company-scoped
//! purely for the access guard. Reading is open to every access level,
//! writing requires bokforing or admin.

use axum::Json;
use axum::extract::{Path, State};
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

use crate::AppState;
use crate::auth::{ApiError, AuthPerson};
use crate::tilgang::{Rett, krev};

pub async fn list_rates(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(company_id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ApiError> {
    krev(&state, person.person_id, company_id, Rett::ValutaLes).await?;
    let kurser = regnmed_db::latest_kurser(&state.pool).await?;
    Ok(Json(json!({
        "rates": kurser.iter().map(|k| json!({
            "valuta": k.currency,
            "dato": k.date.to_string(),
            "kurs": regnmed_core::currency::rate_str(k.rate_micro),
            "kurs_micro": k.rate_micro,
            "kilde": k.kilde,
        })).collect::<Vec<_>>(),
    })))
}

#[derive(Deserialize)]
pub struct ManualRateRequest {
    #[serde(rename = "valuta")]
    currency: String,
    #[serde(rename = "dato")]
    date: chrono::NaiveDate,
    /// Decimal, e.g. "11.6543".
    kurs: String,
    kilde: Option<String>,
}

pub async fn add_rate(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(company_id): Path<Uuid>,
    Json(request): Json<ManualRateRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    krev(&state, person.person_id, company_id, Rett::ValutaSkriv).await?;
    let rate_micro = regnmed_core::currency::parse_rate(&request.kurs)
        .ok_or_else(|| ApiError::BadRequest(format!("uparselig kurs {:?}", request.kurs)))?;
    let registered_by = person.name.as_deref().unwrap_or(&person.sub);
    regnmed_db::insert_kurs(
        &state.pool,
        &request.currency.to_uppercase(),
        request.date,
        rate_micro,
        request
            .kilde
            .as_deref()
            .unwrap_or(&format!("manuell ({registered_by})")),
    )
    .await
    .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    Ok(Json(json!({ "registered": true })))
}

#[derive(Deserialize)]
pub struct FetchRequest {
    valutaer: Vec<String>,
    /// Recent noteringer per currency; default 10.
    days: Option<u32>,
}

pub async fn fetch_rates(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(company_id): Path<Uuid>,
    Json(request): Json<FetchRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    krev(&state, person.person_id, company_id, Rett::ValutaSkriv).await?;
    let valutaer: Vec<String> = request
        .valutaer
        .iter()
        .map(|v| v.trim().to_uppercase())
        .filter(|v| !v.is_empty())
        .collect();
    let client = regnmed_gov::norgesbank::NorgesBankClient::from_env();
    let noteringer = client
        .fetch_rates(&valutaer, request.days.unwrap_or(10))
        .await
        .map_err(|e| ApiError::BadRequest(format!("Norges Bank: {e:#}")))?;
    for n in &noteringer {
        regnmed_db::insert_kurs(
            &state.pool,
            &n.currency,
            n.date,
            n.rate_micro,
            "Norges Bank EXR",
        )
        .await?;
    }
    Ok(Json(json!({ "fetched": noteringer.len() })))
}

#[derive(Deserialize)]
pub struct RegulateRequest {
    #[serde(rename = "dato")]
    date: chrono::NaiveDate,
    /// Balansekonto for the urealiserte regulering — chosen
    /// consciously per kontoplan, no default.
    #[serde(rename = "balansekonto")]
    balance_account: String,
    #[serde(rename = "gevinstkonto")]
    gain_account: Option<String>,
    #[serde(rename = "tapskonto")]
    loss_account: Option<String>,
}

pub async fn regulate(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(company_id): Path<Uuid>,
    Json(request): Json<RegulateRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    krev(&state, person.person_id, company_id, Rett::ValutaSkriv).await?;
    let created_by = person.name.as_deref().unwrap_or(&person.sub);
    let result = regnmed_db::kursregulering(
        &state.pool,
        company_id,
        request.date,
        &request.balance_account,
        request.gain_account.as_deref().unwrap_or("8060"),
        request.loss_account.as_deref().unwrap_or("8160"),
        created_by,
    )
    .await
    .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    Ok(Json(match result {
        None => json!({ "diff_ore": 0, "voucher": null, "reversal": null }),
        Some((diff, voucher, reversal)) => json!({
            "diff_ore": diff,
            "voucher": format!("{}-{}", voucher.0, voucher.1),
            "reversal": format!("{}-{}", reversal.0, reversal.1),
        }),
    }))
}
