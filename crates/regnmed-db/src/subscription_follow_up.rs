//! Automatisk abonnementsoppfølging (#75, docs/abonnement.md §5.3):
//! send, purr, sperr, gjenopprett — uten mennesker. "Sending is a human
//! action" governs the CUSTOMERS' bookkeeping; the drift company's own
//! invoicing is precisely the machine's job.
//!
//! Every function takes `today` so the whole ladder is testable on a
//! shifted clock. The decisions that change coverage are logged in the
//! insert-only `abonnement_oppfolging` table (migration 0048) — which
//! is also the machine's own memory: restoration happens ONLY for
//! coverage the machine itself ended for non-payment, never for an
//! oppsigelse.

use anyhow::{Context, Result, ensure};
use chrono::NaiveDate;
use regnmed_core::rates::value_on;
use regnmed_core::reminders::{Step, next_step};
use regnmed_core::subscription::SPERR_ETTER_FORFALL_DAGER;
use sqlx::{PgPool, Row};
use uuid::Uuid;

use crate::rates::load_rates;
use crate::reminders::{ReminderDraft, create_reminder};

pub const PERFORMED_BY: &str = "regnmed abonnement-oppfolging";

/// One abonnement invoice in the drift company, joined to the customer
/// company it bills (via the run log — the only invoice→customer link).
#[derive(Debug)]
pub struct SubscriptionInvoice {
    pub company_id: Uuid,
    pub company_name: String,
    /// The customer company's own e-mail (Firmaopplysninger) — the
    /// recipient; the party in the drift company carries none.
    pub email: Option<String>,
    pub invoice_id: Uuid,
    pub invoice_no: i64,
    pub due_date: NaiveDate,
    pub remaining_ore: i64,
    /// At least one mail about this invoice has gone out.
    pub sent: bool,
    pub last_steg: Option<String>,
    pub last_sent: Option<NaiveDate>,
}

/// Open (unpaid) abonnement invoices, newest reminder step included.
/// Kreditnotaer and paid invoices are not follow-up material.
pub async fn open_invoices(pool: &PgPool, drift: Uuid) -> Result<Vec<SubscriptionInvoice>> {
    let rows = sqlx::query(
        "select r.company_id, c.name as company_navn, c.email as epost,
                i.id as invoice_id, i.invoice_no, i.due_date,
                e.amount_ore
                - coalesce((select sum(m.amount_ore) from reskontro_match m
                            where m.entry_a = e.id), 0)::bigint
                + coalesce((select sum(m.amount_ore) from reskontro_match m
                            where m.entry_b = e.id), 0)::bigint as remaining_ore,
                exists(select 1 from utsendelse u where u.invoice_id = i.id) as sendt,
                rem.steg as last_steg, rem.sent_date as last_sent
         from abonnement_faktura_run r
         join invoice i on i.id = r.invoice_id
         join company c on c.id = r.company_id
         join entry e on e.id = i.receivable_entry_id
         left join lateral (
             select steg, sent_date from invoice_reminder
             where invoice_id = i.id
             order by created_at desc limit 1
         ) rem on true
         where i.company_id = $1 and i.credits_invoice_id is null
         order by i.due_date, i.invoice_no",
    )
    .bind(drift)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .iter()
        .filter(|r| r.get::<i64, _>("remaining_ore") > 0)
        .map(|r| SubscriptionInvoice {
            company_id: r.get("company_id"),
            company_name: r.get("company_navn"),
            email: r.get("epost"),
            invoice_id: r.get("invoice_id"),
            invoice_no: r.get("invoice_no"),
            due_date: r.get("due_date"),
            remaining_ore: r.get("remaining_ore"),
            sent: r.get("sendt"),
            last_steg: r.get("last_steg"),
            last_sent: r.get("last_sent"),
        })
        .collect())
}

/// Takes the next reminder step for one invoice if the cadence says one
/// is due — the pure rule decides, the rate register prices the fee,
/// and `create_reminder` applies the statutory checks and posts the
/// claim in one transaction. Returns the step taken.
pub async fn purr(
    pool: &PgPool,
    drift: Uuid,
    invoice: &SubscriptionInvoice,
    today: NaiveDate,
) -> Result<Option<(Uuid, String)>> {
    let last = match (&invoice.last_steg, invoice.last_sent) {
        (Some(s), Some(d)) => Some((
            Step::parse(s).with_context(|| format!("ukjent steg «{s}» i historikken"))?,
            d,
        )),
        _ => None,
    };
    let Some(neste) = next_step(last, invoice.due_date, today) else {
        return Ok(None);
    };
    let fee_ore = if neste.with_fee {
        // Abonnement customers are companies: the næringsdrivende
        // ceiling (standardkompensasjon) applies, and we charge it.
        let rates = load_rates(pool).await?;
        value_on(&rates, "standardkompensasjon", today)
            .context("standardkompensasjon mangler i satsregisteret")?
    } else {
        0
    };
    // The claim voucher needs its accounts; the operating company's starter
    // kontoplan does not include them. Idempotent, like fakturer_maned's
    // own ensures.
    if fee_ore > 0 || neste.with_interest {
        crate::ensure_account(pool, drift, "3950", "Annen driftsrelatert inntekt").await?;
        crate::ensure_account(pool, drift, "8050", "Annen renteinntekt").await?;
    }
    let draft = ReminderDraft {
        step: neste.step.as_str().into(),
        sent_date: Some(today),
        grace_date: today + chrono::Days::new(neste.grace_days as u64),
        fee_ore,
        with_interest: neste.with_interest,
        naeringsdrivende: true,
        fee_account: "3950".into(),
        interest_account: "8050".into(),
    };
    let result = create_reminder(pool, drift, invoice.invoice_id, &draft, PERFORMED_BY).await?;
    let reminder_id = result.reminder_id.context("purringen fikk ingen id")?;
    Ok(Some((reminder_id, result.step)))
}

/// Ends the customer's coverage when the invoice is
/// [`SPERR_ETTER_FORFALL_DAGER`] past the due date and a reminder (or more)
/// has been sent — asked twice, still unpaid. Ending coverage does not
/// block by itself: the ordinary deadline runs on top
/// (`regnmed_core::abonnement::status`), so the customer still has 14
/// days before the actual sperre. Logged in the oppfølging trail; a
/// company without open coverage (already ended, or on Stripe) is left
/// alone.
pub async fn sperr_om_moden(
    pool: &PgPool,
    invoice: &SubscriptionInvoice,
    today: NaiveDate,
) -> Result<bool> {
    let reminded = invoice
        .last_steg
        .as_deref()
        .and_then(Step::parse)
        .is_some_and(|s| s >= Step::Demand);
    if !reminded || today < invoice.due_date + chrono::Days::new(SPERR_ETTER_FORFALL_DAGER as u64) {
        return Ok(false);
    }
    let apen: bool = sqlx::query_scalar(
        "select exists(select 1 from abonnement
         where company_id = $1 and valid_to is null)",
    )
    .bind(invoice.company_id)
    .fetch_one(pool)
    .await?;
    if !apen {
        return Ok(false);
    }
    crate::subscription::close_coverage(pool, invoice.company_id, today).await?;
    logg(
        pool,
        invoice.company_id,
        Some(invoice.invoice_id),
        "sperret",
        &format!(
            "faktura {} ubetalt {} dager etter forfall ({}) — dekningen avsluttet",
            invoice.invoice_no,
            (today - invoice.due_date).num_days(),
            invoice.due_date
        ),
    )
    .await?;
    Ok(true)
}

/// Companies whose LATEST oppfølging entry is 'sperret' — the ones the
/// machine ended coverage for and should watch for payment.
pub async fn auto_sperrede(pool: &PgPool) -> Result<Vec<Uuid>> {
    let rows = sqlx::query(
        "select distinct on (company_id) company_id, aksjon
         from abonnement_oppfolging
         order by company_id, created_at desc",
    )
    .fetch_all(pool)
    .await?;
    Ok(rows
        .iter()
        .filter(|r| r.get::<String, _>("aksjon") == "sperret")
        .map(|r| r.get("company_id"))
        .collect())
}

/// Restores coverage once every abonnement invoice is paid — but ONLY
/// when the machine itself ended it for non-payment (the oppfølging
/// trail is the memory). An oppsigelse looks identical in the
/// abonnement table and must never be resurrected by a paid final
/// invoice. The plan continues as it was.
pub async fn restore_if_paid(
    pool: &PgPool,
    drift: Uuid,
    company_id: Uuid,
    today: NaiveDate,
) -> Result<bool> {
    let last: Option<String> = sqlx::query_scalar(
        "select aksjon from abonnement_oppfolging
         where company_id = $1 order by created_at desc limit 1",
    )
    .bind(company_id)
    .fetch_optional(pool)
    .await?;
    if last.as_deref() != Some("sperret") {
        return Ok(false);
    }
    let utestaaende = open_invoices(pool, drift)
        .await?
        .into_iter()
        .any(|f| f.company_id == company_id);
    if utestaaende {
        return Ok(false);
    }
    let plan: Option<String> = sqlx::query_scalar(
        "select plan from abonnement where company_id = $1
         order by valid_from desc, created_at desc limit 1",
    )
    .bind(company_id)
    .fetch_optional(pool)
    .await?;
    let plan = plan.context("selskapet har aldri hatt en dekningsrad")?;
    crate::subscription::open_coverage(
        pool,
        company_id,
        &plan,
        today,
        None,
        "innbetaling registrert — dekningen gjenopprettet automatisk (#75)",
        PERFORMED_BY,
    )
    .await?;
    logg(
        pool,
        company_id,
        None,
        "gjenopprettet",
        &format!("alle abonnementsfakturaer betalt — dekning fra {today}, plan {plan}"),
    )
    .await?;
    Ok(true)
}

/// Reminders on abonnement invoices that never went out (no utsendelse
/// row) — the daily retry, same dedup discipline as the invoices.
#[derive(Debug)]
pub struct UnsentReminder {
    pub company_id: Uuid,
    pub company_name: String,
    pub email: Option<String>,
    pub invoice_id: Uuid,
    pub invoice_no: i64,
    pub reminder_id: Uuid,
    pub step: String,
}

pub async fn unsent_reminders(pool: &PgPool, drift: Uuid) -> Result<Vec<UnsentReminder>> {
    let rows = sqlx::query(
        "select r.company_id, c.name as company_navn, c.email as epost,
                i.id as invoice_id, i.invoice_no, ir.id as reminder_id, ir.steg
         from invoice_reminder ir
         join invoice i on i.id = ir.invoice_id
         join abonnement_faktura_run r on r.invoice_id = i.id
         join company c on c.id = r.company_id
         where i.company_id = $1
           and not exists (select 1 from utsendelse u where u.reminder_id = ir.id)
         order by ir.created_at",
    )
    .bind(drift)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .iter()
        .map(|r| UnsentReminder {
            company_id: r.get("company_id"),
            company_name: r.get("company_navn"),
            email: r.get("epost"),
            invoice_id: r.get("invoice_id"),
            invoice_no: r.get("invoice_no"),
            reminder_id: r.get("reminder_id"),
            step: r.get("steg"),
        })
        .collect())
}

async fn logg(
    pool: &PgPool,
    company_id: Uuid,
    invoice_id: Option<Uuid>,
    aksjon: &str,
    detail: &str,
) -> Result<()> {
    ensure!(!detail.is_empty(), "detail is required");
    sqlx::query(
        "insert into abonnement_oppfolging (id, company_id, invoice_id, aksjon, detail)
         values ($1, $2, $3, $4, $5)",
    )
    .bind(Uuid::new_v4())
    .bind(company_id)
    .bind(invoice_id)
    .bind(aksjon)
    .bind(detail)
    .execute(pool)
    .await?;
    Ok(())
}
