//! Skatteetatens API «Trekkpålegg» (docs/trekkpalegg.md §1, #334). OpenAPI
//! 1.4 on SwaggerHub (`skatteetaten/trekkpaalegg-app`).
//!
//! One call, with the **systembruker token for the employer**:
//! `GET {base}` -- the latest version of EVERY order for the employer, each
//! with `trekkstatus` `aktiv` or `avsluttet`. regnmed never pages it
//! (`fraSekvensnummer`/`maksAntall` are for employers with thousands of
//! orders), so every successful answer is the whole list, and an order
//! missing from it is an anomaly (§2 step 1).
//!
//! [`parse_list`] reads the answer. The shape is the specification's; the
//! example values are Skatteetaten's own documentation examples
//! (docs/trekkpalegg/liste-fra-dokumentasjonen.json), not a live answer:
//! the scope is still on order (SSV-6690).

use anyhow::{Context, Result, bail};
use chrono::{DateTime, NaiveDate, Utc};
use regnmed_core::garnishment::Size;
use serde_json::Value;

use crate::maskinporten::TokenProvider;

pub const SCOPES: &str = "skatteetaten:trekkpaalegg";

pub struct GarnishmentClient {
    /// `https://api-test.sits.no/api/trekkpaalegg/v1` in test.
    pub base_url: String,
    http: reqwest::Client,
}

impl GarnishmentClient {
    pub fn new(base_url: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into().trim_end_matches('/').to_string(),
            http: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(30))
                .build()
                .expect("a TLS backend"),
        }
    }

    /// Reads TREKKPALEGG_URL, defaulting to the TEST environment --
    /// production is opted into, never fallen into.
    pub fn from_env() -> Self {
        Self::new(
            std::env::var("TREKKPALEGG_URL")
                .unwrap_or_else(|_| "https://api-test.sits.no/api/trekkpaalegg/v1".into()),
        )
    }

    /// The whole list, verbatim. Any non-2xx is an error carrying
    /// Skatteetaten's body (`kode`, `melding`) as it came: the documented
    /// codes are `KB-0xx`, the specification's example `TKL-0xx`, and
    /// which one a 404 means is not settled (§4), so none is read as "no
    /// orders".
    pub async fn list(&self, tokens: &TokenProvider, orgnumber: &str) -> Result<Value> {
        let token = tokens.token_for_systemuser(orgnumber).await?;
        let response = self
            .http
            .get(&self.base_url)
            .bearer_auth(token)
            .header("accept", "application/json")
            .send()
            .await
            .context("fetching trekkpålegg")?;
        let status = response.status();
        let text = response.text().await.unwrap_or_default();
        if !status.is_success() {
            bail!("{status}: {text}");
        }
        serde_json::from_str(&text).with_context(|| format!("reading the trekkpålegg list {text}"))
    }
}

/// One version of one order, as Skatteetaten sent it.
#[derive(Debug, Clone, PartialEq)]
pub struct Order {
    /// `trekkid`: stable across versions.
    pub id: String,
    pub version: i32,
    pub sequence: i64,
    /// `opprettet`. Per version in the documentation's examples (10003 v1
    /// and v2 differ), so the run uses the earliest one stored (§2 step 3).
    pub created: DateTime<Utc>,
    pub case_number: String,
    /// `trekkstatus == "aktiv"`.
    pub active: bool,
    /// `trekkpliktig`: the employer.
    pub employer: String,
    /// `skyldner`: the employee's fødsels- or D-nummer.
    pub debtor: String,
    pub kid: String,
    pub account: String,
    pub payee: String,
    pub periods: Vec<Period>,
    /// The order, verbatim -- the evidence.
    pub raw: Value,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Period {
    pub from: NaiveDate,
    pub to: Option<NaiveDate>,
    /// `Err` with the reason when the size cannot be read without
    /// guessing (more than two decimals, negative, both or neither).
    pub size: std::result::Result<Size, String>,
}

impl Order {
    /// The period covering the pay date (§1: "den som dekker
    /// lønnsutbetalingsdatoen"); the latest-starting one if they overlap.
    /// A `sluttdato` may lie before `startdato` (an order ended before it
    /// began); such a period covers nothing.
    pub fn period_on(&self, date: NaiveDate) -> Option<&Period> {
        self.periods
            .iter()
            .filter(|p| p.from <= date && p.to.is_none_or(|to| date <= to))
            .max_by_key(|p| p.from)
    }
}

/// Every order in a list answer. A list that is not an array, or an order
/// without its identity, fails the whole list: a partial list would make
/// the orders left out look stopped (§2 step 1).
pub fn parse_list(answer: &Value) -> std::result::Result<Vec<Order>, String> {
    answer
        .as_array()
        .ok_or("svaret fra Skatteetaten er ikke en liste")?
        .iter()
        .map(parse_order)
        .collect()
}

/// The messages name the order by `trekkid`, never by its content: the
/// content carries the employee's fødselsnummer, and the message is shown
/// in the portal.
pub fn parse_order(o: &Value) -> std::result::Result<Order, String> {
    let who = o["trekkid"].as_str().unwrap_or("uten trekkid").to_string();
    let text = |field: &str| -> std::result::Result<String, String> {
        o[field]
            .as_str()
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .ok_or_else(|| format!("trekkpålegg {who} mangler {field}"))
    };
    let pay = &o["betalingsinformasjon"];
    let pay_text = |field: &str| -> std::result::Result<String, String> {
        pay[field]
            .as_str()
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .ok_or_else(|| format!("trekkpålegg {who} mangler betalingsinformasjon/{field}"))
    };
    let status = text("trekkstatus")?;
    let periods = o["trekkstoerrelseForPeriode"]
        .as_array()
        .ok_or_else(|| format!("trekkpålegg {who} mangler trekkstoerrelseForPeriode"))?
        .iter()
        .map(|p| {
            let date = |field: &str| {
                p[field]
                    .as_str()
                    .and_then(|d| NaiveDate::parse_from_str(d, "%Y-%m-%d").ok())
            };
            Ok(Period {
                from: date("startdato")
                    .ok_or_else(|| format!("trekkpålegg {who} har en periode uten startdato"))?,
                to: date("sluttdato"),
                size: size(p),
            })
        })
        .collect::<std::result::Result<_, String>>()?;
    Ok(Order {
        id: text("trekkid")?,
        version: o["trekkversjon"]
            .as_i64()
            .and_then(|v| i32::try_from(v).ok())
            .filter(|v| *v >= 1)
            .ok_or_else(|| format!("trekkpålegg {who} mangler trekkversjon"))?,
        sequence: o["sekvensnummer"].as_i64().unwrap_or(0),
        created: o["opprettet"]
            .as_str()
            .and_then(|t| DateTime::parse_from_rfc3339(t).ok())
            .map(|t| t.with_timezone(&Utc))
            .ok_or_else(|| format!("trekkpålegg {who} mangler opprettet"))?,
        case_number: o["saksnummer"].as_str().unwrap_or("").to_string(),
        active: match status.as_str() {
            "aktiv" => true,
            "avsluttet" => false,
            other => {
                return Err(format!(
                    "trekkpålegg {who} har ukjent trekkstatus «{other}»"
                ));
            }
        },
        employer: text("trekkpliktig")?,
        debtor: text("skyldner")?,
        kid: pay_text("kidnummer")?,
        account: pay_text("kontonummer")?,
        payee: pay_text("betalingsmottaker")?,
        periods,
        raw: o.clone(),
    })
}

/// `trekkbeloep` is kroner per month and `trekkprosent` a percent, both
/// JSON numbers (`double`); read from their decimal text, never through a
/// float (§2 step 4). An explicit `null` for the alternative not used is
/// read as absent.
fn size(p: &Value) -> std::result::Result<Size, String> {
    let field = |name: &str| p.get(name).filter(|t| !t.is_null()).map(|t| &t[name]);
    let (amount, percent) = (field("trekkbeloep"), field("trekkprosent"));
    match (amount, percent) {
        (Some(a), None) => crate::tax_card::hundredths(a)
            .map(Size::AmountOre)
            .ok_or_else(|| format!("uleselig trekkbeløp {a}")),
        (None, Some(p)) => crate::tax_card::basis_points(p)
            .map(Size::PercentBp)
            .ok_or_else(|| format!("uleselig trekkprosent {p}")),
        _ => Err("perioden har ikke nøyaktig én av trekkbeløp og trekkprosent".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn documented() -> Vec<Order> {
        let list: Value = serde_json::from_str(include_str!(
            "../../../docs/trekkpalegg/liste-fra-dokumentasjonen.json"
        ))
        .unwrap();
        parse_list(&list).unwrap()
    }

    fn date(s: &str) -> NaiveDate {
        NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
    }

    #[test]
    fn the_documentations_examples_are_read() {
        let orders = documented();
        assert_eq!(orders.len(), 9);
        let o = orders.iter().find(|o| o.id == "10006").unwrap();
        assert_eq!(o.version, 3);
        assert!(o.active);
        assert_eq!(o.kid, "6487719756");
        assert_eq!(o.account, "70213997155");
        assert_eq!(o.debtor, "10108412345");
        // Kroner to øre, per period, covering the pay date.
        let size = |d: &str| o.period_on(date(d)).map(|p| p.size.clone());
        assert_eq!(size("2025-10-15"), Some(Ok(Size::AmountOre(800_000))));
        assert_eq!(size("2025-10-16"), Some(Ok(Size::AmountOre(600_000))));
        assert_eq!(size("2026-01-25"), Some(Ok(Size::AmountOre(700_000))));
        assert_eq!(size("2025-09-30"), None, "before it starts");
        let percent = orders.iter().find(|o| o.id == "10001").unwrap();
        assert_eq!(
            percent.period_on(date("2025-09-25")).unwrap().size,
            Ok(Size::PercentBp(1700))
        );
        assert!(
            !orders.iter().find(|o| o.id == "10004").unwrap().active,
            "avsluttet"
        );
    }

    #[test]
    fn sizes_never_pass_through_a_float() {
        let period = |inner: Value| size(&inner);
        let amount = |s: &str| {
            period(serde_json::json!({"trekkbeloep": {"trekkbeloep":
                serde_json::from_str::<Value>(s).unwrap()}}))
        };
        assert_eq!(amount("4400"), Ok(Size::AmountOre(440_000)));
        assert_eq!(amount("4400.5"), Ok(Size::AmountOre(440_050)));
        assert_eq!(amount("0"), Ok(Size::AmountOre(0)), "0 is allowed (§1)");
        assert!(amount("4400.123").is_err(), "never rounded");
        assert!(amount("-1").is_err());
        let percent = |s: &str| {
            period(serde_json::json!({"trekkprosent": {"trekkprosent":
                serde_json::from_str::<Value>(s).unwrap()}}))
        };
        assert_eq!(percent("22.25"), Ok(Size::PercentBp(2225)));
        assert!(percent("12.345").is_err());
        assert!(percent("101").is_err());
        assert!(period(serde_json::json!({})).is_err());
        // The alternative not used, sent as null, is absent.
        assert_eq!(
            period(serde_json::json!({"trekkbeloep": null,
                "trekkprosent": {"trekkprosent": 17}})),
            Ok(Size::PercentBp(1700))
        );
        assert_eq!(
            period(serde_json::json!({"trekkbeloep": {"trekkbeloep": 4400},
                "trekkprosent": null})),
            Ok(Size::AmountOre(440_000))
        );
        assert!(period(serde_json::json!({"trekkbeloep": null, "trekkprosent": null})).is_err());
    }

    #[test]
    fn an_order_without_its_identity_fails_the_whole_list() {
        let mut list: Value = serde_json::from_str(include_str!(
            "../../../docs/trekkpalegg/liste-fra-dokumentasjonen.json"
        ))
        .unwrap();
        list[2].as_object_mut().unwrap().remove("trekkid");
        assert!(parse_list(&list).is_err());
        assert!(parse_list(&serde_json::json!({"kode": "KB-009"})).is_err());
    }

    #[test]
    fn a_period_ended_before_it_began_covers_nothing() {
        let mut o = documented().remove(0);
        o.periods = vec![Period {
            from: date("2026-02-01"),
            to: Some(date("2026-01-31")),
            size: Ok(Size::AmountOre(100)),
        }];
        assert_eq!(o.period_on(date("2026-02-01")), None);
        assert_eq!(o.period_on(date("2026-01-31")), None);
    }
}
