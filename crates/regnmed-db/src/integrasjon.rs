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
    pub navn: String,
    pub kontakt: Option<String>,
    pub rate_limit_min: i32,
    pub registrert_av: String,
    pub created_at: DateTime<Utc>,
}

fn from_row(r: &sqlx::postgres::PgRow) -> Integration {
    Integration {
        id: r.get("id"),
        person_id: r.get("person_id"),
        client_id: r.get("client_id"),
        navn: r.get("navn"),
        kontakt: r.get("kontakt"),
        rate_limit_min: r.get("rate_limit_min"),
        registrert_av: r.get("registrert_av"),
        created_at: r.get("created_at"),
    }
}

const SELECT: &str = "select i.id, i.person_id, p.oidc_sub as client_id, i.navn, i.kontakt,
                             i.rate_limit_min, i.registrert_av, i.created_at
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
    navn: &str,
    kontakt: Option<&str>,
    access: &str,
    created_by: &str,
) -> Result<Uuid> {
    let client_id = client_id.trim();
    ensure!(!client_id.is_empty(), "client_id mangler");
    ensure!(!navn.trim().is_empty(), "integrasjonen trenger et navn");
    ensure!(
        access == "les" || access == "bokforing",
        "tilgangsnivået må være 'les' eller 'bokforing'"
    );

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
        Some((id, kind, har_tilgang)) => {
            ensure!(
                kind == "integrasjon" || !har_tilgang,
                "{client_id} tilhører allerede en innlogget bruker — \
                 en integrasjon må ha sin egen klient-id"
            );
            if kind != "integrasjon" {
                sqlx::query("update person set kind = 'integrasjon', name = $2 where id = $1")
                    .bind(id)
                    .bind(navn.trim())
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
            .bind(navn.trim())
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
                .bind(navn.trim())
                .bind(kontakt.map(str::trim).filter(|k| !k.is_empty()))
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

#[derive(Debug)]
pub struct GrantRow {
    pub integration_id: Uuid,
    pub client_id: String,
    pub navn: String,
    pub kontakt: Option<String>,
    pub access: String,
    pub valid_from: NaiveDate,
    pub valid_to: Option<NaiveDate>,
    pub created_by: String,
    pub revoked_by: Option<String>,
    pub rate_limit_min: i32,
    /// Calls today, from the usage counter.
    pub kall_i_dag: i64,
}

pub async fn list_integrations(pool: &PgPool, company_id: Uuid) -> Result<Vec<GrantRow>> {
    let rows = sqlx::query(
        "select i.id as integration_id, p.oidc_sub as client_id, i.navn, i.kontakt,
                i.rate_limit_min, g.access, g.valid_from, g.valid_to, g.created_by, g.revoked_by,
                coalesce((select u.kall from integration_usage u
                          where u.integration_id = i.id and u.company_id = g.company_id
                            and u.dag = current_date), 0) as kall_i_dag
         from integration_grant g
         join integration i on i.id = g.integration_id
         join person p on p.id = i.person_id
         where g.company_id = $1
         order by (g.valid_to is null) desc, i.navn",
    )
    .bind(company_id)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .iter()
        .map(|r| GrantRow {
            integration_id: r.get("integration_id"),
            client_id: r.get("client_id"),
            navn: r.get("navn"),
            kontakt: r.get("kontakt"),
            access: r.get("access"),
            valid_from: r.get("valid_from"),
            valid_to: r.get("valid_to"),
            created_by: r.get("created_by"),
            revoked_by: r.get("revoked_by"),
            rate_limit_min: r.get("rate_limit_min"),
            kall_i_dag: r.get("kall_i_dag"),
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
    pub navn: String,
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
            navn: r.get("navn"),
            method: r.get("method"),
            path: r.get("path"),
            status: r.get("status"),
            created_at: r.get("created_at"),
        })
        .collect())
}
