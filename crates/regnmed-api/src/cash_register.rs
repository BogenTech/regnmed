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
use crate::access::{Right, require};
use crate::auth::{ApiError, AuthPerson};

#[derive(Deserialize)]
pub struct SalgLinje {
    #[serde(rename = "konto")]
    account: String,
    vat_code: Option<String>,
    #[serde(rename = "brutto_ore")]
    gross_ore: i64,
}

#[derive(Deserialize)]
pub struct BetalingLinje {
    #[serde(rename = "konto")]
    account: String,
    #[serde(rename = "belop_ore")]
    amount_ore: i64,
}

#[derive(Deserialize)]
pub struct DagsoppgjorRequest {
    #[serde(rename = "dato")]
    date: chrono::NaiveDate,
    z_nummer: String,
    salg: Vec<SalgLinje>,
    betaling: Vec<BetalingLinje>,
    #[serde(rename = "mva_konto")]
    vat_account: Option<String>,
    /// The cash account and what was counted in it. Omitted = the till
    /// was not counted, and no difference voucher is posted — we do not
    /// infer that a missing count means it agreed.
    #[serde(rename = "kontantkonto")]
    cash_account: Option<String>,
    opptalt_kontant_ore: Option<i64>,
    /// Where a discrepancy is charged. 7830 Kassadifferanse by default.
    #[serde(rename = "differansekonto")]
    difference_account: Option<String>,
}

fn til_inn(request: DagsoppgjorRequest) -> regnmed_db::cash_register::DagsoppgjorInn {
    regnmed_db::cash_register::DagsoppgjorInn {
        date: request.date,
        z_nummer: request.z_nummer,
        salg: request
            .salg
            .into_iter()
            .map(|l| (l.account, l.vat_code, l.gross_ore))
            .collect(),
        betaling: request
            .betaling
            .into_iter()
            .map(|b| (b.account, b.amount_ore))
            .collect(),
        vat_account: request.vat_account.unwrap_or_else(|| "2700".into()),
        cash_account: request.cash_account,
        opptalt_kontant_ore: request.opptalt_kontant_ore,
        difference_account: request.difference_account.unwrap_or_else(|| "7830".into()),
    }
}

fn response(bokfort: regnmed_db::cash_register::BokfortOppgjor) -> Json<serde_json::Value> {
    Json(json!({
        "bilag": format!("{}-{}", bokfort.voucher.0, bokfort.voucher.1),
        "differanse_ore": bokfort.difference_ore,
        "differansebilag": bokfort.difference.map(|(y, n)| format!("{y}-{n}")),
    }))
}

pub async fn dagsoppgjor(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(company_id): Path<Uuid>,
    Json(request): Json<DagsoppgjorRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require(&state, person.person_id, company_id, Right::VoucherPost).await?;
    let created_by = person.name.as_deref().unwrap_or(&person.sub);
    let bokfort = regnmed_db::cash_register::bokfor_dagsoppgjor(
        &state.pool,
        company_id,
        &til_inn(request),
        None,
        created_by,
    )
    .await
    .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    Ok(response(bokfort))
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
    require(&state, person.person_id, company_id, Right::VoucherPost).await?;
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
    let bokfort = regnmed_db::cash_register::bokfor_dagsoppgjor(
        &state.pool,
        company_id,
        &til_inn(request),
        Some((&query.filename, &content_type, &body)),
        created_by,
    )
    .await
    .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    Ok(response(bokfort))
}
