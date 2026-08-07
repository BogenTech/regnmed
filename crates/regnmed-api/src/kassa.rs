//! Kassaoppgjør over the web (#89, docs/kontantsalg.md).
//!
//! `BILAG_BOKFOR`: a day's settlement is a posting, and the person who
//! may not post may not settle a till either.

use axum::Json;
use axum::body::Bytes;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, header};
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

use crate::AppState;
use crate::auth::{ApiError, AuthPerson};
use crate::tilgang::{Rett, krev};

#[derive(Deserialize)]
pub struct SalgLinje {
    konto: String,
    vat_code: Option<String>,
    brutto_ore: i64,
}

#[derive(Deserialize)]
pub struct BetalingLinje {
    konto: String,
    belop_ore: i64,
}

#[derive(Deserialize)]
pub struct DagsoppgjorRequest {
    dato: chrono::NaiveDate,
    z_nummer: String,
    salg: Vec<SalgLinje>,
    betaling: Vec<BetalingLinje>,
    mva_konto: Option<String>,
    /// The cash account and what was counted in it. Omitted = the till
    /// was not counted, and no difference voucher is posted — we do not
    /// infer that a missing count means it agreed.
    kontantkonto: Option<String>,
    opptalt_kontant_ore: Option<i64>,
    /// Where a discrepancy is charged. 7830 Kassadifferanse by default.
    differansekonto: Option<String>,
}

fn til_inn(request: DagsoppgjorRequest) -> regnmed_db::kassa::DagsoppgjorInn {
    regnmed_db::kassa::DagsoppgjorInn {
        dato: request.dato,
        z_nummer: request.z_nummer,
        salg: request
            .salg
            .into_iter()
            .map(|l| (l.konto, l.vat_code, l.brutto_ore))
            .collect(),
        betaling: request
            .betaling
            .into_iter()
            .map(|b| (b.konto, b.belop_ore))
            .collect(),
        mva_konto: request.mva_konto.unwrap_or_else(|| "2700".into()),
        kontantkonto: request.kontantkonto,
        opptalt_kontant_ore: request.opptalt_kontant_ore,
        differansekonto: request.differansekonto.unwrap_or_else(|| "7830".into()),
    }
}

fn svar(bokfort: regnmed_db::kassa::BokfortOppgjor) -> Json<serde_json::Value> {
    Json(json!({
        "bilag": format!("{}-{}", bokfort.voucher.0, bokfort.voucher.1),
        "differanse_ore": bokfort.differanse_ore,
        "differansebilag": bokfort.differanse.map(|(y, n)| format!("{y}-{n}")),
    }))
}

pub async fn dagsoppgjor(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(company_id): Path<Uuid>,
    Json(request): Json<DagsoppgjorRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    krev(&state, person.person_id, company_id, Rett::BilagBokfor).await?;
    let created_by = person.name.as_deref().unwrap_or(&person.sub);
    let bokfort = regnmed_db::kassa::bokfor_dagsoppgjor(
        &state.pool,
        company_id,
        &til_inn(request),
        None,
        created_by,
    )
    .await
    .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    Ok(svar(bokfort))
}

#[derive(Deserialize)]
pub struct ZRapportQuery {
    filename: String,
}

/// The settlement WITH its Z-report in one call. §5-4 wants the report
/// kept as the documentation, and uploading it separately would leave a
/// window where the bilag exists without it.
pub async fn dagsoppgjor_med_rapport(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(company_id): Path<Uuid>,
    Query(query): Query<ZRapportQuery>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Json<serde_json::Value>, ApiError> {
    krev(&state, person.person_id, company_id, Rett::BilagBokfor).await?;
    // The settlement itself travels as a JSON header field so the body
    // can stay the raw report — same shape as the other upload routes.
    let meta = headers
        .get("x-dagsoppgjor")
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| ApiError::BadRequest("mangler X-Dagsoppgjor-hodet".into()))?;
    let request: DagsoppgjorRequest = serde_json::from_str(meta)
        .map_err(|e| ApiError::BadRequest(format!("ugyldig dagsoppgjør: {e}")))?;
    let content_type = headers
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("application/octet-stream")
        .to_string();
    let created_by = person.name.as_deref().unwrap_or(&person.sub);
    let bokfort = regnmed_db::kassa::bokfor_dagsoppgjor(
        &state.pool,
        company_id,
        &til_inn(request),
        Some((&query.filename, &content_type, &body)),
        created_by,
    )
    .await
    .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    Ok(svar(bokfort))
}
