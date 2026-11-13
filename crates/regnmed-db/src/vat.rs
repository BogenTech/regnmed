//! The mva-spesifikasjon: basis and computed tax per standard code,
//! aggregated from the ledger for a date range (typically a VAT period).
//!
//! The beregning uses the rate valid on each voucher's date (dated
//! `vat_rate` table), then sums per (code, rate) — so a period spanning a
//! rate change reports one line per rate, exactly as an accountant needs
//! to see it.

use anyhow::{Context, Result, ensure};
use chrono::NaiveDate;
use regnmed_core::vat::{RatePeriod, SpecLine, vat_of_base};
use sqlx::{PgPool, Row};
use uuid::Uuid;

/// The full dated rate table, for `regnmed_core::mva::rate_on`.
pub async fn load_vat_rates(pool: &PgPool) -> Result<Vec<RatePeriod>> {
    let rows =
        sqlx::query("select rate_class, valid_from, rate_bp from vat_rate order by valid_from")
            .fetch_all(pool)
            .await?;
    Ok(rows
        .iter()
        .map(|r| RatePeriod {
            rate_class: r.get("rate_class"),
            valid_from: r.get("valid_from"),
            rate_bp: i64::from(r.get::<i32, _>("rate_bp")),
        })
        .collect())
}

pub async fn vat_specification(
    pool: &PgPool,
    company_id: Uuid,
    start: NaiveDate,
    end: NaiveDate,
) -> Result<Vec<SpecLine>> {
    let rows = sqlx::query(
        "select e.vat_code as code, vc.description, r.rate_bp,
                sum(e.amount_ore)::bigint as grunnlag_ore
         from entry e
         join voucher v on v.id = e.voucher_id
         join vat_code vc on vc.code = e.vat_code
         left join lateral (
             select rate_bp from vat_rate
             where rate_class = vc.rate_class and valid_from <= v.voucher_date
             order by valid_from desc limit 1
         ) r on true
         where v.company_id = $1
           and v.voucher_date between $2 and $3
           and e.vat_code is not null
         group by e.vat_code, vc.description, r.rate_bp
         order by e.vat_code, r.rate_bp",
    )
    .bind(company_id)
    .bind(start)
    .bind(end)
    .fetch_all(pool)
    .await?;

    rows.iter()
        .map(|row| {
            let code: String = row.get("code");
            let rate_bp: i64 = row
                .get::<Option<i32>, _>("rate_bp")
                .map(i64::from)
                .with_context(|| {
                    format!("no VAT rate on record for code {code} in this period (pre-2016?)")
                })?;
            let basis_ore: i64 = row.get("grunnlag_ore");
            ensure!(!code.is_empty(), "empty vat code in ledger");
            Ok(SpecLine {
                code,
                description: row.get("description"),
                rate_bp,
                basis_ore,
                tax_ore: vat_of_base(basis_ore, rate_bp),
            })
        })
        .collect()
}

/// One posting behind a line of the mva-spesifikasjon: the voucher it
/// sits on, and its basis at the rate valid on the voucher's date — what
/// the accountant checks before the return is filed.
#[derive(Debug)]
pub struct VatPosting {
    pub voucher_id: Uuid,
    pub voucher_label: String,
    pub date: NaiveDate,
    pub account: String,
    pub account_name: String,
    pub description: String,
    pub party_no: Option<String>,
    pub basis_ore: i64,
    pub rate_bp: Option<i64>,
}

/// The postings with `code` in the period, oldest first, at most
/// `limit`, with the total count. Same filter as `vat_specification`,
/// so the rows add up to the line they open.
pub async fn vat_postings(
    pool: &PgPool,
    company_id: Uuid,
    start: NaiveDate,
    end: NaiveDate,
    code: &str,
    limit: i64,
) -> Result<(Vec<VatPosting>, i64)> {
    let rows = sqlx::query(
        "select v.id as voucher_id, v.fiscal_year, v.voucher_number, j.code as journal,
                v.voucher_date, a.number, a.name as account_name,
                coalesce(e.description, v.description) as description, p.party_no,
                e.amount_ore, r.rate_bp, count(*) over () as total
         from entry e
         join voucher v on v.id = e.voucher_id
         join journal j on j.id = v.journal_id
         join account a on a.id = e.account_id
         join vat_code vc on vc.code = e.vat_code
         left join party p on p.id = e.party_id
         left join lateral (
             select rate_bp from vat_rate
             where rate_class = vc.rate_class and valid_from <= v.voucher_date
             order by valid_from desc limit 1
         ) r on true
         where v.company_id = $1
           and v.voucher_date between $2 and $3
           and e.vat_code = $4
         order by v.voucher_date, v.chain_seq, e.line_no
         limit $5",
    )
    .bind(company_id)
    .bind(start)
    .bind(end)
    .bind(code)
    .bind(limit)
    .fetch_all(pool)
    .await?;
    let total = rows.first().map(|r| r.get::<i64, _>("total")).unwrap_or(0);
    Ok((
        rows.iter()
            .map(|r| {
                let journal: String = r.get("journal");
                let label = format!(
                    "{}-{}",
                    r.get::<i32, _>("fiscal_year"),
                    r.get::<i64, _>("voucher_number")
                );
                VatPosting {
                    voucher_id: r.get("voucher_id"),
                    voucher_label: if journal == "GL" {
                        label
                    } else {
                        format!("{journal} {label}")
                    },
                    date: r.get("voucher_date"),
                    account: r.get("number"),
                    account_name: r.get("account_name"),
                    description: r.get("description"),
                    party_no: r.get("party_no"),
                    basis_ore: r.get("amount_ore"),
                    rate_bp: r.get::<Option<i32>, _>("rate_bp").map(i64::from),
                }
            })
            .collect(),
        total,
    ))
}

/// The company's terminordning valid on `dato` (docs/mva.md, #51):
/// the newest registered row on or before the date; to-måneder when
/// none is registered — the lawful default needs no row.
pub async fn period_scheme_on(
    pool: &PgPool,
    company_id: uuid::Uuid,
    date: chrono::NaiveDate,
) -> Result<regnmed_core::vat::PeriodScheme> {
    let row: Option<String> = sqlx::query_scalar(
        "select ordning from mva_terminordning
         where company_id = $1 and valid_from <= $2
         order by valid_from desc limit 1",
    )
    .bind(company_id)
    .bind(date)
    .fetch_optional(pool)
    .await?;
    Ok(row
        .as_deref()
        .and_then(regnmed_core::vat::PeriodScheme::parse)
        .unwrap_or(regnmed_core::vat::PeriodScheme::ToManeder))
}

#[derive(Debug)]
pub struct PeriodSchemeRow {
    pub valid_from: chrono::NaiveDate,
    pub scheme: String,
    pub note: Option<String>,
    pub created_by: String,
}

pub async fn list_period_schemes(
    pool: &PgPool,
    company_id: uuid::Uuid,
) -> Result<Vec<PeriodSchemeRow>> {
    let rows = sqlx::query(
        "select valid_from, ordning, note, created_by from mva_terminordning
         where company_id = $1 order by valid_from desc",
    )
    .bind(company_id)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .iter()
        .map(|r| PeriodSchemeRow {
            valid_from: r.get("valid_from"),
            scheme: r.get("ordning"),
            note: r.get("note"),
            created_by: r.get("created_by"),
        })
        .collect())
}

/// Records the ordning Skatteetaten has granted, effective from a
/// date. Append-only: a change back is a new row.
pub async fn set_period_scheme(
    pool: &PgPool,
    company_id: uuid::Uuid,
    valid_from: chrono::NaiveDate,
    scheme: regnmed_core::vat::PeriodScheme,
    note: Option<&str>,
    created_by: &str,
) -> Result<()> {
    sqlx::query(
        "insert into mva_terminordning (company_id, valid_from, ordning, note, created_by)
         values ($1, $2, $3, $4, $5)",
    )
    .bind(company_id)
    .bind(valid_from)
    .bind(scheme.as_str())
    .bind(note)
    .bind(created_by)
    .execute(pool)
    .await
    .map_err(|e| anyhow::anyhow!("kunne ikke registrere terminordning: {e}"))?;
    Ok(())
}
