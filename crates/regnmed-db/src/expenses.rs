//! Utlegg og kjøregodtgjørelse (docs/utlegg.md, #42): claims with
//! immutable content and one-way decisions (the innboks discipline),
//! kjøregodtgjørelse computed from the dated satsregister AT
//! SUBMISSION (the row is evidence), approval posting kostnad mot
//! mellomregning with the receipt attached to the voucher in ONE
//! transaction, and utbetaling posting mellomregning mot bank.

use anyhow::{Context, Result, bail, ensure};
use chrono::NaiveDate;
use regnmed_core::Ore;
use regnmed_core::expenses::mileage_allowance;
use regnmed_core::hash::sha256;
use regnmed_core::rates::value_on;
use regnmed_core::vat::{rate_on, split_gross};
use regnmed_core::voucher::{EntryDraft, VoucherDraft};
use sqlx::{PgPool, Row};
use uuid::Uuid;

use crate::attachment::add_attachment_in;
use crate::ledger::post_voucher_in;
use crate::rates::load_rates;
use crate::vat::load_vat_rates;

/// SQL, for an expense row aliased `x`: the claimant is someone the
/// payroll run can still pay — linked to an employee whose employment
/// has not ended. One copy, so the pay guard, the approval warning and
/// the portal can never disagree about how a kjøring is paid.
fn paid_with_pay(x: &str) -> String {
    format!(
        "exists (select 1 from employee emp
                 where emp.company_id = {x}.company_id and emp.person_id = {x}.person_id
                   and (emp.ansatt_til is null or emp.ansatt_til >= current_date))"
    )
}

/// SQL, for an expense row aliased `x`: the claim is on a payment list
/// on its way to the bank.
pub(crate) fn in_open_run(x: &str) -> String {
    format!(
        "exists (select 1 from payment_run_item i
                 join payment_run r on r.id = i.run_id
                 where i.expense_id = {x}.id and r.status in ('utkast', 'godkjent'))"
    )
}

/// Registers an utlegg with its receipt. The receipt is immutable from
/// this moment (SHA-256 stored, trigger + grants) and follows the
/// claim onto the voucher at approval — oppbevaringsplikten covers the
/// original documentation, not a copy of our making.
#[allow(clippy::too_many_arguments)]
pub async fn create_expense(
    pool: &PgPool,
    company_id: Uuid,
    person_id: Uuid,
    date: NaiveDate,
    description: &str,
    amount_ore: i64,
    filename: &str,
    content_type: &str,
    content: &[u8],
    created_by: &str,
) -> Result<Uuid> {
    ensure!(amount_ore > 0, "beløpet må være positivt");
    ensure!(!content.is_empty(), "kvitteringen er tom");
    ensure!(!description.trim().is_empty(), "skriv hva utlegget gjaldt");
    let digest = sha256(content).to_vec();
    // Outside the transaction: with an object store this is a network
    // PUT, and content addressing makes a refused duplicate harmless.
    let in_row = crate::blob::blobs().store(content).await?;
    let mut tx = pool.begin().await?;
    refuse_duplicate_receipt(&mut tx, company_id, &digest, None).await?;
    let id = Uuid::now_v7();
    sqlx::query(
        "insert into expense (id, company_id, person_id, kind, dato, beskrivelse, belop_ore,
                              receipt_filename, receipt_content_type, receipt_content,
                              receipt_sha256, created_by)
         values ($1, $2, $3, 'utlegg', $4, $5, $6, $7, $8, $9, $10, $11)",
    )
    .bind(id)
    .bind(company_id)
    .bind(person_id)
    .bind(date)
    .bind(description)
    .bind(amount_ore)
    .bind(filename)
    .bind(content_type)
    .bind(in_row)
    .bind(digest)
    .bind(created_by)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(id)
}

/// The same receipt twice is a double-click or a resubmission, never two
/// expenses — refused while the first claim stands (a draft counts). A
/// REJECTED or discarded claim does not block: resubmitting is the normal
/// way back. The advisory lock makes two simultaneous uploads of the same
/// bytes take turns. `except` is the claim whose receipt is being
/// replaced.
async fn refuse_duplicate_receipt(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    company_id: Uuid,
    digest: &[u8],
    except: Option<Uuid>,
) -> Result<()> {
    sqlx::query("select pg_advisory_xact_lock(hashtextextended($1::text || encode($2, 'hex'), 0))")
        .bind(company_id)
        .bind(digest)
        .execute(&mut **tx)
        .await?;
    let earlier: Option<(Option<NaiveDate>, String, String)> = sqlx::query_as(
        "select dato, beskrivelse, status from expense
         where company_id = $1 and receipt_sha256 = $2
           and status not in ('avvist', 'forkastet')
           and ($3::uuid is null or id <> $3)
         limit 1",
    )
    .bind(company_id)
    .bind(digest)
    .bind(except)
    .fetch_optional(&mut **tx)
    .await?;
    if let Some((date, what, status)) = earlier {
        let when = date
            .map(|d| format!(", {}", d.format("%d.%m.%Y")))
            .unwrap_or_default();
        let what = if what.is_empty() {
            "uten beskrivelse".into()
        } else {
            format!("«{what}»")
        };
        if status == "utkast" {
            bail!("denne kvitteringen ligger allerede i et utkast ({what}{when})");
        }
        bail!("denne kvitteringen er allerede sendt inn ({what}{when})");
    }
    Ok(())
}

/// What a draft carries — everything optional until submission. The
/// same struct updates a draft: every field is written as given.
#[derive(Debug, Default, Clone)]
pub struct ExpenseDraft {
    pub date: Option<NaiveDate>,
    pub description: String,
    pub amount_ore: Option<i64>,
    /// Total kilometres paid (a round trip counts both ways).
    pub km: Option<i64>,
    pub route_from: Option<String>,
    pub route_to: Option<String>,
    pub round_trip: bool,
    pub vehicle: Option<String>,
    /// Names of the colleagues on board (one supplement each).
    pub passengers: Vec<String>,
    /// Dimension codes.
    pub department: Option<String>,
    pub project: Option<String>,
    pub billable: bool,
    /// A trip (`reise`): the reiseregning's times, destination and døgn.
    pub departure_at: Option<chrono::NaiveDateTime>,
    pub return_at: Option<chrono::NaiveDateTime>,
    pub destination: Option<String>,
    pub overnight: bool,
    pub over_15_km: bool,
    pub travel_days: Vec<crate::travel::TravelDayDraft>,
    /// Paid with this company card: the company's money, nothing to
    /// reimburse (docs/utlegg.md «Firmakort»).
    pub company_card_id: Option<Uuid>,
    /// Filed from this card statement line, which approval reconciles.
    pub bank_transaction_id: Option<Uuid>,
}

/// A card purchase must be on a card the claimant may use, and a claim
/// filed from a statement line takes that line's card (set on `draft`).
async fn check_card(
    pool: &PgPool,
    company_id: Uuid,
    claimant: Uuid,
    kind: &str,
    expense_id: Option<Uuid>,
    draft: &mut ExpenseDraft,
) -> Result<()> {
    if draft.company_card_id.is_none() && draft.bank_transaction_id.is_none() {
        return Ok(());
    }
    ensure!(
        kind == "utlegg",
        "bare et utlegg med kvittering kan være betalt med firmakort"
    );
    if let Some(line_id) = draft.bank_transaction_id {
        let line = crate::company_card::card_line(pool, company_id, line_id, expense_id).await?;
        ensure!(
            draft.company_card_id.is_none_or(|c| c == line.card_id),
            "kortlinjen hører til et annet kort ({})",
            line.card_label
        );
        draft.company_card_id = Some(line.card_id);
    }
    let card_id = draft.company_card_id.expect("set above");
    let card = crate::company_card::get_card(pool, company_id, card_id).await?;
    // A retired card's last purchases still need their receipts, from the
    // statement lines; nothing new goes on it.
    ensure!(
        card.active || draft.bank_transaction_id.is_some(),
        "kortet {} er ikke i bruk lenger",
        card.label()
    );
    ensure!(
        card.holder_person_id.is_none_or(|h| h == claimant),
        "kortet {} tilhører {}",
        card.label(),
        card.holder_name.as_deref().unwrap_or("en annen")
    );
    Ok(())
}

impl ExpenseDraft {
    fn is_travel(&self) -> bool {
        self.departure_at.is_some()
            || self.return_at.is_some()
            || given(&self.destination).is_some()
            || self.overnight
            || self.over_15_km
            || !self.travel_days.is_empty()
    }
}

pub struct Receipt<'a> {
    pub filename: &'a str,
    pub content_type: &'a str,
    pub content: &'a [u8],
}

/// Blank strings are "not given" — a form sends "" for an empty field.
fn given(s: &Option<String>) -> Option<&str> {
    s.as_deref().map(str::trim).filter(|s| !s.is_empty())
}

/// What can be checked while a draft is still a draft: the values that
/// ARE given must be valid. Completeness waits for submission.
async fn check_draft(pool: &PgPool, company_id: Uuid, d: &ExpenseDraft) -> Result<()> {
    if let Some(a) = d.amount_ore {
        ensure!(a > 0, "beløpet må være positivt");
    }
    if let Some(km) = d.km {
        ensure!(km > 0, "antall kilometer må være positivt");
    }
    if let Some(v) = given(&d.vehicle) {
        ensure!(
            regnmed_core::expenses::vehicle_rate_domains(v).is_some(),
            "ukjent kjøretøy «{v}»"
        );
    }
    ensure!(
        d.passengers.iter().all(|p| !p.trim().is_empty()),
        "skriv navnet på hver passasjer"
    );
    for day in &d.travel_days {
        if let Some(l) = given(&day.lodging) {
            ensure!(
                regnmed_core::travel::Lodging::from_slug(l).is_some(),
                "ukjent overnatting «{l}»"
            );
        }
    }
    if let (Some(from), Some(to)) = (d.departure_at, d.return_at) {
        ensure!(to > from, "hjemkomst må være etter avreise");
    }
    for (kind, code) in [
        ("avdeling", given(&d.department)),
        ("prosjekt", given(&d.project)),
    ] {
        let Some(code) = code else { continue };
        let active: Option<bool> = sqlx::query_scalar(
            "select active from dimension where company_id = $1 and kind = $2 and code = $3",
        )
        .bind(company_id)
        .bind(kind)
        .bind(code)
        .fetch_optional(pool)
        .await?;
        match active {
            None => bail!("ukjent {kind} «{code}»"),
            Some(false) => bail!("{kind} «{code}» er avsluttet"),
            Some(true) => {}
        }
    }
    Ok(())
}

/// Creates a draft: an utlegg (with or without its receipt yet) or a
/// kjøring. `registered_by` is who entered it — the claimant themself,
/// or someone filing on their behalf. A cost tied to a trip
/// (`trip_expense_id`: tolls, ferry, parking) must belong to the same
/// person and inherits the trip's dimensions when it has none of its own.
#[allow(clippy::too_many_arguments)]
pub async fn create_draft(
    pool: &PgPool,
    company_id: Uuid,
    person_id: Uuid,
    registered_by: Uuid,
    kind: &str,
    trip_expense_id: Option<Uuid>,
    draft: &ExpenseDraft,
    receipt: Option<Receipt<'_>>,
    created_by: &str,
) -> Result<Uuid> {
    ensure!(
        matches!(kind, "utlegg" | "kjoring" | "reise"),
        "ukjent type «{kind}»"
    );
    ensure!(
        kind == "utlegg" || receipt.is_none(),
        "bare et utlegg har kvittering — bompenger, hotell og billetter legges til som egne utlegg"
    );
    ensure!(
        kind == "reise" || !draft.is_travel(),
        "avreise, hjemkomst og døgn hører til en reise"
    );
    ensure!(
        kind != "reise" || trip_expense_id.is_none(),
        "en reise kan ikke knyttes til noe annet"
    );
    let mut draft = draft.clone();
    if let Some(trip) = trip_expense_id {
        let trip_row = sqlx::query(
            "select kind, person_id, department, project, billable from expense
             where id = $1 and company_id = $2 and status <> 'forkastet'",
        )
        .bind(trip)
        .bind(company_id)
        .fetch_optional(pool)
        .await?
        .context("fant ikke turen kravet skal knyttes til")?;
        // Costs hang under a drive or a trip; a drive under a trip.
        let parent: String = trip_row.get("kind");
        ensure!(
            match kind {
                "utlegg" => parent == "kjoring" || parent == "reise",
                _ => parent == "reise",
            },
            "et utlegg kan knyttes til en kjøring eller reise, en kjøring bare til en reise"
        );
        ensure!(
            trip_row.get::<Uuid, _>("person_id") == person_id,
            "kravet må gjelde samme person som turen det hører til"
        );
        if given(&draft.department).is_none() && given(&draft.project).is_none() {
            draft.department = trip_row.get("department");
            draft.project = trip_row.get("project");
            draft.billable = trip_row.get("billable");
        }
    }
    check_draft(pool, company_id, &draft).await?;
    check_card(pool, company_id, person_id, kind, None, &mut draft).await?;
    let stored = match &receipt {
        Some(r) => {
            ensure!(!r.content.is_empty(), "kvitteringen er tom");
            Some((
                sha256(r.content).to_vec(),
                crate::blob::blobs().store(r.content).await?,
            ))
        }
        None => None,
    };
    let mut tx = pool.begin().await?;
    if let Some((digest, _)) = &stored {
        refuse_duplicate_receipt(&mut tx, company_id, digest, None).await?;
    }
    let id = Uuid::now_v7();
    sqlx::query(
        "insert into expense (id, company_id, person_id, registered_by_person, kind, status,
                              dato, beskrivelse, belop_ore, km, route_from, route_to,
                              round_trip, vehicle, passengers, department, project, billable,
                              trip_expense_id, receipt_filename, receipt_content_type,
                              receipt_content, receipt_sha256, created_by, departure_at,
                              return_at, destination, overnight, over_15_km, travel_days,
                              company_card_id, bank_transaction_id)
         values ($1, $2, $3, $4, $5, 'utkast', $6, $7, $8, $9, $10, $11, $12, $13, $14,
                 $15, $16, $17, $18, $19, $20, $21, $22, $23, $24, $25, $26, $27, $28, $29,
                 $30, $31)",
    )
    .bind(id)
    .bind(company_id)
    .bind(person_id)
    .bind(registered_by)
    .bind(kind)
    .bind(draft.date)
    .bind(draft.description.trim())
    .bind(draft.amount_ore)
    .bind(draft.km)
    .bind(given(&draft.route_from))
    .bind(given(&draft.route_to))
    .bind(draft.round_trip)
    .bind(given(&draft.vehicle))
    .bind(trimmed(&draft.passengers))
    .bind(given(&draft.department))
    .bind(given(&draft.project))
    .bind(draft.billable)
    .bind(trip_expense_id)
    .bind(receipt.as_ref().map(|r| r.filename))
    .bind(receipt.as_ref().map(|r| r.content_type))
    .bind(stored.as_ref().and_then(|(_, in_row)| *in_row))
    .bind(stored.as_ref().map(|(digest, _)| digest.clone()))
    .bind(created_by)
    .bind(draft.departure_at)
    .bind(draft.return_at)
    .bind(given(&draft.destination))
    .bind(draft.overnight)
    .bind(draft.over_15_km)
    .bind(serde_json::to_value(&draft.travel_days)?)
    .bind(draft.company_card_id)
    .bind(draft.bank_transaction_id)
    .execute(&mut *tx)
    .await
    .map_err(line_taken)?;
    tx.commit().await?;
    Ok(id)
}

fn trimmed(names: &[String]) -> Vec<String> {
    names.iter().map(|n| n.trim().to_string()).collect()
}

/// Locks a draft for writing; anything past the draft is evidence.
async fn lock_draft(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    company_id: Uuid,
    expense_id: Uuid,
) -> Result<String> {
    let row: Option<(String, String)> = sqlx::query_as(
        "select kind, status from expense where id = $1 and company_id = $2 for update",
    )
    .bind(expense_id)
    .bind(company_id)
    .fetch_optional(&mut **tx)
    .await?;
    let (kind, status) = row.context("no such expense")?;
    ensure!(
        status == "utkast",
        "kravet er {status} og kan ikke endres lenger"
    );
    Ok(kind)
}

/// Writes every field of a draft as given.
pub async fn update_draft(
    pool: &PgPool,
    company_id: Uuid,
    expense_id: Uuid,
    draft: &ExpenseDraft,
) -> Result<()> {
    check_draft(pool, company_id, draft).await?;
    let mut draft = draft.clone();
    let mut tx = pool.begin().await?;
    let kind = lock_draft(&mut tx, company_id, expense_id).await?;
    let (claimant, line): (Uuid, Option<Uuid>) =
        sqlx::query_as("select person_id, bank_transaction_id from expense where id = $1")
            .bind(expense_id)
            .fetch_one(&mut *tx)
            .await?;
    // The statement line a draft was filed from stays with it while it is
    // still paid by card; choosing «own money» lets go of it.
    if draft.company_card_id.is_some() && draft.bank_transaction_id.is_none() {
        draft.bank_transaction_id = line;
    }
    check_card(
        pool,
        company_id,
        claimant,
        &kind,
        Some(expense_id),
        &mut draft,
    )
    .await?;
    ensure!(
        kind == "kjoring" || (draft.km.is_none() && given(&draft.vehicle).is_none()),
        "kilometer og kjøretøy hører til en kjøring"
    );
    ensure!(
        kind == "reise" || !draft.is_travel(),
        "avreise, hjemkomst og døgn hører til en reise"
    );
    sqlx::query(
        "update expense set dato = $3, beskrivelse = $4, belop_ore = $5, km = $6,
                route_from = $7, route_to = $8, round_trip = $9, vehicle = $10,
                passengers = $11, department = $12, project = $13, billable = $14,
                departure_at = $15, return_at = $16, destination = $17, overnight = $18,
                over_15_km = $19, travel_days = $20, company_card_id = $21,
                bank_transaction_id = $22
         where id = $1 and company_id = $2",
    )
    .bind(expense_id)
    .bind(company_id)
    .bind(draft.date)
    .bind(draft.description.trim())
    .bind(if kind == "utlegg" {
        draft.amount_ore
    } else {
        None
    })
    .bind(draft.km)
    .bind(given(&draft.route_from))
    .bind(given(&draft.route_to))
    .bind(draft.round_trip)
    .bind(given(&draft.vehicle))
    .bind(trimmed(&draft.passengers))
    .bind(given(&draft.department))
    .bind(given(&draft.project))
    .bind(draft.billable)
    .bind(draft.departure_at)
    .bind(draft.return_at)
    .bind(given(&draft.destination))
    .bind(draft.overnight)
    .bind(draft.over_15_km)
    .bind(serde_json::to_value(&draft.travel_days)?)
    .bind(draft.company_card_id)
    .bind(draft.bank_transaction_id)
    .execute(&mut *tx)
    .await
    .map_err(line_taken)?;
    tx.commit().await?;
    Ok(())
}

/// Two receipts racing for one card statement line: the unique index
/// decides, and the loser hears it in words.
fn line_taken(e: sqlx::Error) -> anyhow::Error {
    let constraint = e.as_database_error().and_then(|d| d.constraint());
    if constraint == Some("expense_bank_transaction_once") {
        anyhow::anyhow!("kortlinjen har allerede en kvittering — se Kortkjøp uten kvittering")
    } else {
        e.into()
    }
}

/// Replaces a draft's receipt — rotated, cropped, or the toll statement
/// that arrived after the trip. Until submission the picture is not yet
/// evidence; from submission it is, and the trigger holds it.
pub async fn replace_draft_receipt(
    pool: &PgPool,
    company_id: Uuid,
    expense_id: Uuid,
    receipt: Receipt<'_>,
) -> Result<()> {
    ensure!(!receipt.content.is_empty(), "kvitteringen er tom");
    let digest = sha256(receipt.content).to_vec();
    let in_row = crate::blob::blobs().store(receipt.content).await?;
    let mut tx = pool.begin().await?;
    let kind = lock_draft(&mut tx, company_id, expense_id).await?;
    ensure!(kind == "utlegg", "en kjøring har ingen kvittering");
    refuse_duplicate_receipt(&mut tx, company_id, &digest, Some(expense_id)).await?;
    sqlx::query(
        "update expense set receipt_filename = $3, receipt_content_type = $4,
                receipt_content = $5, receipt_sha256 = $6
         where id = $1 and company_id = $2",
    )
    .bind(expense_id)
    .bind(company_id)
    .bind(receipt.filename)
    .bind(receipt.content_type)
    .bind(in_row)
    .bind(digest)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(())
}

#[derive(Debug)]
pub struct SubmittedExpense {
    pub amount_ore: i64,
    pub taxable_ore: i64,
    /// Costs (and, on a trip, drives) tied to it that went in with it.
    pub costs_submitted: i64,
    /// Costs tied to it still waiting (no receipt yet, say), with what
    /// each lacks.
    pub costs_waiting: Vec<String>,
}

/// The row a submission reads.
#[derive(sqlx::FromRow)]
struct DraftRow {
    id: Uuid,
    kind: String,
    dato: Option<NaiveDate>,
    beskrivelse: String,
    belop_ore: Option<i64>,
    receipt_sha256: Option<Vec<u8>>,
    km: Option<i64>,
    route_from: Option<String>,
    route_to: Option<String>,
    vehicle: Option<String>,
    passengers: Vec<String>,
    project: Option<String>,
    billable: bool,
    departure_at: Option<chrono::NaiveDateTime>,
    return_at: Option<chrono::NaiveDateTime>,
    destination: Option<String>,
    overnight: bool,
    over_15_km: bool,
    travel_days: serde_json::Value,
}

const DRAFT_SELECT: &str = "select id, kind, dato, beskrivelse, belop_ore, receipt_sha256, km,
                                   route_from, route_to, vehicle, passengers, project, billable,
                                   departure_at, return_at, destination, overnight, over_15_km,
                                   travel_days
                            from expense";

/// What a draft still lacks, said the way the claimant reads it — the
/// first missing thing, or None when it can go in.
fn missing(d: &DraftRow) -> Option<&'static str> {
    if d.kind == "reise" {
        if d.departure_at.is_none() || d.return_at.is_none() {
            return Some("oppgi avreise og hjemkomst, med klokkeslett");
        }
        if given(&d.destination).is_none() {
            return Some("oppgi hvor du reiste");
        }
        if d.beskrivelse.trim().is_empty() {
            return Some("skriv formålet med reisen");
        }
        if !d.over_15_km {
            return Some(
                "bekreft at reisen gikk minst 15 km fra der du starter — ellers gir den ingen diett",
            );
        }
        if d.billable && given(&d.project).is_none() {
            return Some("velg prosjekt — det er prosjektet som viderefaktureres");
        }
        return None;
    }
    if d.dato.is_none() {
        return Some("velg dato");
    }
    if d.billable && given(&d.project).is_none() {
        return Some("velg prosjekt — det er prosjektet som viderefaktureres");
    }
    if d.kind == "utlegg" {
        if d.receipt_sha256.is_none() {
            return Some("legg ved kvitteringen");
        }
        if d.belop_ore.is_none() {
            return Some("oppgi beløpet");
        }
        if d.beskrivelse.trim().is_empty() {
            return Some("skriv hva utlegget gjaldt");
        }
    } else {
        if given(&d.route_from).is_none() || given(&d.route_to).is_none() {
            return Some("oppgi hvor du kjørte fra og til");
        }
        if d.km.is_none() {
            return Some("oppgi antall kilometer");
        }
        if d.beskrivelse.trim().is_empty() {
            return Some("skriv formålet med turen");
        }
    }
    None
}

/// Submits ONE complete draft row: its rates are resolved and stored now
/// (outside the register's coverage: refused, never guessed). Returns
/// (amount, taxable).
async fn submit_row(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    pool: &PgPool,
    company_id: Uuid,
    d: &DraftRow,
) -> Result<(i64, i64)> {
    match d.kind.as_str() {
        "kjoring" => {
            let date = d.dato.expect("checked by missing()");
            let vehicle = d.vehicle.as_deref().unwrap_or("bil");
            let (rate_domain, tax_free_domain) =
                regnmed_core::expenses::vehicle_rate_domains(vehicle)
                    .with_context(|| format!("ukjent kjøretøy «{vehicle}»"))?;
            let rates = load_rates(pool).await?;
            let lookup = |domain: &str| {
                value_on(&rates, domain, date)
                    .with_context(|| format!("ingen sats «{domain}» for {date} i satsregisteret"))
            };
            let vehicle_rates = (lookup(rate_domain)?, lookup(tax_free_domain)?);
            let passengers = d.passengers.len() as i64;
            let passenger_rates = if passengers > 0 {
                let (state, tax_free) = regnmed_core::expenses::PASSENGER_RATE_DOMAINS;
                (lookup(state)?, lookup(tax_free)?)
            } else {
                (0, 0)
            };
            let km = d.km.expect("checked by missing()");
            let allowance = regnmed_core::expenses::trip_allowance(
                km,
                vehicle_rates,
                passenger_rates,
                passengers,
            );
            sqlx::query(
                "update expense set status = 'innsendt', vehicle = $2, belop_ore = $3,
                        sats_ore_per_km = $4, trekkfri_ore = $5, trekkpliktig_ore = $6,
                        passenger_rate_ore_per_km = $7
                 where id = $1",
            )
            .bind(d.id)
            .bind(vehicle)
            .bind(allowance.amount_ore)
            .bind(vehicle_rates.0)
            .bind(allowance.tax_free_ore)
            .bind(allowance.taxable_ore)
            .bind((passengers > 0).then_some(passenger_rates.0))
            .execute(&mut **tx)
            .await?;
            Ok((allowance.amount_ore, allowance.taxable_ore))
        }
        "reise" => {
            let departure = d.departure_at.expect("checked by missing()");
            let return_at = d.return_at.expect("checked by missing()");
            // A reiseregning is written after the trip: one sent in ahead
            // would be paid by payroll for days not yet travelled. The
            // times are Norwegian wall clock and the server runs on UTC,
            // so two hours of slack keep a trip that just ended in.
            ensure!(
                return_at <= chrono::Utc::now().naive_utc() + chrono::Duration::hours(2),
                "reisen er ikke over ennå — send inn reiseregningen når du er hjemme"
            );
            let days: Vec<crate::travel::TravelDayDraft> =
                serde_json::from_value(d.travel_days.clone()).context("travel_days")?;
            // The scheme is the company's on the day the claim goes in,
            // and stays with the claim.
            let scheme = crate::travel::travel_scheme(pool, company_id).await?;
            let (allowance, lines) =
                crate::travel::compute(pool, departure, return_at, d.overnight, &days, scheme)
                    .await?;
            ensure!(
                allowance.amount_ore > 0,
                "reisen gir ingen diett — alle måltidene ble dekket av andre"
            );
            sqlx::query(
                "update expense set status = 'innsendt', dato = $2, belop_ore = $3,
                        trekkfri_ore = $4, trekkpliktig_ore = $5, rate_scheme = $6,
                        travel_lines = $7
                 where id = $1",
            )
            .bind(d.id)
            .bind(departure.date())
            .bind(allowance.amount_ore)
            .bind(allowance.tax_free_ore)
            .bind(allowance.taxable_ore)
            .bind(scheme.slug())
            .bind(serde_json::to_value(&lines)?)
            .execute(&mut **tx)
            .await?;
            Ok((allowance.amount_ore, allowance.taxable_ore))
        }
        _ => {
            sqlx::query("update expense set status = 'innsendt' where id = $1")
                .bind(d.id)
                .execute(&mut **tx)
                .await?;
            Ok((d.belop_ore.unwrap_or(0), 0))
        }
    }
}

/// Submits the complete drafts hanging under `parent`, and returns the
/// ids of the drives among them (whose own costs go in next). An
/// incomplete one stays a draft, and the answer says what it lacks.
async fn submit_children(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    pool: &PgPool,
    company_id: Uuid,
    parent: Uuid,
    submitted: &mut SubmittedExpense,
) -> Result<Vec<Uuid>> {
    let children: Vec<DraftRow> = sqlx::query_as(&format!(
        "{DRAFT_SELECT} where trip_expense_id = $1 and status = 'utkast'
         order by created_at for update"
    ))
    .bind(parent)
    .fetch_all(&mut **tx)
    .await?;
    let mut drives = Vec::new();
    for child in &children {
        match missing(child) {
            Some(what) => {
                let name = if !child.beskrivelse.is_empty() {
                    child.beskrivelse.as_str()
                } else if child.kind == "kjoring" {
                    "kjøring"
                } else {
                    "utlegg"
                };
                submitted.costs_waiting.push(format!("{name}: {what}"));
            }
            None => {
                submit_row(tx, pool, company_id, child).await?;
                submitted.costs_submitted += 1;
                if child.kind == "kjoring" {
                    drives.push(child.id);
                }
            }
        }
    }
    Ok(drives)
}

/// Submits a draft: from here the content is evidence. A kjøring's and a
/// trip's rates are resolved from the register NOW and stored, and what
/// hangs under it that is complete goes in with it in the same
/// transaction — a trip's costs and drives, and those drives' tolls. The
/// rest stay drafts, and the answer says why.
pub async fn submit_expense(
    pool: &PgPool,
    company_id: Uuid,
    expense_id: Uuid,
) -> Result<SubmittedExpense> {
    let mut tx = pool.begin().await?;
    lock_draft(&mut tx, company_id, expense_id).await?;
    let d: DraftRow = sqlx::query_as(&format!("{DRAFT_SELECT} where id = $1"))
        .bind(expense_id)
        .fetch_one(&mut *tx)
        .await?;
    if let Some(what) = missing(&d) {
        bail!("kravet kan ikke sendes inn ennå: {what}");
    }
    let (amount, taxable) = submit_row(&mut tx, pool, company_id, &d).await?;
    let mut submitted = SubmittedExpense {
        amount_ore: amount,
        taxable_ore: taxable,
        costs_submitted: 0,
        costs_waiting: Vec::new(),
    };
    if d.kind != "utlegg" {
        let drives = submit_children(&mut tx, pool, company_id, d.id, &mut submitted).await?;
        for drive in drives {
            submit_children(&mut tx, pool, company_id, drive, &mut submitted).await?;
        }
    }
    tx.commit().await?;
    Ok(submitted)
}

/// Discards a draft, one way — never deleted. A trip takes the drafts
/// tied to it along — on a journey also the drafts under its draft drives;
/// submitted claims stand on their own, with what hangs under them.
pub async fn discard_expense(pool: &PgPool, company_id: Uuid, expense_id: Uuid) -> Result<()> {
    let mut tx = pool.begin().await?;
    lock_draft(&mut tx, company_id, expense_id).await?;
    sqlx::query(
        "update expense set status = 'forkastet'
         where company_id = $1 and status = 'utkast'
           and (id = $2 or trip_expense_id = $2
                or trip_expense_id in (select id from expense
                                       where trip_expense_id = $2 and status = 'utkast'))",
    )
    .bind(company_id)
    .bind(expense_id)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(())
}

/// The claimant, whoever entered it, and the status — who may write a
/// draft, and who may see one at all.
pub async fn expense_people(
    pool: &PgPool,
    company_id: Uuid,
    expense_id: Uuid,
) -> Result<(Uuid, Option<Uuid>, String)> {
    sqlx::query_as(
        "select person_id, registered_by_person, status from expense
         where id = $1 and company_id = $2",
    )
    .bind(expense_id)
    .bind(company_id)
    .fetch_optional(pool)
    .await?
    .context("no such utlegg")
}

/// Whether a claim may be filed for `person_id`: an active direct
/// member of the company. Access through an oppdrag is the firm's
/// people, who are not the company's to reimburse.
pub async fn is_claimant(pool: &PgPool, company_id: Uuid, person_id: Uuid) -> Result<bool> {
    Ok(sqlx::query_scalar(
        "select exists (select 1 from company_member
                        where company_id = $1 and person_id = $2 and active)",
    )
    .bind(company_id)
    .bind(person_id)
    .fetch_one(pool)
    .await?)
}

/// Statens km-sats and its trekkfri part on `date`, from the
/// satsregister — what a kjøring that day is paid. Outside the
/// register's coverage: an error, never a guess.
pub async fn mileage_rates(pool: &PgPool, date: NaiveDate) -> Result<(i64, i64)> {
    let rates = load_rates(pool).await?;
    let rate = value_on(&rates, "km_godtgjorelse", date)
        .with_context(|| format!("ingen km-sats for {date} i satsregisteret"))?;
    let trekkfri = value_on(&rates, "km_godtgjorelse_trekkfri", date)
        .with_context(|| format!("ingen trekkfri km-sats for {date} i satsregisteret"))?;
    Ok((rate, trekkfri))
}

pub struct TripRates {
    pub rate: i64,
    pub tax_free: i64,
    /// (state, trekkfri) per passenger per km; None when the register
    /// has no supplement for the date.
    pub passenger: Option<(i64, i64)>,
}

/// What a trip with `vehicle` on `date` is paid per km — so the form can
/// show the amount before submitting. Same lookups as [`submit_expense`].
pub async fn trip_rates(pool: &PgPool, date: NaiveDate, vehicle: &str) -> Result<TripRates> {
    let (state, tax_free) = regnmed_core::expenses::vehicle_rate_domains(vehicle)
        .with_context(|| format!("ukjent kjøretøy «{vehicle}»"))?;
    let rates = load_rates(pool).await?;
    let lookup = |domain: &str| {
        value_on(&rates, domain, date)
            .with_context(|| format!("ingen sats «{domain}» for {date} i satsregisteret"))
    };
    let (p_state, p_tax_free) = regnmed_core::expenses::PASSENGER_RATE_DOMAINS;
    Ok(TripRates {
        rate: lookup(state)?,
        tax_free: lookup(tax_free)?,
        passenger: value_on(&rates, p_state, date).zip(value_on(&rates, p_tax_free, date)),
    })
}

/// Registers one kjøring. The satser valid on `dato` are resolved from
/// the satsregister NOW and stored on the row — a claim outside the
/// register's coverage is refused loudly, never guessed.
pub async fn create_expense_run(
    pool: &PgPool,
    company_id: Uuid,
    person_id: Uuid,
    date: NaiveDate,
    description: &str,
    km: i64,
    created_by: &str,
) -> Result<(Uuid, i64, i64)> {
    ensure!(km > 0, "oppgi antall kilometer");
    ensure!(!description.trim().is_empty(), "skriv strekning og formål");
    let (rate, trekkfri_sats) = mileage_rates(pool, date).await?;
    let beregnet = mileage_allowance(km, rate, trekkfri_sats);
    let id = Uuid::now_v7();
    sqlx::query(
        "insert into expense (id, company_id, person_id, kind, dato, beskrivelse, belop_ore,
                              km, sats_ore_per_km, trekkfri_ore, trekkpliktig_ore, created_by)
         values ($1, $2, $3, 'kjoring', $4, $5, $6, $7, $8, $9, $10, $11)",
    )
    .bind(id)
    .bind(company_id)
    .bind(person_id)
    .bind(date)
    .bind(description)
    .bind(beregnet.amount_ore)
    .bind(km)
    .bind(rate)
    .bind(beregnet.tax_free_ore)
    .bind(beregnet.taxable_ore)
    .bind(created_by)
    .execute(pool)
    .await?;
    Ok((id, beregnet.amount_ore, beregnet.taxable_ore))
}

#[derive(Debug)]
pub struct ExpenseRow {
    pub id: Uuid,
    pub person_name: String,
    pub own: bool,
    /// Who entered it, when not the claimant themself.
    pub registered_by: Option<String>,
    /// The viewer may edit it while it is a draft.
    pub editable: bool,
    /// The viewer entered it (on someone's behalf or their own).
    pub entered_by_me: bool,
    pub kind: String,
    pub date: Option<NaiveDate>,
    pub description: String,
    pub amount_ore: Option<i64>,
    pub department: Option<String>,
    pub project: Option<String>,
    pub billable: bool,
    pub route_from: Option<String>,
    pub route_to: Option<String>,
    pub round_trip: bool,
    pub vehicle: Option<String>,
    pub passengers: Vec<String>,
    pub passenger_rate_ore_per_km: Option<i64>,
    pub trip_expense_id: Option<Uuid>,
    pub km: Option<i64>,
    pub sats_ore_per_km: Option<i64>,
    pub trekkfri_ore: Option<i64>,
    pub trekkpliktig_ore: Option<i64>,
    pub receipt_filename: Option<String>,
    pub status: String,
    pub avvist_note: Option<String>,
    pub decided_by: Option<String>,
    pub decided_at: Option<chrono::DateTime<chrono::Utc>>,
    /// The kostnad voucher posted at approval.
    pub voucher: Option<String>,
    /// The utbetaling voucher.
    pub paid_voucher: Option<String>,
    pub utbetalt_at: Option<chrono::DateTime<chrono::Utc>>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    /// How an approved claim gets paid: `lonn` (kjøring to an employee —
    /// the next payroll run), `betalingsliste` (on a list on its way to
    /// the bank) or None (by hand, or a list not made yet).
    pub paid_by: Option<&'static str>,
    /// The invoice that rebilled it, and how.
    pub rebilled_invoice_id: Option<Uuid>,
    pub rebilled_as: Option<String>,
    pub departure_at: Option<chrono::NaiveDateTime>,
    pub return_at: Option<chrono::NaiveDateTime>,
    pub destination: Option<String>,
    pub overnight: bool,
    pub over_15_km: bool,
    pub travel_days: serde_json::Value,
    pub rate_scheme: Option<String>,
    /// The computed døgn, stored at submission.
    pub travel_lines: Option<serde_json::Value>,
    pub company_card_id: Option<Uuid>,
    pub card_label: Option<String>,
    /// The card's ledger account, which approval credits.
    pub card_account: Option<String>,
    pub bank_transaction_id: Option<Uuid>,
}

/// Which claims a list shows. `status` is a TAB, not a stored value:
/// `apne` = innsendt + godkjent (money not yet out), or one of the
/// stored statuses. `expense_id` picks one claim (the detail page),
/// `trip_expense_id` the costs tied to a trip. Drafts ('utkast') are
/// only ever the viewer's own — the claimant's or the one who entered
/// them — and never in a list without asking for them; discarded ones
/// never show in a list at all.
#[derive(Debug, Default, Clone)]
pub struct ExpenseFilter {
    pub status: Option<String>,
    pub expense_id: Option<Uuid>,
    pub trip_expense_id: Option<Uuid>,
    pub limit: i64,
    pub offset: i64,
}

#[derive(Debug, Default)]
pub struct ExpenseCounts {
    pub utkast: i64,
    pub innsendt: i64,
    pub godkjent: i64,
    pub utbetalt: i64,
    pub avvist: i64,
}

#[derive(Debug)]
pub struct ExpensePage {
    pub rows: Vec<ExpenseRow>,
    /// Rows matching the filter, before paging.
    pub total: i64,
    /// Per status over the viewer's whole scope — the tab counters.
    pub counts: ExpenseCounts,
}

/// `only_own` scopes the list to the viewer's own claims — what an
/// ansatt gets. For everyone else the flag is false and the list is the
/// whole company's, with `own` marking the viewer's own rows.
pub async fn list_expenses(
    pool: &PgPool,
    company_id: Uuid,
    viewer: Uuid,
    only_own: bool,
    filter: &ExpenseFilter,
) -> Result<ExpensePage> {
    let statuses: Option<Vec<&str>> = match filter.status.as_deref() {
        None | Some("") => None,
        Some("apne") => Some(vec!["innsendt", "godkjent"]),
        Some(s @ ("utkast" | "innsendt" | "godkjent" | "utbetalt" | "avvist")) => Some(vec![s]),
        Some(other) => bail!("ukjent status «{other}»"),
    };
    let sql = format!(
        "select e.id, coalesce(p.name, p.oidc_sub) as person_name, e.person_id = $2 as own,
                case when e.registered_by_person <> e.person_id
                     then coalesce(rp.name, rp.oidc_sub) end as registered_by,
                coalesce(e.status = 'utkast' and $2 in (e.person_id, e.registered_by_person),
                         false) as editable,
                coalesce(e.registered_by_person = $2, false) as entered_by_me,
                e.department, e.project, e.billable, e.route_from, e.route_to, e.round_trip,
                e.vehicle, e.passengers, e.passenger_rate_ore_per_km, e.trip_expense_id,
                e.kind, e.dato, e.beskrivelse, e.belop_ore, e.km, e.sats_ore_per_km,
                e.trekkfri_ore, e.trekkpliktig_ore, e.receipt_filename, e.status,
                e.avvist_note, e.decided_by, e.decided_at, e.utbetalt_at, e.created_at,
                v.fiscal_year, v.voucher_number,
                pv.fiscal_year as paid_year, pv.voucher_number as paid_number,
                e.kind <> 'utlegg' and {by_payroll} as by_payroll, {in_run} as in_run,
                e.departure_at, e.return_at, e.destination, e.overnight, e.over_15_km,
                e.travel_days, e.rate_scheme, e.travel_lines, e.company_card_id,
                e.bank_transaction_id,
                (select cc.name || ' ••' || cc.last4 from company_card cc
                  where cc.id = e.company_card_id) as card_label,
                (select ca.number from company_card cc join account ca on ca.id = cc.account_id
                  where cc.id = e.company_card_id) as card_account,
                ei.invoice_id as rebilled_invoice_id, ei.treatment as rebilled_as
         from expense e
         left join expense_invoice ei on ei.expense_id = e.id
         join person p on p.id = e.person_id
         left join person rp on rp.id = e.registered_by_person
         left join voucher v on v.id = e.voucher_id
         left join voucher pv on pv.id = e.utbetalt_voucher_id
         where e.company_id = $1
           and ($3 = false or e.person_id = $2 or e.registered_by_person = $2)
           and (e.status <> 'utkast' or $2 in (e.person_id, e.registered_by_person))
           and case when $5::uuid is not null then e.id = $5
                    when $8::uuid is not null then e.trip_expense_id = $8
                                                   and e.status <> 'forkastet'
                    when $4::text[] is not null then e.status = any($4)
                    else e.status not in ('utkast', 'forkastet') end
         order by e.created_at desc
         limit $6 offset $7",
        by_payroll = paid_with_pay("e"),
        in_run = in_open_run("e"),
    );
    let rows_query = sqlx::query(&sql)
        .bind(company_id)
        .bind(viewer)
        .bind(only_own)
        .bind(statuses.clone())
        .bind(filter.expense_id)
        .bind(filter.limit.clamp(1, 500))
        .bind(filter.offset.max(0))
        .bind(filter.trip_expense_id)
        .fetch_all(pool);
    let counts_query = sqlx::query(
        "select status, count(*) as n from expense
         where company_id = $1 and ($3 = false or person_id = $2 or registered_by_person = $2)
           and (status <> 'utkast' or $2 in (person_id, registered_by_person))
         group by status",
    )
    .bind(company_id)
    .bind(viewer)
    .bind(only_own)
    .fetch_all(pool);
    let (rows, count_rows) = tokio::try_join!(rows_query, counts_query)?;
    let voucher = |r: &sqlx::postgres::PgRow, year: &str, no: &str| match (
        r.get::<Option<i32>, _>(year),
        r.get::<Option<i64>, _>(no),
    ) {
        (Some(year), Some(no)) => Some(format!("{year}-{no}")),
        _ => None,
    };
    let rows: Vec<ExpenseRow> = rows
        .iter()
        .map(|r| ExpenseRow {
            id: r.get("id"),
            person_name: r.get("person_name"),
            own: r.get("own"),
            registered_by: r.get("registered_by"),
            editable: r.get("editable"),
            entered_by_me: r.get("entered_by_me"),
            department: r.get("department"),
            project: r.get("project"),
            billable: r.get("billable"),
            route_from: r.get("route_from"),
            route_to: r.get("route_to"),
            round_trip: r.get("round_trip"),
            vehicle: r.get("vehicle"),
            passengers: r.get("passengers"),
            passenger_rate_ore_per_km: r.get("passenger_rate_ore_per_km"),
            trip_expense_id: r.get("trip_expense_id"),
            kind: r.get("kind"),
            date: r.get("dato"),
            description: r.get("beskrivelse"),
            amount_ore: r.get("belop_ore"),
            km: r.get("km"),
            sats_ore_per_km: r.get("sats_ore_per_km"),
            trekkfri_ore: r.get("trekkfri_ore"),
            trekkpliktig_ore: r.get("trekkpliktig_ore"),
            receipt_filename: r.get("receipt_filename"),
            status: r.get("status"),
            avvist_note: r.get("avvist_note"),
            decided_by: r.get("decided_by"),
            decided_at: r.get("decided_at"),
            voucher: voucher(r, "fiscal_year", "voucher_number"),
            paid_voucher: voucher(r, "paid_year", "paid_number"),
            utbetalt_at: r.get("utbetalt_at"),
            created_at: r.get("created_at"),
            paid_by: if r.get::<Option<Uuid>, _>("company_card_id").is_some() {
                Some("firmakort")
            } else if r.get("by_payroll") {
                Some("lonn")
            } else if r.get("in_run") {
                Some("betalingsliste")
            } else {
                None
            },
            rebilled_invoice_id: r.get("rebilled_invoice_id"),
            rebilled_as: r.get("rebilled_as"),
            departure_at: r.get("departure_at"),
            return_at: r.get("return_at"),
            destination: r.get("destination"),
            overnight: r.get("overnight"),
            over_15_km: r.get("over_15_km"),
            travel_days: r.get("travel_days"),
            rate_scheme: r.get("rate_scheme"),
            travel_lines: r.get("travel_lines"),
            company_card_id: r.get("company_card_id"),
            card_label: r.get("card_label"),
            card_account: r.get("card_account"),
            bank_transaction_id: r.get("bank_transaction_id"),
        })
        .collect();

    let mut counts = ExpenseCounts::default();
    for r in &count_rows {
        let n: i64 = r.get("n");
        match r.get::<String, _>("status").as_str() {
            "utkast" => counts.utkast = n,
            "innsendt" => counts.innsendt = n,
            "godkjent" => counts.godkjent = n,
            "utbetalt" => counts.utbetalt = n,
            "avvist" => counts.avvist = n,
            _ => {}
        }
    }
    // The total comes from the counts, not from the page: a page past
    // the end still knows how many rows there are to page back to.
    let total = if filter.expense_id.is_some() || filter.trip_expense_id.is_some() {
        rows.len() as i64
    } else {
        let all = ["innsendt", "godkjent", "utbetalt", "avvist"];
        let n = |st: &str| match st {
            "utkast" => counts.utkast,
            "innsendt" => counts.innsendt,
            "godkjent" => counts.godkjent,
            "utbetalt" => counts.utbetalt,
            _ => counts.avvist,
        };
        statuses
            .as_deref()
            .unwrap_or(&all[..])
            .iter()
            .map(|st| n(st))
            .sum()
    };
    Ok(ExpensePage {
        rows,
        total,
        counts,
    })
}

/// The receipt, integrity-checked against the stored SHA-256 on every
/// read — a claim whose documentation cannot be verified is an error,
/// never silently served.
/// Who submitted a claim — the caller needs it to decide whether a
/// viewer with only `UTLEGG_LES_EGNE` may see it.
pub async fn expense_owner(pool: &PgPool, company_id: Uuid, expense_id: Uuid) -> Result<Uuid> {
    sqlx::query_scalar("select person_id from expense where id = $1 and company_id = $2")
        .bind(expense_id)
        .bind(company_id)
        .fetch_optional(pool)
        .await?
        .context("no such utlegg")
}

pub async fn expense_receipt(
    pool: &PgPool,
    company_id: Uuid,
    expense_id: Uuid,
) -> Result<(String, String, Vec<u8>)> {
    let row = sqlx::query(
        "select receipt_filename, receipt_content_type, receipt_content, receipt_sha256
         from expense where id = $1 and company_id = $2 and kind = 'utlegg'",
    )
    .bind(expense_id)
    .bind(company_id)
    .fetch_optional(pool)
    .await?
    .context("no such utlegg")?;
    let stored: Vec<u8> = row.get("receipt_sha256");
    let content = crate::blob::blobs()
        .load(row.get("receipt_content"), &stored)
        .await
        .with_context(|| format!("receipt of expense {expense_id}"))?;
    Ok((
        row.get("receipt_filename"),
        row.get("receipt_content_type"),
        content,
    ))
}

#[derive(Debug)]
pub struct ApprovedExpense {
    pub voucher_number: i64,
    pub fiscal_year: i32,
    /// Set for kjøring with a trekkpliktig del: the honest warning the
    /// caller must surface (lønnsinnberetning is not built yet).
    pub warning: Option<String>,
}

/// Approves a claim in ONE transaction: the kostnad voucher (utlegg:
/// netto + eventuell inngående mva mot brutto; kjøring: hele beløpet),
/// kredit mellomregning, the receipt copied onto the voucher as an
/// attachment, and the one-way status flip. Rows lock FOR UPDATE so a
/// claim can never be approved twice.
#[allow(clippy::too_many_arguments)]
pub async fn approve_expense(
    pool: &PgPool,
    company_id: Uuid,
    expense_id: Uuid,
    account: &str,
    vat_code: Option<&str>,
    vat_account: &str,
    contra_account: &str,
    decided_by: &str,
    decided_by_person: Uuid,
) -> Result<ApprovedExpense> {
    let mut tx = pool.begin().await?;
    let expense = sqlx::query(&format!(
        "select kind, dato, beskrivelse, belop_ore, km, sats_ore_per_km, trekkpliktig_ore,
                receipt_filename, receipt_content_type, receipt_content, receipt_sha256,
                status, person_id, registered_by_person, department, project,
                route_from, route_to, round_trip, passengers, departure_at, return_at,
                destination, travel_lines, company_card_id, bank_transaction_id,
                coalesce((select p.name from person p where p.id = expense.person_id), '') as navn,
                {employee} as employee
         from expense where id = $1 and company_id = $2 for update",
        employee = paid_with_pay("expense"),
    ))
    .bind(expense_id)
    .bind(company_id)
    .fetch_optional(&mut *tx)
    .await?
    .context("no such expense")?;
    let status: String = expense.get("status");
    ensure!(status == "innsendt", "kravet er allerede {status}");

    // Self-approval was allowed in v1 (#42); with attestering active
    // (docs/attestering.md, #47) the approver must be someone other than
    // the person who submitted the claim.
    if let Some(policy) = crate::approval::current_policy(&mut *tx, company_id).await?
        && policy.active
    {
        // Whoever ENTERED a claim on someone's behalf is as much its
        // author as the claimant.
        let claimant: Uuid = expense.get("person_id");
        let entered_by: Option<Uuid> = expense.get("registered_by_person");
        ensure!(
            claimant != decided_by_person && entered_by != Some(decided_by_person),
            "utleggskrav kan ikke godkjennes av den som sendte det inn (fire øyne)"
        );
    }
    let kind: String = expense.get("kind");
    let date: NaiveDate = expense.get("dato");
    let amount: i64 = expense.get("belop_ore");
    // The claim's dimensions go on its cost lines — not on the VAT or the
    // debt to the claimant, which belong to no project.
    let department: Option<String> = expense.get("department");
    let project: Option<String> = expense.get("project");
    let description: String = expense.get("beskrivelse");
    let name: String = expense.get("navn");

    let mut entries = Vec::new();
    let mut warning = None;
    match kind.as_str() {
        "utlegg" => {
            // Deducting input VAT needs the registration too (#99) —
            // otherwise approval quietly books a deduction the company
            // has no right to, and the cost comes out too low.
            if vat_code.is_some_and(crate::settings::requires_vat_registration) {
                crate::settings::require_vat_registered(pool, company_id, date).await?;
            }
            let (net, vat) = match vat_code {
                Some(code) => {
                    let rate_class: String =
                        sqlx::query_scalar("select rate_class from vat_code where code = $1")
                            .bind(code)
                            .fetch_optional(pool)
                            .await?
                            .with_context(|| format!("unknown vat code {code}"))?;
                    let rates = load_vat_rates(pool).await?;
                    let rate_bp = rate_on(&rates, &rate_class, date)
                        .with_context(|| format!("no rate for {date}"))?;
                    split_gross(amount, rate_bp)
                }
                None => (amount, 0),
            };
            entries.push(EntryDraft {
                account_number: account.to_string(),
                amount: Ore(net),
                vat_code: vat_code.map(str::to_string),
                description: Some(description.clone()),
                party_no: None,
                department: department.clone(),
                project: project.clone(),
                currency: None,
            });
            if vat != 0 {
                entries.push(EntryDraft {
                    account_number: vat_account.to_string(),
                    amount: Ore(vat),
                    vat_code: None,
                    description: Some("Inngående mva".into()),
                    party_no: None,
                    department: None,
                    project: None,
                    currency: None,
                });
            }
        }
        "reise" => {
            ensure!(vat_code.is_none(), "diett har ingen mva");
            // The reiseregning's essentials on the posting: where, when,
            // how many døgn — the documentation that keeps it trekkfri.
            let departure: chrono::NaiveDateTime = expense.get("departure_at");
            let return_at: chrono::NaiveDateTime = expense.get("return_at");
            let destination: String = expense
                .get::<Option<String>, _>("destination")
                .unwrap_or_default();
            let lines: Vec<crate::travel::TravelLine> = serde_json::from_value(
                expense
                    .get::<Option<serde_json::Value>, _>("travel_lines")
                    .context("innsendt reise uten døgnlinjer")?,
            )?;
            entries.push(EntryDraft {
                account_number: account.to_string(),
                amount: Ore(amount),
                vat_code: None,
                description: Some(format!(
                    "{description} — {destination}, {} til {}, {} døgn",
                    departure.format("%d.%m.%Y %H:%M"),
                    return_at.format("%d.%m.%Y %H:%M"),
                    lines.len()
                )),
                party_no: None,
                department,
                project,
                currency: None,
            });
            if !expense.get::<bool, _>("employee") {
                warning = Some(format!(
                    "{name} er ikke ansatt under Lønn nå, så ingen lønnskjøring kan utbetale \
                     diettgodtgjørelsen og melde den i a-meldingen — den må meldes særskilt. Er \
                     {name} ansatt, registrer og koble personen, så tar neste lønnskjøring kravet."
                ));
            }
        }
        _ => {
            ensure!(vat_code.is_none(), "kjøregodtgjørelse har ingen mva");
            let km: i64 = expense.get::<Option<i64>, _>("km").unwrap_or(0);
            let rate: i64 = expense
                .get::<Option<i64>, _>("sats_ore_per_km")
                .unwrap_or(0);
            // The route and the passengers are the documentation that
            // keeps the allowance trekkfri; the posting carries them.
            let route = match (
                expense.get::<Option<String>, _>("route_from"),
                expense.get::<Option<String>, _>("route_to"),
            ) {
                (Some(from), Some(to)) => {
                    let back = if expense.get::<bool, _>("round_trip") {
                        " t/r"
                    } else {
                        ""
                    };
                    format!(" — {from}–{to}{back}")
                }
                _ => String::new(),
            };
            let passengers: Vec<String> = expense.get("passengers");
            let with = if passengers.is_empty() {
                String::new()
            } else {
                format!(", passasjer: {}", passengers.join(", "))
            };
            entries.push(EntryDraft {
                account_number: account.to_string(),
                amount: Ore(amount),
                vat_code: None,
                description: Some(format!(
                    "{description}{route} — {km} km à {},{:02} kr{with}",
                    rate / 100,
                    rate % 100
                )),
                party_no: None,
                department,
                project,
                currency: None,
            });
            // An employee is paid with the next payroll run, which takes
            // trekk on the taxable part and reports both parts. Anyone
            // else has no arbeidsforhold regnmed can report on.
            if !expense.get::<bool, _>("employee") {
                warning = Some(format!(
                    "{name} er ikke ansatt under Lønn nå, så ingen lønnskjøring kan utbetale \
                     godtgjørelsen og melde den i a-meldingen — den må meldes særskilt. Er \
                     {name} ansatt, registrer og koble personen, så tar neste lønnskjøring kravet."
                ));
            }
        }
    }
    // Paid with a company card: the money left the card's account, and
    // nobody is owed anything.
    let card = match expense.get::<Option<Uuid>, _>("company_card_id") {
        Some(id) => Some(crate::company_card::get_card(pool, company_id, id).await?),
        None => None,
    };
    let contra_account = card
        .as_ref()
        .map_or(contra_account, |c| c.account_number.as_str());
    entries.push(EntryDraft {
        account_number: contra_account.to_string(),
        amount: Ore(-amount),
        vat_code: None,
        description: Some(match &card {
            Some(c) => format!("Betalt med {}", c.label()),
            None => format!("Skyldig {name}"),
        }),
        party_no: None,
        department: None,
        project: None,
        currency: None,
    });

    let label = match kind.as_str() {
        "utlegg" => "Utlegg",
        "reise" => "Diett",
        _ => "Kjøregodtgjørelse",
    };
    let draft = VoucherDraft {
        journal_code: "GL".into(),
        voucher_date: date,
        description: format!("{label} {name}: {description}"),
        reverses: None,
        entries,
    };
    draft.validate().map_err(|e| anyhow::anyhow!("{e}"))?;
    let posted = post_voucher_in(&mut tx, company_id, &draft, decided_by).await?;

    if kind == "utlegg" {
        let stored: Vec<u8> = expense.get("receipt_sha256");
        let content = crate::blob::blobs()
            .load(expense.get("receipt_content"), &stored)
            .await?;
        add_attachment_in(
            &mut tx,
            company_id,
            posted.id,
            expense.get("receipt_filename"),
            expense.get("receipt_content_type"),
            &content,
            decided_by,
        )
        .await?;
    }

    sqlx::query(
        "update expense set status = 'godkjent', decided_by = $3, decided_at = now(),
                            voucher_id = $4, motkonto = $5
         where id = $1 and company_id = $2",
    )
    .bind(expense_id)
    .bind(company_id)
    .bind(decided_by)
    .bind(posted.id)
    .bind(contra_account)
    .execute(&mut *tx)
    .await?;

    if card.is_some() {
        // Filed from a statement line: the line is reconciled against the
        // card entry now, in this transaction — equal amounts by
        // construction, and never twice.
        if let Some(line) = expense.get::<Option<Uuid>, _>("bank_transaction_id") {
            let entry_id: Uuid = sqlx::query_scalar(
                "select e.id from entry e join account a on a.id = e.account_id
                 where e.voucher_id = $1 and a.number = $2 and e.amount_ore = $3",
            )
            .bind(posted.id)
            .bind(contra_account)
            .bind(-amount)
            .fetch_one(&mut *tx)
            .await?;
            let line_amount: Option<i64> = sqlx::query_scalar(
                "select t.amount_ore from bank_transaction t
                 join bank_statement s on s.id = t.statement_id
                 join account a on a.id = s.account_id
                 where t.id = $1 and s.company_id = $2 and a.number = $3
                   and not exists (select 1 from bank_match m where m.bank_transaction_id = t.id)",
            )
            .bind(line)
            .bind(company_id)
            .bind(contra_account)
            .fetch_optional(&mut *tx)
            .await?;
            match line_amount {
                None => bail!(
                    "kortlinjen kravet ble sendt inn fra er allerede avstemt mot et annet bilag \
                     — avvis kravet, eller send det inn på nytt uten kortlinjen"
                ),
                Some(a) => ensure!(
                    a == -amount,
                    "kortlinjen er på {} kr, kvitteringen på {} kr — beløpene må være like",
                    regnmed_core::Ore(-a),
                    regnmed_core::Ore(amount)
                ),
            }
            sqlx::query(
                "insert into bank_match (bank_transaction_id, entry_id, method, matched_by)
                 values ($1, $2, 'manual', $3)",
            )
            .bind(line)
            .bind(entry_id)
            .bind(decided_by)
            .execute(&mut *tx)
            .await?;
        }
        // Nothing to pay out: the claim is settled with its own voucher.
        sqlx::query(
            "update expense set status = 'utbetalt', utbetalt_voucher_id = voucher_id,
                                utbetalt_at = now()
             where id = $1",
        )
        .bind(expense_id)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    // A card receipt filed on its own: the card statement may already be
    // in, so its line is matched now the way an import would match it
    // (equal amount, nearest date, ties left to a human) — this receipt's
    // entry only, nothing else on the account.
    if card.is_some()
        && expense
            .get::<Option<Uuid>, _>("bank_transaction_id")
            .is_none()
    {
        let (account_id, entry_id): (Uuid, Uuid) = sqlx::query_as(
            "select a.id, e.id from entry e join account a on a.id = e.account_id
             where e.voucher_id = $1 and a.number = $2 and e.amount_ore = $3",
        )
        .bind(posted.id)
        .bind(contra_account)
        .bind(-amount)
        .fetch_one(pool)
        .await?;
        // The approval stands either way; a line left unmatched is for
        // the bank reconciliation, and the caller hears about it.
        if let Err(e) =
            crate::bank::match_entry(pool, company_id, account_id, entry_id, decided_by).await
        {
            warning = Some(format!(
                "Godkjent, men kortlinjen ble ikke avstemt automatisk ({e}) — avstem den under Bank"
            ));
        }
    }
    Ok(ApprovedExpense {
        voucher_number: posted.voucher_number,
        fiscal_year: posted.fiscal_year,
        warning,
    })
}

/// Avvisning is one-way and requires a begrunnelse — a rejected claim
/// with its note is part of the story, never deleted.
pub async fn reject_expense(
    pool: &PgPool,
    company_id: Uuid,
    expense_id: Uuid,
    note: &str,
    decided_by: &str,
) -> Result<()> {
    ensure!(!note.trim().is_empty(), "avvisning krever begrunnelse");
    let updated = sqlx::query(
        "update expense set status = 'avvist', decided_by = $3, decided_at = now(),
                            avvist_note = $4
         where id = $1 and company_id = $2 and status = 'innsendt'",
    )
    .bind(expense_id)
    .bind(company_id)
    .bind(decided_by)
    .bind(note)
    .execute(pool)
    .await?;
    if updated.rows_affected() != 1 {
        bail!("kravet finnes ikke eller er allerede avgjort");
    }
    Ok(())
}

#[derive(Debug)]
pub struct PaidExpense {
    pub voucher_number: i64,
    pub fiscal_year: i32,
}

/// Marks the reimbursement paid: debet mellomregning (the konto the
/// approval credited), kredit bank — one transaction with the one-way
/// status flip. When remittering (#33) lands, the betalingsliste will
/// drive this step instead of a manual click.
pub async fn pay_expense(
    pool: &PgPool,
    company_id: Uuid,
    expense_id: Uuid,
    date: NaiveDate,
    bank_account: &str,
    paid_by: &str,
) -> Result<PaidExpense> {
    let mut tx = pool.begin().await?;
    let expense = sqlx::query(&format!(
        "select status, kind, belop_ore, motkonto, beskrivelse,
                coalesce((select p.name from person p where p.id = expense.person_id), '') as navn,
                {employee} as employee
         from expense where id = $1 and company_id = $2 for update",
        employee = paid_with_pay("expense"),
    ))
    .bind(expense_id)
    .bind(company_id)
    .fetch_optional(&mut *tx)
    .await?
    .context("no such expense")?;
    let status: String = expense.get("status");
    ensure!(
        status == "godkjent",
        "bare godkjente krav kan utbetales (kravet er {status})"
    );
    // A statement of its own, AFTER the row lock: one that started
    // before a concurrent list was committed would not see its item.
    let in_run: bool = sqlx::query_scalar(&format!(
        "select {} from expense x where x.id = $1",
        in_open_run("x")
    ))
    .bind(expense_id)
    .fetch_one(&mut *tx)
    .await?;
    ensure!(
        !in_run,
        "kravet ligger i en betalingsliste — det betales når listen registreres betalt"
    );
    // The taxable part needs trekk, and both parts belong in the
    // a-melding: the payroll run does all three.
    ensure!(
        expense.get::<String, _>("kind") == "utlegg" || !expense.get::<bool, _>("employee"),
        "kjøregodtgjørelse og diett til en ansatt utbetales med neste lønnskjøring, som også \
         trekker skatt av den trekkpliktige delen og melder den i a-meldingen"
    );
    let amount: i64 = expense.get("belop_ore");
    let contra_account: String = expense
        .get::<Option<String>, _>("motkonto")
        .unwrap_or_default();
    let name: String = expense.get("navn");

    let draft = VoucherDraft {
        journal_code: "GL".into(),
        voucher_date: date,
        description: format!("Utbetaling utlegg {name}"),
        reverses: None,
        entries: vec![
            EntryDraft {
                account_number: contra_account,
                amount: Ore(amount),
                vat_code: None,
                description: Some(expense.get("beskrivelse")),
                party_no: None,
                department: None,
                project: None,
                currency: None,
            },
            EntryDraft {
                account_number: bank_account.to_string(),
                amount: Ore(-amount),
                vat_code: None,
                description: None,
                party_no: None,
                department: None,
                project: None,
                currency: None,
            },
        ],
    };
    draft.validate().map_err(|e| anyhow::anyhow!("{e}"))?;
    let posted = post_voucher_in(&mut tx, company_id, &draft, paid_by).await?;
    sqlx::query(
        "update expense set status = 'utbetalt', utbetalt_voucher_id = $3, utbetalt_at = now()
         where id = $1 and company_id = $2",
    )
    .bind(expense_id)
    .bind(company_id)
    .bind(posted.id)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(PaidExpense {
        voucher_number: posted.voucher_number,
        fiscal_year: posted.fiscal_year,
    })
}

/// The company's own customers and suppliers whose name matches, with
/// the address on file — the first suggestions in the trip form.
pub async fn party_addresses(
    pool: &PgPool,
    company_id: Uuid,
    text: &str,
) -> Result<Vec<(String, String)>> {
    Ok(sqlx::query_as(
        "select distinct on (name) name, address from party
         where company_id = $1 and coalesce(address, '') <> ''
           and strpos(lower(name), lower($2)) > 0
         order by name limit 3",
    )
    .bind(company_id)
    .bind(text)
    .fetch_all(pool)
    .await?)
}

/// Addresses the viewer has driven from or to before, newest first —
/// the first suggestions in the trip form, and the only ones for a
/// field that is still (nearly) empty.
pub async fn own_addresses(
    pool: &PgPool,
    company_id: Uuid,
    person_id: Uuid,
    text: &str,
) -> Result<Vec<String>> {
    Ok(sqlx::query_scalar(
        "select address from (
             select a.address, max(e.created_at) as used
             from expense e,
                  lateral (values (e.route_from), (e.route_to)) as a(address)
             where e.company_id = $1 and e.person_id = $2 and e.kind = 'kjoring'
               and e.status <> 'forkastet' and coalesce(a.address, '') <> ''
               and strpos(lower(a.address), lower($3)) > 0
             group by a.address
         ) recent order by used desc limit 5",
    )
    .bind(company_id)
    .bind(person_id)
    .bind(text)
    .fetch_all(pool)
    .await?)
}

/// How a billable claim goes onto the customer's invoice.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RebillAs {
    /// Part of what the company sells: the cost the claim booked, with
    /// output VAT like the hours it travels with.
    Resale,
    /// Bought in the customer's name and for their account, passed on
    /// without markup and outside the VAT base (mval. § 4-1 second
    /// paragraph letter a). Only a claim approved WITHOUT input VAT onto
    /// a balance account qualifies: the invoice line clears that account.
    Disbursement,
}

impl RebillAs {
    fn stored(self) -> &'static str {
        match self {
            Self::Resale => "videresalg",
            Self::Disbursement => "utlegg",
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Rebill {
    pub expense_id: Uuid,
    pub treatment: RebillAs,
}

/// The invoice lines for the chosen claims, and the latest claim date
/// (the delivery date must not precede what it bills). Every claim must
/// be approved, billable and not rebilled before — anything else fails
/// the whole call rather than billing less than what was chosen.
pub(crate) async fn rebill_lines(
    pool: &PgPool,
    company_id: Uuid,
    picks: &[Rebill],
    invoice_date: NaiveDate,
) -> Result<(Vec<crate::invoice::InvoiceLineDraft>, Option<NaiveDate>)> {
    // A company outside the VAT register resells without VAT: charging it
    // would be collecting a tax it has no right to. A derived code is 7,
    // as for every other line such a company issues (`settings`).
    let registered = crate::settings::registrering_on(pool, company_id, invoice_date)
        .await?
        .vat_registered;
    let resale_vat = if registered { "3" } else { "7" };
    let ids: Vec<Uuid> = picks.iter().map(|p| p.expense_id).collect();
    ensure!(
        ids.len() == ids.iter().collect::<std::collections::HashSet<_>>().len(),
        "samme utleggskrav er valgt to ganger"
    );
    // The approval's first line is the cost (approve_expense writes it
    // first): net of any input VAT deducted, on the account it chose.
    let rows = sqlx::query(
        "select x.id, x.kind, x.dato, x.beskrivelse, x.belop_ore, x.status, x.billable,
                x.project, x.department, a.number as account, e.amount_ore as cost_ore,
                e.vat_code as cost_vat,
                exists (select 1 from expense_invoice r where r.expense_id = x.id) as rebilled
         from expense x
         join entry e on e.voucher_id = x.voucher_id and e.line_no = 1
         join account a on a.id = e.account_id
         where x.company_id = $1 and x.id = any($2)",
    )
    .bind(company_id)
    .bind(&ids)
    .fetch_all(pool)
    .await?;
    let mut lines = Vec::new();
    let mut last = None;
    for pick in picks {
        let r = rows
            .iter()
            .find(|r| r.get::<Uuid, _>("id") == pick.expense_id)
            .with_context(|| {
                format!(
                    "utleggskrav {} er ikke godkjent eller finnes ikke",
                    pick.expense_id
                )
            })?;
        let what: String = r.get("beskrivelse");
        let status: String = r.get("status");
        ensure!(
            status == "godkjent" || status == "utbetalt",
            "«{what}» er {status} — bare godkjente krav kan viderefaktureres"
        );
        ensure!(
            r.get::<bool, _>("billable"),
            "«{what}» er ikke merket for viderefakturering"
        );
        ensure!(
            !r.get::<bool, _>("rebilled"),
            "«{what}» er allerede viderefakturert"
        );
        let date: NaiveDate = r.get("dato");
        let kind: String = r.get("kind");
        // The customer reads the line: a Norwegian date, not ISO.
        let shown = date.format("%d.%m.%Y");
        let line = match pick.treatment {
            RebillAs::Resale => crate::invoice::InvoiceLineDraft {
                description: match kind.as_str() {
                    "kjoring" => format!("Kjøring {shown}: {what}"),
                    "reise" => format!("Diett, reise {shown}: {what}"),
                    _ => format!("{what} ({shown})"),
                },
                account_number: "3000".into(),
                quantity_milli: 1000,
                unit_price_ore: r.get("cost_ore"),
                vat_code: Some(resale_vat.into()),
                department: r.get("department"),
                project: r.get("project"),
                product_id: None,
            },
            RebillAs::Disbursement => {
                let account: String = r.get("account");
                ensure!(
                    kind == "utlegg",
                    "kjøregodtgjørelse og diett er kostnader for selskapet og kan bare \
                     viderefaktureres som videresalg"
                );
                ensure!(
                    r.get::<Option<String>, _>("cost_vat").is_none()
                        && r.get::<i64, _>("cost_ore") == r.get::<i64, _>("belop_ore"),
                    "«{what}» ble godkjent med fradrag for inngående mva, og et utlegg for kunden \
                     har ingen mva hos oss — fakturer det som videresalg"
                );
                ensure!(
                    account.starts_with('1'),
                    "«{what}» ble godkjent som kostnad på konto {account}. Et utlegg kjøpt i \
                     kundens navn føres på en fordringskonto (f.eks. 1579) — fakturer det som \
                     videresalg, eller godkjenn slike krav på en balansekonto"
                );
                crate::invoice::InvoiceLineDraft {
                    description: format!("Utlegg: {what} ({shown})"),
                    account_number: account,
                    quantity_milli: 1000,
                    unit_price_ore: r.get("belop_ore"),
                    vat_code: None,
                    department: None,
                    project: None,
                    product_id: None,
                }
            }
        };
        lines.push(line);
        last = last.max(Some(date));
    }
    Ok((lines, last))
}

/// Records which invoice took each claim, inside the invoice's own
/// transaction. The primary key makes a second rebilling impossible even
/// if two invoices race for the same claim.
pub(crate) async fn mark_rebilled(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    picks: &[Rebill],
    invoice_id: Uuid,
    created_by: &str,
) -> Result<()> {
    for pick in picks {
        sqlx::query(
            "insert into expense_invoice (expense_id, invoice_id, treatment, created_by)
             values ($1, $2, $3, $4)",
        )
        .bind(pick.expense_id)
        .bind(invoice_id)
        .bind(pick.treatment.stored())
        .bind(created_by)
        .execute(&mut **tx)
        .await
        .map_err(|e| match &e {
            sqlx::Error::Database(d) if d.code().as_deref() == Some("23505") => {
                anyhow::anyhow!("et av utleggskravene ble viderefakturert samtidig — prøv igjen")
            }
            _ => e.into(),
        })?;
    }
    Ok(())
}

/// A claim waiting to be rebilled.
#[derive(Debug)]
pub struct BillableExpense {
    pub expense_id: Uuid,
    pub kind: String,
    pub date: NaiveDate,
    pub description: String,
    pub claimant: String,
    pub amount_ore: i64,
    /// What a resale line bills: the cost the approval booked, net of any
    /// input VAT deducted.
    pub cost_ore: i64,
    pub project: Option<String>,
    /// The project's customer (docs/dimensjoner.md) — a suggestion for
    /// whom to invoice, never a rule.
    pub party_no: Option<String>,
    pub party_name: Option<String>,
    /// Approved without input VAT onto a balance account: may go on as a
    /// disbursement.
    pub can_disburse: bool,
}

/// Approved billable claims not yet rebilled, oldest first.
pub async fn billable_expenses(pool: &PgPool, company_id: Uuid) -> Result<Vec<BillableExpense>> {
    let rows = sqlx::query(
        "select x.id, x.kind, x.dato, x.beskrivelse, x.belop_ore, x.project,
                coalesce(p.name, '') as claimant, e.amount_ore as cost_ore,
                e.vat_code is null and e.amount_ore = x.belop_ore and a.number like '1%'
                    and x.kind = 'utlegg' as can_disburse,
                party.party_no, party.name as party_name
         from expense x
         join entry e on e.voucher_id = x.voucher_id and e.line_no = 1
         join account a on a.id = e.account_id
         left join person p on p.id = x.person_id
         left join dimension d on d.company_id = x.company_id and d.kind = 'prosjekt'
                              and d.code = x.project
         left join party on party.id = d.party_id
         where x.company_id = $1 and x.billable and x.status in ('godkjent', 'utbetalt')
           and not exists (select 1 from expense_invoice r where r.expense_id = x.id)
         order by x.dato, x.id",
    )
    .bind(company_id)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .iter()
        .map(|r| BillableExpense {
            expense_id: r.get("id"),
            kind: r.get("kind"),
            date: r.get("dato"),
            description: r.get("beskrivelse"),
            claimant: r.get("claimant"),
            amount_ore: r.get("belop_ore"),
            cost_ore: r.get("cost_ore"),
            project: r.get("project"),
            party_no: r.get("party_no"),
            party_name: r.get("party_name"),
            can_disburse: r.get("can_disburse"),
        })
        .collect())
}
