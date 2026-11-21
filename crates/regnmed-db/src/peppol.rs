//! Persistence for Peppol dispatches (docs/efaktura.md §3.3).
//!
//! Only workers call the writers here. An attempt is committed before the
//! provider is called, so a crash can never hide that a POST may have left.

use anyhow::{Result, bail};
use regnmed_core::peppol::{EventKind, attempt_status, own_event_key};
use sqlx::{PgPool, Row};
use uuid::Uuid;

/// How long an unresolved attempt must age before a human may resend it,
/// so a request still in flight has time to appear at the provider.
pub const UNRESOLVED_RESEND_DELAY_SECS: f64 = 30.0 * 60.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResendPolicy {
    /// Only after proof that the previous attempt did not go out.
    SafeOnly,
    /// A human's explicit resend of an unresolved attempt, after the delay
    /// and after the worker's hash check found no provider document.
    AllowAgedUnresolved,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BeginAttempt {
    Started(i32),
    /// The latest attempt may have gone out, so nothing may be posted.
    Blocked {
        attempt: i32,
        status: EventKind,
    },
}

pub async fn create_dispatch(
    pool: &PgPool,
    company_id: Uuid,
    invoice_id: Uuid,
    receiver: &str,
    attachment_id: Uuid,
    subject: &str,
    sent_by: &str,
) -> Result<Uuid> {
    let id = Uuid::now_v7();
    sqlx::query(
        "insert into utsendelse
             (id, company_id, invoice_id, channel, peppol_receiver, attachment_id, subject, sent_by)
         values ($1, $2, $3, 'peppol', $4, $5, $6, $7)",
    )
    .bind(id)
    .bind(company_id)
    .bind(invoice_id)
    .bind(receiver)
    .bind(attachment_id)
    .bind(subject)
    .bind(sent_by)
    .execute(pool)
    .await?;
    Ok(id)
}

/// Commits the next `sendeforsok`, unless the latest attempt may have
/// reached the provider. Serialized per dispatch with an advisory lock,
/// because the app role has no UPDATE right to take a row lock here.
pub async fn begin_attempt(
    pool: &PgPool,
    dispatch_id: Uuid,
    policy: ResendPolicy,
) -> Result<BeginAttempt> {
    let mut tx = pool.begin().await?;
    sqlx::query("select pg_advisory_xact_lock(hashtextextended($1::text, 0))")
        .bind(dispatch_id)
        .execute(&mut *tx)
        .await?;
    let row = sqlx::query(
        "select coalesce(max(attempt), 0) as attempt from dispatch_event where dispatch_id = $1",
    )
    .bind(dispatch_id)
    .fetch_one(&mut *tx)
    .await?;
    let latest: i32 = row.get("attempt");
    if latest > 0 {
        let rows = sqlx::query(
            "select kind,
                    received_at <= now() - make_interval(secs => $3) as aged
             from dispatch_event where dispatch_id = $1 and attempt = $2",
        )
        .bind(dispatch_id)
        .bind(latest)
        .bind(UNRESOLVED_RESEND_DELAY_SECS)
        .fetch_all(&mut *tx)
        .await?;
        let kinds = parse_kinds(rows.iter().map(|r| r.get::<String, _>("kind")))?;
        let Some(status) = attempt_status(kinds.iter().copied()) else {
            bail!("dispatch {dispatch_id} attempt {latest} has no transport event");
        };
        let unresolved_aged = matches!(status, EventKind::Unresolved | EventKind::DeliveryFailed)
            && rows.iter().all(|r| r.get::<bool, _>("aged"));
        let allowed = status.is_safe_to_retry()
            || (policy == ResendPolicy::AllowAgedUnresolved && unresolved_aged);
        if !allowed {
            return Ok(BeginAttempt::Blocked {
                attempt: latest,
                status,
            });
        }
    }
    let attempt = latest + 1;
    insert_event(
        &mut tx,
        dispatch_id,
        attempt,
        EventKind::SendAttempt,
        &own_event_key(dispatch_id, attempt, EventKind::SendAttempt),
        "",
    )
    .await?;
    tx.commit().await?;
    Ok(BeginAttempt::Started(attempt))
}

/// Records an event once. Returns false when its key was already there.
pub async fn record_event(
    pool: &PgPool,
    dispatch_id: Uuid,
    attempt: i32,
    kind: EventKind,
    event_key: &str,
    detail: &str,
) -> Result<bool> {
    let mut tx = pool.begin().await?;
    let inserted = insert_event(&mut tx, dispatch_id, attempt, kind, event_key, detail).await?;
    tx.commit().await?;
    Ok(inserted)
}

async fn insert_event(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    dispatch_id: Uuid,
    attempt: i32,
    kind: EventKind,
    event_key: &str,
    detail: &str,
) -> Result<bool> {
    let result = sqlx::query(
        "insert into dispatch_event (id, dispatch_id, event_key, attempt, kind, detail)
         values ($1, $2, $3, $4, $5, $6)
         on conflict (event_key) do nothing",
    )
    .bind(Uuid::now_v7())
    .bind(dispatch_id)
    .bind(event_key)
    .bind(attempt)
    .bind(kind.as_str())
    .bind(detail)
    .execute(&mut **tx)
    .await?;
    Ok(result.rows_affected() == 1)
}

/// Links a provider document to an attempt. Refuses to move a document
/// or to give one attempt two documents: either would hide a duplicate.
pub async fn link_provider_document(
    pool: &PgPool,
    provider_document_id: &str,
    dispatch_id: Uuid,
    attempt: i32,
) -> Result<()> {
    sqlx::query(
        "insert into dispatch_provider_document (provider_document_id, dispatch_id, attempt)
         values ($1, $2, $3) on conflict do nothing",
    )
    .bind(provider_document_id)
    .bind(dispatch_id)
    .bind(attempt)
    .execute(pool)
    .await?;
    let linked: Option<(Uuid, i32)> = sqlx::query_as(
        "select dispatch_id, attempt from dispatch_provider_document
         where provider_document_id = $1",
    )
    .bind(provider_document_id)
    .fetch_optional(pool)
    .await?;
    if linked != Some((dispatch_id, attempt)) {
        bail!("provider document {provider_document_id} conflicts with attempt {attempt}");
    }
    Ok(())
}

/// The computed status of a dispatch's latest attempt.
pub async fn latest_status(pool: &PgPool, dispatch_id: Uuid) -> Result<Option<(i32, EventKind)>> {
    let rows = sqlx::query(
        "select attempt, kind from dispatch_event
         where dispatch_id = $1
           and attempt = (select max(attempt) from dispatch_event where dispatch_id = $1)",
    )
    .bind(dispatch_id)
    .fetch_all(pool)
    .await?;
    let Some(attempt) = rows.first().map(|r| r.get::<i32, _>("attempt")) else {
        return Ok(None);
    };
    let kinds = parse_kinds(rows.iter().map(|r| r.get::<String, _>("kind")))?;
    Ok(attempt_status(kinds).map(|status| (attempt, status)))
}

#[derive(Debug, Clone)]
pub struct PeppolDispatch {
    pub id: Uuid,
    pub receiver: String,
    pub attachment_id: Uuid,
}

#[derive(Debug, Clone)]
pub struct PeppolSender {
    pub provider_company_id: String,
    pub is_verified: bool,
}

/// The invoice's Peppol dispatch, if it has one.
pub async fn latest_dispatch(
    pool: &PgPool,
    company_id: Uuid,
    invoice_id: Uuid,
) -> Result<Option<PeppolDispatch>> {
    let row = sqlx::query(
        "select id, peppol_receiver, attachment_id from utsendelse
         where company_id = $1 and invoice_id = $2 and channel = 'peppol'
         order by created_at desc, id desc limit 1",
    )
    .bind(company_id)
    .bind(invoice_id)
    .fetch_optional(pool)
    .await?;
    Ok(row.map(|r| PeppolDispatch {
        id: r.get("id"),
        receiver: r.get("peppol_receiver"),
        attachment_id: r.get("attachment_id"),
    }))
}

/// One Peppol dispatch per invoice, also when two workers arrive at once.
pub async fn find_or_create_dispatch(
    pool: &PgPool,
    company_id: Uuid,
    invoice_id: Uuid,
    receiver: &str,
    attachment_id: Uuid,
    subject: &str,
    sent_by: &str,
) -> Result<PeppolDispatch> {
    let mut tx = pool.begin().await?;
    sqlx::query("select pg_advisory_xact_lock(hashtextextended($1::text, 1))")
        .bind(invoice_id)
        .execute(&mut *tx)
        .await?;
    let existing = sqlx::query(
        "select id, peppol_receiver, attachment_id from utsendelse
         where company_id = $1 and invoice_id = $2 and channel = 'peppol'
         order by created_at desc, id desc limit 1",
    )
    .bind(company_id)
    .bind(invoice_id)
    .fetch_optional(&mut *tx)
    .await?;
    if let Some(r) = existing {
        return Ok(PeppolDispatch {
            id: r.get("id"),
            receiver: r.get("peppol_receiver"),
            attachment_id: r.get("attachment_id"),
        });
    }
    let id = Uuid::now_v7();
    sqlx::query(
        "insert into utsendelse
             (id, company_id, invoice_id, channel, peppol_receiver, attachment_id, subject, sent_by)
         values ($1, $2, $3, 'peppol', $4, $5, $6, $7)",
    )
    .bind(id)
    .bind(company_id)
    .bind(invoice_id)
    .bind(receiver)
    .bind(attachment_id)
    .bind(subject)
    .bind(sent_by)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(PeppolDispatch {
        id,
        receiver: receiver.to_string(),
        attachment_id,
    })
}

pub async fn first_attempt_at(
    pool: &PgPool,
    dispatch_id: Uuid,
) -> Result<Option<chrono::DateTime<chrono::Utc>>> {
    Ok(
        sqlx::query_scalar("select min(received_at) from dispatch_event where dispatch_id = $1")
            .bind(dispatch_id)
            .fetch_one(pool)
            .await?,
    )
}

pub async fn attempt_has_document(pool: &PgPool, dispatch_id: Uuid, attempt: i32) -> Result<bool> {
    Ok(sqlx::query_scalar(
        "select exists(select 1 from dispatch_provider_document
                       where dispatch_id = $1 and attempt = $2)",
    )
    .bind(dispatch_id)
    .bind(attempt)
    .fetch_one(pool)
    .await?)
}

pub async fn linked_documents(pool: &PgPool, dispatch_id: Uuid) -> Result<Vec<String>> {
    Ok(sqlx::query_scalar(
        "select provider_document_id from dispatch_provider_document where dispatch_id = $1",
    )
    .bind(dispatch_id)
    .fetch_all(pool)
    .await?)
}

pub async fn load_sender(pool: &PgPool, company_id: Uuid) -> Result<Option<PeppolSender>> {
    let row = sqlx::query(
        "select provider_company_id, is_verified from peppol_sender where company_id = $1",
    )
    .bind(company_id)
    .fetch_optional(pool)
    .await?;
    Ok(row.map(|r| PeppolSender {
        provider_company_id: r.get("provider_company_id"),
        is_verified: r.get("is_verified"),
    }))
}

pub async fn insert_sender(
    pool: &PgPool,
    company_id: Uuid,
    provider_company_id: &str,
    is_verified: bool,
) -> Result<()> {
    sqlx::query(
        "insert into peppol_sender (company_id, provider_company_id, is_verified)
         values ($1, $2, $3)",
    )
    .bind(company_id)
    .bind(provider_company_id)
    .bind(is_verified)
    .execute(pool)
    .await?;
    Ok(())
}

/// Applies `company.verification`. An unknown company is not ours.
pub async fn set_verified_by_provider_id(
    pool: &PgPool,
    provider_company_id: &str,
    verified: bool,
) -> Result<()> {
    sqlx::query(
        "update peppol_sender set is_verified = $2, updated_at = now()
         where provider_company_id = $1",
    )
    .bind(provider_company_id)
    .bind(verified)
    .execute(pool)
    .await?;
    Ok(())
}

/// Which dispatch and attempt a provider document belongs to.
pub async fn linked_attempt(
    pool: &PgPool,
    provider_document_id: &str,
) -> Result<Option<(Uuid, i32)>> {
    Ok(sqlx::query_as(
        "select dispatch_id, attempt from dispatch_provider_document
         where provider_document_id = $1",
    )
    .bind(provider_document_id)
    .fetch_optional(pool)
    .await?)
}

#[derive(Debug, Clone)]
pub struct OpenDispatch {
    pub id: Uuid,
    pub company_id: Uuid,
    pub attachment_id: Uuid,
    pub attempt: i32,
    pub status: EventKind,
    pub attempt_started: chrono::DateTime<chrono::Utc>,
    pub document_id: Option<String>,
}

/// One page of recent dispatches that reconciliation can still act on, in
/// id order. Paging by id keeps old stuck rows from starving newer ones.
pub async fn open_dispatches(
    pool: &PgPool,
    after: Option<Uuid>,
    limit: i64,
) -> Result<Vec<OpenDispatch>> {
    let rows = sqlx::query(
        "with latest as (
             select dispatch_id, max(attempt) as attempt from dispatch_event group by dispatch_id
         )
         select u.id, u.company_id, u.attachment_id, l.attempt,
                array_agg(e.kind) as kinds, min(e.received_at) as attempt_started,
                d.provider_document_id
         from latest l
         join utsendelse u on u.id = l.dispatch_id
         join dispatch_event e on e.dispatch_id = l.dispatch_id and e.attempt = l.attempt
         left join dispatch_provider_document d
                on d.dispatch_id = l.dispatch_id and d.attempt = l.attempt
         where u.channel = 'peppol' and u.created_at > now() - interval '14 days'
           and ($1::uuid is null or u.id > $1)
         group by u.id, u.company_id, u.attachment_id, l.attempt, d.provider_document_id
         having not bool_or(e.kind in ('levert', 'avvist', 'ikke_sendt', 'leveringsfeil'))
            -- Without a document, an old attempt only needs marking unresolved.
            and (d.provider_document_id is not null
                 or min(e.received_at) > now() - interval '24 hours'
                 or not bool_or(e.kind = 'uavklart'))
         order by u.id
         limit $2",
    )
    .bind(after)
    .bind(limit)
    .fetch_all(pool)
    .await?;
    let mut open = Vec::with_capacity(rows.len());
    for row in rows {
        let kinds = parse_kinds(row.get::<Vec<String>, _>("kinds").into_iter())?;
        let Some(status) = attempt_status(kinds) else {
            continue;
        };
        open.push(OpenDispatch {
            id: row.get("id"),
            company_id: row.get("company_id"),
            attachment_id: row.get("attachment_id"),
            attempt: row.get("attempt"),
            status,
            attempt_started: row.get("attempt_started"),
            document_id: row.get("provider_document_id"),
        });
    }
    Ok(open)
}

/// A dispatch's latest attempt as the portal shows it.
#[derive(Debug, Clone)]
pub struct DispatchSummary {
    pub attempt: i32,
    pub status: EventKind,
    pub detail: String,
    /// Whether a human may ask for a new attempt now.
    pub can_resend: bool,
}

pub async fn dispatch_summary(pool: &PgPool, dispatch_id: Uuid) -> Result<Option<DispatchSummary>> {
    let rows = sqlx::query(
        "select attempt, kind, detail,
                received_at <= now() - make_interval(secs => $2) as aged
         from dispatch_event
         where dispatch_id = $1
           and attempt = (select max(attempt) from dispatch_event where dispatch_id = $1)",
    )
    .bind(dispatch_id)
    .bind(UNRESOLVED_RESEND_DELAY_SECS)
    .fetch_all(pool)
    .await?;
    let Some(attempt) = rows.first().map(|r| r.get::<i32, _>("attempt")) else {
        return Ok(None);
    };
    let kinds = parse_kinds(rows.iter().map(|r| r.get::<String, _>("kind")))?;
    let Some(status) = attempt_status(kinds) else {
        return Ok(None);
    };
    let detail = rows
        .iter()
        .find(|r| r.get::<String, _>("kind") == status.as_str())
        .map(|r| r.get::<String, _>("detail"))
        .unwrap_or_default();
    let aged = rows.iter().all(|r| r.get::<bool, _>("aged"));
    let can_resend = status.is_safe_to_retry()
        || (aged && matches!(status, EventKind::Unresolved | EventKind::DeliveryFailed));
    Ok(Some(DispatchSummary {
        attempt,
        status,
        detail,
        can_resend,
    }))
}

pub async fn set_sender_verified(pool: &PgPool, company_id: Uuid, verified: bool) -> Result<()> {
    sqlx::query(
        "update peppol_sender set is_verified = $2, updated_at = now() where company_id = $1",
    )
    .bind(company_id)
    .bind(verified)
    .execute(pool)
    .await?;
    Ok(())
}

fn parse_kinds(values: impl Iterator<Item = String>) -> Result<Vec<EventKind>> {
    values
        .map(|value| {
            EventKind::parse(&value)
                .ok_or_else(|| anyhow::anyhow!("unknown dispatch event kind {value}"))
        })
        .collect()
}
