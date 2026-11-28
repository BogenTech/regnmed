//! Schedules (docs/jobs.md, «Tidsplaner»): when a kind of background job
//! runs by itself for a company. The scheduler is general; which kinds can
//! be scheduled and how often is `regnmed_db::schedule::rules`, and which
//! right it takes to change one is decided here, per kind.

use axum::extract::{Path, State};
use axum::{Extension, Json};
use regnmed_db::job::Kind;
use regnmed_db::schedule::{self, ScheduleRow};
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

use crate::AppState;
use crate::access::{Right, require};
use crate::auth::{ApiError, AuthPerson};
use crate::platform::PlatformContext;

/// The right it takes to change a kind's schedule: the same one it takes to
/// do by hand what the schedule does.
fn right_to_change(kind: Kind) -> Option<Right> {
    match kind {
        Kind::BankSync => Some(Right::BankReconcile),
        Kind::GarnishmentFetch => Some(Right::PayrollWrite),
        Kind::SaftImport
        | Kind::AuditReport
        | Kind::PeppolLookup
        | Kind::InvoiceDispatch
        | Kind::PeppolSenderSetup
        | Kind::VatFilingSubmit => None,
    }
}

fn times(row: &ScheduleRow) -> Vec<String> {
    row.times
        .iter()
        .map(|t| t.format("%H:%M").to_string())
        .collect()
}

fn name(kind: &str) -> &'static str {
    Kind::parse(kind)
        .and_then(schedule::rules)
        .map_or("Planlagt jobb", |r| r.name)
}

fn company_json(row: &ScheduleRow) -> serde_json::Value {
    json!({
        "kind": row.kind,
        "navn": name(&row.kind),
        "aktiv": row.active,
        "tider": times(row),
        "maks_per_dag": Kind::parse(&row.kind).and_then(schedule::rules).map(|r| r.max_per_day),
        // A paused schedule has no next run, whatever the row holds.
        "neste": row.active.then(|| row.next_run_at.to_rfc3339()),
        "sist": row.last.as_ref().map(|(status, finished, result, error)| json!({
            "status": status,
            "ferdig": finished.map(|t| t.to_rfc3339()),
            "resultat": result,
            "feil": error,
        })),
    })
}

/// The company's schedules. Anyone who can see the company can see when
/// things run for it; changing one takes the kind's own right.
pub async fn list(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(company_id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require(&state, person.person_id, company_id, Right::CompanyRead).await?;
    let rows = schedule::for_company(&state.pool, company_id).await?;
    Ok(Json(json!({
        "tidsplaner": rows.iter().map(company_json).collect::<Vec<_>>(),
    })))
}

#[derive(Deserialize)]
pub struct SetRequest {
    tider: Vec<String>,
    aktiv: bool,
}

pub async fn set(
    State(state): State<AppState>,
    person: AuthPerson,
    Path((company_id, kind)): Path<(Uuid, String)>,
    Json(request): Json<SetRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    // An unknown kind and one that cannot be scheduled look the same to a
    // caller without access: the right is resolved first where there is one.
    let kind = Kind::parse(&kind);
    let right = kind
        .and_then(right_to_change)
        .unwrap_or(Right::CompanyAdmin);
    require(&state, person.person_id, company_id, right).await?;
    let Some(kind) = kind.filter(|k| schedule::rules(*k).is_some()) else {
        return Err(ApiError::BadRequest(
            "denne typen jobb kan ikke tidsplanlegges".into(),
        ));
    };
    let parsed = schedule::validate(kind, &request.tider).map_err(ApiError::BadRequest)?;
    if !schedule::set(
        &state.pool,
        company_id,
        kind,
        &parsed,
        request.aktiv,
        person.display(),
    )
    .await?
    {
        return Err(ApiError::NotFound);
    }
    let rows = schedule::for_company(&state.pool, company_id).await?;
    let row = rows
        .iter()
        .find(|r| r.kind == kind.as_str())
        .ok_or(ApiError::NotFound)?;
    Ok(Json(company_json(row)))
}

/// Every schedule on the platform, for the console: what runs when, and how
/// the last run went. No ledger data; a job's result stays with its company.
pub async fn platform_list(
    State(state): State<AppState>,
    Extension(_ctx): Extension<PlatformContext>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let rows = schedule::all(&state.pool).await?;
    Ok(Json(json!({
        "tidsplaner": rows.iter().map(|r| json!({
            "company_id": r.company_id,
            "selskap": r.company_name,
            "kind": r.kind,
            "navn": name(&r.kind),
            "aktiv": r.active,
            "tider": times(r),
            "neste": r.active.then(|| r.next_run_at.to_rfc3339()),
            "sist_status": r.last.as_ref().map(|l| l.0.clone()),
            "sist_ferdig": r.last.as_ref().and_then(|l| l.1).map(|t| t.to_rfc3339()),
        })).collect::<Vec<_>>(),
    })))
}
