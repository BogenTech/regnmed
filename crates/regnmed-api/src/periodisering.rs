//! Periodisering over the web (#87, docs/periodisering.md).
//!
//! Creating and stopping a plan is bookkeeping work (`BILAG_BOKFOR`),
//! reading it is `BILAG_LES` — the plan decides what gets posted every
//! month, so it belongs on the same side of the boundary as the posting
//! itself. Running the månedskjøring by hand is offered too, for the
//! same reason `assets/depreciate` is: waiting for a CronJob to see
//! whether the plan was right is a poor way to work.

use axum::Json;
use axum::extract::{Path, State};
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

use crate::AppState;
use crate::auth::{ApiError, AuthPerson};
use crate::tilgang::{Rett, krev};

#[derive(Deserialize)]
pub struct PeriodiseringRequest {
    kilde_voucher: Option<Uuid>,
    beskrivelse: String,
    resultatkonto: String,
    balansekonto: String,
    /// Nettobeløp i øre, hovedbokens fortegn (forskuddsbetalt kostnad
    /// positiv). Aldri inkludert mva — avgiften hører kildebilaget til.
    total_ore: i64,
    fra_ar: i32,
    fra_maned: u32,
    til_ar: i32,
    til_maned: u32,
    avdeling: Option<String>,
    prosjekt: Option<String>,
    notat: Option<String>,
}

pub async fn list(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(company_id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ApiError> {
    krev(&state, person.person_id, company_id, Rett::BilagLes).await?;
    let planer = regnmed_db::periodisering::list_periodiseringer(&state.pool, company_id).await?;
    Ok(Json(json!({
        "periodiseringer": planer.iter().map(|p| json!({
            "id": p.id,
            "beskrivelse": p.beskrivelse,
            "resultatkonto": p.resultatkonto,
            "balansekonto": p.balansekonto,
            "total_ore": p.total_ore,
            "fra_maned": p.fra_maned.to_string(),
            "til_maned": p.til_maned.to_string(),
            "avdeling": p.avdeling,
            "prosjekt": p.prosjekt,
            "notat": p.notat,
            "stoppet_dato": p.stoppet_dato.map(|d| d.to_string()),
            // Computed, never stored: what is left is the difference.
            "fort_ore": p.fort_ore,
            "forte_maneder": p.forte_maneder,
            "gjenstar_ore": p.total_ore - p.fort_ore,
        })).collect::<Vec<_>>(),
    })))
}

pub async fn create(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(company_id): Path<Uuid>,
    Json(request): Json<PeriodiseringRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    krev(&state, person.person_id, company_id, Rett::BilagBokfor).await?;
    let id = regnmed_db::periodisering::create_periodisering(
        &state.pool,
        company_id,
        &regnmed_db::periodisering::PeriodiseringDraft {
            kilde_voucher: request.kilde_voucher,
            beskrivelse: request.beskrivelse,
            resultatkonto: request.resultatkonto,
            balansekonto: request.balansekonto,
            total_ore: request.total_ore,
            fra: (request.fra_ar, request.fra_maned),
            til: (request.til_ar, request.til_maned),
            avdeling: request.avdeling,
            prosjekt: request.prosjekt,
            notat: request.notat,
        },
        person.name.as_deref().unwrap_or(&person.sub),
    )
    .await
    .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    Ok(Json(json!({ "id": id })))
}

pub async fn stopp(
    State(state): State<AppState>,
    person: AuthPerson,
    Path((company_id, id)): Path<(Uuid, Uuid)>,
) -> Result<Json<serde_json::Value>, ApiError> {
    krev(&state, person.person_id, company_id, Rett::BilagBokfor).await?;
    let idag: chrono::NaiveDate = sqlx::query_scalar("select current_date")
        .fetch_one(&state.pool)
        .await
        .map_err(anyhow::Error::from)?;
    regnmed_db::periodisering::stopp_periodisering(&state.pool, company_id, id, idag)
        .await
        .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    Ok(Json(json!({ "stoppet": idag.to_string() })))
}

/// Runs every due month for one plan now, rather than waiting for the
/// CronJob. Idempotent by the partial unique index, like the CronJob.
pub async fn kjor(
    State(state): State<AppState>,
    person: AuthPerson,
    Path((company_id, id)): Path<(Uuid, Uuid)>,
) -> Result<Json<serde_json::Value>, ApiError> {
    krev(&state, person.person_id, company_id, Rett::BilagBokfor).await?;
    let idag: chrono::NaiveDate = sqlx::query_scalar("select current_date")
        .fetch_one(&state.pool)
        .await
        .map_err(anyhow::Error::from)?;
    let utfall = regnmed_db::periodisering::periodiser_plan(&state.pool, company_id, id, idag)
        .await
        .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    Ok(Json(json!({
        "kjort": utfall.iter().map(|u| json!({
            "period": u.period.to_string(),
            "belop_ore": u.belop_ore,
            "bilag": u.voucher.map(|(y, n)| format!("{y}-{n}")),
            "feil": u.detail,
        })).collect::<Vec<_>>(),
    })))
}

pub async fn runs(
    State(state): State<AppState>,
    person: AuthPerson,
    Path((company_id, id)): Path<(Uuid, Uuid)>,
) -> Result<Json<serde_json::Value>, ApiError> {
    krev(&state, person.person_id, company_id, Rett::BilagLes).await?;
    let rader = regnmed_db::periodisering::list_runs(&state.pool, company_id, id)
        .await
        .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    Ok(Json(json!({
        "kjoringer": rader.iter().map(|(period, belop, detail)| json!({
            "period": period.to_string(),
            "belop_ore": belop,
            "feil": detail,
        })).collect::<Vec<_>>(),
    })))
}
