//! The general scheduler (migration 0097, docs/jobs.md «Tidsplaner»).
//!
//! A schedule says when a kind of background job runs for a company. The
//! worker asks `enqueue_due` on every sweep; what is due becomes an ordinary
//! job row in the same transaction that moves the schedule on, and from
//! there the job machinery owns it. The scheduler runs nothing itself, so
//! every guarantee a job has (claimed once, heartbeat, retries, any number
//! of replicas) holds for scheduled work without being built twice.
//!
//! Which kinds may be scheduled, and how often, is decided here: `rules`.

use anyhow::{Result, ensure};
use chrono::{DateTime, NaiveTime, Utc};
use serde_json::Value;
use sqlx::{PgPool, Row};
use uuid::Uuid;

use crate::job::Kind;

/// What a kind allows.
pub struct Rules {
    /// The text a user sees for the schedule.
    pub name: &'static str,
    /// Most runs a day. For the bank fetch: banks limit how often data may
    /// be read without the user present (commonly four times a day).
    pub max_per_day: usize,
    /// A SQL condition on the company (`$1`) that says whether there is
    /// anything to run at all. When it is false a due schedule moves on
    /// without creating a job, so a schedule with nothing left to do does
    /// not write a row every night — and starts again by itself the day the
    /// condition holds.
    pub applies: &'static str,
}

/// The kinds that can be scheduled. Adding one is a line here plus its
/// `job::Kind`; a kind that is not listed cannot get a schedule.
pub fn rules(kind: Kind) -> Option<Rules> {
    match kind {
        Kind::BankSync => Some(Rules {
            name: "Automatisk henting fra banken",
            max_per_day: 4,
            // A live connection, and the add-on that pays for it.
            applies: "exists (select 1 from bank_connection b
                        where b.company_id = $1 and b.ended_at is null
                          and b.session_id is not null and b.valid_until > now())
                  and exists (select 1 from company_addon a
                        where a.company_id = $1 and a.addon = 'bank'
                          and a.valid_from <= current_date
                          and (a.valid_to is null or a.valid_to > current_date))",
        }),
        Kind::GarnishmentFetch => Some(Rules {
            name: "Daglig henting av trekkpålegg fra Skatteetaten",
            max_per_day: 1,
            // Skatteetaten serves the list only with the company's approved
            // systembruker, and only once it has delivered an a-melding
            // through regnmed (docs/trekkpalegg.md §1).
            applies: "exists (select 1 from altinn_system_user s
                        where s.company_id = $1 and s.status = 'Accepted')
                  and exists (select 1 from amelding_submission a
                        join amelding_submission_event e on e.submission_id = a.id
                        where a.company_id = $1 and e.kind = 'receipt')",
        }),
        Kind::SaftImport
        | Kind::AuditReport
        | Kind::PeppolLookup
        | Kind::InvoiceDispatch
        | Kind::PeppolSenderSetup
        | Kind::VatFilingSubmit => None,
    }
}

/// A night-time default spread per company (02:00–04:59), so that not every
/// company asks the same provider in the same minute.
pub fn default_time(company_id: Uuid) -> NaiveTime {
    let spread = company_id.as_u128() % 180;
    NaiveTime::from_hms_opt(2 + (spread / 60) as u32, (spread % 60) as u32, 0).expect("valid time")
}

/// "03:15" -> a time. Whole minutes only.
pub fn parse_time(text: &str) -> Option<NaiveTime> {
    NaiveTime::parse_from_str(text.trim(), "%H:%M").ok()
}

/// Checks a set of times against the kind's rules; returns them sorted.
pub fn validate(kind: Kind, times: &[String]) -> std::result::Result<Vec<NaiveTime>, String> {
    let rules = rules(kind).ok_or("denne typen jobb kan ikke tidsplanlegges")?;
    let mut parsed = Vec::new();
    for t in times {
        parsed.push(
            parse_time(t).ok_or_else(|| format!("«{t}» er ikke et klokkeslett (bruk TT:MM)"))?,
        );
    }
    parsed.sort();
    parsed.dedup();
    if parsed.is_empty() {
        return Err("oppgi minst ett klokkeslett".into());
    }
    if parsed.len() > rules.max_per_day {
        return Err(format!(
            "høyst {} ganger i døgnet for {}",
            rules.max_per_day,
            rules.name.to_lowercase()
        ));
    }
    Ok(parsed)
}

#[derive(Debug, Clone)]
pub struct ScheduleRow {
    pub id: Uuid,
    pub company_id: Uuid,
    pub company_name: String,
    pub kind: String,
    pub active: bool,
    pub times: Vec<NaiveTime>,
    pub next_run_at: DateTime<Utc>,
    /// The newest job this schedule created: status, finished, result, error.
    pub last: Option<(String, Option<DateTime<Utc>>, Option<Value>, Option<String>)>,
}

const SELECT: &str = "select s.id, s.company_id, c.name, s.kind, s.active, s.times, s.next_run_at,
        j.status, j.finished_at, j.result, j.error
     from schedule s
     join company c on c.id = s.company_id
     left join lateral (
         select status, finished_at, result, error from job
         where schedule_id = s.id order by created_at desc limit 1
     ) j on true";

fn row(r: &sqlx::postgres::PgRow) -> ScheduleRow {
    ScheduleRow {
        id: r.get("id"),
        company_id: r.get("company_id"),
        company_name: r.get("name"),
        kind: r.get("kind"),
        active: r.get("active"),
        times: r.get("times"),
        next_run_at: r.get("next_run_at"),
        last: r.get::<Option<String>, _>("status").map(|status| {
            (
                status,
                r.get("finished_at"),
                r.get("result"),
                r.get("error"),
            )
        }),
    }
}

pub async fn for_company(pool: &PgPool, company_id: Uuid) -> Result<Vec<ScheduleRow>> {
    Ok(
        sqlx::query(&format!("{SELECT} where s.company_id = $1 order by s.kind"))
            .bind(company_id)
            .fetch_all(pool)
            .await?
            .iter()
            .map(row)
            .collect(),
    )
}

/// Every schedule, for the platform console.
pub async fn all(pool: &PgPool) -> Result<Vec<ScheduleRow>> {
    Ok(sqlx::query(&format!("{SELECT} order by c.name, s.kind"))
        .fetch_all(pool)
        .await?
        .iter()
        .map(row)
        .collect())
}

/// Makes sure a company has a schedule of this kind, at the default time if
/// it has none. An existing one — also a paused one — is left exactly as it
/// is: the company's own choice outlives a renewed bank connection.
pub async fn ensure(pool: &PgPool, company_id: Uuid, kind: Kind, by: &str) -> Result<()> {
    ensure!(
        rules(kind).is_some(),
        "job kind {} cannot be scheduled",
        kind.as_str()
    );
    sqlx::query(
        "insert into schedule (company_id, kind, times, next_run_at, updated_by)
         values ($1, $2, $3, schedule_next($3, now()), $4)
         on conflict (company_id, kind) do nothing",
    )
    .bind(company_id)
    .bind(kind.as_str())
    .bind(vec![default_time(company_id)])
    .bind(by)
    .execute(pool)
    .await?;
    Ok(())
}

/// Sets when an existing schedule runs and whether it is active. Returns
/// false when the company has no schedule of this kind.
pub async fn set(
    pool: &PgPool,
    company_id: Uuid,
    kind: Kind,
    times: &[NaiveTime],
    active: bool,
    by: &str,
) -> Result<bool> {
    let n = sqlx::query(
        "update schedule set times = $3, active = $4, next_run_at = schedule_next($3, now()),
                updated_by = $5, updated_at = now()
         where company_id = $1 and kind = $2",
    )
    .bind(company_id)
    .bind(kind.as_str())
    .bind(times)
    .bind(active)
    .bind(by)
    .execute(pool)
    .await?
    .rows_affected();
    Ok(n == 1)
}

/// Turns every due schedule into a job. One transaction: the schedule moves
/// on to its next time and the job row is written, or neither. Replicas that
/// call this at the same moment take different schedules (`skip locked`), so
/// a due run becomes exactly one job.
///
/// Time that was missed (the worker was down) is not made up for: a schedule
/// that is overdue runs once and continues from now. A schedule whose
/// previous job has not finished gets no second one on top of it.
///
/// Two things make a due schedule move on WITHOUT a job: the kind says there
/// is nothing to run (`Rules::applies`), and a blocked subscription — a
/// blocked company's data is not changed by hand, so it is not changed by a
/// schedule either (docs/abonnement.md).
pub async fn enqueue_due(pool: &PgPool) -> Result<Vec<Uuid>> {
    let mut tx = pool.begin().await?;
    let due = sqlx::query(
        "update schedule s set next_run_at = schedule_next(s.times, now())
         where s.id in (
             select id from schedule where active and next_run_at <= now()
             order by next_run_at for update skip locked limit 100
         )
         returning s.id, s.company_id, s.kind",
    )
    .fetch_all(&mut *tx)
    .await?;
    let mut jobs = Vec::new();
    for r in &due {
        let (schedule_id, company_id, kind): (Uuid, Uuid, String) =
            (r.get("id"), r.get("company_id"), r.get("kind"));
        let Some(rules) = Kind::parse(&kind).and_then(rules) else {
            continue;
        };
        let applies: bool = sqlx::query_scalar(&format!("select {}", rules.applies))
            .bind(company_id)
            .fetch_one(&mut *tx)
            .await?;
        if !applies || crate::subscription::blocked(&mut *tx, company_id).await? {
            continue;
        }
        let job_id = Uuid::now_v7();
        let created = sqlx::query(
            "insert into job (id, company_id, kind, created_by, schedule_id)
             select $1, $2, $3, 'tidsplan', $4
             where not exists (
                 select 1 from job where schedule_id = $4 and status in ('queued', 'running')
             )",
        )
        .bind(job_id)
        .bind(company_id)
        .bind(&kind)
        .bind(schedule_id)
        .execute(&mut *tx)
        .await?
        .rows_affected();
        if created == 1 {
            jobs.push(job_id);
        }
    }
    tx.commit().await?;
    Ok(jobs)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn times(t: &[&str]) -> Vec<String> {
        t.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn times_are_validated_against_the_kinds_rules() {
        let ok = validate(Kind::BankSync, &times(&["12:00", "03:15", "03:15"])).unwrap();
        assert_eq!(ok.len(), 2, "sorted and deduplicated");
        assert!(ok[0] < ok[1]);
        assert!(validate(Kind::BankSync, &times(&[])).is_err());
        assert!(validate(Kind::BankSync, &times(&["25:00"])).is_err());
        assert!(validate(Kind::BankSync, &times(&["3"])).is_err());
        assert!(
            validate(
                Kind::BankSync,
                &times(&["01:00", "07:00", "13:00", "19:00", "23:00"])
            )
            .is_err(),
            "banks limit background fetches: at most four a day"
        );
        assert!(
            validate(Kind::AuditReport, &times(&["03:00"])).is_err(),
            "not a schedulable kind"
        );
    }

    #[test]
    fn the_default_time_is_at_night_and_differs_between_companies() {
        let (a, b) = (Uuid::from_u128(1), Uuid::from_u128(77));
        for id in [a, b] {
            let t = default_time(id);
            assert!(t >= NaiveTime::from_hms_opt(2, 0, 0).unwrap());
            assert!(t < NaiveTime::from_hms_opt(5, 0, 0).unwrap());
        }
        assert_ne!(default_time(a), default_time(b));
    }
}
