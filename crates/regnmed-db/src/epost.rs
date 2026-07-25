//! E-post-inn til bilagsinnboksen (docs/epost-inn.md, #35).
//!
//! Vedleggene i en mottatt e-post blir innboksdokumenter gjennom
//! nøyaktig samme vei som en opplasting: uforanderlig innhold,
//! SHA-256 ved ankomst, ingen beslutning tatt (migration 0015).
//!
//! Det som er nytt er hvem som får levere. En ukjent avsender havner i
//! **karantene**: e-posten lagres hel (rå melding og alt), men ingen
//! dokumenter opprettes før en admin slipper den gjennom. Alternativene
//! var å importere i stillhet (da kan hvem som helst fylle innboksen)
//! eller å forkaste i stillhet (da forsvinner et bilag noen faktisk
//! sendte) — begge er verre.

use anyhow::{Context, Result, ensure};
use chrono::{DateTime, Utc};
use regnmed_core::epost::{avsender_tillatt, local_part, normaliser_avsender};
use sqlx::{PgPool, Row};
use uuid::Uuid;

/// One attachment, already decoded by the caller — the db layer stores
/// bytes, it does not parse wire formats.
#[derive(Debug, Clone)]
pub struct MottattVedlegg {
    pub filename: String,
    pub content_type: String,
    pub content: Vec<u8>,
}

/// An inbound message as the mail rail delivered it. Decoding the wire
/// shape belongs to the caller (regnmed-api::mailq_in); what reaches
/// here is already bytes and text.
#[derive(Debug, Clone)]
pub struct MottattEpost {
    /// The full recipient address the mail was delivered to.
    pub to: String,
    pub from: String,
    pub subject: Option<String>,
    pub text: Option<String>,
    pub message_id: String,
    /// The message as received, stored verbatim as documentation of
    /// origin. Opaque here — never parsed by this layer.
    pub raw: Vec<u8>,
    pub attachments: Vec<MottattVedlegg>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Mottak {
    /// Documents were created (count).
    Mottatt(usize),
    /// Held for an admin to decide.
    Karantene,
    /// Already seen — the same message delivered twice.
    Duplikat,
    /// Nothing usable in the mail; recorded, never silently dropped.
    Avvist(String),
}

/// The company's active inbound local-part, if it has one.
pub async fn mail_address(pool: &PgPool, company_id: Uuid) -> Result<Option<String>> {
    Ok(sqlx::query_scalar(
        "select local_part from company_mail_inbox
         where company_id = $1 and active order by created_at desc limit 1",
    )
    .bind(company_id)
    .fetch_optional(pool)
    .await?)
}

/// Creates a fresh address, revoking the previous one. Rotation is how
/// a leaked address is handled — the old one stops working the moment
/// the new one exists.
pub async fn rotate_mail_address(
    pool: &PgPool,
    company_id: Uuid,
    created_by: &str,
) -> Result<String> {
    let navn: String = sqlx::query_scalar("select name from company where id = $1")
        .bind(company_id)
        .fetch_optional(pool)
        .await?
        .context("no such company")?;
    let mut tx = pool.begin().await?;
    sqlx::query(
        "update company_mail_inbox set active = false, revoked_at = now()
         where company_id = $1 and active",
    )
    .bind(company_id)
    .execute(&mut *tx)
    .await?;
    // The unguessable tail comes from a v4 UUID's randomness.
    let tail = Uuid::new_v4().simple().to_string();
    let local = local_part(&navn, &tail);
    sqlx::query(
        "insert into company_mail_inbox (id, company_id, local_part, created_by)
         values ($1, $2, $3, $4)",
    )
    .bind(Uuid::now_v7())
    .bind(company_id)
    .bind(&local)
    .bind(created_by)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(local)
}

#[derive(Debug)]
pub struct AllowRow {
    pub id: Uuid,
    pub sender: String,
    pub note: Option<String>,
    pub created_by: String,
}

pub async fn list_allowed_senders(pool: &PgPool, company_id: Uuid) -> Result<Vec<AllowRow>> {
    let rows = sqlx::query(
        "select id, sender, note, created_by from mail_sender_allow
         where company_id = $1 and active order by sender",
    )
    .bind(company_id)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .iter()
        .map(|r| AllowRow {
            id: r.get("id"),
            sender: r.get("sender"),
            note: r.get("note"),
            created_by: r.get("created_by"),
        })
        .collect())
}

pub async fn allow_sender(
    pool: &PgPool,
    company_id: Uuid,
    sender: &str,
    note: Option<&str>,
    created_by: &str,
) -> Result<()> {
    let sender = normaliser_avsender(sender);
    ensure!(
        sender.contains('@') && !sender.starts_with("@@"),
        "oppgi en e-postadresse (post@firma.no) eller et domene (@firma.no)"
    );
    sqlx::query(
        "insert into mail_sender_allow (id, company_id, sender, note, created_by)
         values ($1, $2, $3, $4, $5)
         on conflict (company_id, sender) where active do nothing",
    )
    .bind(Uuid::now_v7())
    .bind(company_id)
    .bind(&sender)
    .bind(note.map(str::trim).filter(|n| !n.is_empty()))
    .bind(created_by)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn revoke_sender(pool: &PgPool, company_id: Uuid, id: Uuid) -> Result<()> {
    let updated = sqlx::query(
        "update mail_sender_allow set active = false where id = $1 and company_id = $2 and active",
    )
    .bind(id)
    .bind(company_id)
    .execute(pool)
    .await?
    .rows_affected();
    ensure!(updated == 1, "no such sender entry");
    Ok(())
}

/// Routes and stores an inbound mail. All-or-nothing per message: the
/// log row and the documents it produced land in one transaction.
pub async fn receive_mail(pool: &PgPool, mail: &MottattEpost) -> Result<Mottak> {
    let local = mail
        .to
        .rsplit('<')
        .next()
        .unwrap_or(&mail.to)
        .trim_end_matches('>')
        .split('@')
        .next()
        .unwrap_or_default()
        .trim()
        .to_lowercase();
    let company_id: Uuid = sqlx::query_scalar(
        "select company_id from company_mail_inbox where local_part = $1 and active",
    )
    .bind(&local)
    .fetch_optional(pool)
    .await?
    .with_context(|| format!("ingen aktiv mottaksadresse for «{local}»"))?;

    let from = normaliser_avsender(&mail.from);
    let allowed: Vec<String> =
        sqlx::query_scalar("select sender from mail_sender_allow where company_id = $1 and active")
            .bind(company_id)
            .fetch_all(pool)
            .await?;
    let tillatt = avsender_tillatt(&from, &allowed);

    let vedlegg: Vec<&MottattVedlegg> = mail
        .attachments
        .iter()
        .filter(|a| !a.content.is_empty())
        .collect();

    let status = if vedlegg.is_empty() {
        "avvist"
    } else if tillatt {
        "mottatt"
    } else {
        "karantene"
    };
    let note = match status {
        "avvist" => Some("e-posten hadde ingen vedlegg å bokføre".to_string()),
        "karantene" => Some(format!(
            "{from} står ikke på selskapets avsenderliste — venter på godkjenning"
        )),
        _ => None,
    };

    let mut tx = pool.begin().await?;
    let mail_id = Uuid::now_v7();
    let inserted = sqlx::query(
        "insert into inbox_mail (id, company_id, message_id, from_address, subject, body,
                                 raw, antall_vedlegg, status, note)
         values ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
         on conflict (company_id, message_id) do nothing",
    )
    .bind(mail_id)
    .bind(company_id)
    .bind(&mail.message_id)
    .bind(&from)
    .bind(&mail.subject)
    .bind(&mail.text)
    .bind(&mail.raw)
    .bind(vedlegg.len() as i32)
    .bind(status)
    .bind(&note)
    .execute(&mut *tx)
    .await?
    .rows_affected();
    if inserted == 0 {
        // The same Message-Id already arrived; mail queues retry.
        return Ok(Mottak::Duplikat);
    }

    // The attachments are stored decoded either way: quarantine must be
    // releasable later without the sender resending anything.
    for a in &vedlegg {
        let digest = regnmed_core::hash::sha256(&a.content);
        sqlx::query(
            "insert into inbox_mail_attachment (id, mail_id, filename, content_type,
                                                byte_size, sha256, content)
             values ($1, $2, $3, $4, $5, $6, $7)",
        )
        .bind(Uuid::now_v7())
        .bind(mail_id)
        .bind(&a.filename)
        .bind(&a.content_type)
        .bind(a.content.len() as i64)
        .bind(digest.as_slice())
        .bind(&a.content)
        .execute(&mut *tx)
        .await?;
        if status == "mottatt" {
            insert_document(
                &mut tx,
                company_id,
                mail_id,
                &from,
                &a.filename,
                &a.content_type,
                &a.content,
            )
            .await?;
        }
    }
    tx.commit().await?;

    Ok(match status {
        "mottatt" => Mottak::Mottatt(vedlegg.len()),
        "karantene" => Mottak::Karantene,
        _ => Mottak::Avvist(note.unwrap_or_default()),
    })
}

async fn insert_document(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    company_id: Uuid,
    mail_id: Uuid,
    from: &str,
    filename: &str,
    content_type: &str,
    bytes: &[u8],
) -> Result<()> {
    let digest = regnmed_core::hash::sha256(bytes);
    sqlx::query(
        "insert into inbox_document (id, company_id, filename, content_type, byte_size,
                                     sha256, content, uploaded_by, inbox_mail_id)
         values ($1, $2, $3, $4, $5, $6, $7, $8, $9)",
    )
    .bind(Uuid::now_v7())
    .bind(company_id)
    .bind(filename)
    .bind(content_type)
    .bind(bytes.len() as i64)
    .bind(digest.as_slice())
    .bind(bytes)
    // The sender's address IS the uploader — that is who handed us the
    // document, and the innboks shows it.
    .bind(from)
    .bind(mail_id)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

#[derive(Debug)]
pub struct MailRow {
    pub id: Uuid,
    pub message_id: String,
    pub from_address: String,
    pub subject: Option<String>,
    pub body: Option<String>,
    pub antall_vedlegg: i32,
    pub received_at: DateTime<Utc>,
    pub status: String,
    pub note: Option<String>,
    pub decided_by: Option<String>,
}

pub async fn list_mail(
    pool: &PgPool,
    company_id: Uuid,
    status: Option<&str>,
) -> Result<Vec<MailRow>> {
    let rows = sqlx::query(
        "select id, message_id, from_address, subject, body, antall_vedlegg, received_at,
                status, note, decided_by
         from inbox_mail
         where company_id = $1 and ($2::text is null or status = $2)
         order by received_at desc limit 200",
    )
    .bind(company_id)
    .bind(status)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .iter()
        .map(|r| MailRow {
            id: r.get("id"),
            message_id: r.get("message_id"),
            from_address: r.get("from_address"),
            subject: r.get("subject"),
            body: r.get("body"),
            antall_vedlegg: r.get("antall_vedlegg"),
            received_at: r.get("received_at"),
            status: r.get("status"),
            note: r.get("note"),
            decided_by: r.get("decided_by"),
        })
        .collect())
}

/// Releases a quarantined mail: its attachments become inbox documents
/// now, in one transaction with the status change. Optionally adds the
/// sender to the allow-list so the next mail goes straight through.
pub async fn release_mail(
    pool: &PgPool,
    company_id: Uuid,
    mail_id: Uuid,
    tillat_avsender: bool,
    decided_by: &str,
) -> Result<usize> {
    let row = sqlx::query(
        "select from_address, status from inbox_mail
         where id = $1 and company_id = $2 for update",
    )
    .bind(mail_id)
    .bind(company_id)
    .fetch_optional(pool)
    .await?
    .context("no such mail")?;
    let status: String = row.get("status");
    ensure!(status == "karantene", "e-posten er allerede {status}");
    let from: String = row.get("from_address");

    let attachments = sqlx::query(
        "select filename, content_type, content, sha256 from inbox_mail_attachment
         where mail_id = $1 order by id",
    )
    .bind(mail_id)
    .fetch_all(pool)
    .await?;
    ensure!(
        !attachments.is_empty(),
        "e-posten har ingen vedlegg å slippe gjennom"
    );

    let mut tx = pool.begin().await?;
    let mut antall = 0usize;
    for a in &attachments {
        let content: Vec<u8> = a.get("content");
        let stored: Vec<u8> = a.get("sha256");
        ensure!(
            regnmed_core::hash::sha256(&content).as_slice() == stored.as_slice(),
            "vedlegget feilet hash-sjekken"
        );
        insert_document(
            &mut tx,
            company_id,
            mail_id,
            &from,
            &a.get::<String, _>("filename"),
            &a.get::<String, _>("content_type"),
            &content,
        )
        .await?;
        antall += 1;
    }
    sqlx::query(
        "update inbox_mail set status = 'mottatt', decided_by = $3, decided_at = now()
         where id = $1 and company_id = $2",
    )
    .bind(mail_id)
    .bind(company_id)
    .bind(decided_by)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;

    if tillat_avsender {
        allow_sender(
            pool,
            company_id,
            &from,
            Some("godkjent fra karantene"),
            decided_by,
        )
        .await?;
    }
    Ok(antall)
}

/// Rejects a quarantined mail. The row stays — with who decided and
/// why — so nothing a supplier sent ever vanishes without a trace.
pub async fn reject_mail(
    pool: &PgPool,
    company_id: Uuid,
    mail_id: Uuid,
    note: &str,
    decided_by: &str,
) -> Result<()> {
    ensure!(
        !note.trim().is_empty(),
        "en avvisning krever en begrunnelse"
    );
    let updated = sqlx::query(
        "update inbox_mail set status = 'avvist', note = $3, decided_by = $4, decided_at = now()
         where id = $1 and company_id = $2 and status = 'karantene'",
    )
    .bind(mail_id)
    .bind(company_id)
    .bind(note.trim())
    .bind(decided_by)
    .execute(pool)
    .await?
    .rows_affected();
    ensure!(updated == 1, "ingen e-post i karantene med den id-en");
    Ok(())
}
