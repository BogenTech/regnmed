//! The engagement (oppdrag) flow: directory of verified firms, requests,
//! decisions, lifecycle. Accepting a request opens an engagement — the
//! same first-class relationship the entire authorization model resolves
//! access through, so a fresh oppdrag takes effect on the accountant's
//! next request, no re-login.

use anyhow::{Context, Result, ensure};
use sqlx::{PgPool, Row};
use uuid::Uuid;

#[derive(Debug)]
pub struct DirectoryFirm {
    pub firm_id: Uuid,
    pub orgnr: String,
    pub name: String,
    pub kind: String,
    pub client_count: i64,
}

/// The public directory: only firms with verified autorisasjon, with an
/// honest signal of size (active engagements).
pub async fn list_verified_firms(pool: &PgPool, kind: Option<&str>) -> Result<Vec<DirectoryFirm>> {
    let rows = sqlx::query(
        "select f.id, f.orgnr, f.name, f.kind,
                (select count(*) from engagement e
                 where e.firm_id = f.id and e.valid_to is null)::bigint as client_count
         from firm f
         where f.autorisasjon_verified_at is not null
           and ($1::text is null or f.kind = $1)
         order by f.name",
    )
    .bind(kind)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .iter()
        .map(|r| DirectoryFirm {
            firm_id: r.get("id"),
            orgnr: r.get("orgnr"),
            name: r.get("name"),
            kind: r.get("kind"),
            client_count: r.get("client_count"),
        })
        .collect())
}

pub async fn is_firm_member(pool: &PgPool, person_id: Uuid, firm_id: Uuid) -> Result<bool> {
    Ok(sqlx::query_scalar(
        "select exists (select 1 from firm_member
         where firm_id = $1 and person_id = $2 and active)",
    )
    .bind(firm_id)
    .bind(person_id)
    .fetch_one(pool)
    .await?)
}

#[derive(Debug)]
pub struct MyFirm {
    pub firm_id: Uuid,
    pub name: String,
    pub kind: String,
    pub verified: bool,
    pub pending_requests: i64,
}

pub async fn my_firms(pool: &PgPool, person_id: Uuid) -> Result<Vec<MyFirm>> {
    let rows = sqlx::query(
        "select f.id, f.name, f.kind, (f.autorisasjon_verified_at is not null) as verified,
                (select count(*) from engagement_request r
                 where r.firm_id = f.id and r.status = 'pending')::bigint as pending_requests
         from firm f
         join firm_member m on m.firm_id = f.id and m.person_id = $1 and m.active
         order by f.name",
    )
    .bind(person_id)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .iter()
        .map(|r| MyFirm {
            firm_id: r.get("id"),
            name: r.get("name"),
            kind: r.get("kind"),
            verified: r.get("verified"),
            pending_requests: r.get("pending_requests"),
        })
        .collect())
}

/// A company asks a verified firm for an oppdrag. The kind is the firm's
/// kind — a revisor firm gives revisjon, a regnskapsfirma gives regnskap.
pub async fn request_engagement(
    pool: &PgPool,
    company_id: Uuid,
    firm_id: Uuid,
    message: Option<&str>,
    requested_by: Uuid,
) -> Result<Uuid> {
    let firm = sqlx::query(
        "select kind, autorisasjon_verified_at is not null as verified from firm where id = $1",
    )
    .bind(firm_id)
    .fetch_optional(pool)
    .await?
    .context("no such firm")?;
    ensure!(
        firm.get::<bool, _>("verified"),
        "firm's autorisasjon is not verified"
    );
    let kind: String = firm.get("kind");

    let open: bool = sqlx::query_scalar(
        "select exists (select 1 from engagement
         where firm_id = $1 and company_id = $2 and kind = $3 and valid_to is null)",
    )
    .bind(firm_id)
    .bind(company_id)
    .bind(&kind)
    .fetch_one(pool)
    .await?;
    ensure!(!open, "an active engagement of this kind already exists");

    let id = Uuid::now_v7();
    sqlx::query(
        "insert into engagement_request (id, firm_id, company_id, kind, message, requested_by)
         values ($1, $2, $3, $4, $5, $6)",
    )
    .bind(id)
    .bind(firm_id)
    .bind(company_id)
    .bind(&kind)
    .bind(message)
    .bind(requested_by)
    .execute(pool)
    .await
    .context("creating request (already pending?)")?;
    Ok(id)
}

#[derive(Debug)]
pub struct RequestRow {
    pub request_id: Uuid,
    pub company_name: String,
    pub company_orgnr: String,
    pub kind: String,
    pub message: Option<String>,
    pub status: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

pub async fn firm_requests(pool: &PgPool, firm_id: Uuid) -> Result<Vec<RequestRow>> {
    let rows = sqlx::query(
        "select r.id, c.name, c.orgnr, r.kind, r.message, r.status, r.created_at
         from engagement_request r
         join company c on c.id = r.company_id
         where r.firm_id = $1
         order by r.status = 'pending' desc, r.created_at desc
         limit 100",
    )
    .bind(firm_id)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .iter()
        .map(|r| RequestRow {
            request_id: r.get("id"),
            company_name: r.get("name"),
            company_orgnr: r.get("orgnr"),
            kind: r.get("kind"),
            message: r.get("message"),
            status: r.get("status"),
            created_at: r.get("created_at"),
        })
        .collect())
}

/// Firm-side decision. Accepting opens the engagement in the same
/// transaction as the status flip — a request can only be decided once.
pub async fn decide_request(
    pool: &PgPool,
    firm_id: Uuid,
    request_id: Uuid,
    decided_by: Uuid,
    accept: bool,
) -> Result<()> {
    let mut tx = pool.begin().await?;
    let request = sqlx::query(
        "update engagement_request
         set status = $3, decided_by = $4, decided_at = now()
         where id = $1 and firm_id = $2 and status = 'pending'
         returning company_id, kind",
    )
    .bind(request_id)
    .bind(firm_id)
    .bind(if accept { "accepted" } else { "declined" })
    .bind(decided_by)
    .fetch_optional(&mut *tx)
    .await?
    .context("no pending request with that id for this firm")?;

    if accept {
        sqlx::query(
            "insert into engagement (id, firm_id, company_id, kind) values ($1, $2, $3, $4)
             on conflict (firm_id, company_id, kind) where valid_to is null do nothing",
        )
        .bind(Uuid::now_v7())
        .bind(firm_id)
        .bind(request.get::<Uuid, _>("company_id"))
        .bind(request.get::<String, _>("kind"))
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    Ok(())
}

#[derive(Debug)]
pub struct EngagementRow {
    pub engagement_id: Uuid,
    pub firm_id: Uuid,
    pub firm_name: String,
    pub company_id: Uuid,
    pub company_name: String,
    pub kind: String,
    pub valid_from: chrono::NaiveDate,
    pub valid_to: Option<chrono::NaiveDate>,
}

pub async fn company_engagements(pool: &PgPool, company_id: Uuid) -> Result<Vec<EngagementRow>> {
    engagements(pool, "e.company_id = $1", company_id).await
}

pub async fn firm_clients(pool: &PgPool, firm_id: Uuid) -> Result<Vec<EngagementRow>> {
    engagements(pool, "e.firm_id = $1", firm_id).await
}

async fn engagements(pool: &PgPool, filter: &str, id: Uuid) -> Result<Vec<EngagementRow>> {
    let sql = format!(
        "select e.id, e.firm_id, f.name as firm_name, e.company_id, c.name as company_name,
                e.kind, e.valid_from, e.valid_to
         from engagement e
         join firm f on f.id = e.firm_id
         join company c on c.id = e.company_id
         where {filter}
         order by e.valid_to is not null, e.valid_from desc"
    );
    let rows = sqlx::query(&sql).bind(id).fetch_all(pool).await?;
    Ok(rows
        .iter()
        .map(|r| EngagementRow {
            engagement_id: r.get("id"),
            firm_id: r.get("firm_id"),
            firm_name: r.get("firm_name"),
            company_id: r.get("company_id"),
            company_name: r.get("company_name"),
            kind: r.get("kind"),
            valid_from: r.get("valid_from"),
            valid_to: r.get("valid_to"),
        })
        .collect())
}

/// Ends an active engagement (valid_to = today). `scope` restricts to
/// the caller's side: a company ends its own oppdrag, a firm its own.
pub async fn end_engagement(
    pool: &PgPool,
    engagement_id: Uuid,
    company_scope: Option<Uuid>,
    firm_scope: Option<Uuid>,
) -> Result<()> {
    let ended = sqlx::query(
        "update engagement set valid_to = current_date
         where id = $1 and valid_to is null
           and ($2::uuid is null or company_id = $2)
           and ($3::uuid is null or firm_id = $3)",
    )
    .bind(engagement_id)
    .bind(company_scope)
    .bind(firm_scope)
    .execute(pool)
    .await?;
    ensure!(ended.rows_affected() == 1, "no active engagement to end");
    Ok(())
}
