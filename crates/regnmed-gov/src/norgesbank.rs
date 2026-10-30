//! Norges Banks åpne valutakurs-API (docs/currency.md, #44).
//!
//! `GET {base}/api/data/EXR/B.{VALUTAER}.NOK.SP?format=sdmx-json` — daily
//! quotes (banking days) in SDMX-JSON. The parser is pure and is tested
//! against a vendored sample (docs/currency/norges-bank-exr-sample.json);
//! fetching live is the same code path with the network in the middle.
//!
//! Two traps the parser handles explicitly:
//! - `UNIT_MULT`: SEK/DKK/JPY quotes per 100 (multiplier 2) — the
//!   published number must be divided accordingly, or every SEK beløp
//!   is wrong by two orders of magnitude.
//! - Values parse as DECIMAL STRINGS into mikro-NOK
//!   (`regnmed_core::currency::parse_kurs`) — floats never touch rates.

use anyhow::{Context, Result, bail};
use chrono::NaiveDate;
use regnmed_core::valuta::parse_kurs;
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Quote {
    pub currency: String,
    pub date: NaiveDate,
    pub rate_micro: i64,
}

pub struct NorgesBankClient {
    base_url: String,
    http: reqwest::Client,
}

impl NorgesBankClient {
    pub fn new(base_url: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into(),
            http: reqwest::Client::new(),
        }
    }

    /// NORGES_BANK_URL overrides the public endpoint (tests, pinning).
    pub fn from_env() -> Self {
        Self::new(
            std::env::var("NORGES_BANK_URL")
                .unwrap_or_else(|_| "https://data.norges-bank.no".into()),
        )
    }

    /// The last `last_n` noteringer for each currency.
    pub async fn fetch_rates(&self, valutaer: &[String], last_n: u32) -> Result<Vec<Quote>> {
        if valutaer.is_empty() {
            return Ok(Vec::new());
        }
        let url = format!(
            "{}/api/data/EXR/B.{}.NOK.SP?format=sdmx-json&lastNObservations={last_n}",
            self.base_url.trim_end_matches('/'),
            valutaer.join("+"),
        );
        let response = self
            .http
            .get(&url)
            .send()
            .await
            .with_context(|| format!("GET {url}"))?;
        if !response.status().is_success() {
            bail!("Norges Bank svarte {} på {url}", response.status());
        }
        let body: Value = response
            .json()
            .await
            .context("ugyldig JSON fra Norges Bank")?;
        parse_sdmx(&body)
    }
}

/// Parses the SDMX-JSON body into noteringer. Pure; fails loudly on
/// any structural surprise — a half-parsed rate table is worse than
/// none.
pub fn parse_sdmx(body: &Value) -> Result<Vec<Quote>> {
    let data = &body["data"];
    let structure = &data["structure"];
    let series_dims = structure["dimensions"]["series"]
        .as_array()
        .context("mangler dimensions.series")?;
    let base_cur_pos = series_dims
        .iter()
        .position(|d| d["id"] == "BASE_CUR")
        .context("mangler BASE_CUR-dimensjonen")?;
    let currency_of = |index: usize| -> Result<String> {
        series_dims[base_cur_pos]["values"]
            .as_array()
            .and_then(|v| v.get(index))
            .and_then(|v| v["id"].as_str())
            .map(str::to_string)
            .context("BASE_CUR-indeks utenfor verdilisten")
    };
    let dates: Vec<NaiveDate> = structure["dimensions"]["observation"]
        .as_array()
        .and_then(|obs| obs.iter().find(|d| d["id"] == "TIME_PERIOD"))
        .and_then(|d| d["values"].as_array())
        .context("mangler TIME_PERIOD")?
        .iter()
        .map(|v| {
            v["id"]
                .as_str()
                .context("datoverdi uten id")
                .and_then(|s| s.parse::<NaiveDate>().context("ugyldig dato"))
        })
        .collect::<Result<_>>()?;
    let attr_defs = structure["attributes"]["series"]
        .as_array()
        .context("mangler attributes.series")?;
    let unit_mult_pos = attr_defs.iter().position(|a| a["id"] == "UNIT_MULT");

    let series = data["dataSets"][0]["series"]
        .as_object()
        .context("mangler dataSets[0].series")?;
    let mut noteringer = Vec::new();
    for (key, serie) in series {
        let indexes: Vec<usize> = key
            .split(':')
            .map(|part| part.parse().context("ugyldig serienøkkel"))
            .collect::<Result<_>>()?;
        let currency = currency_of(
            *indexes
                .get(base_cur_pos)
                .context("serienøkkel kortere enn dimensjonene")?,
        )?;
        // UNIT_MULT: exponent — quoted per 10^n units of the currency.
        let unit_mult: u32 = match unit_mult_pos {
            Some(pos) => {
                let attr_index = serie["attributes"]
                    .as_array()
                    .and_then(|a| a.get(pos))
                    .and_then(Value::as_u64)
                    .context("mangler UNIT_MULT-attributt på serien")?;
                attr_defs[pos]["values"]
                    .as_array()
                    .and_then(|v| v.get(attr_index as usize))
                    .and_then(|v| v["id"].as_str())
                    .context("UNIT_MULT-indeks utenfor verdilisten")?
                    .parse()
                    .context("UNIT_MULT er ikke et tall")?
            }
            None => 0,
        };
        let divisor = 10_i64.pow(unit_mult);
        let observations = serie["observations"]
            .as_object()
            .context("serie uten observations")?;
        for (obs_key, obs_value) in observations {
            let date_index: usize = obs_key.parse().context("ugyldig observasjonsnøkkel")?;
            let date = *dates
                .get(date_index)
                .context("observasjonsindeks utenfor datolisten")?;
            let raw = match &obs_value[0] {
                Value::String(s) => s.clone(),
                Value::Number(n) => n.to_string(),
                other => bail!("uventet observasjonsverdi {other}"),
            };
            let kurs = parse_kurs(&raw)
                .with_context(|| format!("uparselig kurs {raw:?} for {currency}"))?;
            noteringer.push(Quote {
                currency: currency.clone(),
                date,
                rate_micro: kurs / divisor,
            });
        }
    }
    noteringer.sort_by(|a, b| (&a.currency, a.date).cmp(&(&b.currency, b.date)));
    Ok(noteringer)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_vendored_sample_with_unit_mult() {
        let sample: Value = serde_json::from_str(include_str!(
            "../../../docs/valuta/norges-bank-exr-sample.json"
        ))
        .unwrap();
        let noteringer = parse_sdmx(&sample).unwrap();
        let eur: Vec<_> = noteringer.iter().filter(|n| n.currency == "EUR").collect();
        let sek: Vec<_> = noteringer.iter().filter(|n| n.currency == "SEK").collect();
        assert_eq!(eur.len(), 2);
        assert_eq!(eur[0].date, NaiveDate::from_ymd_opt(2026, 7, 23).unwrap());
        assert_eq!(eur[0].rate_micro, 11_648_500);
        assert_eq!(eur[1].rate_micro, 11_654_300);
        // SEK quotes per 100 (UNIT_MULT 2): 94.35 → 0,9435 kr.
        assert_eq!(sek.len(), 2);
        assert_eq!(sek[1].rate_micro, 943_500);
    }
}
