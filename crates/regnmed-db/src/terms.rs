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

/// Every version the company has accepted, newest first: which text bound
/// the company at a given time.
pub async fn acceptances(pool: &PgPool, company_id: Uuid) -> Result<Vec<Acceptance>> {
    let rows = sqlx::query(
        "select terms_sha256, accepted_by, accepted_at from terms_acceptance
         where company_id = $1 order by accepted_at desc",
    )
    .bind(company_id)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|r| Acceptance {
            terms_sha256: r.get("terms_sha256"),
            accepted_by: r.get("accepted_by"),
            accepted_at: r.get("accepted_at"),
        })
        .collect())
}

/// One call regnmed made to an authority on the company's behalf.
#[derive(Debug, Clone)]
pub struct AuthorityCall {
    /// `a-melding`, `aksjonaerregisteroppgave`, `altinn-hendelser` or `skattekort`.
    pub form: String,
    pub id: Uuid,
    /// Period, income year or resource -- what the call was about.
    pub reference: String,
    pub requested_by: String,
    pub requested_at: DateTime<Utc>,
    /// What came back, oldest first: receipt, failure or the authority's
    /// answer, each with when and who recorded it. Answers are summarized,
    /// not reproduced: their full text stays behind the form's own rights.
    pub events: serde_json::Value,
}

/// The register terms §7.1 promises: every call to Altinn or the
/// authorities on the company's behalf, what it was, when, who asked, and
/// what came back. NOT a second log -- a read over the insert-only records
/// the calls already leave (docs/vilkar-teknisk.md lists which): filings
/// and skattekort requests are written BEFORE the call, so the register
/// cannot miss one; an Altinn event subscription is written only once
/// Altinn has created it.
///
/// It says THAT a call failed, never why: the failure text is
/// Skatteetaten's answer, which can carry payroll or shareholder details,
/// and the register is readable with SELSKAP_LES alone. The text stays
/// behind each form's own right.
pub async fn authority_calls(pool: &PgPool, company_id: Uuid) -> Result<Vec<AuthorityCall>> {
    const EVENTS: &str = "coalesce((select jsonb_agg(jsonb_build_object(
            'hendelse', e.kind, 'tidspunkt', e.created_at, 'av', e.created_by,
            'forsendelse_id', e.forsendelse_id)
            order by e.created_at) from {table} e where e.submission_id = s.id), '[]'::jsonb)";
    let sql = format!(
        "select 'a-melding' as form, s.id, s.period as reference, s.created_by, s.created_at,
                {} as events
           from amelding_submission s where s.company_id = $1
         union all
         select 'aksjonaerregisteroppgave', s.id, s.income_year::text, s.created_by,
                s.created_at, {}
           from shareholder_return_submission s where s.company_id = $1
         union all
         select 'altinn-hendelser', s.id, s.resource_filter, s.created_by, s.created_at,
                '[]'::jsonb
           from altinn_event_subscription s where s.company_id = $1
         union all
         select 'skattekort', r.id, r.income_year::text, r.requested_by, r.created_at,
                coalesce((select jsonb_agg(x.ev order by x.at) from (
                    select f.created_at as at, jsonb_build_object(
                        'hendelse', 'failed', 'tidspunkt', f.created_at, 'av', f.created_by,
                        'forsendelse_id', null) as ev
                      from tax_card_request_failure f where f.request_id = r.id
                    union all
                    select o.created_at, jsonb_build_object(
                        'hendelse', 'receipt', 'tidspunkt', o.created_at, 'av', o.requested_by,
                        'forsendelse_id', o.reference)
                      from tax_card_order o where o.id = r.id
                    union all
                    select min(c.created_at), jsonb_build_object(
                        'hendelse', 'answer', 'tidspunkt', min(c.created_at), 'av', null,
                        'forsendelse_id', null)
                      from tax_card c where c.order_id = r.id having count(*) > 0
                ) x), '[]'::jsonb)
           from tax_card_request r where r.company_id = $1
         order by created_at desc",
        EVENTS.replace("{table}", "amelding_submission_event"),
        EVENTS.replace("{table}", "shareholder_return_submission_event"),
    );
    let rows = sqlx::query(&sql).bind(company_id).fetch_all(pool).await?;
    Ok(rows
        .into_iter()
        .map(|r| AuthorityCall {
            form: r.get("form"),
            id: r.get("id"),
            reference: r.get("reference"),
            requested_by: r.get("created_by"),
            requested_at: r.get("created_at"),
            events: r.get("events"),
        })
        .collect())
}
