//! Loader for the satsregisteret (migration 0016, docs/regelverk.md);
//! lookup and staleness logic are pure in `regnmed-core::sats`.

use anyhow::Result;
use regnmed_core::sats::SatsPeriode;
use sqlx::{PgPool, Row};

pub async fn load_satser(pool: &PgPool) -> Result<Vec<SatsPeriode>> {
    let rows =
        sqlx::query("select domene, valid_from, verdi from sats order by domene, valid_from")
            .fetch_all(pool)
            .await?;
    Ok(rows
        .iter()
        .map(|r| SatsPeriode {
            domene: r.get("domene"),
            valid_from: r.get("valid_from"),
            verdi: r.get("verdi"),
        })
        .collect())
}
