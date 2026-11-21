//! Where a company's Altinn system user stands (docs/gov.md).

use anyhow::Result;
use sqlx::{PgPool, Row};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SystemUserRow {
    pub altinn_request_id: Option<String>,
    pub status: String,
    pub confirm_url: Option<String>,
}

pub async fn get(pool: &PgPool, company_id: Uuid) -> Result<Option<SystemUserRow>> {
    let row = sqlx::query(
        "select altinn_request_id, status, confirm_url
         from altinn_system_user where company_id = $1",
    )
    .bind(company_id)
    .fetch_optional(pool)
    .await?;
    Ok(row.map(|r| SystemUserRow {
        altinn_request_id: r.get("altinn_request_id"),
        status: r.get("status"),
        confirm_url: r.get("confirm_url"),
    }))
}

pub async fn save(
    pool: &PgPool,
    company_id: Uuid,
    row: &SystemUserRow,
    requested_by: &str,
) -> Result<()> {
    sqlx::query(
        "insert into altinn_system_user
             (company_id, altinn_request_id, status, confirm_url, requested_by)
         values ($1, $2, $3, $4, $5)
         on conflict (company_id) do update set
             altinn_request_id = excluded.altinn_request_id,
             status = excluded.status,
             confirm_url = excluded.confirm_url,
             requested_by = excluded.requested_by,
             updated_at = now()",
    )
    .bind(company_id)
    .bind(&row.altinn_request_id)
    .bind(&row.status)
    .bind(&row.confirm_url)
    .bind(requested_by)
    .execute(pool)
    .await?;
    Ok(())
}

/// Records what Altinn said about this request. A check that raced a newer
/// request changes nothing, and who asked for the approval stays recorded.
pub async fn update_status(
    pool: &PgPool,
    company_id: Uuid,
    altinn_request_id: &str,
    status: &str,
    confirm_url: Option<&str>,
) -> Result<()> {
    sqlx::query(
        "update altinn_system_user
         set status = $3, confirm_url = $4, updated_at = now()
         where company_id = $1 and altinn_request_id = $2",
    )
    .bind(company_id)
    .bind(altinn_request_id)
    .bind(status)
    .bind(confirm_url)
    .execute(pool)
    .await?;
    Ok(())
}
