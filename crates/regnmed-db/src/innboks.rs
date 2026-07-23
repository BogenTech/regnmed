//! Bilagsinnboks: uploaded dokumentasjon waiting to become vouchers.
//!
//! The key operation is [`bokfor_inbox_document`], which is atomic in
//! one transaction: post the voucher through the normal posting path,
//! copy the document into the append-only `attachment` table (bound to
//! the new voucher — oppbevaringsplikt starts here), and mark the inbox
//! entry 'bokfort'. Either everything lands or nothing does; a document
//! can never end up "posted" without its voucher, or attached twice.

use anyhow::{Context, Result, bail, ensure};
use chrono::{DateTime, Utc};
use regnmed_core::hash::sha256;
use regnmed_core::voucher::VoucherDraft;
use sqlx::{PgPool, Row};
use uuid::Uuid;

use crate::ledger::{PostedVoucher, post_voucher_in};

#[derive(Debug)]
pub struct InboxRow {
    pub id: Uuid,
    pub filename: String,
    pub content_type: String,
    pub byte_size: i64,
    pub sha256_hex: String,
    pub uploaded_by: String,
    pub created_at: DateTime<Utc>,
    pub status: String,
    pub voucher_id: Option<Uuid>,
    pub decided_by: Option<String>,
    pub note: Option<String>,
}

pub async fn upload_inbox_document(
    pool: &PgPool,
    company_id: Uuid,
    filename: &str,
    content_type: &str,
    content: &[u8],
    uploaded_by: &str,
) -> Result<Uuid> {
    ensure!(!content.is_empty(), "document is empty");
    let digest = sha256(content);
    let id = Uuid::now_v7();
    sqlx::query(
        "insert into inbox_document (id, company_id, filename, content_type, byte_size,
                                     sha256, content, uploaded_by)
         values ($1, $2, $3, $4, $5, $6, $7, $8)",
    )
    .bind(id)
    .bind(company_id)
    .bind(filename)
    .bind(content_type)
    .bind(content.len() as i64)
    .bind(digest.as_slice())
    .bind(content)
    .bind(uploaded_by)
    .execute(pool)
    .await?;
    Ok(id)
}

pub async fn list_inbox(
    pool: &PgPool,
    company_id: Uuid,
    status: Option<&str>,
) -> Result<Vec<InboxRow>> {
    let rows = sqlx::query(
        "select id, filename, content_type, byte_size, sha256, uploaded_by, created_at,
                status, voucher_id, decided_by, note
         from inbox_document
         where company_id = $1 and ($2::text is null or status = $2)
         order by created_at desc",
    )
    .bind(company_id)
    .bind(status)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .iter()
        .map(|r| InboxRow {
            id: r.get("id"),
            filename: r.get("filename"),
            content_type: r.get("content_type"),
            byte_size: r.get("byte_size"),
            sha256_hex: hex::encode(r.get::<Vec<u8>, _>("sha256")),
            uploaded_by: r.get("uploaded_by"),
            created_at: r.get("created_at"),
            status: r.get("status"),
            voucher_id: r.get("voucher_id"),
            decided_by: r.get("decided_by"),
            note: r.get("note"),
        })
        .collect())
}

/// Content download, hash-checked on the way out like attachments.
pub async fn get_inbox_document(
    pool: &PgPool,
    company_id: Uuid,
    document_id: Uuid,
) -> Result<(String, String, Vec<u8>)> {
    let row = sqlx::query(
        "select filename, content_type, sha256, content
         from inbox_document where id = $1 and company_id = $2",
    )
    .bind(document_id)
    .bind(company_id)
    .fetch_optional(pool)
    .await?
    .context("no such inbox document")?;
    let content: Vec<u8> = row.get("content");
    let stored: Vec<u8> = row.get("sha256");
    if sha256(&content).as_slice() != stored.as_slice() {
        bail!("inbox document {document_id} failed its hash check");
    }
    Ok((row.get("filename"), row.get("content_type"), content))
}

/// Post the voucher, attach the document to it, mark the entry bokført —
/// one transaction, all or nothing.
pub async fn bokfor_inbox_document(
    pool: &PgPool,
    company_id: Uuid,
    document_id: Uuid,
    draft: &VoucherDraft,
    decided_by: &str,
) -> Result<PostedVoucher> {
    let mut tx = pool.begin().await?;

    let document = sqlx::query(
        "select filename, content_type, sha256, content, status
         from inbox_document where id = $1 and company_id = $2 for update",
    )
    .bind(document_id)
    .bind(company_id)
    .fetch_optional(&mut *tx)
    .await?
    .context("no such inbox document")?;
    let status: String = document.get("status");
    ensure!(status == "ny", "document is already decided ({status})");

    let posted = post_voucher_in(&mut tx, company_id, draft, decided_by).await?;

    let content: Vec<u8> = document.get("content");
    sqlx::query(
        "insert into attachment (id, company_id, voucher_id, filename, content_type,
                                 byte_size, sha256, content, uploaded_by)
         values ($1, $2, $3, $4, $5, $6, $7, $8, $9)",
    )
    .bind(Uuid::now_v7())
    .bind(company_id)
    .bind(posted.id)
    .bind(document.get::<String, _>("filename"))
    .bind(document.get::<String, _>("content_type"))
    .bind(content.len() as i64)
    .bind(document.get::<Vec<u8>, _>("sha256"))
    .bind(&content)
    .bind(decided_by)
    .execute(&mut *tx)
    .await?;

    sqlx::query(
        "update inbox_document
         set status = 'bokfort', voucher_id = $3, decided_by = $4, decided_at = now()
         where id = $1 and company_id = $2",
    )
    .bind(document_id)
    .bind(company_id)
    .bind(posted.id)
    .bind(decided_by)
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;
    Ok(posted)
}

pub async fn avvis_inbox_document(
    pool: &PgPool,
    company_id: Uuid,
    document_id: Uuid,
    note: &str,
    decided_by: &str,
) -> Result<()> {
    ensure!(!note.trim().is_empty(), "a rejection needs a note");
    let updated = sqlx::query(
        "update inbox_document
         set status = 'avvist', decided_by = $4, decided_at = now(), note = $3
         where id = $1 and company_id = $2 and status = 'ny'",
    )
    .bind(document_id)
    .bind(company_id)
    .bind(note.trim())
    .bind(decided_by)
    .execute(pool)
    .await?
    .rows_affected();
    ensure!(updated == 1, "no undecided inbox document with that id");
    Ok(())
}
