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
use crate::auth::{ApiError, AuthPerson};
use crate::tilgang::{Krav, krev};

#[derive(Deserialize)]
pub struct CreateAssetRequest {
    navn: String,
    anskaffelsesdato: chrono::NaiveDate,
    kostpris_ore: i64,
    #[serde(default)]
    restverdi_ore: i64,
    levetid_maneder: i32,
    /// Defaults: balanse 1250, avskrivning 6000.
    balansekonto: Option<String>,
    avskrivningskonto: Option<String>,
    saldogruppe: String,
    anskaffelse_voucher_id: Option<Uuid>,
}

pub async fn create(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(company_id): Path<Uuid>,
    Json(request): Json<CreateAssetRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    krev(&state, person.person_id, company_id, Krav::Bokfor).await?;
    let created_by = person.name.as_deref().unwrap_or(&person.sub);
    let (id, warning) = regnmed_db::create_asset(
        &state.pool,
        company_id,
        &regnmed_db::AssetDraft {
            navn: request.navn,
            anskaffelsesdato: request.anskaffelsesdato,
            kostpris_ore: request.kostpris_ore,
            restverdi_ore: request.restverdi_ore,
            levetid_maneder: request.levetid_maneder,
            balansekonto: request.balansekonto.unwrap_or_else(|| "1250".into()),
            avskrivningskonto: request.avskrivningskonto.unwrap_or_else(|| "6000".into()),
            saldogruppe: request.saldogruppe,
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
    krev(&state, person.person_id, company_id, Krav::Les).await?;
    let assets = regnmed_db::list_assets(&state.pool, company_id).await?;
    Ok(Json(json!({
        "assets": assets.iter().map(|a| json!({
            "asset_id": a.id,
            "navn": a.navn,
            "anskaffelsesdato": a.anskaffelsesdato.to_string(),
            "kostpris_ore": a.kostpris_ore,
            "restverdi_ore": a.restverdi_ore,
            "levetid_maneder": a.levetid_maneder,
            "balansekonto": a.balansekonto,
            "avskrivningskonto": a.avskrivningskonto,
            "saldogruppe": a.saldogruppe,
            "akkumulert_ore": a.akkumulert_ore,
            "bokfort_ore": a.bokfort_ore,
            "avhendet_dato": a.avhendet_dato.map(|d| d.to_string()),
            "vederlag_ore": a.vederlag_ore,
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
    krev(&state, person.person_id, company_id, Krav::Bokfor).await?;
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
            "navn": o.navn,
            "period": o.period.to_string(),
            "amount_ore": o.amount_ore,
            "voucher": o.voucher.map(|(year, no)| format!("{year}-{no}")),
            "detail": o.detail,
        })).collect::<Vec<_>>(),
    })))
}

#[derive(Deserialize)]
pub struct DisposeRequest {
    dato: chrono::NaiveDate,
    #[serde(default)]
    vederlag_ore: i64,
    /// Defaults: motkonto 1920, gevinst 3880, tap 7880.
    motkonto: Option<String>,
    gevinstkonto: Option<String>,
    tapskonto: Option<String>,
}

pub async fn dispose(
    State(state): State<AppState>,
    person: AuthPerson,
    Path((company_id, asset_id)): Path<(Uuid, Uuid)>,
    Json(request): Json<DisposeRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    krev(&state, person.person_id, company_id, Krav::Bokfor).await?;
    let created_by = person.name.as_deref().unwrap_or(&person.sub);
    let disposal = regnmed_db::dispose_asset(
        &state.pool,
        company_id,
        asset_id,
        request.dato,
        request.vederlag_ore,
        request.motkonto.as_deref().unwrap_or("1920"),
        request.gevinstkonto.as_deref().unwrap_or("3880"),
        request.tapskonto.as_deref().unwrap_or("7880"),
        created_by,
    )
    .await
    .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    Ok(Json(json!({
        "bokfort_ore": disposal.bokfort_ore,
        "gevinst_ore": disposal.gevinst_ore,
        "voucher": disposal.voucher.map(|(year, no)| format!("{year}-{no}")),
    })))
}

pub async fn runs(
    State(state): State<AppState>,
    person: AuthPerson,
    Path((company_id, asset_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<serde_json::Value>, ApiError> {
    krev(&state, person.person_id, company_id, Krav::Les).await?;
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
pub struct SaldoQuery {
    year: i32,
}

pub async fn saldo(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(company_id): Path<Uuid>,
    Query(query): Query<SaldoQuery>,
) -> Result<Json<serde_json::Value>, ApiError> {
    krev(&state, person.person_id, company_id, Krav::Les).await?;
    let rapport = regnmed_db::saldo_rapport(&state.pool, company_id, query.year)
        .await
        .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    Ok(Json(json!({
        "year": rapport.year,
        "grupper": rapport.grupper.iter().map(|g| json!({
            "gruppe": g.gruppe,
            "beskrivelse": g.beskrivelse,
            "inngaende_ore": g.inngaende_ore,
            "tilgang_ore": g.tilgang_ore,
            "vederlag_ore": g.vederlag_ore,
            "grunnlag_ore": g.grunnlag_ore,
            "sats_bp": g.sats_bp,
            "avskrivning_ore": g.avskrivning_ore,
            "utgaende_ore": g.utgaende_ore,
        })).collect::<Vec<_>>(),
        "bokfort_ore": rapport.bokfort_ore,
        "skattemessig_ore": rapport.skattemessig_ore,
        "forskjell_ore": rapport.forskjell_ore,
    })))
}
