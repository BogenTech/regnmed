//! Brønnøysundregistrene: Enhetsregisteret lookups (open API, no auth).
//!
//! `https://data.brreg.no/enhetsregisteret/api/enheter/{orgnr}` — the
//! base URL is configurable (BRREG_API_URL) so tests run against a local
//! mock and an outage can be pointed at a mirror.

use anyhow::{Context, Result, bail};
use chrono::NaiveDate;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct BrregEnhet {
    pub organisasjonsnummer: String,
    pub navn: String,
    #[serde(default)]
    pub organisasjonsform: Option<Kode>,
    #[serde(default)]
    pub naeringskode1: Option<Kode>,
    #[serde(default, rename = "registrertIMvaregisteret")]
    pub registrert_i_mvaregisteret: bool,
    /// Registrert i Foretaksregisteret. Governs the "Foretaksregisteret"
    /// note on the salgsdokument (§5-1-2) — which covers more than
    /// AS/ASA: ANS/DA, NUF and næringsdrivende ENK register too, so it
    /// cannot be inferred from the organisasjonsform (#81).
    #[serde(default, rename = "registrertIForetaksregisteret")]
    pub registrert_i_foretaksregisteret: bool,
    /// The DATE the enhet entered each register. The flags alone are not
    /// enough: registreringsstatus is dated master data (#81), and a
    /// company that registered for mva in 2019 must not have the "MVA"
    /// note on documents dated before that.
    #[serde(default, rename = "registreringsdatoMerverdiavgiftsregisteret")]
    pub registreringsdato_mva: Option<String>,
    #[serde(default, rename = "registreringsdatoForetaksregisteret")]
    pub registreringsdato_foretaksregisteret: Option<String>,
    #[serde(default)]
    pub forretningsadresse: Option<Adresse>,
    /// Registered contact address — a better default for `company.email`
    /// than an empty field the user has to discover.
    #[serde(default)]
    pub epostadresse: Option<String>,
    #[serde(default)]
    pub kapital: Option<Kapital>,
    #[serde(default)]
    pub konkurs: bool,
    /// Voluntary liquidation, and the compulsory kind. Neither is
    /// bankruptcy, so the konkurs check misses both — but an enhet being
    /// wound up is not a going concern to onboard.
    #[serde(default, rename = "underAvvikling")]
    pub under_avvikling: bool,
    #[serde(default, rename = "underTvangsavviklingEllerTvangsopplosning")]
    pub under_tvangsavvikling: bool,
    #[serde(default)]
    pub slettedato: Option<String>,
}

/// Registered share capital. Only meaningful for AS/ASA.
#[derive(Debug, Deserialize)]
pub struct Kapital {
    #[serde(default)]
    pub belop: Option<serde_json::Number>,
    #[serde(default, rename = "antallAksjer")]
    pub antall_aksjer: Option<i64>,
    #[serde(default)]
    pub valuta: Option<String>,
}

impl Kapital {
    /// Aksjekapital in øre. The registry sends a JSON number; it is
    /// stringified and parsed DECIMALLY (the same parser the valutakurs
    /// uses) so no float ever touches money.
    pub fn belop_ore(&self) -> Option<i64> {
        // Capital in another currency is not comparable to the ledger's
        // øre, so it is not converted — better absent than wrong.
        if self.valuta.as_deref().is_some_and(|v| v != "NOK") {
            return None;
        }
        let raw = self.belop.as_ref()?.to_string();
        regnmed_core::valuta::parse_kurs(&raw).map(|micro| micro / 10_000)
    }
}

/// Enhetsregisterets adresseform: `adresse` is an array of street
/// lines, with postnummer/poststed alongside.
#[derive(Debug, Deserialize)]
pub struct Adresse {
    #[serde(default)]
    pub adresse: Vec<String>,
    #[serde(default)]
    pub postnummer: Option<String>,
    #[serde(default)]
    pub poststed: Option<String>,
}

impl Adresse {
    /// One line, in the shape the rest of regnmed stores addresses in
    /// ("Storgata 1, 0155 Oslo") — the form `regnmed_db::ehf` knows how
    /// to split again for EHF.
    pub fn en_linje(&self) -> Option<String> {
        let gate = self
            .adresse
            .iter()
            .map(|l| l.trim())
            .filter(|l| !l.is_empty())
            .collect::<Vec<_>>()
            .join(", ");
        let sted = match (&self.postnummer, &self.poststed) {
            (Some(nr), Some(sted)) => format!("{} {}", nr.trim(), sted.trim()),
            (None, Some(sted)) => sted.trim().to_string(),
            (Some(nr), None) => nr.trim().to_string(),
            (None, None) => String::new(),
        };
        match (gate.is_empty(), sted.is_empty()) {
            (true, true) => None,
            (false, true) => Some(gate),
            (true, false) => Some(sted),
            (false, false) => Some(format!("{gate}, {sted}")),
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct Kode {
    pub kode: String,
    #[serde(default)]
    pub beskrivelse: String,
}

impl BrregEnhet {
    /// The registration history as dated observations
    /// `(fra og med, mva-registrert, foretaksregistrert)`, oldest first.
    ///
    /// The two registers have INDEPENDENT dates — Equinor entered
    /// Foretaksregisteret in 1988 and Merverdiavgiftsregisteret in 1989 —
    /// so a single row cannot express both. One row per change lets a
    /// document dated in between carry exactly the påtegninger that
    /// applied then (#81).
    ///
    /// `i_dag` is the fallback for a register the enhet is in but has no
    /// date for, and for the "registered in neither" record.
    pub fn registreringstidslinje(&self, i_dag: NaiveDate) -> Vec<(NaiveDate, bool, bool)> {
        fn dato(raw: &Option<String>) -> Option<NaiveDate> {
            NaiveDate::parse_from_str(raw.as_deref()?.trim(), "%Y-%m-%d").ok()
        }
        let mut hendelser: Vec<(NaiveDate, bool)> = Vec::new();
        if self.registrert_i_mvaregisteret {
            hendelser.push((dato(&self.registreringsdato_mva).unwrap_or(i_dag), true));
        }
        if self.registrert_i_foretaksregisteret {
            hendelser.push((
                dato(&self.registreringsdato_foretaksregisteret).unwrap_or(i_dag),
                false,
            ));
        }
        if hendelser.is_empty() {
            // Registered in neither: still recorded, so the absence of a
            // påtegning is an observation rather than a missing row.
            return vec![(i_dag, false, false)];
        }
        hendelser.sort_by_key(|(d, _)| *d);
        let (mut mva, mut foretak) = (false, false);
        let mut rader: Vec<(NaiveDate, bool, bool)> = Vec::new();
        for (d, er_mva) in hendelser {
            if er_mva {
                mva = true;
            } else {
                foretak = true;
            }
            match rader.last_mut() {
                // Both registers on the same day collapse into one row.
                Some(siste) if siste.0 == d => *siste = (d, mva, foretak),
                _ => rader.push((d, mva, foretak)),
            }
        }
        rader
    }
}

pub struct BrregClient {
    base_url: String,
    http: reqwest::Client,
}

impl BrregClient {
    pub fn from_env() -> Self {
        Self {
            base_url: std::env::var("BRREG_API_URL")
                .unwrap_or_else(|_| "https://data.brreg.no/enhetsregisteret/api".into()),
            http: reqwest::Client::new(),
        }
    }

    /// Looks up an enhet; `Ok(None)` when the orgnr is unknown to the
    /// register.
    pub async fn enhet(&self, orgnr: &str) -> Result<Option<BrregEnhet>> {
        let url = format!("{}/enheter/{orgnr}", self.base_url.trim_end_matches('/'));
        let response = self
            .http
            .get(&url)
            .header("accept", "application/json")
            .send()
            .await
            .context("Enhetsregisteret unreachable")?;
        if response.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(None);
        }
        if !response.status().is_success() {
            bail!("Enhetsregisteret returned {}", response.status());
        }
        Ok(Some(
            response
                .json()
                .await
                .context("parsing Enhetsregisteret response")?,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_registry_shape() {
        let enhet: BrregEnhet = serde_json::from_str(
            r#"{
                "organisasjonsnummer": "923609016",
                "navn": "EQUINOR ASA",
                "organisasjonsform": {"kode": "ASA", "beskrivelse": "Allmennaksjeselskap"},
                "naeringskode1": {"kode": "06.100", "beskrivelse": "Utvinning av råolje"},
                "registrertIMvaregisteret": true,
                "registrertIForetaksregisteret": true,
                "forretningsadresse": {
                    "land": "Norge",
                    "landkode": "NO",
                    "postnummer": "3152",
                    "poststed": "TOLVSRØD",
                    "adresse": ["Øvre Bogenvei 67"]
                },
                "konkurs": false,
                "ignored_field": 42
            }"#,
        )
        .unwrap();
        assert_eq!(enhet.navn, "EQUINOR ASA");
        assert_eq!(enhet.organisasjonsform.unwrap().kode, "ASA");
        assert!(enhet.registrert_i_mvaregisteret);
        assert!(enhet.registrert_i_foretaksregisteret);
        assert_eq!(
            enhet.forretningsadresse.unwrap().en_linje().unwrap(),
            "Øvre Bogenvei 67, 3152 TOLVSRØD"
        );
        assert!(enhet.slettedato.is_none());
    }

    fn enhet(json: &str) -> BrregEnhet {
        serde_json::from_str(json).unwrap()
    }

    fn d(s: &str) -> NaiveDate {
        NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
    }

    /// The two registers have independent dates, so the timeline has one
    /// row per change — a document dated between them must carry only
    /// the påtegning that applied then (#81).
    #[test]
    fn the_registration_timeline_follows_each_register_own_date() {
        let e = enhet(
            r#"{
                "organisasjonsnummer": "923609016", "navn": "EQUINOR ASA",
                "registrertIMvaregisteret": true,
                "registrertIForetaksregisteret": true,
                "registreringsdatoMerverdiavgiftsregisteret": "1989-07-01",
                "registreringsdatoForetaksregisteret": "1988-04-28"
            }"#,
        );
        let rader = e.registreringstidslinje(d("2026-08-07"));
        assert_eq!(
            rader,
            vec![
                (d("1988-04-28"), false, true),
                (d("1989-07-01"), true, true),
            ]
        );
    }

    /// Registered in both on the same day: one row, not two.
    #[test]
    fn same_day_registrations_collapse_into_one_row() {
        let e = enhet(
            r#"{
                "organisasjonsnummer": "935115086", "navn": "X AS",
                "registrertIMvaregisteret": true,
                "registrertIForetaksregisteret": true,
                "registreringsdatoMerverdiavgiftsregisteret": "2025-02-26",
                "registreringsdatoForetaksregisteret": "2025-02-26"
            }"#,
        );
        assert_eq!(
            e.registreringstidslinje(d("2026-08-07")),
            vec![(d("2025-02-26"), true, true)]
        );
    }

    /// Registered in neither is still an OBSERVATION — the row records
    /// that we asked and the answer was no, dated today.
    #[test]
    fn no_registrations_still_yields_a_dated_record() {
        let e = enhet(r#"{"organisasjonsnummer": "1", "navn": "ENK"}"#);
        assert_eq!(
            e.registreringstidslinje(d("2026-08-07")),
            vec![(d("2026-08-07"), false, false)]
        );
    }

    /// In a register but without a date (older records): fall back to
    /// today rather than dropping the registration.
    #[test]
    fn a_registration_without_a_date_falls_back_to_today() {
        let e = enhet(
            r#"{"organisasjonsnummer": "1", "navn": "Y AS",
                "registrertIForetaksregisteret": true}"#,
        );
        assert_eq!(
            e.registreringstidslinje(d("2026-08-07")),
            vec![(d("2026-08-07"), false, true)]
        );
    }

    /// Aksjekapital arrives as a JSON number and must reach the ledger as
    /// integer øre without a float in between.
    #[test]
    fn share_capital_becomes_integer_ore() {
        let e = enhet(
            r#"{"organisasjonsnummer": "1", "navn": "Z AS",
                "kapital": {"belop": 30000.0, "antallAksjer": 1000,
                            "type": "Aksjekapital", "valuta": "NOK"}}"#,
        );
        let k = e.kapital.unwrap();
        assert_eq!(k.belop_ore(), Some(3_000_000));
        assert_eq!(k.antall_aksjer, Some(1000));
    }

    /// Capital in another currency is not comparable to the ledger's øre
    /// and is left absent rather than silently treated as kroner.
    #[test]
    fn foreign_currency_capital_is_not_converted() {
        let e = enhet(
            r#"{"organisasjonsnummer": "1", "navn": "Æ AS",
                "kapital": {"belop": 25000.0, "valuta": "EUR"}}"#,
        );
        assert_eq!(e.kapital.unwrap().belop_ore(), None);
    }

    /// An enhet without a registered address must not become the string
    /// ", " — the salgsdokument would carry a comma where the seller's
    /// address should be.
    #[test]
    fn a_missing_address_is_none_not_an_empty_line() {
        let tom = Adresse {
            adresse: vec![String::new()],
            postnummer: None,
            poststed: None,
        };
        assert!(tom.en_linje().is_none());
        let bare_sted = Adresse {
            adresse: vec![],
            postnummer: Some("0155".into()),
            poststed: Some("Oslo".into()),
        };
        assert_eq!(bare_sted.en_linje().unwrap(), "0155 Oslo");
    }
}
