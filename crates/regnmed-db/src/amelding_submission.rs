//! The a-melding submission record (migration 0074, docs/lonn.md).
//!
//! The rule the record exists for: **what is sent is stored before it is
//! sent.** Skatteetaten adds meldinger for the same month together, so
//!
//! - a retry of a submission that never got a receipt resends the SAME
//!   body with the SAME idempotency key (Skatteetaten then answers with the
//!   first receipt instead of counting it twice), and
//! - once a submission has a receipt, the run can only be sent again as an
//!   explicit replacement (`erstatterMeldingsId`), never as a second
//!   melding.
//!
//! Status is computed from the insert-only event log, never stored.

use anyhow::{Context, Result, bail};
use chrono::{DateTime, Utc};
use sqlx::{PgPool, Row};
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct Submission {
    pub id: Uuid,
    pub run_id: Uuid,
    pub melding_id: String,
    pub idempotency_key: Uuid,
    pub period: String,
    pub replaces_melding_id: Option<String>,
    /// The exact JSON to send (and that was sent).
    pub body: String,
    pub created_by: String,
    pub created_at: DateTime<Utc>,
}

/// Where a submission stands, derived from its events.
#[derive(Debug, Clone, PartialEq)]
pub enum SubmissionStatus {
    /// Stored, not (yet) taken by Skatteetaten. `last_error` is the latest
    /// failed attempt, if any.
    NotDelivered { last_error: Option<String> },
    /// Receipt received; Skatteetaten has not answered on the content yet.
    Delivered {
        dialog_id: String,
        forsendelse_id: String,
    },
    /// Skatteetaten's tilbakemelding, verbatim.
    Feedback {
        dialog_id: String,
        forsendelse_id: String,
        feedback: serde_json::Value,
    },
}

impl SubmissionStatus {
    pub fn slug(&self) -> &'static str {
        match self {
            Self::NotDelivered { .. } => "ikke_levert",
            Self::Delivered { .. } => "levert",
            Self::Feedback { .. } => "tilbakemelding",
        }
    }
}

fn from_row(r: &sqlx::postgres::PgRow) -> Submission {
    Submission {
        id: r.get("id"),
        run_id: r.get("run_id"),
        melding_id: r.get("melding_id"),
        idempotency_key: r.get("idempotency_key"),
        period: r.get("period"),
        replaces_melding_id: r.get("replaces_melding_id"),
        body: r.get("body"),
        created_by: r.get("created_by"),
        created_at: r.get("created_at"),
    }
}

const COLUMNS: &str = "id, run_id, melding_id, idempotency_key, period, replaces_melding_id, \
                       body, created_by, created_at";

/// The submission to send for a run now: the undelivered one if there is
/// one (a retry — same body, same key), otherwise a new one rendered from
/// the run as it stands. A run that already has a delivered submission
/// gets a new one only with `replace`, and then as a replacement of the
/// latest delivered melding.
///
/// Refuses, with every hindrance named, a run the a-melding cannot be
/// built from (`payroll::amelding_for_run`).
pub async fn prepare_submission(
    pool: &PgPool,
    company_id: Uuid,
    run_id: Uuid,
    replace: bool,
    created_by: &str,
) -> Result<Submission> {
    let mut tx = pool.begin().await?;
    // Two clicks at once must not create two meldinger: the run row is the
    // lock every submission for it goes through.
    sqlx::query("select 1 from payroll_run where id = $1 and company_id = $2 for update")
        .bind(run_id)
        .bind(company_id)
        .fetch_optional(&mut *tx)
        .await?
        .context("ukjent lønnskjøring")?;

    let latest = sqlx::query(&format!(
        "select {COLUMNS},
                exists (select 1 from amelding_submission_event e
                        where e.submission_id = s.id and e.kind = 'receipt') as delivered
         from amelding_submission s
         where s.company_id = $1 and s.run_id = $2
         order by s.created_at desc limit 1"
    ))
    .bind(company_id)
    .bind(run_id)
    .fetch_optional(&mut *tx)
    .await?;

    let replaces = match &latest {
        Some(row) if !row.get::<bool, _>("delivered") => {
            // Never delivered: send exactly this again.
            let retry = from_row(row);
            tx.commit().await?;
            return Ok(retry);
        }
        Some(row) if !replace => bail!(
            "a-meldingen for denne kjøringen er allerede levert (meldingsId {}). En ny melding \
             for samme måned LEGGES TIL den forrige hos Skatteetaten — send den som en \
             erstatning hvis den skal rettes",
            row.get::<String, _>("melding_id")
        ),
        Some(row) => Some(row.get::<String, _>("melding_id")),
        None if replace => bail!("det finnes ingen levert a-melding for kjøringen å erstatte"),
        None => None,
    };

    let mut draft = crate::payroll::amelding_for_run(pool, company_id, run_id, Utc::now()).await?;
    if !draft.hindrances.is_empty() {
        bail!(
            "a-meldingen kan ikke lages ennå: {}",
            draft.hindrances.join("; ")
        );
    }
    let melding_id = Uuid::new_v4();
    draft.melding.message_id = melding_id.to_string();
    draft.melding.replaces_message_id = replaces.clone();
    let body =
        regnmed_core::amelding::render_json(&draft.melding).map_err(|e| anyhow::anyhow!("{e}"))?;

    let row = sqlx::query(&format!(
        "insert into amelding_submission
             (id, company_id, run_id, melding_id, idempotency_key, period,
              replaces_melding_id, body, created_by)
         values ($1,$2,$3,$4,$5,$6,$7,$8,$9)
         returning {COLUMNS}"
    ))
    .bind(Uuid::new_v4())
    .bind(company_id)
    .bind(run_id)
    .bind(melding_id.to_string())
    .bind(Uuid::new_v4())
    .bind(regnmed_core::amelding::period(&draft.melding))
    .bind(&replaces)
    .bind(&body)
    .bind(created_by)
    .fetch_one(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(from_row(&row))
}

async fn insert_event(
    pool: &PgPool,
    submission_id: Uuid,
    kind: &str,
    receipt: Option<(&str, &str)>,
    detail: Option<serde_json::Value>,
    created_by: &str,
) -> Result<()> {
    sqlx::query(
        "insert into amelding_submission_event
             (id, submission_id, kind, dialog_id, forsendelse_id, detail, created_by)
         values ($1,$2,$3,$4,$5,$6,$7)",
    )
    .bind(Uuid::new_v4())
    .bind(submission_id)
    .bind(kind)
    .bind(receipt.map(|r| r.0))
    .bind(receipt.map(|r| r.1))
    .bind(detail)
    .bind(created_by)
    .execute(pool)
    .await?;
    Ok(())
}

/// Skatteetaten took the melding.
pub async fn record_receipt(
    pool: &PgPool,
    submission_id: Uuid,
    dialog_id: &str,
    forsendelse_id: &str,
    created_by: &str,
) -> Result<()> {
    insert_event(
        pool,
        submission_id,
        "receipt",
        Some((dialog_id, forsendelse_id)),
        None,
        created_by,
    )
    .await
}

/// The call got no receipt. The submission stays undelivered and is
/// retried as it is.
pub async fn record_failure(
    pool: &PgPool,
    submission_id: Uuid,
    error: &str,
    created_by: &str,
) -> Result<()> {
    insert_event(
        pool,
        submission_id,
        "failed",
        None,
        Some(serde_json::json!({ "error": error })),
        created_by,
    )
    .await
}

/// Skatteetaten's tilbakemelding. Once per submission (unique index); a
/// second arrival is ignored rather than an error, since it is the same
/// answer fetched twice.
pub async fn record_feedback(
    pool: &PgPool,
    submission_id: Uuid,
    feedback: serde_json::Value,
    created_by: &str,
) -> Result<()> {
    sqlx::query(
        "insert into amelding_submission_event
             (id, submission_id, kind, detail, created_by)
         values ($1,$2,'feedback',$3,$4)
         on conflict (submission_id) where kind = 'feedback' do nothing",
    )
    .bind(Uuid::new_v4())
    .bind(submission_id)
    .bind(feedback)
    .bind(created_by)
    .execute(pool)
    .await?;
    Ok(())
}

/// The run's submissions, newest first, each with its computed status.
pub async fn list_submissions(
    pool: &PgPool,
    company_id: Uuid,
    run_id: Uuid,
) -> Result<Vec<(Submission, SubmissionStatus)>> {
    let rows = sqlx::query(&format!(
        "select {COLUMNS} from amelding_submission
         where company_id = $1 and run_id = $2 order by created_at desc"
    ))
    .bind(company_id)
    .bind(run_id)
    .fetch_all(pool)
    .await?;
    let mut out = Vec::new();
    for row in &rows {
        let s = from_row(row);
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
        "select {COLUMNS} from amelding_submission where id = $1 and company_id = $2"
    ))
    .bind(submission_id)
    .bind(company_id)
    .fetch_optional(pool)
    .await?
    .context("ukjent innsending")?;
    let s = from_row(&row);
    let status = status(pool, s.id).await?;
    Ok((s, status))
}

async fn status(pool: &PgPool, submission_id: Uuid) -> Result<SubmissionStatus> {
    let events = sqlx::query(
        "select kind, dialog_id, forsendelse_id, detail from amelding_submission_event
         where submission_id = $1 order by created_at",
    )
    .bind(submission_id)
    .fetch_all(pool)
    .await?;
    let receipt = events
        .iter()
        .find(|e| e.get::<String, _>("kind") == "receipt");
    let feedback = events
        .iter()
        .find(|e| e.get::<String, _>("kind") == "feedback");
    Ok(match (receipt, feedback) {
        (Some(r), Some(f)) => SubmissionStatus::Feedback {
            dialog_id: r.get("dialog_id"),
            forsendelse_id: r.get("forsendelse_id"),
            feedback: f.get("detail"),
        },
        (Some(r), None) => SubmissionStatus::Delivered {
            dialog_id: r.get("dialog_id"),
            forsendelse_id: r.get("forsendelse_id"),
        },
        (None, _) => SubmissionStatus::NotDelivered {
            last_error: events
                .iter()
                .rev()
                .find(|e| e.get::<String, _>("kind") == "failed")
                .and_then(|e| e.get::<Option<serde_json::Value>, _>("detail"))
                .and_then(|d| d["error"].as_str().map(str::to_string)),
        },
    })
}

/// Delivered submissions still waiting for feedback whose receipt names
/// this Dialogporten dialog — what an incoming event points at. The
/// event itself is untrusted; this is the only thing taken from it.
pub async fn awaiting_feedback_for_dialog(
    pool: &PgPool,
    dialog_id: &str,
) -> Result<Vec<(Uuid, Submission)>> {
    let rows = sqlx::query(&format!(
        "select s.company_id, {} from amelding_submission s
         join amelding_submission_event r
           on r.submission_id = s.id and r.kind = 'receipt' and r.dialog_id = $1
         where not exists (select 1 from amelding_submission_event f
                           where f.submission_id = s.id and f.kind = 'feedback')",
        COLUMNS
            .split(", ")
            .map(|c| format!("s.{}", c.trim()))
            .collect::<Vec<_>>()
            .join(", ")
    ))
    .bind(dialog_id)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .iter()
        .map(|r| (r.get("company_id"), from_row(r)))
        .collect())
}

/// Altinn's id for the company's subscription to `type_filter` events on
/// `resource_filter` delivered to `end_point`, if regnmed has made one.
pub async fn event_subscription(
    pool: &PgPool,
    company_id: Uuid,
    resource_filter: &str,
    type_filter: &str,
    end_point: &str,
) -> Result<Option<i64>> {
    Ok(sqlx::query_scalar(
        "select altinn_id from altinn_event_subscription
         where company_id = $1 and resource_filter = $2 and type_filter = $3
           and end_point = $4",
    )
    .bind(company_id)
    .bind(resource_filter)
    .bind(type_filter)
    .bind(end_point)
    .fetch_optional(pool)
    .await?)
}

pub async fn record_event_subscription(
    pool: &PgPool,
    company_id: Uuid,
    altinn_id: i64,
    resource_filter: &str,
    type_filter: &str,
    end_point: &str,
    created_by: &str,
) -> Result<()> {
    sqlx::query(
        "insert into altinn_event_subscription
             (id, company_id, altinn_id, resource_filter, type_filter, end_point, created_by)
         values ($1,$2,$3,$4,$5,$6,$7)
         on conflict (company_id, resource_filter, type_filter, end_point) do nothing",
    )
    .bind(Uuid::new_v4())
    .bind(company_id)
    .bind(altinn_id)
    .bind(resource_filter)
    .bind(type_filter)
    .bind(end_point)
    .bind(created_by)
    .execute(pool)
    .await?;
    Ok(())
}
