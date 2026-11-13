//! Bilagsvedlegg: dokumentasjon bound to vouchers, append-only like the
//! ledger (bokføringsloven §13 — oppbevaringsplikt). Content SHA-256 is
//! stored at upload and re-verified by `verify_attachments`, so a
//! swapped or altered document is detectable even if the database's
//! triggers were somehow bypassed.

use crate::blob::blobs;
use anyhow::{Context, Result, ensure};
use regnmed_core::hash::sha256;
use sqlx::{PgPool, Row};
use uuid::Uuid;

#[derive(Debug)]
pub struct AttachmentMeta {
    pub id: Uuid,
    pub voucher_id: Uuid,
    pub filename: String,
    pub content_type: String,
    pub byte_size: i64,
    pub sha256_hex: String,
    pub uploaded_by: String,
}

pub async fn add_attachment(
    pool: &PgPool,
    company_id: Uuid,
    voucher_id: Uuid,
    filename: &str,
    content_type: &str,
    content: &[u8],
    uploaded_by: &str,
) -> Result<AttachmentMeta> {
    let mut tx = pool.begin().await?;
    let meta = add_attachment_in(
        &mut tx,
        company_id,
        voucher_id,
        filename,
        content_type,
        content,
        uploaded_by,
    )
    .await?;
    tx.commit().await?;
    Ok(meta)
}

/// Transaction-taking variant, so document-generating flows (invoice
/// PDF at issue time) can make the attachment atomic with the posting.
pub async fn add_attachment_in(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    company_id: Uuid,
    voucher_id: Uuid,
    filename: &str,
    content_type: &str,
    content: &[u8],
    uploaded_by: &str,
) -> Result<AttachmentMeta> {
    let mut metas = add_attachments_in(
        tx,
        company_id,
        voucher_id,
        &[(filename, content_type, content)],
        uploaded_by,
    )
    .await?;
    Ok(metas.remove(0))
}

/// Several documents on one voucher in one transaction. The object-store
/// writes run concurrently: they happen while the caller's transaction
/// (for an invoice, the gap-free number counter) is held, so issuing a
/// PDF and an EHF costs one round trip, not two.
pub async fn add_attachments_in(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    company_id: Uuid,
    voucher_id: Uuid,
    documents: &[(&str, &str, &[u8])],
    uploaded_by: &str,
) -> Result<Vec<AttachmentMeta>> {
    ensure!(
        documents.iter().all(|(_, _, content)| !content.is_empty()),
        "attachment is empty"
    );
    let belongs: bool = sqlx::query_scalar(
        "select exists (select 1 from voucher where id = $1 and company_id = $2)",
    )
    .bind(voucher_id)
    .bind(company_id)
    .fetch_one(&mut **tx)
    .await?;
    ensure!(belongs, "no such voucher in this company");
    let in_rows = futures_util::future::try_join_all(
        documents
            .iter()
            .map(|(_, _, content)| blobs().store(content)),
    )
    .await?;
    let mut metas = Vec::with_capacity(documents.len());
    for (&(filename, content_type, content), in_row) in documents.iter().zip(in_rows) {
        let digest = sha256(content);
        let id = Uuid::now_v7();
        sqlx::query(
            "insert into attachment (id, company_id, voucher_id, filename, content_type,
                                     byte_size, sha256, content, uploaded_by)
             values ($1, $2, $3, $4, $5, $6, $7, $8, $9)",
        )
        .bind(id)
        .bind(company_id)
        .bind(voucher_id)
        .bind(filename)
        .bind(content_type)
        .bind(content.len() as i64)
        .bind(digest.as_slice())
        .bind(in_row)
        .bind(uploaded_by)
        .execute(&mut **tx)
        .await?;
        metas.push(AttachmentMeta {
            id,
            voucher_id,
            filename: filename.to_string(),
            content_type: content_type.to_string(),
            byte_size: content.len() as i64,
            sha256_hex: hex(&digest),
            uploaded_by: uploaded_by.to_string(),
        });
    }
    Ok(metas)
}

pub async fn list_attachments(
    pool: &PgPool,
    company_id: Uuid,
    voucher_id: Uuid,
) -> Result<Vec<AttachmentMeta>> {
    let rows = sqlx::query(
        "select id, voucher_id, filename, content_type, byte_size, sha256, uploaded_by
         from attachment where company_id = $1 and voucher_id = $2
         order by created_at",
    )
    .bind(company_id)
    .bind(voucher_id)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .iter()
        .map(|r| AttachmentMeta {
            id: r.get("id"),
            voucher_id: r.get("voucher_id"),
            filename: r.get("filename"),
            content_type: r.get("content_type"),
            byte_size: r.get("byte_size"),
            sha256_hex: hex(&r.get::<Vec<u8>, _>("sha256")),
            uploaded_by: r.get("uploaded_by"),
        })
        .collect())
}

/// Content download; the hash is re-checked on the way out, so a client
/// can never receive silently altered dokumentasjon.
pub async fn get_attachment(
    pool: &PgPool,
    company_id: Uuid,
    attachment_id: Uuid,
) -> Result<(AttachmentMeta, Vec<u8>)> {
    let row = sqlx::query(
        "select id, voucher_id, filename, content_type, byte_size, sha256, content, uploaded_by
         from attachment where id = $1 and company_id = $2",
    )
    .bind(attachment_id)
    .bind(company_id)
    .fetch_optional(pool)
    .await?
    .context("no such attachment")?;
    let stored: Vec<u8> = row.get("sha256");
    let content = blobs()
        .load(row.get("content"), &stored)
        .await
        .with_context(|| format!("attachment {attachment_id}"))?;
    Ok((
        AttachmentMeta {
            id: row.get("id"),
            voucher_id: row.get("voucher_id"),
            filename: row.get("filename"),
            content_type: row.get("content_type"),
            byte_size: row.get("byte_size"),
            sha256_hex: hex(&stored),
            uploaded_by: row.get("uploaded_by"),
        },
        content,
    ))
}

/// Re-hashes every attachment for a company against its stored digest.
///
/// The rows are listed without their bytes, and each document is then
/// loaded and checked on its own, eight at a time: holding every
/// attachment in memory at once would not fit the container, and one
/// object-store round trip after another made this the slowest part of
/// the audit report.
pub async fn verify_attachments(pool: &PgPool, company_id: Uuid) -> Result<i64> {
    use futures_util::{StreamExt, TryStreamExt};
    let rows: Vec<(Uuid, Vec<u8>)> =
        sqlx::query_as("select id, sha256 from attachment where company_id = $1")
            .bind(company_id)
            .fetch_all(pool)
            .await?;
    let count = rows.len() as i64;
    futures_util::stream::iter(rows)
        .map(|(id, stored)| async move {
            // From the row or the object store, re-hashed either way: a
            // missing object fails here as loudly as altered bytes do.
            let in_row: Option<Vec<u8>> =
                sqlx::query_scalar("select content from attachment where id = $1")
                    .bind(id)
                    .fetch_one(pool)
                    .await?;
            blobs()
                .load(in_row, &stored)
                .await
                .with_context(|| format!("attachment {id}"))?;
            anyhow::Ok(())
        })
        .buffer_unordered(8)
        .try_collect::<Vec<()>>()
        .await?;
    Ok(count)
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
