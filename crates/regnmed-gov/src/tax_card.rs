//! Skatteetatens API «Skattekort til arbeidsgiver» (docs/lonn.md, #46).
//! OpenAPI 3.0.1 on SwaggerHub (`skatteetaten/skattekort-til-arbeidsgiver`).
//!
//! Two calls, both with the **systembruker token for the employer**:
//!
//! 1. `POST {base}/bestillSkattekort?idempotensid=` -- orders the cards
//!    (JSON). Answers `{bestillingsreferanse: "BR…", dialogreferanse}`.
//! 2. `GET {base}/skattekortTilArbeidsgiver/svar/{bestillingsreferanse}` --
//!    the answer, or 204 while it is not ready. In test it was ready on
//!    the first poll, 3 s after ordering (2026-11-12).
//!
//! [`parse_answer`] and [`decide`] read the answer: what each card means
//! for the employee's withholding. Field values are as the test
//! environment sent them 2026-11-12 (docs/lonn/skattekort-svar-test-2026.json)
//! -- camelCase, although the documentation's own example spells them in
//! capitals. regnmed pays ordinary salary as the main employer, so
//! `loennFraHovedarbeidsgiver` governs; pensjon, NAV and biarbeidsgiver are
//! stored with the card but not applied.
//!
//! Only what regnmed's payroll computes correctly is applied. Everything
//! else is KEPT, with the reason, so a card is never silently wrong:
//! `regnmed_core::payroll::calculate` halves December and leaves feriepenger
//! without trekk, which is right for a card spread over 10,5 months and
//! wrong for any other.

use anyhow::{Context, Result, bail};
use chrono::NaiveDate;
use regnmed_core::payroll::Withholding;
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;

use crate::maskinporten::TokenProvider;

pub const SCOPES: &str = "skatteetaten:skattekorttilarbeidsgiver";

/// At most this many employees per order (the API's own limit).
pub const MAX_PER_ORDER: usize = 1000;

pub struct TaxCardClient {
    /// `https://api-test.sits.no/api/forskudd` in test.
    pub base_url: String,
    http: reqwest::Client,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct Order {
    #[serde(rename = "bestillingsreferanse")]
    pub reference: String,
    #[serde(rename = "dialogreferanse")]
    pub dialog_reference: String,
}

impl TaxCardClient {
    pub fn new(base_url: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into().trim_end_matches('/').to_string(),
            // A hung Skatteetaten must not hold the request until the
            // ingress gives up -- the user would retry and order twice.
            http: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(30))
                .build()
                .expect("a TLS backend"),
        }
    }

    /// Reads SKATTEKORT_URL, defaulting to the TEST environment --
    /// production is opted into, never fallen into.
    pub fn from_env() -> Self {
        Self::new(
            std::env::var("SKATTEKORT_URL")
                .unwrap_or_else(|_| "https://api-test.sits.no/api/forskudd".into()),
        )
    }

    /// Orders every card for these employees (`HENT_ALLE_OPPGITTE`).
    /// `contact` is the person Skatteetaten may contact about the order.
    pub async fn order(
        &self,
        tokens: &TokenProvider,
        orgnumber: &str,
        income_year: i32,
        nins: &[String],
        contact_email: &str,
        idempotency_key: Uuid,
    ) -> Result<Order> {
        anyhow::ensure!(
            !nins.is_empty() && nins.len() <= MAX_PER_ORDER,
            "an order carries 1..={MAX_PER_ORDER} employees, not {}",
            nins.len()
        );
        let token = tokens.token_for_systemuser(orgnumber).await?;
        let body = json!({
            "inntektsaar": income_year,
            "bestillingstype": "HENT_ALLE_OPPGITTE",
            "kontaktinformasjon": { "epostadresse": contact_email },
            "varslingstype": "INGEN_VARSEL",
            "forespoerselOmSkattekortTilArbeidsgiver": { "arbeidsgiver": [{
                "arbeidsgiveridentifikator": { "organisasjonsnummer": orgnumber },
                "arbeidstakeridentifikator": nins,
            }]},
        });
        let response = self
            .http
            .post(format!(
                "{}/bestillSkattekort?idempotensid={idempotency_key}",
                self.base_url
            ))
            .bearer_auth(token)
            .header("accept", "application/json")
            .json(&body)
            .send()
            .await
            .context("ordering skattekort")?;
        let status = response.status();
        let text = response.text().await.unwrap_or_default();
        if !status.is_success() {
            // Skatteetatens feilkode (FOR_001 ...) and feilmelding, verbatim.
            bail!("{status}: {text}");
        }
        serde_json::from_str(&text).with_context(|| format!("reading the order receipt {text}"))
    }

    /// The answer to an order, or `None` while Skatteetaten is still
    /// producing it.
    pub async fn answer(
        &self,
        tokens: &TokenProvider,
        orgnumber: &str,
        reference: &str,
    ) -> Result<Option<serde_json::Value>> {
        let token = tokens.token_for_systemuser(orgnumber).await?;
        let response = self
            .http
            .get(format!(
                "{}/skattekortTilArbeidsgiver/svar/{reference}",
                self.base_url
            ))
            .bearer_auth(token)
            .header("accept", "application/json")
            .send()
            .await
            .context("fetching the skattekort answer")?;
        let status = response.status();
        if status == reqwest::StatusCode::NO_CONTENT {
            return Ok(None);
        }
        let text = response.text().await.unwrap_or_default();
        if !status.is_success() {
            bail!("{status}: {text}");
        }
        Ok(Some(serde_json::from_str(&text).with_context(|| {
            format!("reading the skattekort answer {text}")
        })?))
    }
}

/// The trekkode regnmed applies.
pub const MAIN_EMPLOYER: &str = "loennFraHovedarbeidsgiver";

/// One employee's answer, as Skatteetaten sent it.
#[derive(Debug, Clone, PartialEq)]
pub struct TaxCard {
    pub nin: String,
    /// `resultatForSkattekort`, e.g. `skattekortopplysningerOK`.
    pub result: String,
    pub card_id: Option<i64>,
    pub issued: Option<NaiveDate>,
    /// The employee's whole entry, verbatim -- the evidence.
    pub raw: Value,
}

/// What the card means for the employee's withholding in regnmed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    Apply(Withholding),
    /// Not applied; the employee keeps what they had. Norwegian, for the
    /// person who must act on it.
    Keep(String),
}

/// The employees in an answer. `Err` carries Skatteetaten's status when
/// the order itself failed (`UGYLDIG_INNTEKTSAAR`, ...).
pub fn parse_answer(answer: &Value) -> Result<Vec<TaxCard>, String> {
    let status = answer["status"].as_str().unwrap_or("FORESPOERSEL_OK");
    if status != "FORESPOERSEL_OK" {
        return Err(status.to_string());
    }
    let mut cards = Vec::new();
    for employer in answer["arbeidsgiver"].as_array().into_iter().flatten() {
        for e in employer["arbeidstaker"].as_array().into_iter().flatten() {
            let Some(nin) = e["arbeidstakeridentifikator"].as_str() else {
                continue;
            };
            let card = &e["skattekort"];
            cards.push(TaxCard {
                nin: nin.to_string(),
                result: e["resultatForSkattekort"]
                    .as_str()
                    .unwrap_or("")
                    .to_string(),
                // The documentation's example sends the id as a string too.
                card_id: card["skattekortidentifikator"]
                    .as_i64()
                    .or_else(|| card["skattekortidentifikator"].as_str()?.parse().ok()),
                issued: card["utstedtDato"]
                    .as_str()
                    .and_then(|d| NaiveDate::parse_from_str(d, "%Y-%m-%d").ok()),
                raw: e.clone(),
            });
        }
    }
    Ok(cards)
}

pub fn decide(card: &TaxCard) -> Decision {
    match card.result.as_str() {
        "skattekortopplysningerOK" => {}
        "ikkeTrekkplikt" => return Decision::Apply(Withholding::None),
        "ikkeSkattekort" => {
            return Decision::Keep(
                "Skatteetaten har ikke skattekort for den ansatte. Uten skattekort skal \
                 arbeidsgiver trekke 50 % — registrer trekket for hånd og hent kortet på nytt \
                 senere."
                    .into(),
            );
        }
        other => return Decision::Keep(format!("Skatteetaten svarte «{other}».")),
    }
    let Some(trekk) = card.raw["skattekort"]["forskuddstrekk"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|t| t["trekkode"] == MAIN_EMPLOYER)
    else {
        return Decision::Keep(
            "Skattekortet har ikke trekk for lønn fra hovedarbeidsgiver.".into(),
        );
    };

    if let Some(frikort) = trekk.get("frikort") {
        return match frikort.get("frikortbeloep") {
            None => Decision::Apply(Withholding::None),
            Some(limit) => Decision::Keep(format!(
                "Frikort med beløpsgrense ({limit} kr): over grensen skal det trekkes 50 %, og \
                 det regner ikke regnmed ut ennå — registrer trekket for hånd."
            )),
        };
    }
    if let Some(table) = trekk.get("trekktabell") {
        if let Err(keep) = ten_and_a_half_months(table) {
            return keep;
        }
        return match table["tabellnummer"]
            .as_str()
            .and_then(|n| n.parse().ok())
            .or_else(|| table["tabellnummer"].as_i64().map(|n| n as i32))
        {
            Some(number) => Decision::Apply(Withholding::Table(number)),
            None => Decision::Keep("Skattekortet har en trekktabell uten tabellnummer.".into()),
        };
    }
    if let Some(percent) = trekk.get("trekkprosent") {
        if let Err(keep) = ten_and_a_half_months(percent) {
            return keep;
        }
        return match basis_points(&percent["prosentsats"]) {
            Some(bp) => Decision::Apply(Withholding::Percent(bp)),
            None => Decision::Keep(format!("Uleselig trekkprosent: {}", percent["prosentsats"])),
        };
    }
    Decision::Keep("Skattekortet har en trekkform regnmed ikke kjenner.".into())
}

fn ten_and_a_half_months(trekk: &Value) -> Result<(), Decision> {
    match trekk["antallMaanederForTrekk"].to_string().as_str() {
        // Absent: Skatteetaten leaves it out for the ordinary case.
        "null" | "10.5" => Ok(()),
        months => Err(Decision::Keep(format!(
            "Skattekortet fordeler trekket over {months} måneder (f.eks. kildeskatt). regnmed \
             trekker halvt i desember og ikke i feriepenger, som bare stemmer for 10,5 måneder \
             — registrer trekket for hånd."
        ))),
    }
}

/// `38.0` -> 3800, `22.25` -> 2225, read from the number's decimal text so
/// no float is ever involved. More than two decimals is refused rather
/// than rounded.
fn basis_points(n: &Value) -> Option<i64> {
    let text = n.as_number()?.to_string();
    let (whole, fraction) = text.split_once('.').unwrap_or((&text, ""));
    if fraction.len() > 2 || whole.starts_with('-') {
        return None;
    }
    let bp = whole.parse::<i64>().ok()? * 100 + format!("{fraction:0<2}").parse::<i64>().ok()?;
    (bp <= 10_000).then_some(bp)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn real() -> Vec<TaxCard> {
        let answer: Value = serde_json::from_str(include_str!(
            "../../../docs/lonn/skattekort-svar-test-2026.json"
        ))
        .unwrap();
        parse_answer(&answer).unwrap()
    }

    fn decision(nin: &str) -> Decision {
        decide(real().iter().find(|c| c.nin == nin).unwrap())
    }

    #[test]
    fn the_real_answer_from_the_test_environment() {
        let cards = real();
        assert_eq!(cards.len(), 8);
        let pct = cards.iter().find(|c| c.nin == "15929998666").unwrap();
        assert_eq!(pct.card_id, Some(326999));
        assert_eq!(pct.issued, NaiveDate::from_ymd_opt(2026, 11, 11));

        // Prosent lønn, 10,5 måneder.
        assert_eq!(
            decision("15929998666"),
            Decision::Apply(Withholding::Percent(3800))
        );
        assert_eq!(
            decision("26854497889"),
            Decision::Apply(Withholding::Percent(2250))
        );
        // Tabell lønn: stored as the card says; payroll refuses it loudly.
        assert_eq!(
            decision("20857597940"),
            Decision::Apply(Withholding::Table(9040))
        );
        // Frikort uten beløpsgrense, but only for pensjon: nothing for lønn.
        assert!(
            matches!(decision("11824498484"), Decision::Keep(r) if r.contains("hovedarbeidsgiver"))
        );
        // Frikort med beløpsgrense.
        assert!(matches!(decision("01810199403"), Decision::Keep(r) if r.contains("beløpsgrense")));
        // Kildeskatt over 12 måneder.
        assert!(matches!(decision("05850298728"), Decision::Keep(r) if r.contains("12 måneder")));
        // No card: the 50 % rule is named, never applied.
        assert!(matches!(decision("19910298057"), Decision::Keep(r) if r.contains("50 %")));
    }

    #[test]
    fn frikort_without_limit_means_no_withholding() {
        let card = TaxCard {
            nin: "1".into(),
            result: "skattekortopplysningerOK".into(),
            card_id: None,
            issued: None,
            raw: serde_json::json!({"skattekort": {"forskuddstrekk": [
                {"trekkode": MAIN_EMPLOYER, "frikort": {}}
            ]}}),
        };
        assert_eq!(decide(&card), Decision::Apply(Withholding::None));
    }

    #[test]
    fn percentages_never_pass_through_a_float() {
        let bp = |s: &str| basis_points(&serde_json::from_str(s).unwrap());
        assert_eq!(bp("38"), Some(3800));
        assert_eq!(bp("22.2"), Some(2220));
        assert_eq!(bp("0.05"), Some(5));
        assert_eq!(bp("12.345"), None);
        assert_eq!(bp("101"), None);
    }

    #[test]
    fn a_failed_order_is_an_error() {
        let answer = serde_json::json!({"status": "UGYLDIG_INNTEKTSAAR"});
        assert_eq!(parse_answer(&answer), Err("UGYLDIG_INNTEKTSAAR".into()));
    }
}
