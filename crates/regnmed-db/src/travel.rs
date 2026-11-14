//! Diett on business travel (docs/utlegg.md «Reise og diett»): the rates
//! a trip is computed with — the satsregister for Norway and the trekkfri
//! side, the state's country table abroad, Norges Bank's rate for the
//! foreign currency — and the computed døgn lines as they are stored on
//! the claim at submission.

use anyhow::{Context, Result, bail, ensure};
use chrono::{NaiveDate, NaiveDateTime};
use regnmed_core::travel::{
    DayInput, DayKind, Lodging, Meals, Place, Scheme, StateRates, TaxFreeRates, TravelAllowance,
};
use serde::{Deserialize, Serialize};
use sqlx::{PgPool, Row};
use uuid::Uuid;

use crate::rates::load_rates;

/// What the traveller enters for one døgn. `place` is `NO` or a
/// [`AbroadRate::key`].
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TravelDayDraft {
    #[serde(default)]
    pub place: String,
    pub lodging: Option<String>,
    #[serde(default)]
    pub breakfast: bool,
    #[serde(default)]
    pub lunch: bool,
    #[serde(default)]
    pub dinner: bool,
}

/// One row of the state's table abroad.
#[derive(Debug, Clone, Serialize)]
pub struct AbroadRate {
    pub region: String,
    pub country: String,
    pub area: String,
    pub amount_minor: i64,
    pub currency: String,
}

impl AbroadRate {
    /// How a døgn names it: «Land» or «Land|By».
    pub fn key(&self) -> String {
        if self.area.is_empty() {
            self.country.clone()
        } else {
            format!("{}|{}", self.country, self.area)
        }
    }
}

/// The table on `date`: per country and area, the newest row on or
/// before it.
pub async fn abroad_rates(pool: &PgPool, date: NaiveDate) -> Result<Vec<AbroadRate>> {
    let rows = sqlx::query(
        "select distinct on (country, area) region, country, area, amount_minor, currency
         from travel_rate_abroad where valid_from <= $1
         order by country, area, valid_from desc",
    )
    .bind(date)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .iter()
        .map(|r| AbroadRate {
            region: r.get("region"),
            country: r.get("country"),
            area: r.get("area"),
            amount_minor: r.get("amount_minor"),
            currency: r.get("currency"),
        })
        .collect())
}

/// A computed døgn as stored on the claim (`travel_lines`): what was
/// paid, what of it is trekkfri, and the evidence behind a foreign rate.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TravelLine {
    pub start: NaiveDateTime,
    pub end: NaiveDateTime,
    /// `overnatting`, `rest` or `dagreise`.
    pub kind: String,
    /// `NO` or the abroad key.
    pub place: String,
    pub lodging: Option<String>,
    pub breakfast: bool,
    pub lunch: bool,
    pub dinner: bool,
    /// Abroad: the table's rate, the currency and Norges Bank's rate on
    /// the døgn's date (micro-NOK per unit) and its date.
    pub foreign_amount_minor: Option<i64>,
    pub currency: Option<String>,
    pub exchange_rate_micro: Option<i64>,
    pub exchange_rate_date: Option<NaiveDate>,
    pub kost_paid_ore: i64,
    pub kost_tax_free_ore: i64,
    pub night_paid_ore: i64,
    pub night_tax_free_ore: i64,
}

impl TravelLine {
    /// Back to the core's line, for the a-melding split.
    pub fn core(&self) -> regnmed_core::travel::DayLine {
        regnmed_core::travel::DayLine {
            start: self.start,
            end: self.end,
            kind: match self.kind.as_str() {
                "overnatting" => DayKind::Overnight,
                "rest" => DayKind::Remainder,
                _ => DayKind::DayTrip,
            },
            lodging: self.lodging.as_deref().and_then(Lodging::from_slug),
            // The NOK value of a foreign rate is not kept on the line; the
            // a-melding split reads neither.
            place: if self.place == "NO" {
                Place::Norway
            } else {
                Place::Abroad { state_rate_ore: 0 }
            },
            meals: Meals {
                breakfast: self.breakfast,
                lunch: self.lunch,
                dinner: self.dinner,
            },
            kost_paid_ore: self.kost_paid_ore,
            kost_tax_free_ore: self.kost_tax_free_ore,
            night_paid_ore: self.night_paid_ore,
            night_tax_free_ore: self.night_tax_free_ore,
        }
    }
}

/// A foreign rate older than this on the døgn's date is not "the rate when
/// the trip took place" (særavtale utenfor Norge § 15) — fetch rates.
const MAX_RATE_AGE_DAYS: i64 = 7;

/// The whole computation, for a preview or for submission: the trip's
/// døgn with their rates resolved, and the allowance. Rates are those on
/// the DEPARTURE date (a trip over new year keeps the old year's rates —
/// documented); a foreign rate is converted on each døgn's own date.
pub async fn compute(
    pool: &PgPool,
    departure: NaiveDateTime,
    return_at: NaiveDateTime,
    overnight: bool,
    days: &[TravelDayDraft],
    scheme: Scheme,
) -> Result<(TravelAllowance, Vec<TravelLine>)> {
    let count = regnmed_core::travel::day_count(departure, return_at, overnight)
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    ensure!(
        days.len() == count,
        "reisen varer {count} døgn, men {} døgn er fylt ut",
        days.len()
    );
    let date = departure.date();
    let register = load_rates(pool).await?;
    let tax_free = TaxFreeRates::on(&register, date)
        .map_err(|d| anyhow::anyhow!("ingen sats «{d}» for {date} i satsregisteret"))?;
    let state = StateRates::on(&register, date)
        .map_err(|d| anyhow::anyhow!("ingen sats «{d}» for {date} i satsregisteret"))?;
    let table = abroad_rates(pool, date).await?;

    let mut inputs = Vec::with_capacity(days.len());
    let mut foreign = Vec::with_capacity(days.len());
    for (i, d) in days.iter().enumerate() {
        let day_date = (departure + chrono::Duration::days(i as i64)).date();
        let lodging = match d
            .lodging
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            None => None,
            Some(s) => {
                Some(Lodging::from_slug(s).with_context(|| format!("ukjent overnatting «{s}»"))?)
            }
        };
        let place = d.place.trim();
        let (place, evidence) = if place.is_empty() || place == "NO" {
            (Place::Norway, None)
        } else {
            let rate = table
                .iter()
                .find(|r| r.key() == place)
                .with_context(|| format!("døgn {}: ukjent land «{place}»", i + 1))?;
            // The state's rate is only paid on the state's scheme; on the
            // trekkfri scheme abroad pays the same as at home, and no
            // exchange rate is needed.
            if scheme == Scheme::State && rate.currency != "NOK" {
                let (rate_date, micro) =
                    crate::require_exchange_rate(pool, &rate.currency, day_date).await?;
                if (day_date - rate_date).num_days() > MAX_RATE_AGE_DAYS {
                    bail!(
                        "den nyeste {}-kursen er fra {rate_date}, over en uke før {day_date} — \
                         hent valutakurser og prøv igjen",
                        rate.currency
                    );
                }
                let ore = regnmed_core::currency::nok_ore(rate.amount_minor, micro);
                (
                    Place::Abroad {
                        state_rate_ore: ore,
                    },
                    Some((rate, Some((micro, rate_date)))),
                )
            } else {
                (
                    Place::Abroad {
                        state_rate_ore: rate.amount_minor,
                    },
                    Some((rate, None)),
                )
            }
        };
        inputs.push(DayInput {
            place,
            lodging,
            meals: Meals {
                breakfast: d.breakfast,
                lunch: d.lunch,
                dinner: d.dinner,
            },
        });
        foreign.push(evidence.map(|(r, fx)| (r.key(), r.amount_minor, r.currency.clone(), fx)));
    }
    let allowance = regnmed_core::travel::allowance(
        departure, return_at, overnight, &inputs, scheme, &tax_free, &state,
    )
    .map_err(|e| anyhow::anyhow!("{e}"))?;
    let lines = allowance
        .lines
        .iter()
        .zip(&foreign)
        .map(|(l, f)| TravelLine {
            start: l.start,
            end: l.end,
            kind: match l.kind {
                DayKind::Overnight => "overnatting",
                DayKind::Remainder => "rest",
                DayKind::DayTrip => "dagreise",
            }
            .into(),
            place: f.as_ref().map_or("NO".into(), |(key, ..)| key.clone()),
            lodging: l.lodging.map(|x| x.slug().into()),
            breakfast: l.meals.breakfast,
            lunch: l.meals.lunch,
            dinner: l.meals.dinner,
            foreign_amount_minor: f.as_ref().map(|(_, a, ..)| *a),
            currency: f.as_ref().map(|(_, _, c, _)| c.clone()),
            exchange_rate_micro: f.as_ref().and_then(|(.., fx)| fx.map(|x| x.0)),
            exchange_rate_date: f.as_ref().and_then(|(.., fx)| fx.map(|x| x.1)),
            kost_paid_ore: l.kost_paid_ore,
            kost_tax_free_ore: l.kost_tax_free_ore,
            night_paid_ore: l.night_paid_ore,
            night_tax_free_ore: l.night_tax_free_ore,
        })
        .collect();
    Ok((allowance, lines))
}

/// Whose rates the company pays (`trekkfri` or `statens`).
pub async fn travel_scheme(pool: &PgPool, company_id: Uuid) -> Result<Scheme> {
    let slug: String = sqlx::query_scalar("select travel_rate_scheme from company where id = $1")
        .bind(company_id)
        .fetch_one(pool)
        .await?;
    Scheme::from_slug(&slug).context("unknown travel rate scheme")
}

pub async fn set_travel_scheme(pool: &PgPool, company_id: Uuid, scheme: &str) -> Result<()> {
    ensure!(
        Scheme::from_slug(scheme).is_some(),
        "ukjent satsvalg «{scheme}» — «trekkfri» eller «statens»"
    );
    sqlx::query("update company set travel_rate_scheme = $2 where id = $1")
        .bind(company_id)
        .bind(scheme)
        .execute(pool)
        .await?;
    Ok(())
}
