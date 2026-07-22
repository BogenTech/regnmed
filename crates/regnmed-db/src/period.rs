//! Periodelåsing (ajourhold): an insert-only history of "locked through"
//! dates per company. The current lock is the latest row; every advance
//! and every reopening is audit trail. Enforcement lives in the posting
//! path and in a database trigger — this module only reads and appends.

use anyhow::{Result, ensure};
use chrono::NaiveDate;
use sqlx::{PgPool, Row};
use uuid::Uuid;

#[derive(Debug)]
pub struct PeriodLockRow {
    pub locked_through: NaiveDate,
    pub set_by: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

pub async fn current_period_lock(pool: &PgPool, company_id: Uuid) -> Result<Option<NaiveDate>> {
    Ok(sqlx::query_scalar("select current_period_lock($1)")
        .bind(company_id)
        .fetch_one(pool)
        .await?)
}

pub async fn period_lock_history(pool: &PgPool, company_id: Uuid) -> Result<Vec<PeriodLockRow>> {
    let rows = sqlx::query(
        "select locked_through, set_by, created_at from period_lock
         where company_id = $1 order by created_at desc, id desc",
    )
    .bind(company_id)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .iter()
        .map(|r| PeriodLockRow {
            locked_through: r.get("locked_through"),
            set_by: r.get("set_by"),
            created_at: r.get("created_at"),
        })
        .collect())
}

/// Appends a new lock. Moving the lock backwards (reopening a period) is
/// only allowed when `allow_reopen` — the API grants that to admins only,
/// and the reopening stays in the history forever.
pub async fn set_period_lock(
    pool: &PgPool,
    company_id: Uuid,
    locked_through: NaiveDate,
    set_by: &str,
    allow_reopen: bool,
) -> Result<()> {
    let current = current_period_lock(pool, company_id).await?;
    if let Some(current) = current {
        ensure!(
            locked_through >= current || allow_reopen,
            "reopening a locked period (moving the lock back from {current}) requires admin"
        );
        ensure!(
            locked_through != current,
            "period is already locked through {current}"
        );
    }
    sqlx::query(
        "insert into period_lock (id, company_id, locked_through, set_by)
         values ($1, $2, $3, $4)",
    )
    .bind(Uuid::now_v7())
    .bind(company_id)
    .bind(locked_through)
    .bind(set_by)
    .execute(pool)
    .await?;
    Ok(())
}
