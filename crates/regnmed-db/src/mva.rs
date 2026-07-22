//! Mva-spesifikasjon: grunnlag and beregnet avgift per standard code,
//! aggregated from the ledger for a period (typically a termin).
//!
//! The beregning uses the rate valid on each voucher's date (dated
//! `vat_rate` table), then sums per (code, rate) — so a period spanning a
//! rate change reports one line per rate, exactly as an accountant needs
//! to see it.

use anyhow::{Context, Result, ensure};
use chrono::NaiveDate;
use regnmed_core::mva::{RatePeriod, vat_of_base};
use sqlx::{PgPool, Row};
use uuid::Uuid;

#[derive(Debug)]
pub struct MvaLine {
    pub code: String,
    pub description: String,
    pub rate_bp: i64,
    /// Sum of entry amounts carrying the code, ledger sign (positive =
    /// debit): sales bases are negative, purchase bases positive.
    pub grunnlag_ore: i64,
    /// `vat_of_base(grunnlag, rate)` — beregnet, not posted; comparing it
    /// against the posted VAT accounts is the accountant's control.
    pub avgift_ore: i64,
}

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
) -> Result<Vec<MvaLine>> {
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
            Ok(MvaLine {
                code,
                description: row.get("description"),
                rate_bp,
                grunnlag_ore,
                avgift_ore: vat_of_base(grunnlag_ore, rate_bp),
            })
        })
        .collect()
}
