//! Loads the input for a SAF-T Financial export from the database.
//!
//! Reads only — the export is a projection of the ledger, and rendering is
//! pure (`regnmed_core::saft::render`).

use anyhow::{Context, Result};
use chrono::{DateTime, NaiveDate, Utc};
use regnmed_core::saft::{
    SaftAccount, SaftInput, SaftJournal, SaftLine, SaftTaxCode, SaftTransaction,
};
use sqlx::{PgPool, Row};
use uuid::Uuid;

/// Assembles everything `regnmed_core::saft::render` needs for one company
/// and date range. The contact person is required by the Norwegian SAF-T
/// header and is not master data we hold, so the caller provides it.
pub async fn load_saft_input(
    pool: &PgPool,
    company_id: Uuid,
    start: NaiveDate,
    end: NaiveDate,
    contact_first_name: &str,
    contact_last_name: &str,
) -> Result<SaftInput> {
    let company = sqlx::query("select orgnr, name from company where id = $1")
        .bind(company_id)
        .fetch_optional(pool)
        .await?
        .context("no such company")?;

    let accounts = sqlx::query(
        "select a.number, a.name, a.created_at,
                coalesce(sum(e.amount_ore) filter (where v.voucher_date < $2), 0)::bigint as opening_ore,
                coalesce(sum(e.amount_ore) filter (where v.voucher_date <= $3), 0)::bigint as closing_ore
         from account a
         left join entry e on e.account_id = a.id
         left join voucher v on v.id = e.voucher_id
         where a.company_id = $1
         group by a.id
         order by a.number",
    )
    .bind(company_id)
    .bind(start)
    .bind(end)
    .fetch_all(pool)
    .await?
    .into_iter()
    .map(|row| SaftAccount {
        number: row.get("number"),
        name: row.get("name"),
        created: row.get::<DateTime<Utc>, _>("created_at").date_naive(),
        opening_ore: row.get("opening_ore"),
        closing_ore: row.get("closing_ore"),
    })
    .collect();

    let tax_codes = sqlx::query(
        "select distinct vc.code, vc.description, (vc.rate_percent * 100)::bigint as percent_bp
         from entry e
         join voucher v on v.id = e.voucher_id
         join vat_code vc on vc.code = e.vat_code
         where v.company_id = $1 and v.voucher_date between $2 and $3
         order by vc.code",
    )
    .bind(company_id)
    .bind(start)
    .bind(end)
    .fetch_all(pool)
    .await?
    .into_iter()
    .map(|row| SaftTaxCode {
        code: row.get("code"),
        description: row.get("description"),
        percent_bp: row.get("percent_bp"),
    })
    .collect();

    let voucher_rows = sqlx::query(
        "select j.code as journal_code, j.name as journal_name,
                v.id, v.fiscal_year, v.voucher_number, v.voucher_date, v.description,
                v.created_by, v.created_at,
                rv.fiscal_year as reversed_year, rv.voucher_number as reversed_number
         from voucher v
         join journal j on j.id = v.journal_id
         left join voucher rv on rv.id = v.reverses_voucher_id
         where v.company_id = $1 and v.voucher_date between $2 and $3
         order by j.code, v.fiscal_year, v.voucher_number",
    )
    .bind(company_id)
    .bind(start)
    .bind(end)
    .fetch_all(pool)
    .await?;

    let line_rows = sqlx::query(
        "select e.voucher_id, e.line_no, a.number as account_number,
                e.amount_ore, e.vat_code, e.description
         from entry e
         join voucher v on v.id = e.voucher_id
         join account a on a.id = e.account_id
         where v.company_id = $1 and v.voucher_date between $2 and $3
         order by e.voucher_id, e.line_no",
    )
    .bind(company_id)
    .bind(start)
    .bind(end)
    .fetch_all(pool)
    .await?;

    let mut lines_by_voucher: std::collections::HashMap<Uuid, Vec<SaftLine>> =
        std::collections::HashMap::new();
    for row in line_rows {
        lines_by_voucher
            .entry(row.get("voucher_id"))
            .or_default()
            .push(SaftLine {
                line_no: row.get("line_no"),
                account_number: row.get("account_number"),
                description: row.get("description"),
                amount_ore: row.get("amount_ore"),
                vat_code: row.get("vat_code"),
            });
    }

    let mut journals: Vec<SaftJournal> = Vec::new();
    for row in voucher_rows {
        let journal_code: String = row.get("journal_code");
        if journals.last().is_none_or(|j| j.code != journal_code) {
            journals.push(SaftJournal {
                code: journal_code,
                name: row.get("journal_name"),
                transactions: Vec::new(),
            });
        }
        let voucher_id: Uuid = row.get("id");
        let reverses = match (
            row.get::<Option<i32>, _>("reversed_year"),
            row.get::<Option<i64>, _>("reversed_number"),
        ) {
            (Some(year), Some(number)) => Some(format!("{year}-{number}")),
            _ => None,
        };
        journals
            .last_mut()
            .expect("journal was just pushed")
            .transactions
            .push(SaftTransaction {
                fiscal_year: row.get("fiscal_year"),
                number: row.get("voucher_number"),
                date: row.get("voucher_date"),
                description: row.get("description"),
                created_by: row.get("created_by"),
                created_at: row.get("created_at"),
                reverses,
                lines: lines_by_voucher.remove(&voucher_id).unwrap_or_default(),
            });
    }

    Ok(SaftInput {
        orgnr: company.get("orgnr"),
        company_name: company.get("name"),
        contact_first_name: contact_first_name.to_string(),
        contact_last_name: contact_last_name.to_string(),
        file_created: Utc::now().date_naive(),
        software_version: env!("CARGO_PKG_VERSION").to_string(),
        start,
        end,
        accounts,
        tax_codes,
        journals,
    })
}
