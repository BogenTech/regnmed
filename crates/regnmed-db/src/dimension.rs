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
    /// Whether hours on the prosjekt are billable unless the entry says
    /// otherwise (migration 0052). Always false for avdelinger.
    pub fakturerbar_default: bool,
    /// The VIEWER's effective timesats today (person-specific first,
    /// project default second) — what the grid shows as the row's sats.
    pub min_timesats_ore: Option<i64>,
}

pub async fn list_dimensions(
    pool: &PgPool,
    company_id: Uuid,
    viewer: Uuid,
) -> Result<Vec<DimensionRow>> {
    let rows = sqlx::query(
        "select d.kind, d.code, d.name, d.active, d.fakturerbar_default,
                p.party_no as kunde, p.name as kunde_navn,
                (select s.timesats_ore from prosjekt_sats s
                  where s.dimension_id = d.id
                    and (s.person_id = $2 or s.person_id is null)
                    and s.valid_from <= current_date
                  order by (s.person_id is not null) desc, s.valid_from desc
                  limit 1) as min_timesats_ore
         from dimension d
         left join party p on p.id = d.party_id
         where d.company_id = $1 order by d.kind, d.code",
    )
    .bind(company_id)
    .bind(viewer)
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
            fakturerbar_default: r.get("fakturerbar_default"),
            min_timesats_ore: r.get("min_timesats_ore"),
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
#[allow(clippy::too_many_arguments)]
pub async fn update_dimension(
    pool: &PgPool,
    company_id: Uuid,
    kind: &str,
    code: &str,
    name: Option<&str>,
    active: Option<bool>,
    kunde: Option<&str>,
    fakturerbar_default: Option<bool>,
) -> Result<()> {
    if fakturerbar_default == Some(true) {
        ensure!(
            kind == "prosjekt",
            "bare prosjekter kan være fakturerbare som standard"
        );
    }
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
             party_id = case when $6 then $7 else party_id end,
             fakturerbar_default = coalesce($8, fakturerbar_default)
         where company_id = $1 and kind = $2 and code = $3",
    )
    .bind(company_id)
    .bind(kind)
    .bind(code)
    .bind(name)
    .bind(active)
    .bind(sett_kunde)
    .bind(party_id)
    .bind(fakturerbar_default)
    .execute(pool)
    .await?;
    ensure!(updated.rows_affected() == 1, "no {kind} with code {code}");
    Ok(())
}

/// One row of the dated sats history for a prosjekt (migration 0052).
#[derive(Debug)]
pub struct ProsjektSatsRow {
    pub person_id: Option<Uuid>,
    pub person_navn: Option<String>,
    pub timesats_ore: i64,
    pub valid_from: chrono::NaiveDate,
    pub created_by: String,
}

/// The full dated history, newest first — the editor shows it, nothing
/// is ever deleted (append-only, the satsregister doctrine).
pub async fn list_prosjekt_satser(
    pool: &PgPool,
    company_id: Uuid,
    code: &str,
) -> Result<Vec<ProsjektSatsRow>> {
    let rows = sqlx::query(
        "select s.person_id, coalesce(p.name, p.oidc_sub) as person_navn,
                s.timesats_ore, s.valid_from, s.created_by
         from prosjekt_sats s
         join dimension d on d.id = s.dimension_id
         left join person p on p.id = s.person_id
         where s.company_id = $1 and d.kind = 'prosjekt' and d.code = $2
         order by (s.person_id is null) desc, person_navn, s.valid_from desc",
    )
    .bind(company_id)
    .bind(code)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .iter()
        .map(|r| ProsjektSatsRow {
            person_id: r.get("person_id"),
            person_navn: r
                .get::<Option<Uuid>, _>("person_id")
                .map(|_| r.get("person_navn")),
            timesats_ore: r.get("timesats_ore"),
            valid_from: r.get("valid_from"),
            created_by: r.get("created_by"),
        })
        .collect())
}

/// A rate change is one INSERT: the new row wins from `valid_from`,
/// history stays. `person_id` None sets the project's default rate.
pub async fn set_prosjekt_sats(
    pool: &PgPool,
    company_id: Uuid,
    code: &str,
    person_id: Option<Uuid>,
    timesats_ore: i64,
    valid_from: chrono::NaiveDate,
    created_by: &str,
) -> Result<()> {
    ensure!(timesats_ore >= 0, "timesats kan ikke være negativ");
    let dimension_id: Uuid = sqlx::query_scalar(
        "select id from dimension where company_id = $1 and kind = 'prosjekt' and code = $2",
    )
    .bind(company_id)
    .bind(code)
    .fetch_optional(pool)
    .await?
    .with_context(|| format!("no prosjekt {code}"))?;
    if let Some(pid) = person_id {
        // The person must actually be someone in this company's world —
        // any person the company can see through membership is fine; a
        // random uuid is not. The FK guards existence, this guards typos
        // pointing at a person with no relation to the company.
        let kjent: bool = sqlx::query_scalar(
            "select exists(select 1 from company_member
                            where company_id = $1 and person_id = $2)",
        )
        .bind(company_id)
        .bind(pid)
        .fetch_one(pool)
        .await?;
        ensure!(kjent, "personen er ikke medlem av selskapet");
    }
    sqlx::query(
        "insert into prosjekt_sats (id, company_id, dimension_id, person_id,
                                    timesats_ore, valid_from, created_by)
         values ($1, $2, $3, $4, $5, $6, $7)",
    )
    .bind(Uuid::now_v7())
    .bind(company_id)
    .bind(dimension_id)
    .bind(person_id)
    .bind(timesats_ore)
    .bind(valid_from)
    .bind(created_by)
    .execute(pool)
    .await?;
    Ok(())
}
