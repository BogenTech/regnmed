//! Customer terms acceptance (migration 0077, docs/kundevilkar.md §1.4).
//!
//! An acceptance names the exact text by its SHA-256, so "the company
//! accepted the terms" always means a particular, reproducible text.

use anyhow::Result;
use chrono::{DateTime, Utc};
use sqlx::{PgPool, Row};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq)]
pub struct Acceptance {
    pub terms_sha256: String,
    pub accepted_by: String,
    pub accepted_at: DateTime<Utc>,
}

/// The company's acceptance of the text with this hash, if any.
pub async fn acceptance(
    pool: &PgPool,
    company_id: Uuid,
    terms_sha256: &str,
) -> Result<Option<Acceptance>> {
    let row = sqlx::query(
        "select terms_sha256, accepted_by, accepted_at from terms_acceptance
         where company_id = $1 and terms_sha256 = $2",
    )
    .bind(company_id)
    .bind(terms_sha256)
    .fetch_optional(pool)
    .await?;
    Ok(row.map(|r| Acceptance {
        terms_sha256: r.get("terms_sha256"),
        accepted_by: r.get("accepted_by"),
        accepted_at: r.get("accepted_at"),
    }))
}

/// Records the acceptance. Accepting a text the company has already
/// accepted keeps the first acceptance and returns it.
pub async fn accept(
    pool: &PgPool,
    company_id: Uuid,
    person_id: Uuid,
    terms_sha256: &str,
    accepted_by: &str,
) -> Result<Acceptance> {
    sqlx::query(
        "insert into terms_acceptance
             (id, company_id, person_id, terms_sha256, accepted_by)
         values ($1,$2,$3,$4,$5)
         on conflict (company_id, terms_sha256) do nothing",
    )
    .bind(Uuid::new_v4())
    .bind(company_id)
    .bind(person_id)
    .bind(terms_sha256)
    .bind(accepted_by)
    .execute(pool)
    .await?;
    Ok(acceptance(pool, company_id, terms_sha256)
        .await?
        .expect("just inserted or already there"))
}
