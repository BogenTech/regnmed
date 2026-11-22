//! The one bit `regnmed job-watch` publishes about the scheduled jobs
//! (migration 0103, docs/deploy.md).

use anyhow::Result;
use sqlx::PgPool;

pub async fn record_job_health(pool: &PgPool, healthy: bool) -> Result<()> {
    sqlx::query(
        "insert into job_health (healthy) values ($1)
         on conflict (id) do update set healthy = excluded.healthy, checked_at = now()",
    )
    .bind(healthy)
    .execute(pool)
    .await?;
    Ok(())
}

/// False when a job is unhealthy AND when the watcher has gone quiet: it
/// runs every 10 minutes, so a verdict older than 30 is not a verdict.
pub async fn job_health_ok(pool: &PgPool) -> Result<bool> {
    let ok = sqlx::query_scalar(
        "select healthy and checked_at > now() - interval '30 minutes' from job_health",
    )
    .fetch_optional(pool)
    .await?;
    Ok(ok.unwrap_or(false))
}
