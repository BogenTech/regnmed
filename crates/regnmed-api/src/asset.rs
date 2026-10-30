//! Anleggsregister (docs/anlegg.md, #40):
//!
//! - GET/POST /companies/{id}/assets                 register
//! - POST /companies/{id}/assets/depreciate          generate due months
//! - POST /companies/{id}/assets/{aid}/dispose      avhending
//! - GET  /companies/{id}/assets/{aid}/runs         depreciation log
//! - GET  /companies/{id}/assets/saldo?year=        skattemessig saldo
//!
//! Reading is open to every access level (the revisor reads the
//! register); registering, depreciating and disposing require
//! bokforing or admin.

use axum::Json;
use axum::extract::{Path, Query, State};
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

use crate::AppState;
use crate::access::{Right, require};
use crate::auth::{ApiError, AuthPerson};

#[derive(Deserialize)]
pub struct CreateAssetRequest {
    #[serde(rename = "navn")]
    name: String,
    #[serde(rename = "anskaffelsesdato")]
    acquisition_date: chrono::NaiveDate,
    kostpris_ore: i64,
    #[serde(default)]
    restverdi_ore: i64,
    levetid_maneder: i32,
    /// Defaults: balanse 1250, avskrivning 6000.
    #[serde(rename = "balansekonto")]
    balance_account: Option<String>,
    #[serde(rename = "avskrivningskonto")]
    depreciation_account: Option<String>,
    #[serde(rename = "saldogruppe")]
    depreciation_group: String,
    anskaffelse_voucher_id: Option<Uuid>,
}

pub async fn create(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(company_id): Path<Uuid>,
    Json(request): Json<CreateAssetRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require(&state, person.person_id, company_id, Right::AssetWrite).await?;
    let created_by = person.name.as_deref().unwrap_or(&person.sub);
    let (id, warning) = regnmed_db::create_asset(
        &state.pool,
        company_id,
        &regnmed_db::AssetDraft {
            name: request.name,
            acquisition_date: request.acquisition_date,
            kostpris_ore: request.kostpris_ore,
            restverdi_ore: request.restverdi_ore,
            levetid_maneder: request.levetid_maneder,
            balance_account: request.balance_account.unwrap_or_else(|| "1250".into()),
            depreciation_account: request
                .depreciation_account
                .unwrap_or_else(|| "6000".into()),
            depreciation_group: request.depreciation_group,
            anskaffelse_voucher_id: request.anskaffelse_voucher_id,
        },
        created_by,
    )
    .await
    .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    Ok(Json(json!({ "asset_id": id, "warning": warning })))
}

pub async fn list(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(company_id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require(&state, person.person_id, company_id, Right::AssetRead).await?;
    let assets = regnmed_db::list_assets(&state.pool, company_id).await?;
    Ok(Json(json!({
        "assets": assets.iter().map(|a| json!({
            "asset_id": a.id,
            "navn": a.name,
            "anskaffelsesdato": a.acquisition_date.to_string(),
            "kostpris_ore": a.kostpris_ore,
            "restverdi_ore": a.restverdi_ore,
            "levetid_maneder": a.levetid_maneder,
            "balansekonto": a.balance_account,
            "avskrivningskonto": a.depreciation_account,
            "saldogruppe": a.depreciation_group,
            "akkumulert_ore": a.akkumulert_ore,
            "bokfort_ore": a.posted_ore,
            "avhendet_dato": a.disposed_date.map(|d| d.to_string()),
            "vederlag_ore": a.consideration_ore,
        })).collect::<Vec<_>>(),
    })))
}

#[derive(Deserialize, Default)]
pub struct DepreciateRequest {
    /// Generate every month ending on or before this date; default today.
    through: Option<chrono::NaiveDate>,
}

pub async fn depreciate(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(company_id): Path<Uuid>,
    body: Option<Json<DepreciateRequest>>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require(&state, person.person_id, company_id, Right::AssetWrite).await?;
    let request = body.map(|Json(r)| r).unwrap_or_default();
    let through = match request.through {
        Some(d) => d,
        None => sqlx::query_scalar("select current_date")
            .fetch_one(&state.pool)
            .await
            .map_err(anyhow::Error::from)?,
    };
    let created_by = person.name.as_deref().unwrap_or(&person.sub);
    let outcomes = regnmed_db::depreciate_due(&state.pool, company_id, through, created_by)
        .await
        .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    Ok(Json(json!({
        "generated": outcomes.iter().filter(|o| o.voucher.is_some()).count(),
        "failed": outcomes.iter().filter(|o| o.voucher.is_none()).count(),
        "outcomes": outcomes.iter().map(|o| json!({
            "navn": o.name,
            "period": o.period.to_string(),
            "amount_ore": o.amount_ore,
            "voucher": o.voucher.map(|(year, no)| format!("{year}-{no}")),
            "detail": o.detail,
        })).collect::<Vec<_>>(),
    })))
}

#[derive(Deserialize)]
pub struct DisposeRequest {
    #[serde(rename = "dato")]
    date: chrono::NaiveDate,
    #[serde(default, rename = "vederlag_ore")]
    consideration_ore: i64,
    /// Defaults: motkonto 1920, gevinst 3880, tap 7880.
    #[serde(rename = "motkonto")]
    contra_account: Option<String>,
    #[serde(rename = "gevinstkonto")]
    gain_account: Option<String>,
    #[serde(rename = "tapskonto")]
    loss_account: Option<String>,
}

pub async fn dispose(
    State(state): State<AppState>,
    person: AuthPerson,
    Path((company_id, asset_id)): Path<(Uuid, Uuid)>,
    Json(request): Json<DisposeRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require(&state, person.person_id, company_id, Right::AssetWrite).await?;
    let created_by = person.name.as_deref().unwrap_or(&person.sub);
    let disposal = regnmed_db::dispose_asset(
        &state.pool,
        company_id,
        asset_id,
        request.date,
        request.consideration_ore,
        request.contra_account.as_deref().unwrap_or("1920"),
        request.gain_account.as_deref().unwrap_or("3880"),
        request.loss_account.as_deref().unwrap_or("7880"),
        created_by,
    )
    .await
    .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    Ok(Json(json!({
        "bokfort_ore": disposal.posted_ore,
        "gevinst_ore": disposal.gevinst_ore,
        "voucher": disposal.voucher.map(|(year, no)| format!("{year}-{no}")),
    })))
}

pub async fn runs(
    State(state): State<AppState>,
    person: AuthPerson,
    Path((company_id, asset_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require(&state, person.person_id, company_id, Right::AssetRead).await?;
    let runs = regnmed_db::list_depreciations(&state.pool, company_id, asset_id).await?;
    Ok(Json(json!({
        "runs": runs.iter().map(|r| json!({
            "period": r.period.to_string(),
            "amount_ore": r.amount_ore,
            "voucher": match (r.fiscal_year, r.voucher_number) {
                (Some(year), Some(no)) => Some(format!("{year}-{no}")),
                _ => None,
            },
            "detail": r.detail,
        })).collect::<Vec<_>>(),
    })))
}

#[derive(Deserialize)]
pub struct BalanceQuery {
    year: i32,
}

pub async fn balance(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(company_id): Path<Uuid>,
    Query(query): Query<BalanceQuery>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require(&state, person.person_id, company_id, Right::AssetRead).await?;
    let rapport = regnmed_db::balance_report(&state.pool, company_id, query.year)
        .await
        .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    Ok(Json(json!({
        "year": rapport.year,
        "grupper": rapport.grupper.iter().map(|g| json!({
            "gruppe": g.gruppe,
            "beskrivelse": g.description,
            "inngaende_ore": g.opening_ore,
            "tilgang_ore": g.additions_ore,
            "vederlag_ore": g.consideration_ore,
            "grunnlag_ore": g.basis_ore,
            "sats_bp": g.rate_bp,
            "avskrivning_ore": g.avskrivning_ore,
            "utgaende_ore": g.closing_ore,
        })).collect::<Vec<_>>(),
        "bokfort_ore": rapport.posted_ore,
        "skattemessig_ore": rapport.skattemessig_ore,
        "forskjell_ore": rapport.forskjell_ore,
    })))
}
