//! Årsavslutning over the web (#84, docs/arsavslutning.md).
//!
//! Closing a year posts a voucher AND locks the year, so it needs both
//! rights: `BILAG_BOKFOR` to post and `PERIODE_LAAS` to lock. Requiring
//! both is not belt-and-braces — the lock is half the act, and someone
//! who may not lock a period should not close a year either.

use axum::Json;
use axum::extract::{Path, State};
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

use crate::AppState;
use crate::auth::{ApiError, AuthPerson};
use crate::tilgang::{Rett, krev};

#[derive(Deserialize)]
pub struct AvsluttRequest {
    ar: i32,
    /// Skattekostnaden for året, i øre. Kalleren regner den ut —
    /// skattemessig resultat er ikke regnskapsmessig resultat, og å
    /// utlede den her ville vært å dikte opp en skattemelding. 0 er et
    /// gyldig svar som må oppgis.
    skattekostnad_ore: i64,
}

pub async fn list(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(company_id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ApiError> {
    krev(&state, person.person_id, company_id, Rett::RapportLes).await?;
    let rader = regnmed_db::arsavslutning::list_arsavslutninger(&state.pool, company_id).await?;
    Ok(Json(json!({
        "arsavslutninger": rader.iter().map(|a| json!({
            "ar": a.ar,
            "bilag": format!("{}-{}", a.voucher.0, a.voucher.1),
            "resultat_for_skatt_ore": a.resultat_for_skatt_ore,
            "skattekostnad_ore": a.skattekostnad_ore,
            "disponert_ore": a.disponert_ore,
            "created_by": a.created_by,
        })).collect::<Vec<_>>(),
    })))
}

/// The year's result as the ledger has it — what the closing WOULD
/// disponere. Offered so the form can show the number before anyone
/// commits to it.
pub async fn forslag(
    State(state): State<AppState>,
    person: AuthPerson,
    Path((company_id, ar)): Path<(Uuid, i32)>,
) -> Result<Json<serde_json::Value>, ApiError> {
    krev(&state, person.person_id, company_id, Rett::RapportLes).await?;
    let resultat = regnmed_db::arsavslutning::resultat_for_aret(&state.pool, company_id, ar)
        .await
        .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    Ok(Json(json!({
        "ar": ar,
        "resultat_for_skatt_ore": resultat,
    })))
}

pub async fn avslutt(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(company_id): Path<Uuid>,
    Json(request): Json<AvsluttRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    krev(&state, person.person_id, company_id, Rett::BilagBokfor).await?;
    krev(&state, person.person_id, company_id, Rett::PeriodeLaas).await?;
    let a = regnmed_db::arsavslutning::avslutt_ar(
        &state.pool,
        company_id,
        request.ar,
        request.skattekostnad_ore,
        person.name.as_deref().unwrap_or(&person.sub),
    )
    .await
    .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    Ok(Json(json!({
        "ar": a.ar,
        "bilag": format!("{}-{}", a.voucher.0, a.voucher.1),
        "resultat_for_skatt_ore": a.resultat_for_skatt_ore,
        "skattekostnad_ore": a.skattekostnad_ore,
        "disponert_ore": a.disponert_ore,
    })))
}
