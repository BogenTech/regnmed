//! Machine access to the API (docs/integrations.md, #45).
//!
//! An integration is a `person` with kind = 'integrasjon'. That is not a
//! shortcut — it is the whole point: the access lookup, the attribution
//! and the audit trail are the same for a robot as for a human, so there
//! is no separate machine path that can grow holes of its own.
//!
//! The token proves identity (client_credentials from our IdP; regnmed
//! never issues API keys of its own). What that identity may do is
//! decided here — and without a grant it gets nothing, however valid the
//! token is.

use anyhow::{Context, Result, ensure};
use chrono::{DateTime, NaiveDate, Utc};
use sqlx::{PgPool, Row};
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct Integration {
    pub id: Uuid,
    pub person_id: Uuid,
    pub client_id: String,
    pub name: String,
    pub contact: Option<String>,
    pub rate_limit_min: i32,
    pub registered_by: String,
    pub created_at: DateTime<Utc>,
    /// The face, docs/agent.md §8.1: a daisyUI colour and an icon name.
    /// Plain metadata -- never hashed, never part of authorization.
    pub avatar_farge: Option<String>,
    pub avatar_ikon: Option<String>,
    /// Extra instructions for THIS agent, appended to the role prompt
    /// (docs/agent.md §8.6). Text for a model, never authorization.
    pub system_prompt: Option<String>,
}

fn from_row(r: &sqlx::postgres::PgRow) -> Integration {
    Integration {
        id: r.get("id"),
        person_id: r.get("person_id"),
        client_id: r.get("client_id"),
        name: r.get("navn"),
        contact: r.get("kontakt"),
        rate_limit_min: r.get("rate_limit_min"),
        registered_by: r.get("registrert_av"),
        created_at: r.get("created_at"),
        avatar_farge: r.get("avatar_farge"),
        avatar_ikon: r.get("avatar_ikon"),
        system_prompt: r.get("systemprompt"),
    }
}

const SELECT: &str = "select i.id, i.person_id, p.oidc_sub as client_id, i.navn, i.kontakt,
                             i.rate_limit_min, i.registrert_av, i.created_at,
                             i.avatar_farge, i.avatar_ikon, i.systemprompt
                      from integration i join person p on p.id = i.person_id";

/// The integration behind a token subject, if that subject is one.
/// Cheap enough to run per request: one indexed lookup.
pub async fn integration_by_sub(pool: &PgPool, sub: &str) -> Result<Option<Integration>> {
    let row = sqlx::query(&format!("{SELECT} where p.oidc_sub = $1"))
        .bind(sub)
        .fetch_optional(pool)
        .await?;
    Ok(row.as_ref().map(from_row))
}

/// Registers a machine client and grants it access to one company.
/// Registration and grant are one action on purpose: an integration
/// nobody has granted anything is not worth a row.
pub async fn grant_integration(
    pool: &PgPool,
    company_id: Uuid,
    client_id: &str,
    name: &str,
    contact: Option<&str>,
    access: &str,
    created_by: &str,
) -> Result<Uuid> {
    let client_id = client_id.trim();
    ensure!(!client_id.is_empty(), "client_id mangler");
    ensure!(!name.trim().is_empty(), "integrasjonen trenger et navn");
    // The role NAME is validated by the caller, where the vocabulary
    // lives (`regnmed-api::access`): this crate does not know which
    // names are roles, and a second list here would be a second truth.
    // What the grant cannot give is enforced in the access lookup
    // instead of here — `Rett::kan_delegeres` applies to every row,
    // including the ones written before it (#101).
    ensure!(!access.trim().is_empty(), "rollen mangler");

    let mut tx = pool.begin().await?;
    // Any valid token provisions a person row on first sight, so a
    // client that called BEFORE being registered already has one — an
    // empty shell with no membership anywhere. That may become an
    // integration. A subject that actually belongs to somebody (any
    // membership at all) may not: a robot must never inherit a human's
    // access by being registered under their subject.
    let existing: Option<(Uuid, String, bool)> = sqlx::query_as(
        "select p.id, p.kind,
                exists (select 1 from company_member m where m.person_id = p.id)
                or exists (select 1 from firm_member f where f.person_id = p.id) as har_tilgang
         from person p where p.oidc_sub = $1",
    )
    .bind(client_id)
    .fetch_optional(&mut *tx)
    .await?;
    let person_id = match existing {
        Some((id, kind, has_access)) => {
            ensure!(
                kind == "integrasjon" || !has_access,
                "{client_id} tilhører allerede en innlogget bruker — \
                 en integrasjon må ha sin egen klient-id"
            );
            if kind != "integrasjon" {
                sqlx::query("update person set kind = 'integrasjon', name = $2 where id = $1")
                    .bind(id)
                    .bind(name.trim())
                    .execute(&mut *tx)
                    .await?;
            }
            id
        }
        None => {
            let id = Uuid::now_v7();
            sqlx::query(
                "insert into person (id, oidc_sub, name, kind)
                 values ($1, $2, $3, 'integrasjon')",
            )
            .bind(id)
            .bind(client_id)
            .bind(name.trim())
            .execute(&mut *tx)
            .await?;
            id
        }
    };

    let integration_id: Uuid =
        match sqlx::query_scalar("select id from integration where person_id = $1")
            .bind(person_id)
            .fetch_optional(&mut *tx)
            .await?
        {
            Some(id) => id,
            None => {
                let id = Uuid::now_v7();
                sqlx::query(
                    "insert into integration (id, person_id, navn, kontakt, registrert_av)
                 values ($1, $2, $3, $4, $5)",
                )
                .bind(id)
                .bind(person_id)
                .bind(name.trim())
                .bind(contact.map(str::trim).filter(|k| !k.is_empty()))
                .bind(created_by)
                .execute(&mut *tx)
                .await?;
                id
            }
        };

    sqlx::query(
        "insert into integration_grant (id, integration_id, company_id, access, created_by)
         values ($1, $2, $3, $4, $5)",
    )
    .bind(Uuid::now_v7())
    .bind(integration_id)
    .bind(company_id)
    .bind(access)
    .bind(created_by)
    .execute(&mut *tx)
    .await
    .context("integrasjonen har allerede tilgang til selskapet")?;
    tx.commit().await?;
    Ok(integration_id)
}

/// The colours an agent's face may use (docs/agent.md §8.1). These are
/// daisyUI's own semantic names, so a face follows the theme like
/// everything else. The list is here rather than in the portal because
/// the portal has to spell each class literally for Tailwind to emit it:
/// that table is a renderer, this is the gate.
pub const AVATAR_FARGER: [&str; 8] = [
    "primary",
    "secondary",
    "accent",
    "info",
    "success",
    "warning",
    "error",
    "neutral",
];

/// Renames an agent and sets its face. The access level is deliberately
/// NOT editable here: a grant is immutable by design (0033), so a role
/// change is a revoke plus a new grant, which leaves the dated trail
/// that makes the old role still true for the time it applied.
///
/// `navn` is the integration's own and is shared by every grant it has,
/// exactly as `person.name` is the person's rather than per company —
/// the model from docs/agent.md §1, where an agent is a user. The
/// company in the path is therefore an authorization scope, not an
/// owner: the caller must have something to do with this integration.
#[allow(clippy::too_many_arguments)]
pub async fn update_integration(
    pool: &PgPool,
    company_id: Uuid,
    integration_id: Uuid,
    name: &str,
    contact: Option<&str>,
    avatar_farge: Option<&str>,
    avatar_ikon: Option<&str>,
    system_prompt: Option<&str>,
) -> Result<()> {
    let name = name.trim();
    ensure!(!name.is_empty(), "integrasjonen trenger et navn");
    fn tom(v: Option<&str>) -> Option<&str> {
        v.map(str::trim).filter(|t| !t.is_empty())
    }
    let contact = tom(contact);
    let farge = tom(avatar_farge);
    let ikon = tom(avatar_ikon);
    let prompt = tom(system_prompt);
    if let Some(prompt) = prompt {
        // A cap, not a judgement of content: the whole prompt is sent
        // with every single model turn, so a novel pasted in here is
        // the customer's own bill, turn after turn.
        ensure!(
            prompt.chars().count() <= 4000,
            "instruksjonen er for lang ({} tegn, maks 4000)",
            prompt.chars().count()
        );
    }
    if let Some(farge) = farge {
        ensure!(
            AVATAR_FARGER.contains(&farge),
            "ukjent farge «{farge}» — gyldige er {}",
            AVATAR_FARGER.join(", ")
        );
    }
    if let Some(ikon) = ikon {
        // The icon set belongs to the portal (`ikoner.js`), which falls
        // back when it does not know a name, so this only keeps the
        // column from becoming a place to put prose.
        ensure!(ikon.chars().count() <= 40, "ikonnavnet er for langt");
    }

    let updated = sqlx::query(
        "update integration set navn = $3, kontakt = $4, avatar_farge = $5, avatar_ikon = $6,
                                systemprompt = $7
         where id = $1
           and exists (select 1 from integration_grant g
                       where g.integration_id = $1 and g.company_id = $2)",
    )
    .bind(integration_id)
    .bind(company_id)
    .bind(name)
    .bind(contact)
    .bind(farge)
    .bind(ikon)
    .bind(prompt)
    .execute(pool)
    .await?
    .rows_affected();
    ensure!(updated == 1, "ingen slik integrasjon på dette selskapet");
    Ok(())
}

/// Revokes access with immediate effect: `valid_to` is exclusive, so
/// setting it to today ends access now — not at midnight.
pub async fn revoke_integration(
    pool: &PgPool,
    company_id: Uuid,
    integration_id: Uuid,
    revoked_by: &str,
) -> Result<()> {
    let updated = sqlx::query(
        "update integration_grant set valid_to = current_date, revoked_by = $3
         where integration_id = $1 and company_id = $2 and valid_to is null",
    )
    .bind(integration_id)
    .bind(company_id)
    .bind(revoked_by)
    .execute(pool)
    .await?
    .rows_affected();
    ensure!(updated == 1, "ingen aktiv tilgang å trekke tilbake");
    Ok(())
}

/// Changes an agent's role the way the UI reads it: one control, one
/// action. Underneath it is still revoke plus a new grant, because a
/// grant is immutable (0033) — so the old role stays true for the days
/// it applied, and the trail says who changed it and when.
///
/// Both rows in ONE transaction: the halves must not be separable, or a
/// failure between them leaves the agent with no access at all while
/// the portal shows it as merely re-roled.
pub async fn endre_integrasjonsrolle(
    pool: &PgPool,
    company_id: Uuid,
    integration_id: Uuid,
    access: &str,
    changed_by: &str,
) -> Result<()> {
    // The role name is validated by the caller, where the vocabulary
    // lives — the same split as `grant_integration`.
    ensure!(!access.trim().is_empty(), "rollen mangler");
    let mut tx = pool.begin().await?;
    let gammel: Option<String> = sqlx::query_scalar(
        "select access from integration_grant
         where integration_id = $1 and company_id = $2 and valid_to is null
         for update",
    )
    .bind(integration_id)
    .bind(company_id)
    .fetch_optional(&mut *tx)
    .await?;
    let gammel = gammel.context("ingen aktiv tilgang å endre")?;
    ensure!(
        gammel != access.trim(),
        "agenten har allerede rollen «{access}»"
    );

    sqlx::query(
        "update integration_grant set valid_to = current_date, revoked_by = $3
         where integration_id = $1 and company_id = $2 and valid_to is null",
    )
    .bind(integration_id)
    .bind(company_id)
    .bind(changed_by)
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        "insert into integration_grant (id, integration_id, company_id, access, created_by)
         values ($1, $2, $3, $4, $5)",
    )
    .bind(Uuid::now_v7())
    .bind(integration_id)
    .bind(company_id)
    .bind(access.trim())
    .bind(changed_by)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(())
}

#[derive(Debug)]
pub struct GrantRow {
    /// The GRANT's id, not the integration's. One integration has one
    /// row per grant it has ever held in this company, so a listing can
    /// show the same agent twice — once active, once ended. The caller
    /// needs a key that is unique per ROW.
    pub grant_id: Uuid,
    pub integration_id: Uuid,
    pub client_id: String,
    pub name: String,
    pub contact: Option<String>,
    pub access: String,
    pub valid_from: NaiveDate,
    pub valid_to: Option<NaiveDate>,
    pub created_by: String,
    pub revoked_by: Option<String>,
    pub rate_limit_min: i32,
    /// Calls today, from the usage counter.
    pub calls_today: i64,
    pub avatar_farge: Option<String>,
    pub avatar_ikon: Option<String>,
    pub system_prompt: Option<String>,
    /// The governing LLM provider and model, when one is active
    /// (docs/agent.md §8). Joined here rather than fetched per row:
    /// the card lists agents, and one query is one query.
    pub llm: Option<(String, String)>,
}

pub async fn list_integrations(pool: &PgPool, company_id: Uuid) -> Result<Vec<GrantRow>> {
    let rows = sqlx::query(
        "select g.id as grant_id, i.id as integration_id, p.oidc_sub as client_id,
                i.navn, i.kontakt,
                i.rate_limit_min, i.avatar_farge, i.avatar_ikon, i.systemprompt,
                g.access, g.valid_from, g.valid_to, g.created_by, g.revoked_by,
                coalesce((select u.kall from integration_usage u
                          where u.integration_id = i.id and u.company_id = g.company_id
                            and u.dag = current_date), 0) as kall_i_dag,
                llm.leverandor as llm_leverandor, llm.modell as llm_modell
         from integration_grant g
         join integration i on i.id = g.integration_id
         join person p on p.id = i.person_id
         left join lateral (
             select l.leverandor, l.modell from agent_llm l
             where l.integration_id = i.id and l.company_id = g.company_id and l.aktiv
             order by l.created_at desc, l.id desc limit 1
         ) llm on true
         where g.company_id = $1
         order by (g.valid_to is null) desc, i.navn",
    )
    .bind(company_id)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .iter()
        .map(|r| GrantRow {
            grant_id: r.get("grant_id"),
            integration_id: r.get("integration_id"),
            client_id: r.get("client_id"),
            name: r.get("navn"),
            contact: r.get("kontakt"),
            access: r.get("access"),
            valid_from: r.get("valid_from"),
            valid_to: r.get("valid_to"),
            created_by: r.get("created_by"),
            revoked_by: r.get("revoked_by"),
            rate_limit_min: r.get("rate_limit_min"),
            calls_today: r.get("kall_i_dag"),
            avatar_farge: r.get("avatar_farge"),
            avatar_ikon: r.get("avatar_ikon"),
            system_prompt: r.get("systemprompt"),
            llm: r
                .get::<Option<String>, _>("llm_leverandor")
                .zip(r.get::<Option<String>, _>("llm_modell")),
        })
        .collect())
}

/// Records one API call. Changing requests are kept in full — those are
/// what an admin (and a revisor) needs to see; every call, read or
/// write, moves the per-day counter.
pub async fn log_integration_call(
    pool: &PgPool,
    integration_id: Uuid,
    company_id: Option<Uuid>,
    method: &str,
    path: &str,
    status: u16,
) -> Result<()> {
    if method != "GET" {
        sqlx::query(
            "insert into integration_call (id, integration_id, company_id, method, path, status)
             values ($1, $2, $3, $4, $5, $6)",
        )
        .bind(Uuid::now_v7())
        .bind(integration_id)
        .bind(company_id)
        .bind(method)
        .bind(path)
        .bind(status as i32)
        .execute(pool)
        .await?;
    }
    if let Some(company_id) = company_id {
        sqlx::query(
            "insert into integration_usage (integration_id, company_id, dag, kall)
             values ($1, $2, current_date, 1)
             on conflict (integration_id, company_id, dag)
             do update set kall = integration_usage.kall + 1",
        )
        .bind(integration_id)
        .bind(company_id)
        .execute(pool)
        .await?;
    }
    Ok(())
}

#[derive(Debug)]
pub struct CallRow {
    pub name: String,
    pub method: String,
    pub path: String,
    pub status: i32,
    pub created_at: DateTime<Utc>,
}

/// The changing calls an integration has made for this company.
pub async fn integration_calls(pool: &PgPool, company_id: Uuid) -> Result<Vec<CallRow>> {
    let rows = sqlx::query(
        "select i.navn, c.method, c.path, c.status, c.created_at
         from integration_call c
         join integration i on i.id = c.integration_id
         where c.company_id = $1
         order by c.created_at desc
         limit 100",
    )
    .bind(company_id)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .iter()
        .map(|r| CallRow {
            name: r.get("navn"),
            method: r.get("method"),
            path: r.get("path"),
            status: r.get("status"),
            created_at: r.get("created_at"),
        })
        .collect())
}
