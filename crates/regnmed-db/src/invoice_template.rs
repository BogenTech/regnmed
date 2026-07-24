//! Repeterende faktura (docs/faktura.md, #30): editable templates, and
//! generation that issues ordinary invoices through the existing
//! gap-free path — the recurring machinery adds no posting semantics.
//!
//! Generation is atomic per template: the template row is locked FOR
//! UPDATE, the invoice (counter + posting + PDF) is created in the same
//! transaction as the run-log row and the neste_dato advance, and a
//! partial unique index makes a period impossible to generate twice.
//! Failures roll everything back, log a failure row, and leave
//! neste_dato untouched so the next run retries.

use anyhow::{Context, Result, ensure};
use chrono::NaiveDate;
use regnmed_core::invoice::{interpoler_periodetekst, neste_intervall_dato};
use sqlx::{PgPool, Row};
use uuid::Uuid;

use crate::invoice::{InvoiceDraft, InvoiceLineDraft, create_invoice_in};

#[derive(Debug, Clone)]
pub struct TemplateLineDraft {
    pub description: String,
    pub account_number: String,
    pub quantity_milli: i64,
    pub unit_price_ore: i64,
    pub vat_code: Option<String>,
    pub avdeling: Option<String>,
    pub prosjekt: Option<String>,
}

#[derive(Debug)]
pub struct TemplateDraft {
    pub party_no: String,
    pub intervall: String,
    pub neste_dato: NaiveDate,
    pub slutt_dato: Option<NaiveDate>,
    pub forfall_dager: i32,
    pub merk_utsendelse: bool,
    pub lines: Vec<TemplateLineDraft>,
}

fn check_intervall(intervall: &str) -> Result<()> {
    ensure!(
        matches!(intervall, "manedlig" | "kvartalsvis" | "arlig"),
        "intervall must be manedlig, kvartalsvis or arlig"
    );
    Ok(())
}

pub async fn create_template(
    pool: &PgPool,
    company_id: Uuid,
    draft: &TemplateDraft,
    created_by: &str,
) -> Result<Uuid> {
    check_intervall(&draft.intervall)?;
    ensure!(
        !draft.lines.is_empty(),
        "a template needs at least one line"
    );
    let party_id: Uuid = sqlx::query_scalar(
        "select id from party where company_id = $1 and party_no = $2 and kind = 'kunde'",
    )
    .bind(company_id)
    .bind(&draft.party_no)
    .fetch_optional(pool)
    .await?
    .with_context(|| format!("no kunde {}", draft.party_no))?;

    let mut tx = pool.begin().await?;
    let template_id = Uuid::now_v7();
    sqlx::query(
        "insert into invoice_template (id, company_id, party_id, intervall, neste_dato,
                                       slutt_dato, forfall_dager, merk_utsendelse, created_by)
         values ($1, $2, $3, $4, $5, $6, $7, $8, $9)",
    )
    .bind(template_id)
    .bind(company_id)
    .bind(party_id)
    .bind(&draft.intervall)
    .bind(draft.neste_dato)
    .bind(draft.slutt_dato)
    .bind(draft.forfall_dager)
    .bind(draft.merk_utsendelse)
    .bind(created_by)
    .execute(&mut *tx)
    .await?;
    insert_lines(&mut tx, template_id, &draft.lines).await?;
    tx.commit().await?;
    Ok(template_id)
}

async fn insert_lines(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    template_id: Uuid,
    lines: &[TemplateLineDraft],
) -> Result<()> {
    for (i, line) in lines.iter().enumerate() {
        sqlx::query(
            "insert into invoice_template_line
                 (id, template_id, line_no, description, account_number,
                  quantity_milli, unit_price_ore, vat_code, avdeling, prosjekt)
             values ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)",
        )
        .bind(Uuid::now_v7())
        .bind(template_id)
        .bind((i + 1) as i32)
        .bind(&line.description)
        .bind(&line.account_number)
        .bind(line.quantity_milli)
        .bind(line.unit_price_ore)
        .bind(&line.vat_code)
        .bind(&line.avdeling)
        .bind(&line.prosjekt)
        .execute(&mut **tx)
        .await?;
    }
    Ok(())
}

/// "Gjenta denne": a template copied from an existing invoice — same
/// customer, same lines (descriptions as-is; add {måned}/{år} by
/// editing afterwards).
pub async fn create_template_from_invoice(
    pool: &PgPool,
    company_id: Uuid,
    invoice_id: Uuid,
    intervall: &str,
    neste_dato: NaiveDate,
    forfall_dager: i32,
    merk_utsendelse: bool,
    created_by: &str,
) -> Result<Uuid> {
    let party_no: String = sqlx::query_scalar(
        "select p.party_no from invoice i join party p on p.id = i.party_id
         where i.id = $1 and i.company_id = $2 and i.credits_invoice_id is null",
    )
    .bind(invoice_id)
    .bind(company_id)
    .fetch_optional(pool)
    .await?
    .context("no such invoice (or it is a kreditnota)")?;
    let lines = sqlx::query(
        "select description, account_number, quantity_milli, unit_price_ore,
                vat_code, avdeling, prosjekt
         from invoice_line where invoice_id = $1 order by line_no",
    )
    .bind(invoice_id)
    .fetch_all(pool)
    .await?
    .iter()
    .map(|r| TemplateLineDraft {
        description: r.get("description"),
        account_number: r.get("account_number"),
        quantity_milli: r.get("quantity_milli"),
        unit_price_ore: r.get("unit_price_ore"),
        vat_code: r.get("vat_code"),
        avdeling: r.get("avdeling"),
        prosjekt: r.get("prosjekt"),
    })
    .collect();
    create_template(
        pool,
        company_id,
        &TemplateDraft {
            party_no,
            intervall: intervall.to_string(),
            neste_dato,
            slutt_dato: None,
            forfall_dager,
            merk_utsendelse,
            lines,
        },
        created_by,
    )
    .await
}

/// Editable until generation — and after: the template is a plan.
/// Passing `lines` replaces the whole line set.
#[allow(clippy::too_many_arguments)]
pub async fn update_template(
    pool: &PgPool,
    company_id: Uuid,
    template_id: Uuid,
    intervall: Option<&str>,
    neste_dato: Option<NaiveDate>,
    slutt_dato: Option<Option<NaiveDate>>,
    forfall_dager: Option<i32>,
    merk_utsendelse: Option<bool>,
    active: Option<bool>,
    lines: Option<&[TemplateLineDraft]>,
) -> Result<()> {
    if let Some(intervall) = intervall {
        check_intervall(intervall)?;
    }
    let mut tx = pool.begin().await?;
    let updated = sqlx::query(
        "update invoice_template set
             intervall = coalesce($3, intervall),
             neste_dato = coalesce($4, neste_dato),
             slutt_dato = case when $5 then $6 else slutt_dato end,
             forfall_dager = coalesce($7, forfall_dager),
             merk_utsendelse = coalesce($8, merk_utsendelse),
             active = coalesce($9, active),
             updated_at = now()
         where id = $1 and company_id = $2",
    )
    .bind(template_id)
    .bind(company_id)
    .bind(intervall)
    .bind(neste_dato)
    .bind(slutt_dato.is_some())
    .bind(slutt_dato.flatten())
    .bind(forfall_dager)
    .bind(merk_utsendelse)
    .bind(active)
    .execute(&mut *tx)
    .await?;
    ensure!(updated.rows_affected() == 1, "no such template");
    if let Some(lines) = lines {
        ensure!(!lines.is_empty(), "a template needs at least one line");
        sqlx::query("delete from invoice_template_line where template_id = $1")
            .bind(template_id)
            .execute(&mut *tx)
            .await?;
        insert_lines(&mut tx, template_id, lines).await?;
    }
    tx.commit().await?;
    Ok(())
}

#[derive(Debug)]
pub struct TemplateRow {
    pub id: Uuid,
    pub party_no: String,
    pub party_name: String,
    pub intervall: String,
    pub neste_dato: NaiveDate,
    pub slutt_dato: Option<NaiveDate>,
    pub forfall_dager: i32,
    pub merk_utsendelse: bool,
    pub active: bool,
    pub sum_netto_ore: i64,
    pub runs: i64,
}

pub async fn list_templates(pool: &PgPool, company_id: Uuid) -> Result<Vec<TemplateRow>> {
    let rows = sqlx::query(
        "select t.id, p.party_no, p.name as party_name, t.intervall, t.neste_dato,
                t.slutt_dato, t.forfall_dager, t.merk_utsendelse, t.active,
                coalesce((select sum((l.quantity_milli * l.unit_price_ore) / 1000)
                          from invoice_template_line l where l.template_id = t.id), 0)::bigint
                    as sum_netto_ore,
                (select count(*) from invoice_template_run r
                 where r.template_id = t.id and r.invoice_id is not null) as runs
         from invoice_template t
         join party p on p.id = t.party_id
         where t.company_id = $1
         order by t.created_at",
    )
    .bind(company_id)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .iter()
        .map(|r| TemplateRow {
            id: r.get("id"),
            party_no: r.get("party_no"),
            party_name: r.get("party_name"),
            intervall: r.get("intervall"),
            neste_dato: r.get("neste_dato"),
            slutt_dato: r.get("slutt_dato"),
            forfall_dager: r.get("forfall_dager"),
            merk_utsendelse: r.get("merk_utsendelse"),
            active: r.get("active"),
            sum_netto_ore: r.get("sum_netto_ore"),
            runs: r.get("runs"),
        })
        .collect())
}

#[derive(Debug)]
pub struct GenerationOutcome {
    pub template_id: Uuid,
    pub company_id: Uuid,
    pub generated_for: NaiveDate,
    /// None = failed; the error text is in detail.
    pub invoice_no: Option<i64>,
    pub detail: Option<String>,
}

/// Generates every due period for every active template, oldest first —
/// a template that missed runs (cron down) catches up one period per
/// call to this function per period... rather: loops until nothing is
/// due. Failures never abort the batch.
pub async fn generate_due(pool: &PgPool, today: NaiveDate) -> Result<Vec<GenerationOutcome>> {
    let mut outcomes = Vec::new();
    // Loop so a template that is several periods behind catches up in
    // one run; the per-template lock + unique index keep this safe.
    loop {
        let due: Vec<(Uuid, Uuid)> = sqlx::query(
            "select id, company_id from invoice_template
             where active and neste_dato <= $1
               and (slutt_dato is null or neste_dato <= slutt_dato)
             order by neste_dato",
        )
        .bind(today)
        .fetch_all(pool)
        .await?
        .iter()
        .map(|r| (r.get("id"), r.get("company_id")))
        .collect();
        if due.is_empty() {
            break;
        }
        let mut progressed = false;
        for (template_id, company_id) in due {
            let outcome = generate_one(pool, company_id, template_id, today).await;
            match outcome {
                Ok(Some(outcome)) => {
                    progressed = outcome.invoice_no.is_some() || progressed;
                    outcomes.push(outcome);
                }
                Ok(None) => {} // no longer due (raced or re-checked)
                Err(err) => outcomes.push(GenerationOutcome {
                    template_id,
                    company_id,
                    generated_for: today,
                    invoice_no: None,
                    detail: Some(format!("{err:#}")),
                }),
            }
        }
        if !progressed {
            break; // only failures left — stop, they are logged
        }
    }
    Ok(outcomes)
}

/// One template, one period: lock → re-check due → issue the invoice →
/// log the run → advance neste_dato, all in one transaction. Returns
/// Ok(None) when the template turned out not to be due (lost race).
pub async fn generate_one(
    pool: &PgPool,
    company_id: Uuid,
    template_id: Uuid,
    today: NaiveDate,
) -> Result<Option<GenerationOutcome>> {
    let mut tx = pool.begin().await?;
    let Some(template) = sqlx::query(
        "select t.intervall, t.neste_dato, t.slutt_dato, t.forfall_dager, t.merk_utsendelse,
                t.active, p.party_no
         from invoice_template t
         join party p on p.id = t.party_id
         where t.id = $1 and t.company_id = $2
         for update of t",
    )
    .bind(template_id)
    .bind(company_id)
    .fetch_optional(&mut *tx)
    .await?
    else {
        return Ok(None);
    };
    let neste_dato: NaiveDate = template.get("neste_dato");
    let slutt_dato: Option<NaiveDate> = template.get("slutt_dato");
    if !template.get::<bool, _>("active")
        || neste_dato > today
        || slutt_dato.is_some_and(|slutt| neste_dato > slutt)
    {
        return Ok(None);
    }
    let intervall: String = template.get("intervall");
    let merk_utsendelse: bool = template.get("merk_utsendelse");

    let line_rows = sqlx::query(
        "select description, account_number, quantity_milli, unit_price_ore,
                vat_code, avdeling, prosjekt
         from invoice_template_line where template_id = $1 order by line_no",
    )
    .bind(template_id)
    .fetch_all(&mut *tx)
    .await?;
    ensure!(!line_rows.is_empty(), "template has no lines");

    let draft = InvoiceDraft {
        party_no: template.get("party_no"),
        invoice_date: neste_dato,
        due_date: neste_dato + chrono::Days::new(template.get::<i32, _>("forfall_dager") as u64),
        journal_code: "GL".into(),
        receivable_account: "1500".into(),
        vat_account: "2700".into(),
        lines: line_rows
            .iter()
            .map(|r| InvoiceLineDraft {
                description: interpoler_periodetekst(r.get("description"), neste_dato),
                account_number: r.get("account_number"),
                quantity_milli: r.get("quantity_milli"),
                unit_price_ore: r.get("unit_price_ore"),
                vat_code: r.get("vat_code"),
                avdeling: r.get("avdeling"),
                prosjekt: r.get("prosjekt"),
            })
            .collect(),
    };

    let result = async {
        let issued = create_invoice_in(
            pool,
            &mut tx,
            company_id,
            &draft,
            "system (repeterende faktura)",
            None,
        )
        .await?;
        sqlx::query(
            "insert into invoice_template_run
                 (id, template_id, invoice_id, generated_for, til_utsendelse)
             values ($1, $2, $3, $4, $5)",
        )
        .bind(Uuid::now_v7())
        .bind(template_id)
        .bind(issued.invoice_id)
        .bind(neste_dato)
        .bind(merk_utsendelse)
        .execute(&mut *tx)
        .await?;
        let advanced =
            neste_intervall_dato(neste_dato, &intervall).context("could not advance neste_dato")?;
        sqlx::query(
            "update invoice_template set neste_dato = $2, updated_at = now() where id = $1",
        )
        .bind(template_id)
        .bind(advanced)
        .execute(&mut *tx)
        .await?;
        anyhow::Ok(issued)
    }
    .await;

    match result {
        Ok(issued) => {
            tx.commit().await?;
            Ok(Some(GenerationOutcome {
                template_id,
                company_id,
                generated_for: neste_dato,
                invoice_no: Some(issued.invoice_no),
                detail: None,
            }))
        }
        Err(err) => {
            drop(tx); // roll back the whole attempt
            let detail = format!("{err:#}");
            // The failure row is the human-visible trace; neste_dato is
            // untouched so the next run retries.
            sqlx::query(
                "insert into invoice_template_run (id, template_id, generated_for, detail)
                 values ($1, $2, $3, $4)",
            )
            .bind(Uuid::now_v7())
            .bind(template_id)
            .bind(neste_dato)
            .bind(&detail)
            .execute(pool)
            .await?;
            Ok(Some(GenerationOutcome {
                template_id,
                company_id,
                generated_for: neste_dato,
                invoice_no: None,
                detail: Some(detail),
            }))
        }
    }
}

#[derive(Debug)]
pub struct RunRow {
    pub generated_for: NaiveDate,
    pub invoice_no: Option<i64>,
    pub til_utsendelse: bool,
    pub detail: Option<String>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

pub async fn list_runs(pool: &PgPool, company_id: Uuid, template_id: Uuid) -> Result<Vec<RunRow>> {
    let rows = sqlx::query(
        "select r.generated_for, i.invoice_no, r.til_utsendelse, r.detail, r.created_at
         from invoice_template_run r
         join invoice_template t on t.id = r.template_id
         left join invoice i on i.id = r.invoice_id
         where r.template_id = $1 and t.company_id = $2
         order by r.created_at desc
         limit 50",
    )
    .bind(template_id)
    .bind(company_id)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .iter()
        .map(|r| RunRow {
            generated_for: r.get("generated_for"),
            invoice_no: r.get("invoice_no"),
            til_utsendelse: r.get("til_utsendelse"),
            detail: r.get("detail"),
            created_at: r.get("created_at"),
        })
        .collect())
}
