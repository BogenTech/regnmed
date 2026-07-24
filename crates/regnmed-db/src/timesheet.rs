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
    pub fakturerbar: bool,
    pub timesats_ore: Option<i64>,
}

async fn resolve_prosjekt(
    pool: &PgPool,
    company_id: Uuid,
    code: &Option<String>,
) -> Result<Option<Uuid>> {
    let Some(code) = code else { return Ok(None) };
    let row = sqlx::query(
        "select id, active from dimension
         where company_id = $1 and kind = 'prosjekt' and code = $2",
    )
    .bind(company_id)
    .bind(code)
    .fetch_optional(pool)
    .await?
    .with_context(|| format!("no prosjekt {code}"))?;
    ensure!(row.get::<bool, _>("active"), "prosjekt {code} er avsluttet");
    Ok(Some(row.get("id")))
}

fn check_draft(draft: &TimeEntryDraft) -> Result<()> {
    ensure!(
        (1..=1440).contains(&draft.minutter),
        "minutter must be 1..=1440"
    );
    ensure!(
        !draft.fakturerbar || draft.timesats_ore.is_some(),
        "fakturerbare timer trenger timesats"
    );
    Ok(())
}

pub async fn create_time_entry(
    pool: &PgPool,
    company_id: Uuid,
    person_id: Uuid,
    draft: &TimeEntryDraft,
    created_by: &str,
) -> Result<Uuid> {
    check_draft(draft)?;
    let prosjekt_id = resolve_prosjekt(pool, company_id, &draft.prosjekt).await?;
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
    .bind(draft.fakturerbar)
    .bind(draft.timesats_ore)
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
) -> Result<()> {
    check_draft(draft)?;
    let prosjekt_id = resolve_prosjekt(pool, company_id, &draft.prosjekt).await?;
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
    .bind(draft.fakturerbar)
    .bind(draft.timesats_ore)
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

pub async fn list_time_entries(
    pool: &PgPool,
    company_id: Uuid,
    viewer: Uuid,
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
         order by t.dato, t.created_at",
    )
    .bind(company_id)
    .bind(viewer)
    .bind(from)
    .bind(to)
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
}

/// Totals per prosjekt over a period, plus the unbilled billable value.
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
                    as ufakturert
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
    pub timesats_ore: i64,
    pub minutter: i64,
    pub entry_ids: Vec<Uuid>,
}

pub async fn unbilled_groups(
    pool: &PgPool,
    company_id: Uuid,
    prosjekt: Option<&str>,
    through: Option<NaiveDate>,
) -> Result<Vec<UnbilledGroup>> {
    let rows = sqlx::query(
        "select d.code as prosjekt, t.timesats_ore, t.id, t.minutter
         from time_entry t
         left join dimension d on d.id = t.prosjekt_id
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
    let mut groups: Vec<UnbilledGroup> = Vec::new();
    for row in &rows {
        let prosjekt: Option<String> = row.get("prosjekt");
        let sats: i64 = row.get("timesats_ore");
        let minutter = i64::from(row.get::<i32, _>("minutter"));
        match groups
            .iter_mut()
            .find(|g| g.prosjekt == prosjekt && g.timesats_ore == sats)
        {
            Some(group) => {
                group.minutter += minutter;
                group.entry_ids.push(row.get("id"));
            }
            None => groups.push(UnbilledGroup {
                prosjekt,
                timesats_ore: sats,
                minutter,
                entry_ids: vec![row.get("id")],
            }),
        }
    }
    Ok(groups)
}

/// Quantity in milli-hours, rounded half up: 90 min → 1500.
fn milli_hours(minutter: i64) -> i64 {
    (minutter * 1000 + 30) / 60
}

/// Bills the unbilled hours: one invoice through the ordinary atomic
/// path (line per gruppe, prosjekt dimension carried onto the revenue
/// line) and every entry marked fakturert IN THE SAME TRANSACTION —
/// one-way, enforced by the guard trigger thereafter.
pub async fn bill_hours(
    pool: &PgPool,
    company_id: Uuid,
    party_no: &str,
    prosjekt: Option<&str>,
    through: Option<NaiveDate>,
    vat_code: Option<&str>,
    invoice_date: NaiveDate,
    due_date: NaiveDate,
    created_by: &str,
) -> Result<IssuedInvoice> {
    let groups = unbilled_groups(pool, company_id, prosjekt, through).await?;
    ensure!(!groups.is_empty(), "ingen ufakturerte fakturerbare timer");

    let lines = groups
        .iter()
        .map(|g| InvoiceLineDraft {
            description: match &g.prosjekt {
                Some(p) => format!("Timer — prosjekt {p}"),
                None => "Timer".into(),
            },
            account_number: "3000".into(),
            quantity_milli: milli_hours(g.minutter),
            unit_price_ore: g.timesats_ore,
            vat_code: Some(vat_code.unwrap_or("3").to_string()),
            avdeling: None,
            prosjekt: g.prosjekt.clone(),
        })
        .collect();
    let draft = InvoiceDraft {
        party_no: party_no.to_string(),
        invoice_date,
        due_date,
        journal_code: "GL".into(),
        receivable_account: "1500".into(),
        vat_account: "2700".into(),
        lines,
    };

    let mut tx = pool.begin().await?;
    let issued = create_invoice_in(pool, &mut tx, company_id, &draft, created_by, None).await?;
    let all_ids: Vec<Uuid> = groups.iter().flat_map(|g| g.entry_ids.clone()).collect();
    let marked = sqlx::query(
        "update time_entry set invoice_id = $3, updated_at = now()
         where company_id = $1 and id = any($2) and invoice_id is null",
    )
    .bind(company_id)
    .bind(&all_ids)
    .bind(issued.invoice_id)
    .execute(&mut *tx)
    .await?;
    ensure!(
        marked.rows_affected() == all_ids.len() as u64,
        "timene endret seg under fakturering — prøv igjen"
    );
    tx.commit().await?;
    Ok(issued)
}
