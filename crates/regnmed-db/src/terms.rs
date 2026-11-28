//! Customer terms acceptance (migration 0077, docs/kundevilkar.md §1.4).
//!
//! An acceptance names the exact text by its SHA-256, so "the company
//! accepted the terms" always means a particular, reproducible text.

use std::sync::LazyLock;

use anyhow::Result;
use chrono::{DateTime, Utc};
use include_dir::{Dir, include_dir};
use sqlx::{PgPool, Row};
use uuid::Uuid;

pub const TEXT: &str = include_str!("../../../docs/kundevilkar.md");

/// The version is the hash of the text: a changed text is a new version
/// and needs a new acceptance, with no number for anyone to forget.
pub static VERSION: LazyLock<String> = LazyLock::new(|| {
    regnmed_core::hash::sha256(TEXT.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
});

/// Every version that has been in force, `YYYY-MM-DD-<sha256>.md` (the
/// date it took effect). A customer bound by an older text must be able to
/// read it after the text has moved on; `the_current_text_is_archived`
/// fails CI on a change to the text that is not archived.
static ARCHIVE: Dir = include_dir!("$CARGO_MANIFEST_DIR/../../docs/kundevilkar");

/// (effective date, version, text), newest first.
pub fn versions() -> Vec<(&'static str, &'static str, &'static str)> {
    let mut v: Vec<_> = ARCHIVE
        .files()
        .filter_map(|f| {
            let name = f.path().file_name()?.to_str()?.strip_suffix(".md")?;
            let (date, sha) = (name.get(..10)?, name.get(11..)?);
            Some((date, sha, f.contents_utf8()?))
        })
        .collect();
    // The version in force first, then newest date first. Same-day
    // versions are otherwise unordered: the file name has no time.
    v.sort_by_key(|&(date, sha, _)| (sha != VERSION.as_str(), std::cmp::Reverse(date), sha));
    v
}

/// The date the current text takes effect; it may still lie ahead.
pub fn effective_date() -> &'static str {
    versions()
        .into_iter()
        .find(|(_, sha, _)| *sha == VERSION.as_str())
        .map_or("", |(date, _, _)| date)
}

/// Whether the accepted versions cover the version in force on `today`
/// (`YYYY-MM-DD`): that one, or a newer one already announced. Before the
/// first version takes effect any acceptance counts.
pub fn covered(versions: &[(&str, &str, &str)], accepted: &[String], today: &str) -> bool {
    let in_force = versions
        .iter()
        .map(|(date, _, _)| *date)
        .filter(|date| *date <= today)
        .max()
        .unwrap_or("");
    accepted.iter().any(|sha| {
        versions
            .iter()
            .any(|(date, s, _)| s == sha && *date >= in_force)
    })
}

/// Whether the company may file today under the terms it has accepted.
pub fn may_file(history: &[Acceptance]) -> bool {
    let accepted: Vec<String> = history.iter().map(|a| a.terms_sha256.clone()).collect();
    // shortcut: the day turns at midnight UTC, not Oslo, so a new version
    // takes effect up to two hours late -- the old text governs meanwhile.
    let today = Utc::now().date_naive().to_string();
    covered(&versions(), &accepted, &today)
}

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

/// A company bound by an earlier version of the terms that has not yet
/// been told about this one (terms §14.1).
#[derive(Debug, Clone)]
pub struct NoticeDue {
    pub company_id: Uuid,
    pub company_name: String,
    /// The company's own e-mail address (terms §15.1), else the address of
    /// whoever accepted for it last. None: nobody to tell.
    pub recipient: Option<String>,
}

/// Every company that accepted an earlier version, has not accepted this
/// one and has not been told about it. Companies that never accepted any
/// version are not bound by the terms and are not told.
pub async fn notices_due(pool: &PgPool, terms_sha256: &str) -> Result<Vec<NoticeDue>> {
    let rows = sqlx::query(
        "select c.id, c.name,
                coalesce(nullif(c.email, ''),
                         (select p.email from terms_acceptance a
                            join person p on p.id = a.person_id
                           where a.company_id = c.id and nullif(p.email, '') is not null
                           order by a.accepted_at desc limit 1)) as recipient
           from company c
          where exists (select 1 from terms_acceptance a where a.company_id = c.id)
            and not exists (select 1 from terms_acceptance a
                             where a.company_id = c.id and a.terms_sha256 = $1)
            and not exists (select 1 from terms_notice n
                             where n.company_id = c.id and n.terms_sha256 = $1)
          order by c.name",
    )
    .bind(terms_sha256)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|r| NoticeDue {
            company_id: r.get("id"),
            company_name: r.get("name"),
            recipient: r.get("recipient"),
        })
        .collect())
}

/// Records that the notice went out; `id` is the mail's Nats-Msg-Id.
pub async fn record_notice(
    pool: &PgPool,
    id: Uuid,
    company_id: Uuid,
    terms_sha256: &str,
    recipient: &str,
) -> Result<()> {
    sqlx::query(
        "insert into terms_notice (id, company_id, terms_sha256, recipient)
         values ($1,$2,$3,$4)",
    )
    .bind(id)
    .bind(company_id)
    .bind(terms_sha256)
    .bind(recipient)
    .execute(pool)
    .await?;
    Ok(())
}

/// One call regnmed made to an authority on the company's behalf.
#[derive(Debug, Clone)]
pub struct AuthorityCall {
    /// `a-melding`, `aksjonaerregisteroppgave`, `altinn-hendelser`, `skattekort`,
    /// `trekkpalegg` or `mva-melding`.
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
         union all
         select 'trekkpalegg', r.id, 'trekkpålegg', r.requested_by, r.created_at,
                coalesce((select jsonb_agg(jsonb_build_object(
                    'hendelse', case when o.ok then 'answer' else 'failed' end,
                    'tidspunkt', o.created_at, 'av', null, 'forsendelse_id', null))
                  from garnishment_fetch_outcome o where o.fetch_id = r.id), '[]'::jsonb)
           from garnishment_fetch r where r.company_id = $1
         union all
         select 'mva-melding', f.id, f.year::text || '-' || f.period, f.requested_by, f.created_at,
                coalesce((select jsonb_agg(jsonb_build_object(
                    'hendelse', e.kind, 'tidspunkt', e.created_at, 'av', e.detail->>'name',
                    'forsendelse_id', null) order by e.created_at, e.id)
                  from vat_filing_event e where e.filing_id = f.id), '[]'::jsonb)
           from vat_filing f where f.company_id = $1
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_current_text_is_archived() {
        let all = versions();
        // Every version that was ever in force. A customer may have
        // accepted any of them, so the list only grows, like migrations.
        const KNOWN: &[&str] = &[
            "f2d730ceb33dde33c066d7cc74bfb9d0c725e3958aea3e0711fab11da3a885cd",
            "f376538b16f2bed77e6efabfd80cd7fd247797893c07644a32ea153dcd4329f9",
            "bec6783875729d4f63883b3595bdbf184c0982c92f01958485badc29e0e456b7",
            "d9a890536ea971eb960c7071f73a254e6aeaa149df14c13af6087a04c63a6f3e",
        ];
        for known in KNOWN {
            assert!(
                all.iter().any(|(_, sha, _)| sha == known),
                "archived terms version {known} is gone — a company may have accepted it"
            );
        }
        assert!(
            KNOWN.contains(&VERSION.as_str()),
            "add the new version {} to KNOWN",
            *VERSION
        );
        assert!(
            all.iter()
                .any(|(_, sha, text)| *sha == VERSION.as_str() && *text == TEXT),
            "docs/kundevilkar.md changed: copy it to docs/kundevilkar/<YYYY-MM-DD>-{}.md \
             (the date it takes effect) so customers bound by the old text can still read it",
            *VERSION
        );
        // The file is the newest text: an older-dated text would take
        // effect before the one it replaces.
        assert!(
            all.iter().all(|(date, _, _)| *date <= effective_date()),
            "docs/kundevilkar.md must carry the newest date in docs/kundevilkar/"
        );
        // One version per date, or "in force on that date" is ambiguous.
        // The three drafts of 2026-11-12 predate the rule and stay.
        let mut dates: Vec<_> = all.iter().map(|(date, _, _)| *date).collect();
        dates.retain(|date| *date != "2026-11-12");
        dates.sort();
        assert!(
            dates.windows(2).all(|w| w[0] != w[1]),
            "two terms versions take effect on the same date"
        );
        for (date, sha, text) in &all {
            assert!(
                chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d").is_ok(),
                "{date}"
            );
            let actual: String = regnmed_core::hash::sha256(text.as_bytes())
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect();
            assert_eq!(
                &actual, sha,
                "an archived version was edited after the fact"
            );
        }
    }

    #[test]
    fn an_announced_version_binds_only_from_its_date() {
        let versions = [("2026-01-01", "old", ""), ("2026-03-01", "new", "")];
        let accepted = |shas: &[&str]| shas.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        // During the notice period the old acceptance still files, and an
        // early acceptance of the new text does too.
        assert!(covered(&versions, &accepted(&["old"]), "2026-02-28"));
        assert!(covered(&versions, &accepted(&["new"]), "2026-02-28"));
        assert!(!covered(&versions, &accepted(&[]), "2026-02-28"));
        // From its date only the new text does.
        assert!(!covered(&versions, &accepted(&["old"]), "2026-03-01"));
        assert!(covered(&versions, &accepted(&["new"]), "2026-03-01"));
        assert!(!covered(&versions, &accepted(&["unknown"]), "2026-03-01"));
        // Before the first version takes effect, any acceptance counts.
        assert!(covered(&versions, &accepted(&["new"]), "2025-12-31"));
    }
}
