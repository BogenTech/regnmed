//! Brønnøysundregistrene: Enhetsregisteret lookups (open API, no auth).
//!
//! `https://data.brreg.no/enhetsregisteret/api/enheter/{orgnr}` — the
//! base URL is configurable (BRREG_API_URL) so tests run against a local
//! mock and an outage can be pointed at a mirror.

use anyhow::{Context, Result, bail};
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
    #[serde(default)]
    pub forretningsadresse: Option<Adresse>,
    #[serde(default)]
    pub konkurs: bool,
    #[serde(default)]
    pub slettedato: Option<String>,
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
