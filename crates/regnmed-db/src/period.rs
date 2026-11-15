//! Periodelåsing (ajourhold): an insert-only history of "locked through"
//! dates per company. The current lock is the latest row; every advance
//! and every reopening is audit trail. Enforcement lives in the posting
//! path and in a database trigger — this module only reads and appends.

use anyhow::{Result, ensure};
use chrono::NaiveDate;
use sqlx::{PgConnection, PgPool, Row};
use uuid::Uuid;

#[derive(Debug)]
pub struct PeriodLockRow {
    pub locked_through: NaiveDate,
    pub set_by: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
    /// The row moved the lock BACK from the one before it.
    pub reopened: bool,
}

pub async fn current_period_lock(pool: &PgPool, company_id: Uuid) -> Result<Option<NaiveDate>> {
    Ok(sqlx::query_scalar("select current_period_lock($1)")
        .bind(company_id)
        .fetch_one(pool)
        .await?)
}

pub async fn period_lock_history(pool: &PgPool, company_id: Uuid) -> Result<Vec<PeriodLockRow>> {
    let rows = sqlx::query(
        "select locked_through, set_by, created_at,
                coalesce(locked_through < lag(locked_through)
                             over (order by created_at, id), false) as reopened
         from period_lock
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
            reopened: r.get("reopened"),
        })
        .collect())
}

/// Serializes every change to one company's lock: the read of the
/// current lock and the insert of the next must not interleave with
/// another change, or two of them both pass their checks.
pub(crate) async fn lock_period_changes(conn: &mut PgConnection, company_id: Uuid) -> Result<()> {
    sqlx::query("select pg_advisory_xact_lock(hashtextextended('period_lock:' || $1::text, 0))")
        .bind(company_id)
        .execute(&mut *conn)
        .await?;
    Ok(())
}

pub(crate) fn display_date(d: NaiveDate) -> String {
    d.format("%d.%m.%Y").to_string()
}

/// Appends a new lock. Moving the lock backwards (reopening a period) is
/// only allowed when `allow_reopen` — the API grants that to admins only,
/// and the reopening stays in the history forever. A period that has not
/// ended yet cannot be locked: it would refuse today's postings.
pub async fn set_period_lock(
    pool: &PgPool,
    company_id: Uuid,
    locked_through: NaiveDate,
    set_by: &str,
    allow_reopen: bool,
) -> Result<()> {
    let mut tx = pool.begin().await?;
    lock_period_changes(&mut tx, company_id).await?;
    let (current, today): (Option<NaiveDate>, NaiveDate) =
        sqlx::query_as("select current_period_lock($1), current_date")
            .bind(company_id)
            .fetch_one(&mut *tx)
            .await?;
    ensure!(
        locked_through <= today,
        "Du kan ikke låse en periode som ikke er over ennå. Velg en dato til og med {}.",
        display_date(today)
    );
    if let Some(current) = current {
        ensure!(
            locked_through != current,
            "Perioden er allerede låst til og med {}.",
            display_date(current)
        );
        ensure!(
            locked_through > current || allow_reopen,
            "Bare en administrator kan gjenåpne en låst periode (flytte låsen tilbake fra {}).",
            display_date(current)
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
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(())
}
