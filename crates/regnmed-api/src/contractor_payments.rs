//! Payments to self-employed people without a fixed place of business
//! (third-party report, formerly RF-1321; docs/selvstendig.md, #363).
//!
//! - GET  /companies/{id}/parties/{pid}/self-employed            the judgement and its history (RESKONTRO_LES)
//! - POST /companies/{id}/parties/{pid}/self-employed            record a judgement (RESKONTRO_SKRIV)
//! - GET  /companies/{id}/reports/selvstendig?year=              the report (RESKONTRO_LES); `format=fil` the file
//! - GET  /companies/{id}/reports/selvstendig/submissions?year=  deliveries (RESKONTRO_LES)
//! - POST /companies/{id}/reports/selvstendig/submissions?year=  file with Skatteetaten (RESKONTRO_SKRIV)
//! - POST …/submissions/{sid}/outcome                            fetch Skatteetaten's answer (RESKONTRO_LES)
//!
//! The file carries fødselsnumre, like the RF-1086 file behind the share
//! register's read right; the preview never does (birth date only).

use axum::Json;
use axum::extract::{Path, Query, State};
use regnmed_db::contractor_payments as db;
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

use crate::AppState;
use crate::access::{Right, require};
use crate::auth::{ApiError, AuthPerson};

fn bad(e: anyhow::Error) -> ApiError {
    ApiError::BadRequest(e.to_string())
}

fn birth_date(nin: &Option<String>) -> serde_json::Value {
    json!(
        nin.as_deref()
            .and_then(regnmed_core::nin::birth_date)
            .map(|d| d.to_string())
    )
}

fn line_json(l: &db::Line) -> serde_json::Value {
    json!({
        "party_id": l.party_id,
        "nummer": l.party_no,
        "navn": l.name,
        "orgnr": l.orgnumber,
        "fodselsdato": birth_date(&l.nin),
        "fakturert_ore": l.invoiced_ore,
        "rapporteres_kr": regnmed_core::contractor_payments::kroner(l.invoiced_ore),
    })
}

pub async fn assessment(
    State(state): State<AppState>,
    person: AuthPerson,
    Path((company_id, party_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require(&state, person.person_id, company_id, Right::SubledgerRead).await?;
    let history = db::assessments(&state.pool, company_id, party_id)
        .await
        .map_err(bad)?;
    let json = |a: &db::Assessment| {
        json!({
            "selvstendig": a.self_employed,
            "fodselsdato": birth_date(&a.nin),
            "har_fodselsnummer": a.nin.is_some(),
            "vurdert_av": a.decided_by,
            "vurdert": a.created_at,
        })
    };
    Ok(Json(json!({
        "gjeldende": history.first().map(json),
        "historikk": history.iter().map(json).collect::<Vec<_>>(),
    })))
}

#[derive(Deserialize)]
pub struct AssessRequest {
    #[serde(rename = "selvstendig")]
    self_employed: bool,
    #[serde(default, rename = "fodselsnummer")]
    nin: Option<String>,
}

pub async fn assess(
    State(state): State<AppState>,
    person: AuthPerson,
    Path((company_id, party_id)): Path<(Uuid, Uuid)>,
    Json(r): Json<AssessRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require(&state, person.person_id, company_id, Right::SubledgerWrite).await?;
    db::assess(
        &state.pool,
        company_id,
        party_id,
        r.self_employed,
        r.nin.as_deref(),
        person.display(),
    )
    .await
    .map_err(bad)?;
    assessment(State(state), person, Path((company_id, party_id))).await
}

#[derive(Deserialize)]
pub struct YearQuery {
    year: i32,
    /// `fil`: the JSON to file, as `{filnavn, dokument}`.
    format: Option<String>,
}

pub async fn report(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(company_id): Path<Uuid>,
    Query(q): Query<YearQuery>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require(&state, person.person_id, company_id, Right::SubledgerRead).await?;
    if q.format.as_deref() == Some("fil") {
        // A fresh reference per download: each file filed by hand is its
        // own delivery.
        let doc = db::document(&state.pool, company_id, q.year, &Uuid::now_v7().to_string())
            .await
            .map_err(bad)?;
        return Ok(Json(json!({
            "filnavn": format!("betalinger-selvstendig-{}.json", q.year),
            "dokument": doc,
        })));
    }
    let r = db::report(&state.pool, company_id, q.year)
        .await
        .map_err(bad)?;
    Ok(Json(json!({
        "inntektsar": r.income_year,
        "mottakere": r.recipients.iter().map(line_json).collect::<Vec<_>>(),
        "sum_kr": r.recipients.iter()
            .map(|l| regnmed_core::contractor_payments::kroner(l.invoiced_ore))
            .sum::<i64>(),
        "trekkes_tilbake": r.withdrawn.iter().map(|w| &w.name).collect::<Vec<_>>(),
        "ikke_vurdert": r.unassessed.iter().map(line_json).collect::<Vec<_>>(),
        "hindringer": r.hindrances,
        "leverbar": r.hindrances.is_empty(),
    })))
}

fn rail(state: &AppState) -> Result<std::sync::Arc<db::Rail>, ApiError> {
    state.contractor_payments.clone().ok_or_else(|| {
        ApiError::BadRequest(
            "innsending av oppgaven over betalinger til selvstendig næringsdrivende er ikke \
             satt opp på denne serveren (Maskinporten)"
                .into(),
        )
    })
}

pub async fn list_submissions(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(company_id): Path<Uuid>,
    Query(q): Query<YearQuery>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require(&state, person.person_id, company_id, Right::SubledgerRead).await?;
    let rows = db::submissions(&state.pool, company_id, q.year)
        .await
        .map_err(bad)?;
    Ok(Json(json!({
        "innsending_mulig": state.contractor_payments.is_some(),
        "innsendinger": rows,
    })))
}

pub async fn submit(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(company_id): Path<Uuid>,
    Query(q): Query<YearQuery>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require(&state, person.person_id, company_id, Right::SubledgerWrite).await?;
    let rail = rail(&state)?;
    crate::terms::require_accepted(&state, company_id).await?;
    let s = db::submit(&state.pool, company_id, &rail, q.year, person.display())
        .await
        .map_err(bad)?;
    Ok(Json(json!(s)))
}

pub async fn outcome(
    State(state): State<AppState>,
    person: AuthPerson,
    Path((company_id, submission_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require(&state, person.person_id, company_id, Right::SubledgerRead).await?;
    let rail = rail(&state)?;
    db::fetch_outcome(
        &state.pool,
        company_id,
        &rail,
        submission_id,
        person.display(),
    )
    .await
    .map_err(|e| ApiError::BadRequest(format!("kunne ikke hente svaret: {e:#}")))?;
    Ok(Json(json!({ "ok": true })))
}
