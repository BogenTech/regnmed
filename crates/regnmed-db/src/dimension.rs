//! The dimension register: avdeling and prosjekt (docs/dimensjoner.md).
//! Master data with a restricted lifecycle — insert, rename, open/close.
//! The CODE is immutable (it is inside the v3 voucher hash); enforced by
//! trigger + column grants in migration 0018.
//!
//! A prosjekt may carry its KUNDE (#80): editable metadata like the
//! name, never part of the chain. One customer per project — a project
//! for several customers is two projects.

use anyhow::{Context, Result, ensure};
use sqlx::{PgPool, Row};
use uuid::Uuid;

#[derive(Debug)]
pub struct DimensionRow {
    pub kind: String,
    pub code: String,
    pub name: String,
    pub active: bool,
    /// The customer a prosjekt is for (#80) — party_no + name, None for
    /// avdelinger and unlinked projects.
    pub kunde: Option<String>,
    pub kunde_navn: Option<String>,
}

pub async fn list_dimensions(pool: &PgPool, company_id: Uuid) -> Result<Vec<DimensionRow>> {
    let rows = sqlx::query(
        "select d.kind, d.code, d.name, d.active, p.party_no as kunde, p.name as kunde_navn
         from dimension d
         left join party p on p.id = d.party_id
         where d.company_id = $1 order by d.kind, d.code",
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
            kunde: r.get("kunde"),
            kunde_navn: r.get("kunde_navn"),
        })
        .collect())
}

/// Resolves a party_no to a party that actually IS a customer — the
/// kind check lives here because a partial unique index cannot be an FK
/// target (migration 0047).
async fn kunde_id(pool: &PgPool, company_id: Uuid, party_no: &str) -> Result<Uuid> {
    sqlx::query_scalar(
        "select id from party where company_id = $1 and party_no = $2 and kind = 'kunde'",
    )
    .bind(company_id)
    .bind(party_no)
    .fetch_optional(pool)
    .await?
    .with_context(|| format!("ingen kunde med nummer {party_no}"))
}

pub async fn create_dimension(
    pool: &PgPool,
    company_id: Uuid,
    kind: &str,
    code: &str,
    name: &str,
    kunde: Option<&str>,
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
    let party_id = match kunde.filter(|k| !k.is_empty()) {
        Some(party_no) => {
            ensure!(
                kind == "prosjekt",
                "bare prosjekter kan knyttes til en kunde"
            );
            Some(kunde_id(pool, company_id, party_no).await?)
        }
        None => None,
    };
    sqlx::query(
        "insert into dimension (id, company_id, kind, code, name, party_id)
         values ($1, $2, $3, $4, $5, $6)",
    )
    .bind(Uuid::now_v7())
    .bind(company_id)
    .bind(kind)
    .bind(code)
    .bind(name)
    .bind(party_id)
    .execute(pool)
    .await
    .with_context(|| format!("{kind} {code} finnes allerede?"))?;
    Ok(())
}

/// Rename, open/close and/or (for prosjekter) relink the customer. The
/// code itself can never change — it is referenced by posted entries
/// and covered by their hashes. `kunde`: None leaves the link as it is,
/// Some("") clears it, Some(party_no) points it at that customer.
pub async fn update_dimension(
    pool: &PgPool,
    company_id: Uuid,
    kind: &str,
    code: &str,
    name: Option<&str>,
    active: Option<bool>,
    kunde: Option<&str>,
) -> Result<()> {
    let (sett_kunde, party_id) = match kunde {
        None => (false, None),
        Some("") => (true, None),
        Some(party_no) => {
            ensure!(
                kind == "prosjekt",
                "bare prosjekter kan knyttes til en kunde"
            );
            (true, Some(kunde_id(pool, company_id, party_no).await?))
        }
    };
    let updated = sqlx::query(
        "update dimension
         set name = coalesce($4, name),
             active = coalesce($5, active),
             party_id = case when $6 then $7 else party_id end
         where company_id = $1 and kind = $2 and code = $3",
    )
    .bind(company_id)
    .bind(kind)
    .bind(code)
    .bind(name)
    .bind(active)
    .bind(sett_kunde)
    .bind(party_id)
    .execute(pool)
    .await?;
    ensure!(updated.rows_affected() == 1, "no {kind} with code {code}");
    Ok(())
}
