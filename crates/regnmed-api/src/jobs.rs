//! Background jobs over HTTP (docs/jobs.md).
//!
//! - POST /companies/{id}/jobs/saft-import   same body as /import/saft
//! - POST /companies/{id}/jobs/audit-report  no body
//! - GET  /companies/{id}/jobs/{job_id}      status, then result or error
//!
//! A POST answers 202 with the job id at once; the work runs in
//! `regnmed work`. The synchronous endpoints stay for API clients that
//! already call them -- these are what the portal uses.
//!
//! The rights are the ones the synchronous endpoints demand, checked
//! both when a job is created and when it is read: a job's result is as
//! sensitive as the thing it computed.

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use regnmed_db::job::{self, Kind};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::AppState;
use crate::access::{Right, require};
use crate::auth::{ApiError, AuthPerson};

fn right_for(kind: Kind) -> Right {
    match kind {
        Kind::SaftImport => Right::MigrationAdmin,
        Kind::AuditReport => Right::ReportRead,
    }
}

pub async fn create_saft_import(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(company_id): Path<Uuid>,
    headers: axum::http::HeaderMap,
    body: String,
) -> Result<(StatusCode, Json<Value>), ApiError> {
    require(
        &state,
        person.person_id,
        company_id,
        right_for(Kind::SaftImport),
    )
    .await?;
    // Parsing is fast (well under a second for years of history), so a
    // file that is not SAF-T at all is refused NOW, not as a failed job.
    let is_json = crate::marketplace::is_json(&headers);
    crate::marketplace::parse_import_body(&headers, body.clone())?;
    let content_type = if is_json {
        "application/json"
    } else {
        "application/xml"
    };
    let job_id = job::create(
        &state.pool,
        company_id,
        Kind::SaftImport,
        Some((body.as_bytes(), content_type)),
        person.name.as_deref().unwrap_or(&person.sub),
        Some(person.person_id),
    )
    .await?;
    enqueue(&state, job_id).await;
    Ok((
        StatusCode::ACCEPTED,
        Json(json!({ "job_id": job_id, "status": "queued" })),
    ))
}

pub async fn create_audit_report(
    State(state): State<AppState>,
    person: AuthPerson,
    Path(company_id): Path<Uuid>,
) -> Result<(StatusCode, Json<Value>), ApiError> {
    require(
        &state,
        person.person_id,
        company_id,
        right_for(Kind::AuditReport),
    )
    .await?;
    let job_id = job::create(
        &state.pool,
        company_id,
        Kind::AuditReport,
        None,
        person.name.as_deref().unwrap_or(&person.sub),
        Some(person.person_id),
    )
    .await?;
    enqueue(&state, job_id).await;
    Ok((
        StatusCode::ACCEPTED,
        Json(json!({ "job_id": job_id, "status": "queued" })),
    ))
}

pub async fn get(
    State(state): State<AppState>,
    person: AuthPerson,
    Path((company_id, job_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<Value>, ApiError> {
    // An unknown id is 404, and so is any job to someone without access to
    // the company (`require` answers 404 there), so an outsider cannot tell
    // the two apart. A member lacking the kind's right gets 403, as on the
    // synchronous endpoint.
    let job = job::get(&state.pool, company_id, job_id)
        .await?
        .ok_or(ApiError::NotFound)?;
    let kind = Kind::parse(&job.kind).ok_or(ApiError::NotFound)?;
    require(&state, person.person_id, company_id, right_for(kind)).await?;
    Ok(Json(json!({
        "job_id": job.id,
        "kind": job.kind,
        "status": job.status,
        "attempts": job.attempts,
        "created_by": job.created_by,
        "created_at": job.created_at.to_rfc3339(),
        "started_at": job.started_at.map(|t| t.to_rfc3339()),
        "finished_at": job.finished_at.map(|t| t.to_rfc3339()),
        "result": job.result,
        "error": job.error,
    })))
}

/// Rings the worker's doorbell. Without a queue (local development and
/// the integration tests) the API runs the job itself, in the background
/// -- the same code the worker runs. In a cluster the queue is always
/// configured, and a failed nudge only costs latency: every worker sweeps.
async fn enqueue(state: &AppState, job_id: Uuid) {
    if let Some(js) = state.mailq.as_ref() {
        if let Err(e) = regnmed_mail::jobqueue::nudge(js, job_id).await {
            eprintln!("the job queue rejected the nudge for {job_id}: {e:#}");
        }
        return;
    }
    let pool = state.pool.clone();
    tokio::spawn(async move {
        let worker = "regnmed-api";
        match job::claim(&pool, job_id, worker).await {
            Ok(Some(claimed)) => {
                if let Err(e) = job::process(&pool, &claimed, worker).await {
                    eprintln!("job {job_id}: could not record the outcome: {e:#}");
                }
            }
            Ok(None) => {}
            Err(e) => eprintln!("job {job_id}: could not claim: {e:#}"),
        }
    });
}
