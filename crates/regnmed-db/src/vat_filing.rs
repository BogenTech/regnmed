//! The mva-melding filed by a person (docs/mva.md, «Godkjenn og send»).
//!
//! The rule the record exists for: **what is sent is stored before it is
//! sent, and an uncertain step is never repeated blindly.** The melding is an
//! immutable row, status is computed from insert-only events, and the instance
//! id is committed before anything is uploaded to it. A retry reads where the
//! instance actually stands and continues from there.
//!
//! The person's token lives sealed in `vat_filing_grant`, for as long as the
//! work needs it. Neither it nor the national ID is ever stored in clear.

use anyhow::{Context, Result};
use chrono::{DateTime, Duration, NaiveDate, Utc};
use regnmed_core::secret::{SealingKey, open, seal};
use regnmed_core::vat::{Period, PeriodScheme};
use regnmed_gov::vat_return::{
    ENVELOPE_DATA_TYPE, FilingError, Instance, MELDING_DATA_TYPE, VatFilingClient, VatReturnClient,
    extract_findings, is_rejection, token_expiry,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sqlx::pool::PoolConnection;
use sqlx::{PgConnection, PgExecutor, PgPool, Postgres, Row, Transaction};
use uuid::Uuid;

use crate::job::{Claimed, RetryLater, Runtime};

const FILLING_TASK: &str = "Task_1";
const CONFIRM_TASK: &str = "Task_2";
const LOCK_WAIT: std::time::Duration = std::time::Duration::from_secs(20);
const LOCK_POLL: std::time::Duration = std::time::Duration::from_millis(100);
const FEEDBACK_FILES: [&str; 3] = ["betalingsinformasjon", "kvittering", "valideringsresultat"];

pub const NEEDS_RIGHT: &str = "Personen som godkjente har ikke rett til å sende mva-melding for \
    selskapet i Altinn. Daglig leder eller en annen med tilgangspakken Merverdiavgift kan \
    godkjenne. En regnskapsfører må ha pakken «Regnskapsfører med signeringsrettighet» for \
    denne klienten: kunden gir den til byrået, og byråets klientadministrator gir den videre \
    til den ansatte.";

/// The worker's connection to Altinn and the key its tokens are sealed with.
pub struct Rail {
    pub client: VatFilingClient,
    pub key: SealingKey,
    /// How long a job waits for a run that already holds the filing.
    pub lock_wait: std::time::Duration,
}

impl Rail {
    /// On when `IDPORTEN_SEALING_KEY` and `MVA_INSTANCES_URL` are both set.
    pub fn from_env() -> Result<Option<Self>> {
        let key = std::env::var("IDPORTEN_SEALING_KEY").ok();
        let instances = std::env::var("MVA_INSTANCES_URL").ok();
        let (key, instances) = match (key, instances) {
            (Some(key), Some(instances)) => (key, instances),
            (None, None) => return Ok(None),
            _ => anyhow::bail!("IDPORTEN_SEALING_KEY and MVA_INSTANCES_URL must be set together"),
        };
        let platform = std::env::var("ALTINN_PLATFORM_URL")
            .unwrap_or_else(|_| "https://platform.tt02.altinn.no".into());
        Ok(Some(Self {
            client: VatFilingClient::new(VatReturnClient::from_env(), platform, instances)?,
            key: SealingKey::from_hex(&key)?,
            lock_wait: LOCK_WAIT,
        }))
    }
}

#[derive(Debug, Clone)]
pub struct Filing {
    pub id: Uuid,
    pub company_id: Uuid,
    pub year: i32,
    pub period: i32,
    pub melding_xml: String,
    pub assessed_kr: i64,
    pub requested_by: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct Event {
    pub id: Uuid,
    pub kind: String,
    pub detail: Value,
    pub created_at: DateTime<Utc>,
}

/// Status is the latest event. A renewed sign-in or a note about the
/// envelope only helps the work along, so neither moves the filing.
pub fn status(events: &[Event]) -> &str {
    events
        .iter()
        .rev()
        .find(|e| {
            !matches!(
                e.kind.as_str(),
                "fornyet" | "konvolutt" | "bekrefter" | "bekreftelse_avvist"
            )
        })
        .map_or("opprettet", |e| e.kind.as_str())
}

pub fn instance_url(events: &[Event]) -> Option<&str> {
    events
        .iter()
        .rev()
        .find(|e| e.kind == "instans_opprettet")
        .and_then(|e| e.detail["url"].as_str())
}

/// An instance create that was sent and never accounted for. A definite
/// refusal closes it; nothing else does, so it is not sent a second time.
fn create_unaccounted(events: &[Event]) -> bool {
    events
        .iter()
        .rev()
        .find(|e| {
            matches!(
                e.kind.as_str(),
                "oppretter_instans" | "instans_opprettet" | "feilet"
            )
        })
        .is_some_and(|e| e.kind == "oppretter_instans")
}

/// Whether this filing may have reached Altinn, so that only reading it
/// there can say what happened.
pub fn may_have_reached_altinn(events: &[Event]) -> bool {
    instance_url(events).is_some() || create_unaccounted(events)
}

/// Who stands behind the return: the person who confirmed it once a confirm
/// has been sent. A later sign-in that only read the instance is not them.
pub fn approved_by(events: &[Event]) -> Option<&Event> {
    let kind = if events.iter().any(|e| e.kind == "sendt") || confirm_unsettled(events) {
        "bekrefter"
    } else {
        "godkjent"
    };
    events.iter().rev().find(|e| e.kind == kind)
}

/// The name to show for that approval: the person Altinn says signed in,
/// once the worker has asked, and until then the regnmed user.
pub fn approver_name(events: &[Event]) -> Option<&str> {
    let approved = approved_by(events)?;
    // A confirm speaks for itself: later events may be another person's attempt.
    let signer = if approved.kind == "bekrefter" {
        approved.detail["signer"].as_str()
    } else {
        let at = events.iter().position(|e| e.id == approved.id)?;
        events[at..]
            .iter()
            .find_map(|e| e.detail["signer"].as_str())
    };
    signer.or_else(|| approved.detail["name"].as_str())
}

/// What a person found in Altinn when they closed an unresolved filing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Finding {
    Delivered,
    NotDelivered,
}

impl Finding {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "levert" => Some(Self::Delivered),
            "ikke_levert" => Some(Self::NotDelivered),
            _ => None,
        }
    }

    fn slug(self) -> &'static str {
        match self {
            Self::Delivered => "levert",
            Self::NotDelivered => "ikke_levert",
        }
    }
}

/// The closing event, if a person closed this filing after checking Altinn.
pub fn closing(events: &[Event]) -> Option<&Event> {
    events
        .iter()
        .rev()
        .find(|e| e.kind == "lukket_etter_kontroll")
}

/// Whether a person's check in Altinn found the return delivered.
fn closed_as_delivered(events: &[Event]) -> bool {
    closing(events).is_some_and(|e| e.detail["funn"] == "levert")
}

/// Whether a person may sign in to take this filing further. Work a live job
/// is doing is left alone, and an outcome is final.
pub fn can_authorize(events: &[Event], job_open: bool) -> bool {
    match status(events) {
        "opprettet" | "feilet" | "ma_godkjennes" | "sendt" => !job_open,
        "godkjent" | "oppretter_instans" | "instans_opprettet" => !job_open,
        // Without an instance there is nothing to read back: see `abandon`.
        "uavklart" => !job_open && instance_url(events).is_some(),
        _ => false,
    }
}

/// Whether the person may close this filing and start over. Never with an
/// instance on record: only reading it can tell whether it was filed.
pub fn can_abandon(events: &[Event], job_open: bool) -> bool {
    !job_open
        && !may_have_reached_altinn(events)
        && matches!(status(events), "opprettet" | "feilet" | "ma_godkjennes")
}

/// Whether a person may close an unresolved filing after checking Altinn.
pub fn can_close_unresolved(events: &[Event], job_open: bool) -> bool {
    !job_open && status(events) == "uavklart"
}

/// The melding for a period as the ledger stands now.
pub struct Built {
    pub message: regnmed_core::vat_return::VatReturn,
    pub xml: String,
}

/// None when the period has no VAT postings.
pub async fn build(
    pool: &PgPool,
    company_id: Uuid,
    scheme: PeriodScheme,
    period: Period,
) -> Result<Option<Built>> {
    let orgnumber: String = sqlx::query_scalar("select orgnr from company where id = $1")
        .bind(company_id)
        .fetch_one(pool)
        .await?;
    let lines =
        crate::vat_specification(pool, company_id, scheme.start(period), scheme.end(period))
            .await?;
    if lines.is_empty() {
        return Ok(None);
    }
    let reference = format!("regnmed-{}-{}-{}", orgnumber, period.year, period.number);
    let message = regnmed_core::vat_return::build(
        &orgnumber,
        period,
        scheme,
        &reference,
        env!("CARGO_PKG_VERSION"),
        &lines,
    );
    let xml = regnmed_core::vat_return::render(&message);
    Ok(Some(Built { message, xml }))
}

/// Whether the ledger still produces exactly the stored melding.
async fn unchanged(pool: &PgPool, filing: &Filing) -> Result<bool> {
    let jan1 = NaiveDate::from_ymd_opt(filing.year, 1, 1).context("filing year")?;
    let scheme = crate::period_scheme_on(pool, filing.company_id, jan1).await?;
    let Some(period) = u8::try_from(filing.period)
        .ok()
        .and_then(|p| scheme.new_period(filing.year, p))
    else {
        return Ok(false);
    };
    Ok(build(pool, filing.company_id, scheme, period)
        .await?
        .is_some_and(|built| built.xml == filing.melding_xml))
}

fn filing_from(row: &sqlx::postgres::PgRow) -> Filing {
    Filing {
        id: row.get("id"),
        company_id: row.get("company_id"),
        year: row.get("year"),
        period: row.get("period"),
        melding_xml: row.get("melding_xml"),
        assessed_kr: row.get("assessed_kr"),
        requested_by: row.get("requested_by"),
        created_at: row.get("created_at"),
    }
}

const COLUMNS: &str =
    "id, company_id, year, period, melding_xml, assessed_kr, requested_by, created_at";

pub async fn get(pool: &PgPool, company_id: Uuid, id: Uuid) -> Result<Option<Filing>> {
    let row = sqlx::query(&format!(
        "select {COLUMNS} from vat_filing where id = $1 and company_id = $2"
    ))
    .bind(id)
    .bind(company_id)
    .fetch_optional(pool)
    .await?;
    Ok(row.as_ref().map(filing_from))
}

pub async fn list(pool: &PgPool, company_id: Uuid, year: i32, period: i32) -> Result<Vec<Filing>> {
    let rows = sqlx::query(&format!(
        "select {COLUMNS} from vat_filing
         where company_id = $1 and year = $2 and period = $3
         order by created_at desc"
    ))
    .bind(company_id)
    .bind(year)
    .bind(period)
    .fetch_all(pool)
    .await?;
    Ok(rows.iter().map(filing_from).collect())
}

pub async fn events<'e>(executor: impl PgExecutor<'e>, filing_id: Uuid) -> Result<Vec<Event>> {
    let rows = sqlx::query(
        "select id, kind, detail, created_at from vat_filing_event
         where filing_id = $1 order by created_at, id",
    )
    .bind(filing_id)
    .fetch_all(executor)
    .await?;
    Ok(rows
        .iter()
        .map(|r| Event {
            id: r.get("id"),
            kind: r.get("kind"),
            detail: r.get("detail"),
            created_at: r.get("created_at"),
        })
        .collect())
}

async fn record<'e>(
    executor: impl PgExecutor<'e>,
    filing_id: Uuid,
    kind: &str,
    detail: Value,
) -> Result<Uuid> {
    let id = Uuid::now_v7();
    sqlx::query(
        "insert into vat_filing_event (id, filing_id, kind, detail, created_at)
         values ($1, $2, $3, $4, clock_timestamp())
         on conflict (filing_id) where kind in ('kvittert', 'avvist') do nothing",
    )
    .bind(id)
    .bind(filing_id)
    .bind(kind)
    .bind(detail)
    .execute(executor)
    .await?;
    Ok(id)
}

async fn lock_period(
    conn: &mut PgConnection,
    company_id: Uuid,
    year: i32,
    period: i32,
) -> Result<()> {
    sqlx::query("select pg_advisory_xact_lock(hashtextextended($1, 0))")
        .bind(format!("vat_filing:{company_id}:{year}:{period}"))
        .execute(conn)
        .await?;
    Ok(())
}

/// Serializes everything that adds to a filing's history.
async fn lock_writes(conn: &mut PgConnection, filing_id: Uuid) -> Result<()> {
    sqlx::query("select pg_advisory_xact_lock(hashtextextended($1, 0))")
        .bind(format!("vat_filing_events:{filing_id}"))
        .execute(conn)
        .await?;
    Ok(())
}

/// The session lock one running `advance` holds for a filing.
pub fn run_lock_key(filing_id: Uuid) -> String {
    format!("vat_filing_run:{filing_id}")
}

pub fn dedup_key(filing_id: Uuid) -> String {
    format!("vat_filing:{filing_id}")
}

pub async fn job_open<'e>(
    executor: impl PgExecutor<'e>,
    company_id: Uuid,
    filing_id: Uuid,
) -> Result<bool> {
    Ok(sqlx::query_scalar(
        "select exists (select 1 from job
         where company_id = $1 and kind = 'vat_filing_submit' and dedup_key = $2
           and status in ('queued', 'running'))",
    )
    .bind(company_id)
    .bind(dedup_key(filing_id))
    .fetch_one(executor)
    .await?)
}

pub enum Prepared {
    Ready(Filing),
    /// Why this period cannot be filed now, in the user's language.
    Refused(String),
}

/// The filing to approve for a period: the open one if the melding is
/// unchanged, otherwise a new one. A period that is sent, or whose last
/// attempt may have been, is refused.
#[allow(clippy::too_many_arguments)]
pub async fn prepare(
    pool: &PgPool,
    company_id: Uuid,
    year: i32,
    period: i32,
    melding_xml: &str,
    assessed_kr: i64,
    person_id: Uuid,
    requested_by: &str,
) -> Result<Prepared> {
    let mut tx = pool.begin().await?;
    // Two clicks at once must not store two filings for the same period.
    lock_period(&mut tx, company_id, year, period).await?;
    let latest = sqlx::query(&format!(
        "select {COLUMNS} from vat_filing
         where company_id = $1 and year = $2 and period = $3
         order by created_at desc limit 1"
    ))
    .bind(company_id)
    .bind(year)
    .bind(period)
    .fetch_optional(&mut *tx)
    .await?;
    if let Some(row) = &latest {
        let filing = filing_from(row);
        lock_writes(&mut tx, filing.id).await?;
        let past = events(&mut *tx, filing.id).await?;
        match status(&past) {
            "sendt" | "kvittert" => {
                return Ok(Prepared::Refused(
                    "Mva-meldingen for denne terminen er allerede sendt inn.".into(),
                ));
            }
            "lukket_etter_kontroll" if closed_as_delivered(&past) => {
                return Ok(Prepared::Refused(
                    "Mva-meldingen for denne terminen er levert ifølge kontrollen i Altinn, så \
                     den kan ikke sendes på nytt herfra."
                        .into(),
                ));
            }
            "godkjent" | "oppretter_instans" | "instans_opprettet" | "uavklart" => {
                return Ok(Prepared::Refused(
                    "Mva-meldingen for denne terminen er under innsending. Se status under.".into(),
                ));
            }
            "opprettet" | "feilet" | "ma_godkjennes" => {
                if filing.melding_xml == melding_xml {
                    return Ok(Prepared::Ready(filing));
                }
                if instance_url(&past).is_some() {
                    return Ok(Prepared::Refused(
                        "En innsending for terminen er påbegynt i Altinn. Fortsett den først, \
                         så avsluttes den hvis regnskapet er endret."
                            .into(),
                    ));
                }
                record(
                    &mut *tx,
                    filing.id,
                    "avbrutt",
                    json!({ "grunn": "regnskapet er endret" }),
                )
                .await?;
                sqlx::query("delete from vat_filing_grant where filing_id = $1")
                    .bind(filing.id)
                    .execute(&mut *tx)
                    .await?;
            }
            _ => {}
        }
    }
    let id = Uuid::now_v7();
    sqlx::query(
        "insert into vat_filing (id, company_id, year, period, melding_xml, melding_sha256,
                                 assessed_kr, requested_by_person, requested_by)
         values ($1, $2, $3, $4, $5, $6, $7, $8, $9)",
    )
    .bind(id)
    .bind(company_id)
    .bind(year)
    .bind(period)
    .bind(melding_xml)
    .bind(regnmed_core::hash::sha256(melding_xml.as_bytes()).to_vec())
    .bind(assessed_kr)
    .bind(person_id)
    .bind(requested_by)
    .execute(&mut *tx)
    .await?;
    record(&mut *tx, id, "opprettet", json!({})).await?;
    tx.commit().await?;
    Ok(Prepared::Ready(
        get(pool, company_id, id)
            .await?
            .context("the filing just stored")?,
    ))
}

/// Remembers a sign-in in progress. A later attempt replaces an earlier one.
pub async fn save_login(
    pool: &PgPool,
    filing_id: Uuid,
    person_id: Uuid,
    state_sha256: &[u8],
    sealed: &[u8],
    expires_at: DateTime<Utc>,
) -> Result<()> {
    sqlx::query(
        "insert into vat_filing_grant (filing_id, person_id, kind, state_sha256, sealed, expires_at)
         values ($1, $2, 'innlogging', $3, $4, $5)
         on conflict (filing_id) do update
         set person_id = excluded.person_id, kind = excluded.kind,
             state_sha256 = excluded.state_sha256, sealed = excluded.sealed,
             expires_at = excluded.expires_at",
    )
    .bind(filing_id)
    .bind(person_id)
    .bind(state_sha256)
    .bind(sealed)
    .bind(expires_at)
    .execute(pool)
    .await?;
    Ok(())
}

/// Uses up a sign-in, once, and only for the person who started it.
pub async fn take_login(
    pool: &PgPool,
    filing_id: Uuid,
    person_id: Uuid,
    state_sha256: &[u8],
) -> Result<Option<Vec<u8>>> {
    Ok(sqlx::query_scalar(
        "delete from vat_filing_grant
         where filing_id = $1 and person_id = $2 and kind = 'innlogging'
           and state_sha256 = $3 and expires_at > now()
         returning sealed",
    )
    .bind(filing_id)
    .bind(person_id)
    .bind(state_sha256)
    .fetch_optional(pool)
    .await?)
}

/// Who approved. `sub` is ID-porten's pairwise id, not the national ID.
#[derive(Debug, Serialize)]
pub struct Approver {
    pub person_id: Uuid,
    pub name: String,
    pub sub: String,
    pub acr: String,
    pub auth_time: DateTime<Utc>,
    /// The envelope naming this person, sent with the melding.
    pub envelope_xml: String,
}

async fn lock_identity(
    conn: &mut PgConnection,
    person_id: Uuid,
    issuer: &str,
    client_id: &str,
) -> Result<()> {
    sqlx::query("select pg_advisory_xact_lock(hashtextextended($1, 0))")
        .bind(format!(
            "idporten_identity:{person_id}:{issuer}:{client_id}"
        ))
        .execute(conn)
        .await?;
    Ok(())
}

/// The newest row: `None` never bound, `Some(None)` released by an operator.
async fn bound_identity(
    conn: &mut PgConnection,
    person_id: Uuid,
    issuer: &str,
    client_id: &str,
) -> Result<Option<Option<String>>> {
    Ok(sqlx::query_scalar(
        "select sub from idporten_identity
         where person_id = $1 and issuer = $2 and client_id = $3
         order by created_at desc, id desc limit 1",
    )
    .bind(person_id)
    .bind(issuer)
    .bind(client_id)
    .fetch_optional(conn)
    .await?)
}

/// Releases a person's ID-porten binding, so their next sign-in binds anew.
/// Only ever a new row: who was bound before, and who changed it, stays.
pub async fn rebind_identity(
    pool: &PgPool,
    person_id: Uuid,
    issuer: &str,
    client_id: &str,
    operator: &str,
    reason: &str,
) -> Result<()> {
    let (operator, reason) = (operator.trim(), reason.trim());
    anyhow::ensure!(!operator.is_empty(), "the operator must be named");
    anyhow::ensure!(!reason.is_empty(), "a rebind needs a reason");
    let mut tx = pool.begin().await?;
    lock_identity(&mut tx, person_id, issuer, client_id).await?;
    anyhow::ensure!(
        matches!(
            bound_identity(&mut tx, person_id, issuer, client_id).await?,
            Some(Some(_))
        ),
        "this person has no ID-porten binding for that issuer and client"
    );
    sqlx::query(
        "insert into idporten_identity
             (id, person_id, issuer, client_id, source, reason, recorded_by)
         values ($1, $2, $3, $4, 'operator', $5, $6)",
    )
    .bind(Uuid::now_v7())
    .bind(person_id)
    .bind(issuer)
    .bind(client_id)
    .bind(reason)
    .bind(operator)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(())
}

pub enum ApprovalOutcome {
    Approved {
        job_id: Uuid,
    },
    /// The filing moved on while the person signed in.
    NotApprovable,
    /// Signed in as someone other than this regnmed person's identity.
    WrongIdentity,
}

/// Records the approval, holds the sealed token and queues the work, all
/// or nothing. Decided again here, under the period lock, because the
/// filing may have been replaced while the person was at ID-porten.
#[allow(clippy::too_many_arguments)]
pub async fn approve(
    pool: &PgPool,
    filing: &Filing,
    issuer: &str,
    client_id: &str,
    approver: &Approver,
    sealed_token: &[u8],
    expires_at: DateTime<Utc>,
) -> Result<ApprovalOutcome> {
    let mut tx = pool.begin().await?;
    lock_period(&mut tx, filing.company_id, filing.year, filing.period).await?;
    lock_writes(&mut tx, filing.id).await?;
    let past = events(&mut *tx, filing.id).await?;
    if !can_authorize(
        &past,
        job_open(&mut *tx, filing.company_id, filing.id).await?,
    ) {
        return Ok(ApprovalOutcome::NotApprovable);
    }
    lock_identity(&mut tx, approver.person_id, issuer, client_id).await?;
    match bound_identity(&mut tx, approver.person_id, issuer, client_id).await? {
        Some(Some(sub)) if sub != approver.sub => return Ok(ApprovalOutcome::WrongIdentity),
        Some(Some(_)) => {}
        // Never bound, or released by an operator: this sign-in binds it.
        _ => {
            sqlx::query(
                "insert into idporten_identity
                     (id, person_id, issuer, client_id, sub, source, recorded_by)
                 values ($1, $2, $3, $4, $5, 'godkjenning', $6)",
            )
            .bind(Uuid::now_v7())
            .bind(approver.person_id)
            .bind(issuer)
            .bind(client_id)
            .bind(&approver.sub)
            .bind(&approver.name)
            .execute(&mut *tx)
            .await?;
        }
    }
    sqlx::query(
        "insert into vat_filing_grant (filing_id, person_id, kind, sealed, expires_at)
         values ($1, $2, 'idporten', $3, $4)
         on conflict (filing_id) do update
         set person_id = excluded.person_id, kind = 'idporten', state_sha256 = null,
             sealed = excluded.sealed, expires_at = excluded.expires_at",
    )
    .bind(filing.id)
    .bind(approver.person_id)
    .bind(sealed_token)
    .bind(expires_at)
    .execute(&mut *tx)
    .await?;
    let kind = if past.iter().any(|e| e.kind == "sendt") {
        "fornyet"
    } else {
        "godkjent"
    };
    record(&mut *tx, filing.id, kind, serde_json::to_value(approver)?).await?;
    let job_id = crate::job::insert_open(
        &mut tx,
        filing.company_id,
        crate::job::Kind::VatFilingSubmit,
        &dedup_key(filing.id),
        &json!({ "filing_id": filing.id }),
        &approver.name,
        Some(approver.person_id),
    )
    .await?;
    tx.commit().await?;
    Ok(ApprovalOutcome::Approved { job_id })
}

/// False when the filing can no longer be abandoned.
pub async fn abandon(pool: &PgPool, filing: &Filing, by: &str) -> Result<bool> {
    let mut tx = pool.begin().await?;
    lock_period(&mut tx, filing.company_id, filing.year, filing.period).await?;
    lock_writes(&mut tx, filing.id).await?;
    let past = events(&mut *tx, filing.id).await?;
    if !can_abandon(
        &past,
        job_open(&mut *tx, filing.company_id, filing.id).await?,
    ) {
        return Ok(false);
    }
    record(&mut *tx, filing.id, "avbrutt", json!({ "av": by })).await?;
    sqlx::query("delete from vat_filing_grant where filing_id = $1")
        .bind(filing.id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(true)
}

/// A person has checked Altinn and closes an unresolved filing. This records
/// what they found and changes nothing in Altinn. Only `ikke_levert` lets a
/// new filing start; `levert` keeps the term closed, as their finding and not
/// as a receipt regnmed has seen.
pub async fn close_unresolved(
    pool: &PgPool,
    filing: &Filing,
    person_id: Uuid,
    name: &str,
    finding: Finding,
    note: &str,
) -> Result<bool> {
    let mut tx = pool.begin().await?;
    lock_period(&mut tx, filing.company_id, filing.year, filing.period).await?;
    lock_writes(&mut tx, filing.id).await?;
    let past = events(&mut *tx, filing.id).await?;
    if !can_close_unresolved(
        &past,
        job_open(&mut *tx, filing.company_id, filing.id).await?,
    ) {
        return Ok(false);
    }
    record(
        &mut *tx,
        filing.id,
        "lukket_etter_kontroll",
        json!({ "person_id": person_id, "name": name, "funn": finding.slug(), "notat": note }),
    )
    .await?;
    sqlx::query("delete from vat_filing_grant where filing_id = $1")
        .bind(filing.id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(true)
}

/// Whether the worker can still fetch Skatteetaten's answer by itself.
pub async fn fetches_answer(pool: &PgPool, filing_id: Uuid) -> Result<bool> {
    Ok(sqlx::query_scalar(
        "select exists (select 1 from vat_filing_grant
         where filing_id = $1 and kind = 'altinn' and expires_at > now())",
    )
    .bind(filing_id)
    .fetch_one(pool)
    .await?)
}

pub async fn documents(pool: &PgPool, filing_id: Uuid) -> Result<Vec<String>> {
    Ok(sqlx::query_scalar(
        "select data_type from vat_filing_document where filing_id = $1 order by data_type",
    )
    .bind(filing_id)
    .fetch_all(pool)
    .await?)
}

pub async fn document(
    pool: &PgPool,
    filing_id: Uuid,
    data_type: &str,
) -> Result<Option<(Vec<u8>, String)>> {
    let row = sqlx::query(
        "select content, content_type from vat_filing_document
         where filing_id = $1 and data_type = $2",
    )
    .bind(filing_id)
    .bind(data_type)
    .fetch_optional(pool)
    .await?;
    Ok(row.map(|r| (r.get("content"), r.get("content_type"))))
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SubmitInput {
    pub filing_id: Uuid,
}

/// This run's view of the history is no longer the latest.
#[derive(Debug)]
struct StaleRun;

/// The run no longer holds its lock, so another run may have the filing.
#[derive(Debug)]
struct LostLock;

impl std::fmt::Display for LostLock {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("the run lost its lock on the filing")
    }
}

impl std::error::Error for LostLock {}

impl std::fmt::Display for StaleRun {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("the filing moved on while this run worked")
    }
}

impl std::error::Error for StaleRun {}

/// One worker's turn with a filing. It holds a session lock on its own
/// connection, and each write first checks that nothing was added since the
/// run's last one, so a run that lost its lock cannot overwrite a newer state.
struct Run<'a> {
    pool: &'a PgPool,
    rail: &'a Rail,
    filing: &'a Filing,
    past: Vec<Event>,
    last: Option<Uuid>,
    /// An instance exists, so a failed step cannot claim nothing was sent.
    has_instance: bool,
    lock: PoolConnection<Postgres>,
}

impl<'a> Run<'a> {
    /// None when another run still has the filing after `wait`.
    async fn start(
        pool: &'a PgPool,
        rail: &'a Rail,
        filing: &'a Filing,
        wait: std::time::Duration,
    ) -> Result<Option<Run<'a>>> {
        let mut lock = pool.acquire().await?;
        // Closed instead of pooled, so the session lock ends with the run
        // even if the task is aborted.
        lock.close_on_drop();
        let deadline = std::time::Instant::now() + wait;
        loop {
            let held: bool =
                sqlx::query_scalar("select pg_try_advisory_lock(hashtextextended($1, 0))")
                    .bind(run_lock_key(filing.id))
                    .fetch_one(&mut *lock)
                    .await?;
            if held {
                break;
            }
            if std::time::Instant::now() >= deadline {
                return Ok(None);
            }
            tokio::time::sleep(LOCK_POLL).await;
        }
        let past = events(pool, filing.id).await?;
        let last = past.last().map(|e| e.id);
        let has_instance = instance_url(&past).is_some();
        Ok(Some(Run {
            pool,
            rail,
            filing,
            past,
            last,
            has_instance,
            lock,
        }))
    }

    /// Called right before every call that changes something in Altinn:
    /// this run must still hold its lock, and nothing may have been added.
    async fn ensure_live(&mut self) -> Result<()> {
        let held: Result<bool, _> = sqlx::query_scalar(
            "select exists (select 1 from pg_locks
             where locktype = 'advisory' and pid = pg_backend_pid() and granted
               and objsubid = 1
               and ((classid::bigint << 32) | objid::bigint) = hashtextextended($1, 0))",
        )
        .bind(run_lock_key(self.filing.id))
        .fetch_one(&mut *self.lock)
        .await;
        if !matches!(held, Ok(true)) {
            return Err(LostLock.into());
        }
        self.guarded().await?.rollback().await?;
        Ok(())
    }

    async fn guarded(&self) -> Result<Transaction<'a, Postgres>> {
        let mut tx = self.pool.begin().await?;
        lock_writes(&mut tx, self.filing.id).await?;
        let latest: Option<Uuid> = sqlx::query_scalar(
            "select id from vat_filing_event where filing_id = $1
             order by created_at desc, id desc limit 1",
        )
        .bind(self.filing.id)
        .fetch_optional(&mut *tx)
        .await?;
        if latest != self.last {
            return Err(StaleRun.into());
        }
        Ok(tx)
    }

    /// Adds to the history; the work goes on.
    async fn note(&mut self, kind: &str, detail: Value) -> Result<()> {
        let mut tx = self.guarded().await?;
        let id = record(&mut *tx, self.filing.id, kind, detail).await?;
        tx.commit().await?;
        self.last = Some(id);
        Ok(())
    }

    /// Ends this run's use of the token with this status.
    async fn close(&mut self, kind: &str, detail: Value) -> Result<String> {
        let mut tx = self.guarded().await?;
        let id = record(&mut *tx, self.filing.id, kind, detail).await?;
        sqlx::query("delete from vat_filing_grant where filing_id = $1")
            .bind(self.filing.id)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        self.last = Some(id);
        Ok(kind.to_string())
    }

    async fn clear_grant(&self) -> Result<()> {
        let mut tx = self.guarded().await?;
        sqlx::query("delete from vat_filing_grant where filing_id = $1")
            .bind(self.filing.id)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(())
    }

    /// Skatteetaten answers after the person has left, so the Altinn token
    /// is kept sealed until the answer is in or the token runs out.
    async fn hold_for_feedback(&self, altinn: &str) -> Result<()> {
        let expires_at = token_expiry(altinn).unwrap_or_else(|| Utc::now() + Duration::minutes(30));
        let sealed = seal(&self.rail.key, altinn.as_bytes())?;
        let mut tx = self.guarded().await?;
        sqlx::query(
            "update vat_filing_grant set kind = 'altinn', state_sha256 = null,
                    sealed = $2, expires_at = $3
             where filing_id = $1",
        )
        .bind(self.filing.id)
        .bind(sealed)
        .bind(expires_at)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(())
    }
}

async fn usable_grant(
    pool: &PgPool,
    key: &SealingKey,
    filing_id: Uuid,
) -> Result<Option<(String, String)>> {
    let row = sqlx::query(
        "select kind, sealed from vat_filing_grant
         where filing_id = $1 and kind <> 'innlogging' and expires_at > now()",
    )
    .bind(filing_id)
    .fetch_optional(pool)
    .await?;
    let Some(row) = row else { return Ok(None) };
    let token = String::from_utf8(open(key, row.get::<&[u8], _>("sealed"))?)?;
    Ok(Some((row.get("kind"), token)))
}

const CREATE_UNKNOWN: &str =
    "Vi vet ikke om Altinn opprettet innsendingen. Sjekk Altinn før du starter på nytt.";
const STANDING_UNKNOWN: &str = "Vi vet ikke hvor innsendingen står i Altinn. Sjekk Altinn før du \
    gjør noe annet.";
const CONFIRM_UNKNOWN: &str = "Bekreftelsen fikk ikke sikkert svar, så utkastet er slettet i \
    Altinn og ingenting ble sendt. Godkjenn terminen på nytt.";
const LEDGER_CHANGED: &str =
    "Regnskapet er endret, så denne innsendingen ble ikke sendt. Godkjenn terminen på nytt.";
const OTHER_APPROVER: &str = "Meldingen ble fylt ut i navnet til en annen person, så den ble ikke \
    bekreftet. Ingenting ble sendt. Start en ny innsending for å godkjenne den selv.";

/// Ends the run on a failed call. Only a step that changes something can
/// leave the filing uncertain; a refusal or a failed read cannot.
async fn stop(run: &mut Run<'_>, error: FilingError, changes_state: bool) -> Result<String> {
    eprintln!("vat filing {}: {error}", run.filing.id);
    let (kind, detail) = match error {
        FilingError::Unresolved(_) if changes_state => (
            "uavklart",
            json!({ "melding": "Altinn svarte ikke sikkert. Innsendingen er stoppet til det er \
                                 avklart om steget gikk gjennom." }),
        ),
        FilingError::Refused {
            status: 401 | 403, ..
        } => (
            "feilet",
            json!({ "melding": NEEDS_RIGHT, "mangler_altinn_rett": true }),
        ),
        _ if run.has_instance => (
            "feilet",
            json!({ "melding": "Altinn kunne ikke fullføre steget. Sjekk Altinn før du gjør noe \
                                 annet, og logg inn for å fortsette." }),
        ),
        _ => (
            "feilet",
            json!({ "melding": "Altinn kunne ikke fullføre steget. Ingenting er sendt inn. \
                                 Prøv igjen." }),
        ),
    };
    run.close(kind, detail).await
}

/// The token could not be used. This says nothing about what was sent, so
/// a filing that may have reached Altinn closes as unresolved.
async fn needs_new_approval(run: &mut Run<'_>, past: &[Event]) -> Result<String> {
    if status(past) == "uavklart" {
        run.clear_grant().await?;
        return Ok("uavklart".into());
    }
    if instance_url(past).is_some() {
        return run
            .close("uavklart", json!({ "melding": STANDING_UNKNOWN }))
            .await;
    }
    if create_unaccounted(past) {
        return run
            .close("uavklart", json!({ "melding": CREATE_UNKNOWN }))
            .await;
    }
    run.close(
        "ma_godkjennes",
        json!({ "melding": "Innloggingen kunne ikke brukes. Logg inn på nytt for å fortsette." }),
    )
    .await
}

/// A confirm was sent and Altinn has not definitely refused that very call,
/// so it may still land. No later read or failure can change that.
fn confirm_unsettled(events: &[Event]) -> bool {
    events
        .iter()
        .rev()
        .find(|e| matches!(e.kind.as_str(), "bekrefter" | "bekreftelse_avvist"))
        .is_some_and(|e| e.kind == "bekrefter")
}

/// Whether an earlier step may still take effect.
fn step_unsettled(events: &[Event]) -> bool {
    events.iter().any(|e| e.kind == "uavklart") || confirm_unsettled(events)
}

/// Closes a filing whose instance reads as unconfirmed. If an earlier step
/// may still be under way, that reading proves nothing until the draft is
/// actually gone, so the filing only closes once the delete is confirmed.
async fn abort_unsent(run: &mut Run<'_>, altinn: &str, url: &str, message: &str) -> Result<String> {
    run.ensure_live().await?;
    if step_unsettled(&run.past) {
        return match run.rail.client.delete_instance(altinn, url).await {
            Ok(()) => run.close("avbrutt", json!({ "melding": message })).await,
            Err(e) => {
                eprintln!("vat filing {}: draft not removed: {e}", run.filing.id);
                run.close("uavklart", json!({ "melding": STANDING_UNKNOWN }))
                    .await
            }
        };
    }
    let closed = run.close("avbrutt", json!({ "melding": message })).await?;
    if let Err(e) = run.rail.client.delete_instance(altinn, url).await {
        eprintln!("vat filing {}: draft not removed: {e}", run.filing.id);
    }
    Ok(closed)
}

/// Does what is left of an approved filing.
async fn advance(run: &mut Run<'_>) -> Result<String> {
    let (pool, rail, filing) = (run.pool, run.rail, run.filing);
    let past = run.past.clone();
    let current = status(&past).to_string();
    if matches!(
        current.as_str(),
        "kvittert" | "avvist" | "avvik" | "avbrutt" | "lukket_etter_kontroll"
    ) {
        return Ok(current);
    }
    let sent = past.iter().any(|e| e.kind == "sendt");
    let Some((kind, token)) = usable_grant(pool, &rail.key, filing.id).await? else {
        if sent {
            return Ok(current);
        }
        return needs_new_approval(run, &past).await;
    };
    let altinn = if kind == "altinn" {
        token.clone()
    } else {
        match rail.client.altinn_token(&token).await {
            Ok(t) => t,
            Err(e) => {
                eprintln!("vat filing {}: token exchange failed: {e}", filing.id);
                if sent {
                    run.clear_grant().await?;
                    return Ok(current);
                }
                return needs_new_approval(run, &past).await;
            }
        }
    };
    if sent {
        run.hold_for_feedback(&altinn).await?;
        return collect_feedback(run, &past, &altinn).await;
    }
    let mut signer = None;

    let (url, mut instance) = match instance_url(&past) {
        Some(url) => {
            let instance = match rail.client.instance(&altinn, url).await {
                Ok(instance) => instance,
                Err(
                    e @ FilingError::Refused {
                        status: 401 | 403, ..
                    },
                ) => {
                    return stop(run, e, false).await;
                }
                // Not knowing where it stands is not the same as not filed.
                Err(e) => {
                    return stop(run, FilingError::Unresolved(e.to_string()), true).await;
                }
            };
            // Still unsent, and the books have moved: these numbers must not go in.
            if matches!(instance.task(), Some(FILLING_TASK | CONFIRM_TASK))
                && !unchanged(pool, filing).await?
            {
                return abort_unsent(run, &altinn, url, LEDGER_CHANGED).await;
            }
            (url.to_string(), instance)
        }
        None => {
            if create_unaccounted(&past) {
                return run
                    .close("uavklart", json!({ "melding": CREATE_UNKNOWN }))
                    .await;
            }
            // Asked before anything exists in Altinn, so a failure leaves no draft.
            signer = match signer_name(run, &altinn).await {
                Ok(name) => Some(name),
                Err(e) => return stop(run, e, false).await,
            };
            let outcome = match rail
                .client
                .validation
                .validate_with_token(&token, &filing.melding_xml)
                .await
            {
                Ok(outcome) => outcome,
                Err(e) => {
                    eprintln!("vat filing {}: validation failed: {e:#}", filing.id);
                    return run
                        .close(
                            "feilet",
                            json!({ "melding": "Skatteetatens kontroll svarte ikke. Ingenting \
                                                 er sendt inn. Prøv igjen." }),
                        )
                        .await;
                }
            };
            if !outcome.valid {
                return run
                    .close(
                        "avvik",
                        json!({ "avvik": outcome.findings, "signer": signer }),
                    )
                    .await;
            }
            if !unchanged(pool, filing).await? {
                return run
                    .close("avbrutt", json!({ "melding": LEDGER_CHANGED }))
                    .await;
            }
            let orgnumber: String = sqlx::query_scalar("select orgnr from company where id = $1")
                .bind(filing.company_id)
                .fetch_one(pool)
                .await?;
            run.ensure_live().await?;
            run.note("oppretter_instans", json!({})).await?;
            run.ensure_live().await?;
            let instance = match rail.client.create_instance(&altinn, &orgnumber).await {
                Ok(instance) => instance,
                Err(e) => return stop(run, e, true).await,
            };
            let url = rail.client.instance_url(&instance.id);
            // Committed before the first upload: from here a retry resumes it.
            run.note("instans_opprettet", json!({ "url": url })).await?;
            run.has_instance = true;
            (url, instance)
        }
    };

    let approval = past
        .iter()
        .rev()
        .find(|e| e.kind == "godkjent")
        .context("an approved filing has an approval")?;
    let approver = approval.detail["person_id"].as_str().map(str::to_string);
    // Who the envelope in Altinn names. Noted before the upload, so a crash
    // between the two can only make a later run upload it again.
    let mut envelope_names = past
        .iter()
        .rev()
        .find(|e| e.kind == "konvolutt")
        .and_then(|e| e.detail["person_id"].as_str().map(str::to_string));
    let mut envelope_signer = past
        .iter()
        .rev()
        .find(|e| e.kind == "konvolutt")
        .and_then(|e| e.detail["signer"].as_str().map(str::to_string));

    if instance.task() == Some(FILLING_TASK) {
        let name = match signer.clone() {
            Some(name) => name,
            None => match signer_name(run, &altinn).await {
                Ok(name) => name,
                Err(e) => return stop(run, e, false).await,
            },
        };
        let envelope = regnmed_core::vat_return::name_envelope_signer(
            approval.detail["envelope_xml"]
                .as_str()
                .context("an approved filing has an envelope")?,
            &name,
        );
        let Some(element) = instance.element(ENVELOPE_DATA_TYPE).map(|e| e.id.clone()) else {
            return stop(
                run,
                FilingError::Refused {
                    status: 422,
                    body: "the instance has no envelope element".into(),
                },
                false,
            )
            .await;
        };
        run.note(
            "konvolutt",
            json!({ "person_id": approver, "signer": name }),
        )
        .await?;
        envelope_names = approver.clone();
        signer = Some(name.clone());
        envelope_signer = Some(name);
        run.ensure_live().await?;
        if let Err(e) = rail
            .client
            .put_envelope(&altinn, &url, &element, &envelope)
            .await
        {
            return stop(run, e, false).await;
        }
        if instance.element(MELDING_DATA_TYPE).is_none() {
            run.ensure_live().await?;
            if let Err(e) = rail
                .client
                .upload_melding(&altinn, &url, &filing.melding_xml)
                .await
            {
                return stop(run, e, true).await;
            }
        }
    }

    // Filling, then confirming. Read back after each step, so a step that
    // already happened is never taken twice.
    for _ in 0..2 {
        let confirming = match instance.task() {
            Some(FILLING_TASK) => false,
            Some(CONFIRM_TASK) => {
                if envelope_names.is_none() || envelope_names != approver {
                    return abort_unsent(run, &altinn, &url, OTHER_APPROVER).await;
                }
                // Reading Task_2 does not prove an earlier confirm is dead, so
                // it is never sent again: the draft must go first.
                if confirm_unsettled(&run.past) {
                    return abort_unsent(run, &altinn, &url, CONFIRM_UNKNOWN).await;
                }
                // A resumed run holds a new token, and a regnmed account can
                // be rebound to another ID-porten person.
                let confirming_signer = match signer.clone() {
                    Some(name) => name,
                    None => match signer_name(run, &altinn).await {
                        Ok(name) => name,
                        Err(e) => return stop(run, e, false).await,
                    },
                };
                if envelope_signer.as_deref() != Some(confirming_signer.as_str()) {
                    return abort_unsent(run, &altinn, &url, OTHER_APPROVER).await;
                }
                if !unchanged(pool, filing).await? {
                    return abort_unsent(run, &altinn, &url, LEDGER_CHANGED).await;
                }
                // From here the confirmation may land even if its answer is lost.
                run.note(
                    "bekrefter",
                    json!({
                        "person_id": approver,
                        "name": approval.detail["name"],
                        "signer": envelope_signer,
                    }),
                )
                .await?;
                true
            }
            _ => break,
        };
        run.ensure_live().await?;
        match rail.client.process_next(&altinn, &url).await {
            Ok(()) => {}
            Err(FilingError::Refused { status, body }) => {
                // Altinn refused this very confirm, so it can no longer land.
                if confirming {
                    run.note("bekreftelse_avvist", json!({ "status": status }))
                        .await?;
                }
                if status == 409 {
                    let findings = extract_findings(&body);
                    if !findings.is_empty() {
                        return run.close("avvik", json!({ "avvik": findings })).await;
                    }
                }
                return stop(run, FilingError::Refused { status, body }, false).await;
            }
            Err(e) => return stop(run, e, true).await,
        }
        // The step succeeded, so any failure to read it back is uncertainty.
        instance = match rail.client.instance(&altinn, &url).await {
            Ok(instance) => instance,
            Err(e) => {
                return stop(run, FilingError::Unresolved(e.to_string()), true).await;
            }
        };
    }
    if !instance.is_submitted() {
        return stop(
            run,
            FilingError::Unresolved("the process is not in a documented filed state".into()),
            true,
        )
        .await;
    }

    run.note("sendt", json!({})).await?;
    run.hold_for_feedback(&altinn).await?;
    let past = events(pool, filing.id).await?;
    collect_feedback(run, &past, &altinn).await
}

/// A profile that cannot be read says nothing about the right to file.
async fn signer_name(run: &Run<'_>, altinn: &str) -> Result<String, FilingError> {
    run.rail
        .client
        .signer_name(altinn)
        .await
        .map_err(|e| FilingError::Unresolved(format!("the signer's name: {e}")))
}

async fn store_files(
    run: &Run<'_>,
    token: &str,
    url: &str,
    instance: &Instance,
) -> Result<(), FilingError> {
    let stored: Vec<String> =
        sqlx::query_scalar("select data_type from vat_filing_document where filing_id = $1")
            .bind(run.filing.id)
            .fetch_all(run.pool)
            .await
            .map_err(|e| FilingError::Unresolved(e.to_string()))?;
    for data_type in FEEDBACK_FILES {
        if stored.iter().any(|s| s == data_type) {
            continue;
        }
        let Some(element) = instance.element(data_type) else {
            continue;
        };
        let (bytes, content_type) = run.rail.client.document(token, url, element).await?;
        sqlx::query(
            "insert into vat_filing_document (filing_id, data_type, content_type, content)
             values ($1, $2, $3, $4) on conflict do nothing",
        )
        .bind(run.filing.id)
        .bind(data_type)
        .bind(content_type)
        .bind(bytes)
        .execute(run.pool)
        .await
        .map_err(|e| FilingError::Unresolved(e.to_string()))?;
    }
    Ok(())
}

/// Picks up whatever Skatteetaten has produced. Not ready, or a failed
/// read, leaves the filing `sendt` for the next round.
async fn collect_feedback(run: &mut Run<'_>, past: &[Event], token: &str) -> Result<String> {
    let Some(url) = instance_url(past).map(str::to_string) else {
        return Ok("sendt".into());
    };
    let read = async {
        let ready = run.rail.client.feedback_ready(token, &url).await?;
        let instance = run.rail.client.instance(token, &url).await?;
        store_files(run, token, &url, &instance).await?;
        Ok::<_, FilingError>((ready, instance))
    }
    .await;
    let (ready, instance) = match read {
        Ok(answer) => answer,
        Err(e) => {
            eprintln!("vat filing {}: feedback not read: {e}", run.filing.id);
            return Ok("sendt".into());
        }
    };
    if !ready {
        return Ok("sendt".into());
    }
    let result = document(run.pool, run.filing.id, "valideringsresultat")
        .await?
        .map(|(bytes, _)| String::from_utf8_lossy(&bytes).into_owned());
    let findings = result.as_deref().map(extract_findings).unwrap_or_default();
    let kind = if instance.element("kvittering").is_some() {
        "kvittert"
    } else if result.as_deref().is_some_and(is_rejection) {
        "avvist"
    } else {
        // No receipt and no error yet: the answer is not complete.
        return Ok("sendt".into());
    };
    run.close(kind, json!({ "avvik": findings })).await
}

pub(crate) async fn run_submit(pool: &PgPool, job: &Claimed, runtime: &Runtime) -> Result<Value> {
    let rail = runtime
        .vat_filing
        .as_deref()
        .context("mva-innsending er ikke slått på i arbeideren")?;
    let input: SubmitInput = crate::job::json_input(pool, job.id).await?;
    let filing = get(pool, job.company_id, input.filing_id)
        .await?
        .context("ukjent mva-innsending")?;
    // Another run has it: the job stays open and comes back, so nobody is
    // told it finished while that run is still working.
    let Some(mut run) = Run::start(pool, rail, &filing, rail.lock_wait).await? else {
        return Err(RetryLater.into());
    };
    match advance(&mut run).await {
        Ok(status) => Ok(json!({ "filing_id": filing.id, "status": status })),
        Err(e) if e.is::<LostLock>() => Err(RetryLater.into()),
        Err(e) if e.is::<StaleRun>() => {
            let now = events(pool, filing.id).await?;
            Ok(json!({ "filing_id": filing.id, "status": status(&now), "utdatert": true }))
        }
        Err(e) => Err(e),
    }
}

/// Clears tokens that ran out, and collects answers for filings already sent.
pub async fn reconcile(pool: &PgPool, rail: &Rail) -> Result<()> {
    sqlx::query("delete from vat_filing_grant where expires_at <= now()")
        .execute(pool)
        .await?;
    let waiting: Vec<(Uuid, Uuid)> = sqlx::query_as(
        "select f.id, f.company_id from vat_filing f
         join vat_filing_grant g on g.filing_id = f.id
         where g.kind = 'altinn'
         order by f.created_at",
    )
    .fetch_all(pool)
    .await?;
    for (id, company_id) in waiting {
        // One filing's trouble must not stop the others.
        if let Err(e) = reconcile_one(pool, rail, id, company_id).await {
            eprintln!("vat filing {id}: reconciliation failed: {e:#}");
        }
    }
    Ok(())
}

async fn reconcile_one(pool: &PgPool, rail: &Rail, id: Uuid, company_id: Uuid) -> Result<()> {
    let Some(filing) = get(pool, company_id, id).await? else {
        return Ok(());
    };
    let Some(mut run) = Run::start(pool, rail, &filing, std::time::Duration::ZERO).await? else {
        return Ok(());
    };
    let past = run.past.clone();
    if status(&past) != "sendt" {
        return run.clear_grant().await;
    }
    if let Some((_, token)) = usable_grant(pool, &rail.key, id).await? {
        collect_feedback(&mut run, &past, &token).await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(kind: &str, detail: Value) -> Event {
        Event {
            id: Uuid::now_v7(),
            kind: kind.into(),
            detail,
            created_at: Utc::now(),
        }
    }

    #[test]
    fn a_renewed_sign_in_never_moves_the_status() {
        let events = [
            event("sendt", json!({})),
            event("fornyet", json!({})),
            event("konvolutt", json!({})),
        ];
        assert_eq!(status(&events), "sendt");
    }

    #[test]
    fn an_unaccounted_create_is_closed_only_by_a_clear_answer() {
        let sent = event("oppretter_instans", json!({}));
        assert!(create_unaccounted(std::slice::from_ref(&sent)));
        assert!(create_unaccounted(&[
            sent.clone(),
            event("uavklart", json!({})),
            event("godkjent", json!({})),
        ]));
        assert!(!create_unaccounted(&[
            sent.clone(),
            event("feilet", json!({}))
        ]));
        assert!(!create_unaccounted(&[
            sent,
            event("instans_opprettet", json!({ "url": "u" })),
        ]));
    }

    #[test]
    fn only_a_refusal_of_that_confirm_settles_it() {
        let asked = event("bekrefter", json!({}));
        assert!(step_unsettled(std::slice::from_ref(&asked)));
        assert!(step_unsettled(&[asked.clone(), event("feilet", json!({}))]));
        assert!(!step_unsettled(&[
            asked.clone(),
            event("bekreftelse_avvist", json!({})),
            event("feilet", json!({})),
        ]));
        assert!(step_unsettled(&[
            event("uavklart", json!({})),
            asked,
            event("bekreftelse_avvist", json!({})),
        ]));
    }

    #[test]
    fn the_person_who_confirmed_is_not_replaced_by_a_later_reader() {
        let named = |kind, name| event(kind, json!({ "name": name }));
        let events = [
            named("godkjent", "Ada"),
            named("bekrefter", "Ada"),
            event("uavklart", json!({})),
            named("godkjent", "Bo"),
            event("sendt", json!({})),
        ];
        assert_eq!(approved_by(&events).unwrap().detail["name"], "Ada");
        let refused = [
            named("godkjent", "Ada"),
            named("bekrefter", "Ada"),
            event("bekreftelse_avvist", json!({})),
            named("godkjent", "Bo"),
        ];
        assert_eq!(approved_by(&refused).unwrap().detail["name"], "Bo");
    }

    #[test]
    fn a_confirm_without_a_signer_does_not_borrow_a_later_one() {
        let events = [
            event("godkjent", json!({ "name": "Ada" })),
            event("bekrefter", json!({ "name": "Ada" })),
            event("uavklart", json!({})),
            event("godkjent", json!({ "name": "Bo" })),
            event("konvolutt", json!({ "signer": "BO LESER" })),
        ];
        assert_eq!(approver_name(&events), Some("Ada"));
    }

    #[test]
    fn what_may_have_reached_skatteetaten_cannot_be_abandoned() {
        let no_instance = [
            event("oppretter_instans", json!({})),
            event("uavklart", json!({})),
        ];
        assert!(
            !can_abandon(&no_instance, false),
            "only a person's check closes it"
        );
        assert!(can_close_unresolved(&no_instance, false));
        assert!(!can_close_unresolved(&no_instance, true));
        assert!(!can_authorize(&no_instance, false));

        let with_instance = [
            event("instans_opprettet", json!({ "url": "u" })),
            event("uavklart", json!({})),
        ];
        assert!(!can_abandon(&with_instance, false), "it may be filed");
        assert!(
            can_authorize(&with_instance, false),
            "reading it settles it"
        );

        for final_status in ["sendt", "kvittert", "avvist", "avvik", "avbrutt"] {
            assert!(!can_abandon(&[event(final_status, json!({}))], false));
        }
        assert!(
            !can_authorize(&[event("godkjent", json!({}))], true),
            "a job is on it"
        );
    }
}
