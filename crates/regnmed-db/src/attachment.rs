//! Bilagsvedlegg: dokumentasjon bound to vouchers, append-only like the
//! ledger (bokføringsloven §13 — oppbevaringsplikt). Content SHA-256 is
//! stored at upload and re-verified by `verify_attachments`, so a
//! swapped or altered document is detectable even if the database's
//! triggers were somehow bypassed.

use anyhow::{Context, Result, bail, ensure};
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
    ensure!(!content.is_empty(), "attachment is empty");
    let belongs: bool = sqlx::query_scalar(
        "select exists (select 1 from voucher where id = $1 and company_id = $2)",
    )
    .bind(voucher_id)
    .bind(company_id)
    .fetch_one(pool)
    .await?;
    ensure!(belongs, "no such voucher in this company");

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
    .bind(content)
    .bind(uploaded_by)
    .execute(pool)
    .await?;
    Ok(AttachmentMeta {
        id,
        voucher_id,
        filename: filename.to_string(),
        content_type: content_type.to_string(),
        byte_size: content.len() as i64,
        sha256_hex: hex(&digest),
        uploaded_by: uploaded_by.to_string(),
    })
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
    let content: Vec<u8> = row.get("content");
    let stored: Vec<u8> = row.get("sha256");
    if sha256(&content).as_slice() != stored.as_slice() {
        bail!("attachment {attachment_id} failed its hash check — dokumentasjon altered");
    }
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
pub async fn verify_attachments(pool: &PgPool, company_id: Uuid) -> Result<i64> {
    let rows = sqlx::query("select id, sha256, content from attachment where company_id = $1")
        .bind(company_id)
        .fetch_all(pool)
        .await?;
    for row in &rows {
        let id: Uuid = row.get("id");
        let stored: Vec<u8> = row.get("sha256");
        let content: Vec<u8> = row.get("content");
        if sha256(&content).as_slice() != stored.as_slice() {
            bail!(
                "attachment {id}: content does not match its stored hash — dokumentasjon altered"
            );
        }
    }
    Ok(rows.len() as i64)
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
