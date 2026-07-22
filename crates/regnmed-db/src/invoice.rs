//! Invoice persistence: gap-free issuing atomic with the ledger posting,
//! listing with reskontro remainders, and kreditnotaer.

use anyhow::{Context, Result, bail, ensure};
use chrono::NaiveDate;
use regnmed_core::invoice::{InvoiceLineInput, build_voucher, compute, invoice_kid};
use regnmed_core::mva::rate_on;
use sqlx::{PgPool, Row};
use uuid::Uuid;

use crate::ledger::post_voucher_in;
use crate::mva::load_vat_rates;

#[derive(Debug, Clone)]
pub struct InvoiceLineDraft {
    pub description: String,
    pub account_number: String,
    /// Thousandths; defaults to 1000 (one unit) at the API layer.
    pub quantity_milli: i64,
    pub unit_price_ore: i64,
    pub vat_code: Option<String>,
}

#[derive(Debug)]
pub struct InvoiceDraft {
    pub party_no: String,
    pub invoice_date: NaiveDate,
    pub due_date: NaiveDate,
    pub journal_code: String,
    pub receivable_account: String,
    pub vat_account: String,
    pub lines: Vec<InvoiceLineDraft>,
}

#[derive(Debug)]
pub struct IssuedInvoice {
    pub invoice_id: Uuid,
    pub invoice_no: i64,
    pub kid: String,
    pub net_ore: i64,
    pub vat_ore: i64,
    pub gross_ore: i64,
    pub voucher_number: i64,
    pub fiscal_year: i32,
}

/// Resolves rates per line (dated by invoice date) and turns drafts into
/// the pure computation input. Zero-rate/uncoded lines get rate 0.
async fn resolve_lines(
    pool: &PgPool,
    invoice_date: NaiveDate,
    lines: &[InvoiceLineDraft],
) -> Result<Vec<InvoiceLineInput>> {
    let rates = load_vat_rates(pool).await?;
    let mut resolved = Vec::with_capacity(lines.len());
    for (i, line) in lines.iter().enumerate() {
        let rate_bp = match &line.vat_code {
            Some(code) => {
                let rate_class: String =
                    sqlx::query_scalar("select rate_class from vat_code where code = $1")
                        .bind(code)
                        .fetch_optional(pool)
                        .await?
                        .with_context(|| format!("line {}: unknown vat code {code}", i + 1))?;
                rate_on(&rates, &rate_class, invoice_date)
                    .with_context(|| format!("line {}: no rate for {invoice_date}", i + 1))?
            }
            None => 0,
        };
        ensure!(
            !line.description.is_empty(),
            "line {}: empty description",
            i + 1
        );
        resolved.push(InvoiceLineInput {
            description: line.description.clone(),
            account_number: line.account_number.clone(),
            quantity_milli: line.quantity_milli,
            unit_price_ore: line.unit_price_ore,
            vat_code: line.vat_code.clone(),
            rate_bp,
        });
    }
    Ok(resolved)
}

/// Issues an invoice: one transaction covering the gap-free invoice
/// number, the ledger posting (voucher counter, hash chain) and the
/// invoice rows — everything rolls back together.
pub async fn create_invoice(
    pool: &PgPool,
    company_id: Uuid,
    draft: &InvoiceDraft,
    created_by: &str,
    credits_invoice_id: Option<Uuid>,
) -> Result<IssuedInvoice> {
    ensure!(
        !draft.lines.is_empty(),
        "an invoice needs at least one line"
    );
    let party = sqlx::query("select id, kind from party where company_id = $1 and party_no = $2")
        .bind(company_id)
        .bind(&draft.party_no)
        .fetch_optional(pool)
        .await?
        .with_context(|| format!("no party {}", draft.party_no))?;
    let party_id: Uuid = party.get("id");
    ensure!(
        party.get::<String, _>("kind") == "kunde",
        "party {} is not a kunde",
        draft.party_no
    );

    let lines = resolve_lines(pool, draft.invoice_date, &draft.lines).await?;
    let computed = compute(&lines);
    if credits_invoice_id.is_none() && computed.gross_ore <= 0 {
        bail!("invoice total must be positive (use a kreditnota to credit)");
    }

    let mut tx = pool.begin().await?;
    let invoice_no: i64 = sqlx::query(
        "insert into invoice_counter (company_id, last_number) values ($1, 1)
         on conflict (company_id)
         do update set last_number = invoice_counter.last_number + 1
         returning last_number",
    )
    .bind(company_id)
    .fetch_one(&mut *tx)
    .await?
    .get("last_number");
    let kid = invoice_kid(invoice_no);

    let voucher = build_voucher(
        &draft.journal_code,
        draft.invoice_date,
        invoice_no,
        credits_invoice_id.is_some(),
        &draft.party_no,
        &draft.receivable_account,
        &draft.vat_account,
        &lines,
        &computed,
    )?;
    let posted = post_voucher_in(&mut tx, company_id, &voucher, created_by).await?;

    let receivable_entry_id: Uuid =
        sqlx::query_scalar("select id from entry where voucher_id = $1 and party_id = $2")
            .bind(posted.id)
            .bind(party_id)
            .fetch_one(&mut *tx)
            .await?;

    let invoice_id = Uuid::now_v7();
    sqlx::query(
        "insert into invoice (id, company_id, party_id, invoice_no, invoice_date, due_date,
                              kid, credits_invoice_id, voucher_id, receivable_entry_id, created_by)
         values ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)",
    )
    .bind(invoice_id)
    .bind(company_id)
    .bind(party_id)
    .bind(invoice_no)
    .bind(draft.invoice_date)
    .bind(draft.due_date)
    .bind(&kid)
    .bind(credits_invoice_id)
    .bind(posted.id)
    .bind(receivable_entry_id)
    .bind(created_by)
    .execute(&mut *tx)
    .await?;

    for (i, (line, amounts)) in lines.iter().zip(&computed.lines).enumerate() {
        sqlx::query(
            "insert into invoice_line (id, invoice_id, line_no, description, account_number,
                                       quantity_milli, unit_price_ore, net_ore, vat_code, vat_ore)
             values ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)",
        )
        .bind(Uuid::now_v7())
        .bind(invoice_id)
        .bind((i + 1) as i32)
        .bind(&line.description)
        .bind(&line.account_number)
        .bind(line.quantity_milli)
        .bind(line.unit_price_ore)
        .bind(amounts.net_ore)
        .bind(&line.vat_code)
        .bind(amounts.vat_ore)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;

    Ok(IssuedInvoice {
        invoice_id,
        invoice_no,
        kid,
        net_ore: computed.net_ore,
        vat_ore: computed.vat_ore,
        gross_ore: computed.gross_ore,
        voucher_number: posted.voucher_number,
        fiscal_year: posted.fiscal_year,
    })
}

/// Full kreditnota for an invoice: same lines negated, posted, and the
/// two receivable entries are reskontro-matched for whatever remains
/// open on the original.
pub async fn credit_invoice(
    pool: &PgPool,
    company_id: Uuid,
    invoice_id: Uuid,
    created_by: &str,
) -> Result<IssuedInvoice> {
    let original = sqlx::query(
        "select i.id, i.invoice_no, i.receivable_entry_id, p.party_no,
                (select exists (select 1 from invoice c where c.credits_invoice_id = i.id))
                    as already_credited
         from invoice i
         join party p on p.id = i.party_id
         where i.id = $1 and i.company_id = $2 and i.credits_invoice_id is null",
    )
    .bind(invoice_id)
    .bind(company_id)
    .fetch_optional(pool)
    .await?
    .context("no such invoice (or it is itself a kreditnota)")?;
    ensure!(
        !original.get::<bool, _>("already_credited"),
        "invoice is already credited"
    );

    let line_rows = sqlx::query(
        "select l.description, l.account_number, l.quantity_milli, l.unit_price_ore, l.vat_code,
                v.voucher_date, i.due_date, j.code as journal_code,
                (select a.number from entry e join account a on a.id = e.account_id
                 where e.id = i.receivable_entry_id) as receivable_account
         from invoice_line l
         join invoice i on i.id = l.invoice_id
         join voucher v on v.id = i.voucher_id
         join journal j on j.id = v.journal_id
         where l.invoice_id = $1
         order by l.line_no",
    )
    .bind(invoice_id)
    .fetch_all(pool)
    .await?;
    ensure!(!line_rows.is_empty(), "invoice has no lines");

    let today: NaiveDate = sqlx::query_scalar("select current_date")
        .fetch_one(pool)
        .await?;
    let draft = InvoiceDraft {
        party_no: original.get("party_no"),
        invoice_date: today,
        due_date: today,
        journal_code: line_rows[0].get("journal_code"),
        receivable_account: line_rows[0].get("receivable_account"),
        vat_account: "2700".into(),
        lines: line_rows
            .iter()
            .map(|r| InvoiceLineDraft {
                description: r.get("description"),
                account_number: r.get("account_number"),
                quantity_milli: -r.get::<i64, _>("quantity_milli"),
                unit_price_ore: r.get("unit_price_ore"),
                vat_code: r.get("vat_code"),
            })
            .collect(),
    };
    let credit = create_invoice(pool, company_id, &draft, created_by, Some(invoice_id)).await?;

    // Match the credit against whatever is still open on the original.
    let original_entry: Uuid = original.get("receivable_entry_id");
    let credit_entry: Uuid =
        sqlx::query_scalar("select receivable_entry_id from invoice where id = $1")
            .bind(credit.invoice_id)
            .fetch_one(pool)
            .await?;
    let party_id: Uuid = sqlx::query_scalar("select party_id from entry where id = $1")
        .bind(original_entry)
        .fetch_one(pool)
        .await?;
    let items = crate::reskontro::party_items(pool, company_id, party_id, false).await?;
    let remaining = items
        .iter()
        .find(|i| i.entry_id == original_entry)
        .map(|i| i.remaining_ore)
        .unwrap_or(0);
    let creditable = remaining.min(-credit.gross_ore);
    if creditable > 0 {
        crate::reskontro::match_items(
            pool,
            company_id,
            original_entry,
            credit_entry,
            creditable,
            created_by,
        )
        .await?;
    }
    Ok(credit)
}

#[derive(Debug)]
pub struct InvoiceRow {
    pub invoice_id: Uuid,
    pub invoice_no: i64,
    pub party_no: String,
    pub party_name: String,
    pub invoice_date: NaiveDate,
    pub due_date: NaiveDate,
    pub kid: String,
    pub gross_ore: i64,
    pub remaining_ore: i64,
    pub is_credit_note: bool,
}

pub async fn list_invoices(
    pool: &PgPool,
    company_id: Uuid,
    open_only: bool,
) -> Result<Vec<InvoiceRow>> {
    let rows = sqlx::query(
        "select i.id, i.invoice_no, p.party_no, p.name as party_name, i.invoice_date,
                i.due_date, i.kid, (i.credits_invoice_id is not null) as is_credit_note,
                e.amount_ore as gross_ore,
                e.amount_ore
                - coalesce((select sum(m.amount_ore) from reskontro_match m
                            where m.entry_a = e.id), 0)::bigint
                + coalesce((select sum(m.amount_ore) from reskontro_match m
                            where m.entry_b = e.id), 0)::bigint as remaining_ore
         from invoice i
         join party p on p.id = i.party_id
         join entry e on e.id = i.receivable_entry_id
         where i.company_id = $1
         order by i.invoice_no",
    )
    .bind(company_id)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .iter()
        .map(|r| InvoiceRow {
            invoice_id: r.get("id"),
            invoice_no: r.get("invoice_no"),
            party_no: r.get("party_no"),
            party_name: r.get("party_name"),
            invoice_date: r.get("invoice_date"),
            due_date: r.get("due_date"),
            kid: r.get("kid"),
            gross_ore: r.get("gross_ore"),
            remaining_ore: r.get("remaining_ore"),
            is_credit_note: r.get("is_credit_note"),
        })
        .filter(|row| !open_only || row.remaining_ore != 0)
        .collect())
}
