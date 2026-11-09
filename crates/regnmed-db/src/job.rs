//! Background jobs (docs/jobs.md, migration 0071).
//!
//! The `job` row is the record and the claim; the queue that wakes a
//! worker is a doorbell (`regnmed_mail::jobqueue`). Everything a job does
//! lives here, so the worker (`regnmed work`) and the API's in-process
//! fallback run exactly the same code.
//!
//! Scaling out is the point: any number of workers may run at once. Two
//! things make that safe. A worker claims a job with ONE atomic update
//! before touching it, so whatever delivers the job twice -- a redelivered
//! message, the sweep, a second replica -- only one of them wins. And a
//! worker heartbeats while it runs: one that dies goes quiet, and after
//! `STALE_SECS` the job can be claimed again, at most `MAX_ATTEMPTS`
//! times in all.

use anyhow::{Context, Result, bail};
use serde_json::Value;
use sqlx::{PgPool, Row};
use uuid::Uuid;

use crate::blob::blobs;

/// A heartbeat older than this means the worker is gone.
pub const STALE_SECS: i64 = 60;
pub const HEARTBEAT_SECS: u64 = 15;
/// A job that has killed its worker this many times needs a human.
pub const MAX_ATTEMPTS: i32 = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    SaftImport,
    AuditReport,
}

impl Kind {
    pub fn as_str(self) -> &'static str {
        match self {
            Kind::SaftImport => "saft_import",
            Kind::AuditReport => "audit_report",
        }
    }
    pub fn parse(s: &str) -> Option<Kind> {
        match s {
            "saft_import" => Some(Kind::SaftImport),
            "audit_report" => Some(Kind::AuditReport),
            _ => None,
        }
    }
}

/// A job as its owner sees it -- never the input.
#[derive(Debug, Clone)]
pub struct JobStatus {
    pub id: Uuid,
    pub kind: String,
    pub status: String,
    pub attempts: i32,
    pub created_by: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub started_at: Option<chrono::DateTime<chrono::Utc>>,
    pub finished_at: Option<chrono::DateTime<chrono::Utc>>,
    pub result: Option<Value>,
    pub error: Option<String>,
}

/// A claimed job, as the worker gets it.
#[derive(Debug, Clone)]
pub struct Claimed {
    pub id: Uuid,
    pub company_id: Uuid,
    pub kind: String,
    pub created_by: String,
}

/// Records a new job. The input goes through the blob store, exactly
/// like a document: with an object store configured only its hash stays
/// in the row.
pub async fn create(
    pool: &PgPool,
    company_id: Uuid,
    kind: Kind,
    input: Option<(&[u8], &str)>,
    created_by: &str,
    created_by_person: Option<Uuid>,
) -> Result<Uuid> {
    let id = Uuid::now_v7();
    let (sha, bytes, content_type) = match input {
        Some((bytes, content_type)) => (
            Some(regnmed_core::hash::sha256(bytes).to_vec()),
            blobs().store(bytes).await?.map(<[u8]>::to_vec),
            Some(content_type),
        ),
        None => (None, None, None),
    };
    sqlx::query(
        "insert into job (id, company_id, kind, input_sha256, input, input_content_type,
                          created_by, created_by_person)
         values ($1, $2, $3, $4, $5, $6, $7, $8)",
    )
    .bind(id)
    .bind(company_id)
    .bind(kind.as_str())
    .bind(sha)
    .bind(bytes)
    .bind(content_type)
    .bind(created_by)
    .bind(created_by_person)
    .execute(pool)
    .await?;
    Ok(id)
}

/// One company's job. Another company's id answers `None`, like every
/// company-scoped lookup.
pub async fn get(pool: &PgPool, company_id: Uuid, job_id: Uuid) -> Result<Option<JobStatus>> {
    let row = sqlx::query(
        "select id, kind, status, attempts, created_by, created_at, started_at, finished_at,
                result::text as result, error
         from job where id = $1 and company_id = $2",
    )
    .bind(job_id)
    .bind(company_id)
    .fetch_optional(pool)
    .await?;
    row.map(|r| {
        Ok(JobStatus {
            id: r.get("id"),
            kind: r.get("kind"),
            status: r.get("status"),
            attempts: r.get("attempts"),
            created_by: r.get("created_by"),
            created_at: r.get("created_at"),
            started_at: r.get("started_at"),
            finished_at: r.get("finished_at"),
            result: r
                .get::<Option<String>, _>("result")
                .map(|t| serde_json::from_str(&t))
                .transpose()?,
            error: r.get("error"),
        })
    })
    .transpose()
}

const CLAIMABLE: &str = "attempts < $3
     and (status = 'queued'
          or (status = 'running' and heartbeat_at < now() - make_interval(secs => $4)))";

/// Claims one specific job (the queue's path). `None`: somebody else has
/// it, it is finished, or it has used up its attempts.
pub async fn claim(pool: &PgPool, job_id: Uuid, worker: &str) -> Result<Option<Claimed>> {
    let row = sqlx::query(&format!(
        "update job set status = 'running', claimed_by = $2, attempts = attempts + 1,
                        started_at = coalesce(started_at, now()), heartbeat_at = now()
         where id = $1 and {CLAIMABLE}
         returning id, company_id, kind, created_by"
    ))
    .bind(job_id)
    .bind(worker)
    .bind(MAX_ATTEMPTS)
    .bind(STALE_SECS as f64)
    .fetch_optional(pool)
    .await?;
    Ok(row.map(claimed))
}

/// Claims the oldest claimable job (the sweep's path). `skip locked`
/// lets any number of sweeping workers run this at once without queueing
/// behind each other or taking the same job.
pub async fn claim_next(pool: &PgPool, worker: &str) -> Result<Option<Claimed>> {
    let row = sqlx::query(&format!(
        "update job set status = 'running', claimed_by = $2, attempts = attempts + 1,
                        started_at = coalesce(started_at, now()), heartbeat_at = now()
         where id = (select id from job where {CLAIMABLE}
                     order by created_at for update skip locked limit 1)
           and {CLAIMABLE} and $1::int = 0
         returning id, company_id, kind, created_by"
    ))
    // $1 is a placeholder so CLAIMABLE's parameter numbers match `claim`.
    .bind(0i32)
    .bind(worker)
    .bind(MAX_ATTEMPTS)
    .bind(STALE_SECS as f64)
    .fetch_optional(pool)
    .await?;
    Ok(row.map(claimed))
}

fn claimed(r: sqlx::postgres::PgRow) -> Claimed {
    Claimed {
        id: r.get("id"),
        company_id: r.get("company_id"),
        kind: r.get("kind"),
        created_by: r.get("created_by"),
    }
}

/// Jobs whose worker died on their last attempt: they will never be
/// claimed again, so they are failed rather than left "running".
pub async fn fail_abandoned(pool: &PgPool) -> Result<u64> {
    Ok(sqlx::query(
        "update job set status = 'failed', finished_at = now(),
                        error = 'Jobben stoppet uten å fullføre, flere ganger. Prøv igjen, eller kontakt support.'
         where status = 'running' and attempts >= $1
           and heartbeat_at < now() - make_interval(secs => $2)",
    )
    .bind(MAX_ATTEMPTS)
    .bind(STALE_SECS as f64)
    .execute(pool)
    .await?
    .rows_affected())
}

async fn heartbeat(pool: &PgPool, job_id: Uuid, worker: &str) -> Result<()> {
    sqlx::query(
        "update job set heartbeat_at = now()
         where id = $1 and claimed_by = $2 and status = 'running'",
    )
    .bind(job_id)
    .bind(worker)
    .execute(pool)
    .await?;
    Ok(())
}

/// Records the outcome -- only while this worker still holds the job, so
/// a worker that was presumed dead cannot overwrite its successor.
async fn finish(pool: &PgPool, job_id: Uuid, worker: &str, outcome: Result<Value>) -> Result<()> {
    let (status, result, error) = match outcome {
        Ok(v) => ("done", Some(v.to_string()), None),
        Err(e) => ("failed", None, Some(format!("{e:#}"))),
    };
    sqlx::query(
        "update job set status = $3, result = $4::jsonb, error = $5, finished_at = now()
         where id = $1 and claimed_by = $2 and status = 'running'",
    )
    .bind(job_id)
    .bind(worker)
    .bind(status)
    .bind(result)
    .bind(error)
    .execute(pool)
    .await?;
    Ok(())
}

/// Runs a claimed job to completion: heartbeats while it works, then
/// records the result or the error. A job's own failure (a SAF-T file
/// that does not reconcile) is an outcome, written to the row for the
/// user to read; only failing to record it is an `Err` here.
pub async fn process(pool: &PgPool, job: &Claimed, worker: &str) -> Result<()> {
    let beat = {
        let (pool, id, worker) = (pool.clone(), job.id, worker.to_owned());
        tokio::spawn(async move {
            let mut every = tokio::time::interval(std::time::Duration::from_secs(HEARTBEAT_SECS));
            every.tick().await;
            loop {
                every.tick().await;
                if let Err(e) = heartbeat(&pool, id, &worker).await {
                    eprintln!("job {id}: heartbeat failed: {e:#}");
                }
            }
        })
    };
    let outcome = execute(pool, job).await;
    beat.abort();
    finish(pool, job.id, worker, outcome).await
}

async fn execute(pool: &PgPool, job: &Claimed) -> Result<Value> {
    match Kind::parse(&job.kind) {
        Some(Kind::SaftImport) => {
            let (bytes, content_type) = input(pool, job.id).await?;
            let body = String::from_utf8(bytes).context("the SAF-T file is not UTF-8")?;
            let is_json = content_type.contains("application/json");
            let (file, source) =
                crate::saft_import::parse_import_body(is_json, body).map_err(anyhow::Error::msg)?;
            let report = crate::saft_import::import_saft(
                pool,
                job.company_id,
                &file,
                &source,
                &job.created_by,
            )
            .await?;
            Ok(serde_json::json!({
                "accounts": report.accounts,
                "customers": report.customers,
                "suppliers": report.suppliers,
                "vouchers": report.vouchers,
                "opening_posted": report.opening_posted,
                "opening_reconciled": report.opening_reconciled,
                "warnings": report.warnings,
            }))
        }
        Some(Kind::AuditReport) => {
            let report = crate::audit::build_audit_report(
                pool,
                job.company_id,
                &job.created_by,
                env!("CARGO_PKG_VERSION"),
            )
            .await?;
            let mut value = crate::audit::audit_json(&report);
            value["tekst"] = regnmed_core::audit::render_text(&report).into();
            Ok(value)
        }
        None => bail!("unknown job kind {}", job.kind),
    }
}

async fn input(pool: &PgPool, job_id: Uuid) -> Result<(Vec<u8>, String)> {
    let row = sqlx::query("select input, input_sha256, input_content_type from job where id = $1")
        .bind(job_id)
        .fetch_one(pool)
        .await?;
    let sha: Vec<u8> = row
        .get::<Option<Vec<u8>>, _>("input_sha256")
        .context("the job has no input")?;
    let bytes = blobs().load(row.get("input"), &sha).await?;
    Ok((
        bytes,
        row.get::<Option<String>, _>("input_content_type")
            .unwrap_or_default(),
    ))
}
