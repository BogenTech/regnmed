//! Mva-spesifikasjon: grunnlag and beregnet avgift per standard code,
//! aggregated from the ledger for a period (typically a termin).
//!
//! The beregning uses the rate valid on each voucher's date (dated
//! `vat_rate` table), then sums per (code, rate) — so a period spanning a
//! rate change reports one line per rate, exactly as an accountant needs
//! to see it.

use anyhow::{Context, Result, ensure};
use chrono::NaiveDate;
use regnmed_core::mva::{RatePeriod, SpesLine, vat_of_base};
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

pub async fn mva_spesifikasjon(
    pool: &PgPool,
    company_id: Uuid,
    start: NaiveDate,
    end: NaiveDate,
) -> Result<Vec<SpesLine>> {
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
            let grunnlag_ore: i64 = row.get("grunnlag_ore");
            ensure!(!code.is_empty(), "empty vat code in ledger");
            Ok(SpesLine {
                code,
                description: row.get("description"),
                rate_bp,
                grunnlag_ore,
                avgift_ore: vat_of_base(grunnlag_ore, rate_bp),
            })
        })
        .collect()
}

/// The company's terminordning valid on `dato` (docs/mva.md, #51):
/// the newest registered row on or before the date; to-måneder when
/// none is registered — the lawful default needs no row.
pub async fn terminordning_on(
    pool: &PgPool,
    company_id: uuid::Uuid,
    dato: chrono::NaiveDate,
) -> Result<regnmed_core::mva::Terminordning> {
    let row: Option<String> = sqlx::query_scalar(
        "select ordning from mva_terminordning
         where company_id = $1 and valid_from <= $2
         order by valid_from desc limit 1",
    )
    .bind(company_id)
    .bind(dato)
    .fetch_optional(pool)
    .await?;
    Ok(row
        .as_deref()
        .and_then(regnmed_core::mva::Terminordning::parse)
        .unwrap_or(regnmed_core::mva::Terminordning::ToManeder))
}

#[derive(Debug)]
pub struct TerminordningRow {
    pub valid_from: chrono::NaiveDate,
    pub ordning: String,
    pub note: Option<String>,
    pub created_by: String,
}

pub async fn list_terminordninger(
    pool: &PgPool,
    company_id: uuid::Uuid,
) -> Result<Vec<TerminordningRow>> {
    let rows = sqlx::query(
        "select valid_from, ordning, note, created_by from mva_terminordning
         where company_id = $1 order by valid_from desc",
    )
    .bind(company_id)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .iter()
        .map(|r| TerminordningRow {
            valid_from: r.get("valid_from"),
            ordning: r.get("ordning"),
            note: r.get("note"),
            created_by: r.get("created_by"),
        })
        .collect())
}

/// Records the ordning Skatteetaten has granted, effective from a
/// date. Append-only: a change back is a new row.
pub async fn set_terminordning(
    pool: &PgPool,
    company_id: uuid::Uuid,
    valid_from: chrono::NaiveDate,
    ordning: regnmed_core::mva::Terminordning,
    note: Option<&str>,
    created_by: &str,
) -> Result<()> {
    sqlx::query(
        "insert into mva_terminordning (company_id, valid_from, ordning, note, created_by)
         values ($1, $2, $3, $4, $5)",
    )
    .bind(company_id)
    .bind(valid_from)
    .bind(ordning.as_str())
    .bind(note)
    .bind(created_by)
    .execute(pool)
    .await
    .map_err(|e| anyhow::anyhow!("kunne ikke registrere terminordning: {e}"))?;
    Ok(())
}
