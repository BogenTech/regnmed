//! Dimensjonsregisteret: avdeling og prosjekt (docs/dimensjoner.md).
//! Master data with a restricted lifecycle — insert, rename, open/close.
//! The CODE is immutable (it is inside the v3 voucher hash); enforced by
//! trigger + column grants in migration 0018.

use anyhow::{Context, Result, ensure};
use sqlx::{PgPool, Row};
use uuid::Uuid;

#[derive(Debug)]
pub struct DimensionRow {
    pub kind: String,
    pub code: String,
    pub name: String,
    pub active: bool,
}

pub async fn list_dimensions(pool: &PgPool, company_id: Uuid) -> Result<Vec<DimensionRow>> {
    let rows = sqlx::query(
        "select kind, code, name, active from dimension
         where company_id = $1 order by kind, code",
    )
    .bind(company_id)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .iter()
        .map(|r| DimensionRow {
            kind: r.get("kind"),
            code: r.get("code"),
            name: r.get("name"),
            active: r.get("active"),
        })
        .collect())
}

pub async fn create_dimension(
    pool: &PgPool,
    company_id: Uuid,
    kind: &str,
    code: &str,
    name: &str,
) -> Result<()> {
    ensure!(
        kind == "avdeling" || kind == "prosjekt",
        "kind must be avdeling or prosjekt"
    );
    ensure!(
        !code.is_empty() && !name.is_empty(),
        "code and name are required"
    );
    ensure!(
        code.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'),
        "code must be alphanumeric (A-Z, 0-9, -)"
    );
    sqlx::query(
        "insert into dimension (id, company_id, kind, code, name) values ($1, $2, $3, $4, $5)",
    )
    .bind(Uuid::now_v7())
    .bind(company_id)
    .bind(kind)
    .bind(code)
    .bind(name)
    .execute(pool)
    .await
    .with_context(|| format!("{kind} {code} finnes allerede?"))?;
    Ok(())
}

/// Rename and/or open/close. The code itself can never change — it is
/// referenced by posted entries and covered by their hashes.
pub async fn update_dimension(
    pool: &PgPool,
    company_id: Uuid,
    kind: &str,
    code: &str,
    name: Option<&str>,
    active: Option<bool>,
) -> Result<()> {
    let updated = sqlx::query(
        "update dimension set name = coalesce($4, name), active = coalesce($5, active)
         where company_id = $1 and kind = $2 and code = $3",
    )
    .bind(company_id)
    .bind(kind)
    .bind(code)
    .bind(name)
    .bind(active)
    .execute(pool)
    .await?;
    ensure!(updated.rows_affected() == 1, "no {kind} with code {code}");
    Ok(())
}
