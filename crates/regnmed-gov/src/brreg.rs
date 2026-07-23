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
    #[serde(default)]
    pub konkurs: bool,
    #[serde(default)]
    pub slettedato: Option<String>,
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
                "konkurs": false,
                "ignored_field": 42
            }"#,
        )
        .unwrap();
        assert_eq!(enhet.navn, "EQUINOR ASA");
        assert_eq!(enhet.organisasjonsform.unwrap().kode, "ASA");
        assert!(enhet.registrert_i_mvaregisteret);
        assert!(enhet.slettedato.is_none());
    }
}
