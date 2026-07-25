//! Attestering (docs/attestering.md, #47): valgfri intern kontroll —
//! hvem som godkjenner skilles fra hvem som bokfører og betaler.
//!
//! Policyen er append-only historikk (nyeste rad gjelder); beslutninger
//! er et insert-only spor der nyeste beslutning gjelder. Håndhevingen
//! bor i transaksjonene som bokfører og godkjenner — se
//! [`crate::innboks::bokfor_inbox_document`],
//! [`crate::betaling::approve_run`] og [`crate::utlegg::approve_expense`].

use anyhow::{Context, Result, bail, ensure};
use chrono::{DateTime, Utc};
use sqlx::{PgPool, Row};
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct AttestationPolicy {
    pub aktiv: bool,
    /// Innboksbilag med debetsum >= grensen krever attestering;
    /// None = alle bilag når policyen er aktiv.
    pub belopsgrense_ore: Option<i64>,
    /// Utpekt attestant; None = alle med bokføringstilgang.
    pub attestant_person_id: Option<Uuid>,
    pub created_by: String,
    pub created_at: DateTime<Utc>,
}

fn policy_from_row(row: &sqlx::postgres::PgRow) -> AttestationPolicy {
    AttestationPolicy {
        aktiv: row.get("aktiv"),
        belopsgrense_ore: row.get("belopsgrense_ore"),
        attestant_person_id: row.get("attestant_person_id"),
        created_by: row.get("created_by"),
        created_at: row.get("created_at"),
    }
}

/// The policy in force: the newest row, or None if none was ever set.
pub async fn current_policy<'e, E>(
    executor: E,
    company_id: Uuid,
) -> Result<Option<AttestationPolicy>>
where
    E: sqlx::PgExecutor<'e>,
{
    let row = sqlx::query(
        "select aktiv, belopsgrense_ore, attestant_person_id, created_by, created_at
         from attestation_policy where company_id = $1
         order by created_at desc, id desc limit 1",
    )
    .bind(company_id)
    .fetch_optional(executor)
    .await?;
    Ok(row.as_ref().map(policy_from_row))
}

pub async fn policy_history(pool: &PgPool, company_id: Uuid) -> Result<Vec<AttestationPolicy>> {
    let rows = sqlx::query(
        "select aktiv, belopsgrense_ore, attestant_person_id, created_by, created_at
         from attestation_policy where company_id = $1
         order by created_at desc, id desc",
    )
    .bind(company_id)
    .fetch_all(pool)
    .await?;
    Ok(rows.iter().map(policy_from_row).collect())
}

/// Records a new policy row (append-only — changing the policy is a new
/// row, the history always shows what applied when). A designated
/// attestant must have access to the company.
pub async fn set_policy(
    pool: &PgPool,
    company_id: Uuid,
    aktiv: bool,
    belopsgrense_ore: Option<i64>,
    attestant_person_id: Option<Uuid>,
    created_by: &str,
) -> Result<()> {
    if let Some(grense) = belopsgrense_ore {
        ensure!(grense >= 0, "beløpsgrensen kan ikke være negativ");
    }
    if let Some(attestant) = attestant_person_id {
        let access = crate::tenancy::company_access(pool, attestant, company_id).await?;
        ensure!(
            access.as_deref().is_some_and(|a| a != "les"),
            "attestanten må ha bokføringstilgang til selskapet"
        );
    }
    sqlx::query(
        "insert into attestation_policy
             (id, company_id, aktiv, belopsgrense_ore, attestant_person_id, created_by)
         values ($1, $2, $3, $4, $5, $6)",
    )
    .bind(Uuid::now_v7())
    .bind(company_id)
    .bind(aktiv)
    .bind(belopsgrense_ore)
    .bind(attestant_person_id)
    .bind(created_by)
    .execute(pool)
    .await?;
    Ok(())
}

#[derive(Debug)]
pub struct CompanyMemberRow {
    pub person_id: Uuid,
    pub name: String,
    pub role: String,
}

/// Direct, active members — the candidates for a designated attestant.
pub async fn company_members(pool: &PgPool, company_id: Uuid) -> Result<Vec<CompanyMemberRow>> {
    let rows = sqlx::query(
        "select cm.person_id, coalesce(p.name, p.oidc_sub) as name, cm.role
         from company_member cm join person p on p.id = cm.person_id
         where cm.company_id = $1 and cm.active
         order by name",
    )
    .bind(company_id)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .iter()
        .map(|r| CompanyMemberRow {
            person_id: r.get("person_id"),
            name: r.get("name"),
            role: r.get("role"),
        })
        .collect())
}

#[derive(Debug)]
pub struct AttestationRow {
    pub decision: String,
    pub note: Option<String>,
    pub decided_by_person: Uuid,
    pub decided_by: String,
    pub created_at: DateTime<Utc>,
}

/// The newest decision on a target — the one that governs.
pub async fn attestation_state<'e, E>(
    executor: E,
    company_id: Uuid,
    target_kind: &str,
    target_id: Uuid,
) -> Result<Option<AttestationRow>>
where
    E: sqlx::PgExecutor<'e>,
{
    let row = sqlx::query(
        "select decision, note, decided_by_person, decided_by, created_at
         from attestation
         where company_id = $1 and target_kind = $2 and target_id = $3
         order by created_at desc, id desc limit 1",
    )
    .bind(company_id)
    .bind(target_kind)
    .bind(target_id)
    .fetch_optional(executor)
    .await?;
    Ok(row.map(|r| AttestationRow {
        decision: r.get("decision"),
        note: r.get("note"),
        decided_by_person: r.get("decided_by_person"),
        decided_by: r.get("decided_by"),
        created_at: r.get("created_at"),
    }))
}

/// The full decision trail on a target, newest first — exactly what an
/// ettersyn asks for.
pub async fn attestation_trail(
    pool: &PgPool,
    company_id: Uuid,
    target_kind: &str,
    target_id: Uuid,
) -> Result<Vec<AttestationRow>> {
    let rows = sqlx::query(
        "select decision, note, decided_by_person, decided_by, created_at
         from attestation
         where company_id = $1 and target_kind = $2 and target_id = $3
         order by created_at desc, id desc",
    )
    .bind(company_id)
    .bind(target_kind)
    .bind(target_id)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .iter()
        .map(|r| AttestationRow {
            decision: r.get("decision"),
            note: r.get("note"),
            decided_by_person: r.get("decided_by_person"),
            decided_by: r.get("decided_by"),
            created_at: r.get("created_at"),
        })
        .collect())
}

/// Records an attestation decision on an undecided inbox document.
/// A designated attestant (policy) is the only one allowed to decide;
/// an avvisning requires a note. Re-deciding appends — the newest row
/// governs, the trail keeps everything.
pub async fn attester_inbox_document(
    pool: &PgPool,
    company_id: Uuid,
    document_id: Uuid,
    godkjent: bool,
    note: Option<&str>,
    person_id: Uuid,
    display_name: &str,
) -> Result<()> {
    let note = note.map(str::trim).filter(|n| !n.is_empty());
    if !godkjent {
        ensure!(note.is_some(), "en avvisning i attestering krever et notat");
    }
    if let Some(policy) = current_policy(pool, company_id).await?
        && policy.aktiv
        && let Some(attestant) = policy.attestant_person_id
        && attestant != person_id
    {
        bail!("policyen utpeker en annen attestant for dette selskapet");
    }
    let status: String =
        sqlx::query_scalar("select status from inbox_document where id = $1 and company_id = $2")
            .bind(document_id)
            .bind(company_id)
            .fetch_optional(pool)
            .await?
            .context("no such inbox document")?;
    ensure!(
        status == "ny",
        "bilaget er allerede {status} — attestering avgjør bare ubesluttede bilag"
    );
    sqlx::query(
        "insert into attestation (id, company_id, target_kind, target_id, decision, note,
                                  decided_by_person, decided_by)
         values ($1, $2, 'inbox_document', $3, $4, $5, $6, $7)",
    )
    .bind(Uuid::now_v7())
    .bind(company_id)
    .bind(document_id)
    .bind(if godkjent { "godkjent" } else { "avvist" })
    .bind(note)
    .bind(person_id)
    .bind(display_name)
    .execute(pool)
    .await?;
    Ok(())
}
