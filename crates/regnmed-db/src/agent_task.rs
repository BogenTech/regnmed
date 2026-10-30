//! Oppgaver til en agent (docs/agent.md, #103).
//!
//! An agent that holds a role (#101) and has tools (#102) still needs to
//! be TOLD what to do, in a way the owner can point at afterwards. That
//! is this: an immutable order plus an insert-only trail of what came
//! back.
//!
//! The status is COMPUTED from the newest event, never stored — the same
//! rule as abonnement status and every balance in this system. No event
//! means `ny`.

use anyhow::{Result, ensure};
use chrono::{DateTime, NaiveDate, Utc};
use sqlx::{PgPool, Row};
use uuid::Uuid;

/// Terminal statuses: the trigger in migration 0064 refuses a further
/// event, so an answer cannot be quietly withdrawn.
pub const AVSLUTTET: [&str; 2] = ["utfort", "avvist"];

#[derive(Debug, Clone)]
pub struct Oppgave {
    pub id: Uuid,
    pub integration_id: Option<Uuid>,
    /// The agent the task is addressed to; None = any agent with access.
    pub agent: Option<String>,
    pub message: String,
    pub deadline: Option<NaiveDate>,
    pub opprettet_av: String,
    pub created_at: DateTime<Utc>,
    /// Newest event's status, or "ny" when nothing has happened.
    pub status: String,
    pub siste_notat: Option<String>,
    pub siste_av: Option<String>,
    pub siste_tidspunkt: Option<DateTime<Utc>>,
}

/// Records an order. The content is immutable once given (0064): an
/// amended task is a NEW task, or nobody can see afterwards what the
/// agent was actually asked to do.
pub async fn opprett_oppgave(
    pool: &PgPool,
    company_id: Uuid,
    integration_id: Option<Uuid>,
    message: &str,
    deadline: Option<NaiveDate>,
    opprettet_av: &str,
) -> Result<Uuid> {
    let message = message.trim();
    ensure!(!message.is_empty(), "oppgaven trenger en melding");
    if let Some(id) = integration_id {
        // A task addressed to an integration that has no access to this
        // company would sit unanswerable forever.
        let har_tilgang: bool = sqlx::query_scalar(
            "select exists (
                 select 1 from integration_grant
                 where integration_id = $1 and company_id = $2
                   and valid_from <= current_date
                   and (valid_to is null or valid_to > current_date))",
        )
        .bind(id)
        .bind(company_id)
        .fetch_one(pool)
        .await?;
        ensure!(
            har_tilgang,
            "integrasjonen har ingen aktiv tilgang til selskapet"
        );
    }
    let id = Uuid::now_v7();
    sqlx::query(
        "insert into agent_oppgave
             (id, company_id, integration_id, melding, frist, opprettet_av)
         values ($1, $2, $3, $4, $5, $6)",
    )
    .bind(id)
    .bind(company_id)
    .bind(integration_id)
    .bind(message)
    .bind(deadline)
    .bind(opprettet_av)
    .execute(pool)
    .await?;
    Ok(id)
}

const SELECT: &str = "select o.id, o.integration_id, i.navn as agent, o.melding, o.frist,
                             o.opprettet_av, o.created_at,
                             h.status, h.notat as siste_notat, h.av as siste_av,
                             h.created_at as siste_tidspunkt
                      from agent_oppgave o
                      left join integration i on i.id = o.integration_id
                      left join lateral (
                          select status, notat, av, created_at
                          from agent_oppgave_hendelse e
                          where e.oppgave_id = o.id
                          order by e.created_at desc, e.id desc limit 1
                      ) h on true";

fn from_row(r: &sqlx::postgres::PgRow) -> Oppgave {
    Oppgave {
        id: r.get("id"),
        integration_id: r.get("integration_id"),
        agent: r.get("agent"),
        message: r.get("melding"),
        deadline: r.get("frist"),
        opprettet_av: r.get("opprettet_av"),
        created_at: r.get("created_at"),
        status: r
            .get::<Option<String>, _>("status")
            .unwrap_or_else(|| "ny".to_string()),
        siste_notat: r.get("siste_notat"),
        siste_av: r.get("siste_av"),
        siste_tidspunkt: r.get("siste_tidspunkt"),
    }
}

/// The company's tasks, newest first.
///
/// `for_integration` narrows to what ONE agent should work on: tasks
/// addressed to it, plus the unaddressed ones. An agent must not see
/// another agent's orders — it would act on them.
pub async fn list_oppgaver(
    pool: &PgPool,
    company_id: Uuid,
    for_integration: Option<Uuid>,
    bare_apne: bool,
) -> Result<Vec<Oppgave>> {
    let rows = sqlx::query(&format!(
        "{SELECT} where o.company_id = $1
           and ($2::uuid is null or o.integration_id is null or o.integration_id = $2)
           and (not $3 or coalesce(h.status, 'ny') not in ('utfort', 'avvist'))
         order by o.created_at desc limit 200"
    ))
    .bind(company_id)
    .bind(for_integration)
    .bind(bare_apne)
    .fetch_all(pool)
    .await?;
    Ok(rows.iter().map(from_row).collect())
}

pub async fn hent_oppgave(pool: &PgPool, company_id: Uuid, id: Uuid) -> Result<Option<Oppgave>> {
    let row = sqlx::query(&format!("{SELECT} where o.company_id = $1 and o.id = $2"))
        .bind(company_id)
        .bind(id)
        .fetch_optional(pool)
        .await?;
    Ok(row.as_ref().map(from_row))
}

#[derive(Debug, Clone)]
pub struct Hendelse {
    pub status: String,
    pub note: Option<String>,
    pub av: String,
    pub created_at: DateTime<Utc>,
}

pub async fn history(pool: &PgPool, company_id: Uuid, id: Uuid) -> Result<Vec<Hendelse>> {
    let rows = sqlx::query(
        "select e.status, e.notat, e.av, e.created_at
         from agent_oppgave_hendelse e
         join agent_oppgave o on o.id = e.oppgave_id
         where o.company_id = $1 and o.id = $2
         order by e.created_at, e.id",
    )
    .bind(company_id)
    .bind(id)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .iter()
        .map(|r| Hendelse {
            status: r.get("status"),
            note: r.get("notat"),
            av: r.get("av"),
            created_at: r.get("created_at"),
        })
        .collect())
}

/// Appends an event. Terminality and "a note is required unless the
/// status is pagar/utfort" are both enforced in the database (0064); the
/// error that comes back says what to do.
pub async fn answer(
    pool: &PgPool,
    company_id: Uuid,
    task_id: Uuid,
    status: &str,
    note: Option<&str>,
    av: &str,
) -> Result<()> {
    ensure!(
        matches!(status, "pagar" | "sporsmal" | "utfort" | "avvist"),
        "ukjent status «{status}» — bruk pagar, sporsmal, utfort eller avvist"
    );
    let note = note.map(str::trim).filter(|n| !n.is_empty());
    ensure!(
        matches!(status, "pagar" | "utfort") || note.is_some(),
        "«{status}» krever en begrunnelse"
    );
    let finnes: bool = sqlx::query_scalar(
        "select exists (select 1 from agent_oppgave where id = $1 and company_id = $2)",
    )
    .bind(task_id)
    .bind(company_id)
    .fetch_one(pool)
    .await?;
    ensure!(finnes, "ingen slik oppgave");
    sqlx::query(
        "insert into agent_oppgave_hendelse (id, oppgave_id, status, notat, av)
         values ($1, $2, $3, $4, $5)",
    )
    .bind(Uuid::now_v7())
    .bind(task_id)
    .bind(status)
    .bind(note)
    .bind(av)
    .execute(pool)
    .await?;
    Ok(())
}

/// How long a claim is honoured before another worker may take over.
///
/// It has to outlast the slowest honest run: twelve turns against a slow
/// provider, each with tool calls. Too short and two workers post the
/// same voucher; too long and a crashed worker's task sits idle. Fifteen
/// minutes is comfortably past the worker's own deadline, so a task is
/// only ever re-claimed after the first attempt is genuinely gone.
pub const CLAIM_TTL_MINUTES: i64 = 15;

/// Takes the lease on a task, or returns false if somebody else holds a
/// live one (docs/agent.md §8.7).
///
/// ONE statement, so no two workers can both conclude they won: the
/// insert either creates the row or collides, and the `do update` only
/// fires for a claim old enough to be considered abandoned. `returning`
/// is empty when the conflicting row was still fresh.
pub async fn claim_task(pool: &PgPool, task_id: Uuid, worker: &str) -> Result<bool> {
    let tatt: Option<Uuid> = sqlx::query_scalar(
        "insert into agent_task_claim (oppgave_id, worker)
         values ($1, $2)
         on conflict (oppgave_id) do update
            set worker = excluded.worker, claimed_at = now()
          where agent_task_claim.claimed_at < now() - ($3 || ' minutes')::interval
         returning oppgave_id",
    )
    .bind(task_id)
    .bind(worker)
    .bind(CLAIM_TTL_MINUTES.to_string())
    .fetch_optional(pool)
    .await?;
    Ok(tatt.is_some())
}

/// Releases the lease. Called whatever the outcome — the trail of what
/// happened lives in the hendelser, not here.
pub async fn release_task(pool: &PgPool, task_id: Uuid) -> Result<()> {
    sqlx::query("delete from agent_task_claim where oppgave_id = $1")
        .bind(task_id)
        .execute(pool)
        .await?;
    Ok(())
}
