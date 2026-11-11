//! The aksjonærregisteroppgave submission record (migration 0076,
//! docs/aksjonaer.md, «Innsending»).
//!
//! The rule the record exists for: **what is sent is stored before it is
//! sent**, every form with the idempotency key it is always sent with.
//!
//! - A filing with no receipt is retried as stored when the oppgave renders
//!   the same today: every call repeats its key, and Skatteetaten answers
//!   with its first answer instead of opening a second oppgave.
//! - If the aksjeeierbok has changed since, the undelivered filing is left
//!   as history and a new one is stored — it never reached bekreft, so
//!   Skatteetaten never took it.
//! - A year that already has a receipt is refused. How Skatteetaten treats
//!   a second oppgave for the same year has not been verified, so we do not
//!   send one.
//!
//! Status is computed from the insert-only event log, never stored.

use anyhow::{Context, Result, bail};
use chrono::{DateTime, Utc};
use sqlx::{PgPool, Row};
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct Submission {
    pub id: Uuid,
    pub income_year: i32,
    pub main_form: String,
    pub main_key: Uuid,
    pub confirm_key: Uuid,
    /// (shareholder id, XML, idempotency key) in sending order.
    pub sub_forms: Vec<(Uuid, String, Uuid)>,
    pub created_by: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum SubmissionStatus {
    /// Stored, no receipt. `last_error` is the latest failed attempt.
    NotDelivered { last_error: Option<String> },
    /// Skatteetaten's Leveransebekreftelse.
    Delivered {
        dialog_id: String,
        forsendelse_id: String,
        reference: Option<String>,
        at: DateTime<Utc>,
    },
}

impl SubmissionStatus {
    pub fn slug(&self) -> &'static str {
        match self {
            Self::NotDelivered { .. } => "ikke_levert",
            Self::Delivered { .. } => "levert",
        }
    }
}

const COLUMNS: &str = "id, income_year, main_form, main_key, confirm_key, created_by, created_at";

async fn load(pool: &PgPool, row: &sqlx::postgres::PgRow) -> Result<Submission> {
    let id: Uuid = row.get("id");
    let sub_forms = sqlx::query(
        "select shareholder_id, xml, idempotency_key from shareholder_return_submission_form
         where submission_id = $1 order by position",
    )
    .bind(id)
    .fetch_all(pool)
    .await?
    .iter()
    .map(|r| {
        (
            r.get("shareholder_id"),
            r.get("xml"),
            r.get("idempotency_key"),
        )
    })
    .collect();
    Ok(Submission {
        id,
        income_year: row.get("income_year"),
        main_form: row.get("main_form"),
        main_key: row.get("main_key"),
        confirm_key: row.get("confirm_key"),
        sub_forms,
        created_by: row.get("created_by"),
        created_at: row.get("created_at"),
    })
}

/// The filing to send for `year` now: the stored undelivered one when the
/// oppgave still renders the same (a retry — same bodies, same keys),
/// otherwise a new one rendered from the aksjeeierbok as it stands.
///
/// Refuses, with every hindrance named, an oppgave that cannot be rendered
/// (`ReturnError`, e.g. an unverified transaction type code), and a year
/// that already has a receipt.
pub async fn prepare_submission(
    pool: &PgPool,
    company_id: Uuid,
    year: i32,
    created_by: &str,
) -> Result<Submission> {
    let set = crate::share_register::build_return(pool, company_id, year).await?;
    let mut hindrances = Vec::new();
    let main = regnmed_core::shareholder_return::render_main_form(&set.main_form)
        .map_err(|e| hindrances.push(e.to_string()))
        .ok();
    let mut subs = Vec::new();
    for (shareholder_id, form) in &set.underskjemaer {
        match regnmed_core::shareholder_return::render_sub_form(form) {
            Ok(xml) => subs.push((*shareholder_id, xml)),
            Err(e) => hindrances.push(format!("{}: {e}", form.name)),
        }
    }
    let Some(main) = main.filter(|_| hindrances.is_empty()) else {
        bail!(
            "aksjonærregisteroppgaven kan ikke sendes ennå: {}",
            hindrances.join("; ")
        );
    };
    if subs.is_empty() {
        bail!("aksjonærregisteroppgaven har ingen aksjonærer å sende underskjema for");
    }

    let mut tx = pool.begin().await?;
    // Two clicks at once must not store two filings for the same year.
    sqlx::query("select pg_advisory_xact_lock(hashtextextended($1, 0))")
        .bind(format!("shareholder_return:{company_id}:{year}"))
        .execute(&mut *tx)
        .await?;

    let latest = sqlx::query(&format!(
        "select {COLUMNS},
                exists (select 1 from shareholder_return_submission_event e
                        where e.submission_id = s.id and e.kind = 'receipt') as delivered
         from shareholder_return_submission s
         where s.company_id = $1 and s.income_year = $2
         order by s.created_at desc limit 1"
    ))
    .bind(company_id)
    .bind(year)
    .fetch_optional(&mut *tx)
    .await?;

    if let Some(row) = &latest {
        if row.get::<bool, _>("delivered") {
            bail!(
                "aksjonærregisteroppgaven for {year} er allerede levert. En ny oppgave for samme \
                 år sendes ikke herfra ennå — hvordan Skatteetaten behandler den er ikke \
                 verifisert"
            );
        }
        // Its rows are committed, so reading them outside the lock-holding
        // transaction sees all of them.
        let stored = load(pool, row).await?;
        let same = stored.main_form == main
            && stored
                .sub_forms
                .iter()
                .map(|(id, xml, _)| (*id, xml.as_str()))
                .eq(subs.iter().map(|(id, xml)| (*id, xml.as_str())));
        if same {
            return Ok(stored);
        }
    }

    let id = Uuid::new_v4();
    sqlx::query(
        "insert into shareholder_return_submission
             (id, company_id, income_year, main_form, main_key, confirm_key, created_by)
         values ($1,$2,$3,$4,$5,$6,$7)",
    )
    .bind(id)
    .bind(company_id)
    .bind(year)
    .bind(&main)
    .bind(Uuid::new_v4())
    .bind(Uuid::new_v4())
    .bind(created_by)
    .execute(&mut *tx)
    .await?;
    for (position, (shareholder_id, xml)) in subs.iter().enumerate() {
        sqlx::query(
            "insert into shareholder_return_submission_form
                 (submission_id, position, shareholder_id, xml, idempotency_key)
             values ($1,$2,$3,$4,$5)",
        )
        .bind(id)
        .bind(position as i32)
        .bind(shareholder_id)
        .bind(xml)
        .bind(Uuid::new_v4())
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    Ok(submission(pool, company_id, id).await?.0)
}

/// Skatteetaten confirmed the oppgave.
pub async fn record_receipt(
    pool: &PgPool,
    submission_id: Uuid,
    dialog_id: &str,
    forsendelse_id: &str,
    reference: Option<&str>,
    created_by: &str,
) -> Result<()> {
    sqlx::query(
        "insert into shareholder_return_submission_event
             (id, submission_id, kind, dialog_id, forsendelse_id, reference, created_by)
         values ($1,$2,'receipt',$3,$4,$5,$6)",
    )
    .bind(Uuid::new_v4())
    .bind(submission_id)
    .bind(dialog_id)
    .bind(forsendelse_id)
    .bind(reference)
    .bind(created_by)
    .execute(pool)
    .await?;
    Ok(())
}

/// No receipt. The submission stays undelivered and is retried as stored.
pub async fn record_failure(
    pool: &PgPool,
    submission_id: Uuid,
    error: &str,
    created_by: &str,
) -> Result<()> {
    sqlx::query(
        "insert into shareholder_return_submission_event
             (id, submission_id, kind, detail, created_by)
         values ($1,$2,'failed',$3,$4)",
    )
    .bind(Uuid::new_v4())
    .bind(submission_id)
    .bind(serde_json::json!({ "error": error }))
    .bind(created_by)
    .execute(pool)
    .await?;
    Ok(())
}

/// The year's filings, newest first, each with its computed status.
pub async fn list_submissions(
    pool: &PgPool,
    company_id: Uuid,
    year: i32,
) -> Result<Vec<(Submission, SubmissionStatus)>> {
    let rows = sqlx::query(&format!(
        "select {COLUMNS} from shareholder_return_submission
         where company_id = $1 and income_year = $2 order by created_at desc"
    ))
    .bind(company_id)
    .bind(year)
    .fetch_all(pool)
    .await?;
    let mut out = Vec::new();
    for row in &rows {
        let s = load(pool, row).await?;
        let status = status(pool, s.id).await?;
        out.push((s, status));
    }
    Ok(out)
}

pub async fn submission(
    pool: &PgPool,
    company_id: Uuid,
    submission_id: Uuid,
) -> Result<(Submission, SubmissionStatus)> {
    let row = sqlx::query(&format!(
        "select {COLUMNS} from shareholder_return_submission where id = $1 and company_id = $2"
    ))
    .bind(submission_id)
    .bind(company_id)
    .fetch_optional(pool)
    .await?
    .context("ukjent innsending")?;
    let s = load(pool, &row).await?;
    let status = status(pool, s.id).await?;
    Ok((s, status))
}

async fn status(pool: &PgPool, submission_id: Uuid) -> Result<SubmissionStatus> {
    let events = sqlx::query(
        "select kind, dialog_id, forsendelse_id, reference, detail, created_at
         from shareholder_return_submission_event
         where submission_id = $1 order by created_at",
    )
    .bind(submission_id)
    .fetch_all(pool)
    .await?;
    let kind = |e: &&sqlx::postgres::PgRow, k: &str| e.get::<String, _>("kind") == k;
    Ok(match events.iter().find(|e| kind(e, "receipt")) {
        Some(r) => SubmissionStatus::Delivered {
            dialog_id: r.get("dialog_id"),
            forsendelse_id: r.get("forsendelse_id"),
            reference: r.get("reference"),
            at: r.get("created_at"),
        },
        None => SubmissionStatus::NotDelivered {
            last_error: events
                .iter()
                .rev()
                .find(|e| kind(e, "failed"))
                .and_then(|e| e.get::<Option<serde_json::Value>, _>("detail"))
                .and_then(|d| d["error"].as_str().map(str::to_string)),
        },
    })
}
