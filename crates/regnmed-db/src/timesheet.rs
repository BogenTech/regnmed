//! Timeføring (docs/timer.md, #38): integer minutes, editable until the
//! month is locked or the hours are billed — both enforced by trigger,
//! independently of the checks here. The fakturagrunnlag turns unbilled
//! billable hours into ordinary invoice lines (with the prosjekt
//! dimension carried onto the revenue line) and marks the entries
//! fakturert in the same transaction.

use anyhow::{Context, Result, ensure};
use chrono::NaiveDate;
use sqlx::{PgPool, Row};
use uuid::Uuid;

use crate::invoice::{InvoiceDraft, InvoiceLineDraft, IssuedInvoice, create_invoice_in};

#[derive(Debug)]
pub struct TimeEntryDraft {
    pub dato: NaiveDate,
    pub minutter: i32,
    pub beskrivelse: String,
    /// Prosjekt dimension CODE (resolved against the registry).
    pub prosjekt: Option<String>,
    /// None = the project's fakturerbar_default (false without a
    /// project). Every user may set this per entry.
    pub fakturerbar: Option<bool>,
    /// An EXPLICIT rate. Only honored when the caller may override
    /// (`sats_override`, i.e. TIMER_SATS_SKRIV) — otherwise the rate is
    /// resolved from the project's dated sats register on the entry's
    /// date (person-specific first, project default second).
    pub timesats_ore: Option<i64>,
}

struct Prosjekt {
    id: Uuid,
    fakturerbar_default: bool,
}

async fn resolve_prosjekt(
    pool: &PgPool,
    company_id: Uuid,
    code: &Option<String>,
) -> Result<Option<Prosjekt>> {
    let Some(code) = code else { return Ok(None) };
    let row = sqlx::query(
        "select id, active, fakturerbar_default from dimension
         where company_id = $1 and kind = 'prosjekt' and code = $2",
    )
    .bind(company_id)
    .bind(code)
    .fetch_optional(pool)
    .await?
    .with_context(|| format!("no prosjekt {code}"))?;
    ensure!(row.get::<bool, _>("active"), "prosjekt {code} er avsluttet");
    Ok(Some(Prosjekt {
        id: row.get("id"),
        fakturerbar_default: row.get("fakturerbar_default"),
    }))
}

/// The rate valid on `dato`: the person's own newest row first, the
/// project default second, nothing third (migration 0052).
async fn resolve_sats(
    pool: &PgPool,
    dimension_id: Uuid,
    person_id: Uuid,
    dato: NaiveDate,
) -> Result<Option<i64>> {
    Ok(sqlx::query_scalar(
        "select timesats_ore from prosjekt_sats
         where dimension_id = $1 and (person_id = $2 or person_id is null)
           and valid_from <= $3
         order by (person_id is not null) desc, valid_from desc
         limit 1",
    )
    .bind(dimension_id)
    .bind(person_id)
    .bind(dato)
    .fetch_optional(pool)
    .await?)
}

/// Resolves fakturerbar and sats for a draft: the project owns the
/// defaults, the register owns the rate, and only `sats_override`
/// (TIMER_SATS_SKRIV) lets the caller's own number through. Billable
/// hours without any rate fail loudly — a silent 0 would flow into an
/// invoice.
async fn resolve_billing(
    pool: &PgPool,
    person_id: Uuid,
    draft: &TimeEntryDraft,
    prosjekt: &Option<Prosjekt>,
    sats_override: bool,
) -> Result<(bool, Option<i64>)> {
    let fakturerbar = draft
        .fakturerbar
        .unwrap_or_else(|| prosjekt.as_ref().is_some_and(|p| p.fakturerbar_default));
    if !fakturerbar {
        return Ok((false, None));
    }
    if sats_override && draft.timesats_ore.is_some() {
        return Ok((true, draft.timesats_ore));
    }
    let sats = match prosjekt {
        Some(p) => resolve_sats(pool, p.id, person_id, draft.dato).await?,
        None => None,
    };
    ensure!(
        sats.is_some(),
        "fakturerbare timer trenger timesats — sett den på prosjektet (Prosjekter), \
         eller be noen med rett til å sette timesats"
    );
    Ok((true, sats))
}

fn check_draft(draft: &TimeEntryDraft) -> Result<()> {
    ensure!(
        (1..=1440).contains(&draft.minutter),
        "minutter must be 1..=1440"
    );
    Ok(())
}

pub async fn create_time_entry(
    pool: &PgPool,
    company_id: Uuid,
    person_id: Uuid,
    draft: &TimeEntryDraft,
    sats_override: bool,
    created_by: &str,
) -> Result<Uuid> {
    check_draft(draft)?;
    let prosjekt = resolve_prosjekt(pool, company_id, &draft.prosjekt).await?;
    let (fakturerbar, timesats_ore) =
        resolve_billing(pool, person_id, draft, &prosjekt, sats_override).await?;
    let prosjekt_id = prosjekt.map(|p| p.id);
    let id = Uuid::now_v7();
    sqlx::query(
        "insert into time_entry (id, company_id, person_id, dato, minutter, beskrivelse,
                                 prosjekt_id, fakturerbar, timesats_ore, created_by)
         values ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)",
    )
    .bind(id)
    .bind(company_id)
    .bind(person_id)
    .bind(draft.dato)
    .bind(draft.minutter)
    .bind(&draft.beskrivelse)
    .bind(prosjekt_id)
    .bind(fakturerbar)
    .bind(timesats_ore)
    .bind(created_by)
    .execute(pool)
    .await
    .context("kunne ikke registrere timene (låst måned?)")?;
    Ok(id)
}

/// Full replace of an entry's fields. `own_only` restricts to the
/// caller's entries (admins pass false).
#[allow(clippy::too_many_arguments)]
pub async fn update_time_entry(
    pool: &PgPool,
    company_id: Uuid,
    entry_id: Uuid,
    person_id: Uuid,
    own_only: bool,
    draft: &TimeEntryDraft,
    sats_override: bool,
) -> Result<()> {
    check_draft(draft)?;
    let prosjekt = resolve_prosjekt(pool, company_id, &draft.prosjekt).await?;
    // The entry may belong to someone else (TIMER_SKRIV_ALLE): the rate
    // follows the OWNER of the hours, not the corrector.
    let eier: Uuid =
        sqlx::query_scalar("select person_id from time_entry where id = $1 and company_id = $2")
            .bind(entry_id)
            .bind(company_id)
            .fetch_optional(pool)
            .await?
            .context("no such time entry")?;
    let (fakturerbar, timesats_ore) =
        resolve_billing(pool, eier, draft, &prosjekt, sats_override).await?;
    let prosjekt_id = prosjekt.map(|p| p.id);
    let updated = sqlx::query(
        "update time_entry set dato = $4, minutter = $5, beskrivelse = $6, prosjekt_id = $7,
                fakturerbar = $8, timesats_ore = $9, updated_at = now()
         where id = $1 and company_id = $2 and ($3::uuid is null or person_id = $3)",
    )
    .bind(entry_id)
    .bind(company_id)
    .bind(if own_only { Some(person_id) } else { None })
    .bind(draft.dato)
    .bind(draft.minutter)
    .bind(&draft.beskrivelse)
    .bind(prosjekt_id)
    .bind(fakturerbar)
    .bind(timesats_ore)
    .execute(pool)
    .await
    .context("kunne ikke endre timene (låst måned eller fakturert?)")?;
    ensure!(
        updated.rows_affected() == 1,
        "no such time entry (or not yours)"
    );
    Ok(())
}

pub async fn delete_time_entry(
    pool: &PgPool,
    company_id: Uuid,
    entry_id: Uuid,
    person_id: Uuid,
    own_only: bool,
) -> Result<()> {
    let deleted = sqlx::query(
        "delete from time_entry
         where id = $1 and company_id = $2 and ($3::uuid is null or person_id = $3)",
    )
    .bind(entry_id)
    .bind(company_id)
    .bind(if own_only { Some(person_id) } else { None })
    .execute(pool)
    .await
    .context("kunne ikke slette timene (låst måned eller fakturert?)")?;
    ensure!(
        deleted.rows_affected() == 1,
        "no such time entry (or not yours)"
    );
    Ok(())
}

#[derive(Debug)]
pub struct TimeEntryRow {
    pub id: Uuid,
    pub person_name: String,
    pub own: bool,
    pub dato: NaiveDate,
    pub minutter: i32,
    pub beskrivelse: String,
    pub prosjekt: Option<String>,
    pub fakturerbar: bool,
    pub timesats_ore: Option<i64>,
    pub invoice_no: Option<i64>,
}

/// `own_only` restricts the answer to the viewer's rows — the caller
/// decides from `TIMER_LES_ALLE`, the query only obeys.
pub async fn list_time_entries(
    pool: &PgPool,
    company_id: Uuid,
    viewer: Uuid,
    own_only: bool,
    from: NaiveDate,
    to: NaiveDate,
) -> Result<Vec<TimeEntryRow>> {
    let rows = sqlx::query(
        "select t.id, coalesce(p.name, p.oidc_sub) as person_name,
                (t.person_id = $2) as own, t.dato, t.minutter, t.beskrivelse,
                d.code as prosjekt, t.fakturerbar, t.timesats_ore, i.invoice_no
         from time_entry t
         join person p on p.id = t.person_id
         left join dimension d on d.id = t.prosjekt_id
         left join invoice i on i.id = t.invoice_id
         where t.company_id = $1 and t.dato between $3 and $4
           and (not $5 or t.person_id = $2)
         order by t.dato, t.created_at",
    )
    .bind(company_id)
    .bind(viewer)
    .bind(from)
    .bind(to)
    .bind(own_only)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .iter()
        .map(|r| TimeEntryRow {
            id: r.get("id"),
            person_name: r.get("person_name"),
            own: r.get("own"),
            dato: r.get("dato"),
            minutter: r.get("minutter"),
            beskrivelse: r.get("beskrivelse"),
            prosjekt: r.get("prosjekt"),
            fakturerbar: r.get("fakturerbar"),
            timesats_ore: r.get("timesats_ore"),
            invoice_no: r.get("invoice_no"),
        })
        .collect())
}

#[derive(Debug)]
pub struct ProsjektSum {
    pub prosjekt: Option<String>,
    pub minutter: i64,
    pub fakturerbare_minutter: i64,
    pub ufakturert_ore: i64,
    /// Billed hours and their value at the recorded sats (#71) — the
    /// "fakturert vs ufakturert" split prosjektlønnsomheten shows.
    pub fakturerte_minutter: i64,
    pub fakturert_ore: i64,
}

/// Totals per prosjekt over a period, plus the billable value split
/// into billed and unbilled.
pub async fn timesheet_summary(
    pool: &PgPool,
    company_id: Uuid,
    from: NaiveDate,
    to: NaiveDate,
) -> Result<Vec<ProsjektSum>> {
    let rows = sqlx::query(
        "select d.code as prosjekt,
                sum(t.minutter)::bigint as minutter,
                sum(t.minutter) filter (where t.fakturerbar)::bigint as fakturerbare,
                coalesce(sum((t.minutter::bigint * t.timesats_ore + 30) / 60)
                    filter (where t.fakturerbar and t.invoice_id is null), 0)::bigint
                    as ufakturert,
                sum(t.minutter) filter
                    (where t.fakturerbar and t.invoice_id is not null)::bigint as fakturerte,
                coalesce(sum((t.minutter::bigint * t.timesats_ore + 30) / 60)
                    filter (where t.fakturerbar and t.invoice_id is not null), 0)::bigint
                    as fakturert
         from time_entry t
         left join dimension d on d.id = t.prosjekt_id
         where t.company_id = $1 and t.dato between $2 and $3
         group by d.code
         order by d.code nulls last",
    )
    .bind(company_id)
    .bind(from)
    .bind(to)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .iter()
        .map(|r| ProsjektSum {
            prosjekt: r.get("prosjekt"),
            minutter: r.get("minutter"),
            fakturerbare_minutter: r.get::<Option<i64>, _>("fakturerbare").unwrap_or(0),
            ufakturert_ore: r.get("ufakturert"),
            fakturerte_minutter: r.get::<Option<i64>, _>("fakturerte").unwrap_or(0),
            fakturert_ore: r.get("fakturert"),
        })
        .collect())
}

pub async fn timesheet_lock(pool: &PgPool, company_id: Uuid) -> Result<Option<NaiveDate>> {
    Ok(sqlx::query_scalar("select current_timesheet_lock($1)")
        .bind(company_id)
        .fetch_one(pool)
        .await?)
}

/// Insert-only lock history, exactly like period_lock: the newest row
/// wins, so reopening is an audited insert with an earlier date.
pub async fn set_timesheet_lock(
    pool: &PgPool,
    company_id: Uuid,
    locked_through: NaiveDate,
    locked_by: &str,
    note: Option<&str>,
) -> Result<()> {
    sqlx::query(
        "insert into timesheet_lock (id, company_id, locked_through, locked_by, note)
         values ($1, $2, $3, $4, $5)",
    )
    .bind(Uuid::now_v7())
    .bind(company_id)
    .bind(locked_through)
    .bind(locked_by)
    .bind(note)
    .execute(pool)
    .await?;
    Ok(())
}

/// Fakturagrunnlaget: unbilled billable hours grouped per (prosjekt,
/// timesats) — one invoice line per group, quantity in milli-hours.
#[derive(Debug)]
pub struct UnbilledGroup {
    pub prosjekt: Option<String>,
    /// The customer the prosjekt is linked to (#80) — the SUGGESTED
    /// invoice recipient. A suggestion only: billing still takes an
    /// explicit party_no from the caller.
    pub kunde: Option<String>,
    pub kunde_navn: Option<String>,
    pub timesats_ore: i64,
    pub minutter: i64,
    pub entry_ids: Vec<Uuid>,
    /// Who the hours belong to — the selection unit when billing part of
    /// the grunnlag (a person's hours in or out, never half an entry).
    pub personer: Vec<UnbilledPerson>,
}

#[derive(Debug)]
pub struct UnbilledPerson {
    pub person_id: Uuid,
    pub navn: String,
    pub minutter: i64,
    pub entry_ids: Vec<Uuid>,
}

struct UnbilledRow {
    id: Uuid,
    person_id: Uuid,
    person_navn: String,
    prosjekt: Option<String>,
    kunde: Option<String>,
    kunde_navn: Option<String>,
    timesats_ore: i64,
    minutter: i64,
    /// The day the work was done — the leveringstidspunkt of an hours
    /// invoice is the last of these, not the day someone billed it.
    dato: NaiveDate,
}

async fn unbilled_rows(
    pool: &PgPool,
    company_id: Uuid,
    prosjekt: Option<&str>,
    through: Option<NaiveDate>,
) -> Result<Vec<UnbilledRow>> {
    let rows = sqlx::query(
        "select d.code as prosjekt, p.party_no as kunde, p.name as kunde_navn,
                t.timesats_ore, t.id, t.minutter, t.person_id, t.dato,
                coalesce(pe.name, pe.oidc_sub) as person_navn
         from time_entry t
         join person pe on pe.id = t.person_id
         left join dimension d on d.id = t.prosjekt_id
         left join party p on p.id = d.party_id
         where t.company_id = $1 and t.fakturerbar and t.invoice_id is null
           and ($2::text is null or d.code = $2)
           and ($3::date is null or t.dato <= $3)
         order by d.code nulls last, t.timesats_ore, t.dato",
    )
    .bind(company_id)
    .bind(prosjekt)
    .bind(through)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .iter()
        .map(|row| UnbilledRow {
            id: row.get("id"),
            person_id: row.get("person_id"),
            person_navn: row.get("person_navn"),
            prosjekt: row.get("prosjekt"),
            kunde: row.get("kunde"),
            kunde_navn: row.get("kunde_navn"),
            timesats_ore: row.get("timesats_ore"),
            minutter: i64::from(row.get::<i32, _>("minutter")),
            dato: row.get("dato"),
        })
        .collect())
}

/// Leveringstidspunkt for an hours invoice: the last day any of the
/// billed hours was worked. That is when the ytelse was fully
/// delivered — the invoice date only says when someone got around to
/// billing it, and may be weeks later.
fn siste_arbeidsdag(rows: &[UnbilledRow]) -> Option<NaiveDate> {
    rows.iter().map(|r| r.dato).max()
}

fn grupper(rows: Vec<UnbilledRow>) -> Vec<UnbilledGroup> {
    let mut groups: Vec<UnbilledGroup> = Vec::new();
    for row in rows {
        let group = match groups
            .iter_mut()
            .find(|g| g.prosjekt == row.prosjekt && g.timesats_ore == row.timesats_ore)
        {
            Some(group) => group,
            None => {
                groups.push(UnbilledGroup {
                    prosjekt: row.prosjekt.clone(),
                    kunde: row.kunde.clone(),
                    kunde_navn: row.kunde_navn.clone(),
                    timesats_ore: row.timesats_ore,
                    minutter: 0,
                    entry_ids: Vec::new(),
                    personer: Vec::new(),
                });
                groups.last_mut().unwrap()
            }
        };
        group.minutter += row.minutter;
        group.entry_ids.push(row.id);
        match group
            .personer
            .iter_mut()
            .find(|p| p.person_id == row.person_id)
        {
            Some(p) => {
                p.minutter += row.minutter;
                p.entry_ids.push(row.id);
            }
            None => group.personer.push(UnbilledPerson {
                person_id: row.person_id,
                navn: row.person_navn,
                minutter: row.minutter,
                entry_ids: vec![row.id],
            }),
        }
    }
    groups
}

pub async fn unbilled_groups(
    pool: &PgPool,
    company_id: Uuid,
    prosjekt: Option<&str>,
    through: Option<NaiveDate>,
) -> Result<Vec<UnbilledGroup>> {
    Ok(grupper(
        unbilled_rows(pool, company_id, prosjekt, through).await?,
    ))
}

/// Quantity in milli-hours, rounded half up: 90 min → 1500.
fn milli_hours(minutter: i64) -> i64 {
    (minutter * 1000 + 30) / 60
}

fn hour_line(group: &UnbilledGroup, vat_code: Option<&str>) -> InvoiceLineDraft {
    InvoiceLineDraft {
        description: match &group.prosjekt {
            Some(p) => format!("Timer — prosjekt {p}"),
            None => "Timer".into(),
        },
        account_number: "3000".into(),
        quantity_milli: milli_hours(group.minutter),
        unit_price_ore: group.timesats_ore,
        vat_code: Some(vat_code.unwrap_or("3").to_string()),
        avdeling: None,
        prosjekt: group.prosjekt.clone(),
        product_id: None,
    }
}

/// Narrows the grunnlag to an explicit selection: every id must still be
/// billable and unbilled — anything else fails the whole call rather
/// than silently billing less than what was chosen.
fn behold_utvalg(rows: &mut Vec<UnbilledRow>, valgte: &[Uuid]) -> Result<()> {
    ensure!(!valgte.is_empty(), "ingen timer valgt");
    let finnes: std::collections::HashSet<Uuid> = rows.iter().map(|r| r.id).collect();
    for id in valgte {
        ensure!(
            finnes.contains(id),
            "valgt time {id} er allerede fakturert, ikke fakturerbar eller finnes ikke"
        );
    }
    let valgt_sett: std::collections::HashSet<Uuid> = valgte.iter().copied().collect();
    rows.retain(|r| valgt_sett.contains(&r.id));
    Ok(())
}

/// Marks the entries fakturert INSIDE the invoice transaction — the
/// selection is locked by the invoice itself, there is never a
/// chosen-but-editable window.
async fn merk_fakturert(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    company_id: Uuid,
    entry_ids: &[Uuid],
    invoice_id: Uuid,
) -> Result<()> {
    let marked = sqlx::query(
        "update time_entry set invoice_id = $3, updated_at = now()
         where company_id = $1 and id = any($2) and invoice_id is null",
    )
    .bind(company_id)
    .bind(entry_ids)
    .bind(invoice_id)
    .execute(&mut **tx)
    .await?;
    ensure!(
        marked.rows_affected() == entry_ids.len() as u64,
        "timene endret seg under fakturering — prøv igjen"
    );
    Ok(())
}

/// The Faktura path (docs/faktura.md): ONE invoice carrying the caller's
/// ordinary lines AND the selected unbilled hours — hour lines appended
/// per (prosjekt, sats) group, entries marked fakturert in the same
/// transaction as the invoice.
pub async fn create_invoice_with_hours(
    pool: &PgPool,
    company_id: Uuid,
    draft: &InvoiceDraft,
    entry_ids: &[Uuid],
    vat_code: Option<&str>,
    created_by: &str,
) -> Result<IssuedInvoice> {
    ensure!(
        draft.valuta.is_none(),
        "timelinjer på valutafaktura støttes ikke — satsene er i NOK"
    );
    let mut rows = unbilled_rows(pool, company_id, None, None).await?;
    behold_utvalg(&mut rows, entry_ids)?;
    // A combined invoice carries goods lines AND hours. The caller's
    // leveringsdato governs — it decided what this document delivers —
    // but hours worked after that date would make it a lie, so the
    // later of the two wins.
    let siste_time = siste_arbeidsdag(&rows);
    let groups = grupper(rows);

    let mut lines = draft.lines.clone();
    lines.extend(groups.iter().map(|g| hour_line(g, vat_code)));
    let full = InvoiceDraft {
        kontant_betalingsmiddel: None,
        party_no: draft.party_no.clone(),
        invoice_date: draft.invoice_date,
        due_date: draft.due_date,
        delivery_date: siste_time
            .filter(|d| *d > draft.delivery_date)
            .unwrap_or(draft.delivery_date),
        delivery_place: draft.delivery_place.clone(),
        journal_code: draft.journal_code.clone(),
        receivable_account: draft.receivable_account.clone(),
        vat_account: draft.vat_account.clone(),
        valuta: None,
        valuta_kurs_micro: None,
        lines,
    };

    let mut tx = pool.begin().await?;
    let issued = create_invoice_in(pool, &mut tx, company_id, &full, created_by, None).await?;
    let all_ids: Vec<Uuid> = groups.iter().flat_map(|g| g.entry_ids.clone()).collect();
    merk_fakturert(&mut tx, company_id, &all_ids, issued.invoice_id).await?;
    tx.commit().await?;
    Ok(issued)
}

/// Bills the unbilled hours: one invoice through the ordinary atomic
/// path (line per gruppe, prosjekt dimension carried onto the revenue
/// line) and every entry marked fakturert IN THE SAME TRANSACTION —
/// one-way, enforced by the guard trigger thereafter.
///
/// `entry_ids` narrows the grunnlag to a selection (chosen people, or a
/// hand-picked set): every id must still be billable and unbilled —
/// anything else fails the whole call rather than silently billing less
/// than what was chosen. Selection and lock are one step: the entries
/// are marked fakturert in the invoice transaction itself, so there is
/// never a chosen-but-editable window.
#[allow(clippy::too_many_arguments)]
pub async fn bill_hours(
    pool: &PgPool,
    company_id: Uuid,
    party_no: &str,
    prosjekt: Option<&str>,
    through: Option<NaiveDate>,
    entry_ids: Option<&[Uuid]>,
    vat_code: Option<&str>,
    invoice_date: NaiveDate,
    due_date: NaiveDate,
    created_by: &str,
) -> Result<IssuedInvoice> {
    let mut rows = unbilled_rows(pool, company_id, prosjekt, through).await?;
    if let Some(valgte) = entry_ids {
        behold_utvalg(&mut rows, valgte)?;
    }
    // Leveringstidspunktet er den siste arbeidsdagen som faktureres,
    // ikke fakturadatoen: timene ble levert da de ble utført.
    let levering = siste_arbeidsdag(&rows).unwrap_or(invoice_date);
    let groups = grupper(rows);
    ensure!(!groups.is_empty(), "ingen ufakturerte fakturerbare timer");

    let lines = groups.iter().map(|g| hour_line(g, vat_code)).collect();
    let draft = InvoiceDraft {
        kontant_betalingsmiddel: None,
        party_no: party_no.to_string(),
        invoice_date,
        due_date,
        delivery_date: levering,
        delivery_place: None,
        journal_code: "GL".into(),
        receivable_account: "1500".into(),
        vat_account: "2700".into(),
        valuta: None,
        valuta_kurs_micro: None,
        lines,
    };

    let mut tx = pool.begin().await?;
    let issued = create_invoice_in(pool, &mut tx, company_id, &draft, created_by, None).await?;
    let all_ids: Vec<Uuid> = groups.iter().flat_map(|g| g.entry_ids.clone()).collect();
    merk_fakturert(&mut tx, company_id, &all_ids, issued.invoice_id).await?;
    tx.commit().await?;
    Ok(issued)
}
