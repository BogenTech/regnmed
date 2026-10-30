//! Valutakurser (docs/valuta.md, #44): the global dated rate table.
//! One rate per (valuta, dato), append-only, kilde on every row. The
//! lookup mirrors sats/vat_rate: "the last published rate on or before
//! the date" — Norges Bank publishes bank days only, so weekends and
//! holidays resolve to the previous notering.

use anyhow::{Context, Result, ensure};
use chrono::NaiveDate;
use regnmed_core::currency::is_valid_currency_code;
use sqlx::{PgPool, Row};

pub async fn insert_kurs(
    pool: &PgPool,
    currency: &str,
    date: NaiveDate,
    rate_micro: i64,
    kilde: &str,
) -> Result<()> {
    ensure!(
        is_valid_currency_code(currency),
        "ugyldig valutakode {currency}"
    );
    ensure!(rate_micro > 0, "kursen må være positiv");
    sqlx::query(
        "insert into valutakurs (valuta, dato, kurs_micro, kilde) values ($1, $2, $3, $4)
         on conflict (valuta, dato) do nothing",
    )
    .bind(currency)
    .bind(date)
    .bind(rate_micro)
    .bind(kilde)
    .execute(pool)
    .await?;
    Ok(())
}

/// The rate in effect on `dato`: the newest notering on or before it.
/// None before the register's coverage — the caller fails loudly.
pub async fn kurs_for(
    pool: &PgPool,
    currency: &str,
    date: NaiveDate,
) -> Result<Option<(NaiveDate, i64)>> {
    let row = sqlx::query(
        "select dato, kurs_micro from valutakurs
         where valuta = $1 and dato <= $2
         order by dato desc limit 1",
    )
    .bind(currency)
    .bind(date)
    .fetch_optional(pool)
    .await?;
    Ok(row.map(|r| (r.get("dato"), r.get("kurs_micro"))))
}

/// Convenience with a clear error for the posting paths.
pub async fn require_kurs(
    pool: &PgPool,
    currency: &str,
    date: NaiveDate,
) -> Result<(NaiveDate, i64)> {
    kurs_for(pool, currency, date).await?.with_context(|| {
        format!("ingen {currency}-kurs på eller før {date} — hent eller registrer kurser")
    })
}

#[derive(Debug)]
pub struct KursRow {
    pub currency: String,
    pub date: NaiveDate,
    pub rate_micro: i64,
    pub kilde: String,
}

/// The newest rate per currency (the portal's kurser card).
pub async fn latest_kurser(pool: &PgPool) -> Result<Vec<KursRow>> {
    let rows = sqlx::query(
        "select distinct on (valuta) valuta, dato, kurs_micro, kilde
         from valutakurs order by valuta, dato desc",
    )
    .fetch_all(pool)
    .await?;
    Ok(rows
        .iter()
        .map(|r| KursRow {
            currency: r.get("valuta"),
            date: r.get("dato"),
            rate_micro: r.get("kurs_micro"),
            kilde: r.get("kilde"),
        })
        .collect())
}
