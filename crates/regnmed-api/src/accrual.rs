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
use crate::access::{Right, require};
use crate::auth::{ApiError, AuthPerson};

#[derive(Deserialize)]
pub struct AccrualRequest {
    kilde_voucher: Option<Uuid>,
    #[serde(rename = "beskrivelse")]
    description: String,
    #[serde(rename = "resultatkonto")]
    income_account: String,
    #[serde(rename = "balansekonto")]
    balance_account: String,
    /// Nettobeløp i øre, hovedbokens fortegn (forskuddsbetalt kostnad
    /// positiv). Aldri inkludert mva — avgiften hører kildebilaget til.
    total_ore: i64,
    fra_ar: i32,
    fra_maned: u32,
    til_ar: i32,
    til_maned: u32,
    #[serde(rename = "avdeling")]
    department: Option<String>,
    #[serde(rename = "prosjekt")]
    project: Option<String>,
    #[serde(rename = "notat")]
    note: Option<String>,
}

pub async fn list(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(company_id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require(&state, person.person_id, company_id, Right::VoucherRead).await?;
    let planer = regnmed_db::accrual::list_periodiseringer(&state.pool, company_id).await?;
    Ok(Json(json!({
        "periodiseringer": planer.iter().map(|p| json!({
            "id": p.id,
            "beskrivelse": p.description,
            "resultatkonto": p.income_account,
            "balansekonto": p.balance_account,
            "total_ore": p.total_ore,
            "fra_maned": p.fra_maned.to_string(),
            "til_maned": p.til_maned.to_string(),
            "avdeling": p.department,
            "prosjekt": p.project,
            "notat": p.note,
            "stoppet_dato": p.stopped_date.map(|d| d.to_string()),
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
    Json(request): Json<AccrualRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require(&state, person.person_id, company_id, Right::VoucherPost).await?;
    let id = regnmed_db::accrual::create_accrual(
        &state.pool,
        company_id,
        &regnmed_db::accrual::AccrualDraft {
            kilde_voucher: request.kilde_voucher,
            description: request.description,
            income_account: request.income_account,
            balance_account: request.balance_account,
            total_ore: request.total_ore,
            from: (request.fra_ar, request.fra_maned),
            to: (request.til_ar, request.til_maned),
            department: request.department,
            project: request.project,
            note: request.note,
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
    require(&state, person.person_id, company_id, Right::VoucherPost).await?;
    let today: chrono::NaiveDate = sqlx::query_scalar("select current_date")
        .fetch_one(&state.pool)
        .await
        .map_err(anyhow::Error::from)?;
    regnmed_db::accrual::stop_accrual(&state.pool, company_id, id, today)
        .await
        .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    Ok(Json(json!({ "stoppet": today.to_string() })))
}

/// Runs every due month for one plan now, rather than waiting for the
/// CronJob. Idempotent by the partial unique index, like the CronJob.
pub async fn kjor(
    State(state): State<AppState>,
    person: AuthPerson,
    Path((company_id, id)): Path<(Uuid, Uuid)>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require(&state, person.person_id, company_id, Right::VoucherPost).await?;
    let today: chrono::NaiveDate = sqlx::query_scalar("select current_date")
        .fetch_one(&state.pool)
        .await
        .map_err(anyhow::Error::from)?;
    let outcome = regnmed_db::accrual::periodiser_plan(&state.pool, company_id, id, today)
        .await
        .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    Ok(Json(json!({
        "kjort": outcome.iter().map(|u| json!({
            "period": u.period.to_string(),
            "belop_ore": u.amount_ore,
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
    require(&state, person.person_id, company_id, Right::VoucherRead).await?;
    let rader = regnmed_db::accrual::list_runs(&state.pool, company_id, id)
        .await
        .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    Ok(Json(json!({
        "kjoringer": rader.iter().map(|(period, amount, detail)| json!({
            "period": period.to_string(),
            "belop_ore": amount,
            "feil": detail,
        })).collect::<Vec<_>>(),
    })))
}
