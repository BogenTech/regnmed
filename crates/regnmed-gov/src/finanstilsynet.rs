//! Finanstilsynets virksomhetsregister: does an orgnr hold an active
//! autorisasjon as regnskapsførerselskap or revisjonsselskap?
//!
//! The register is public but its API endpoint is not stably documented,
//! so this adapter is deliberately thin and fully configurable
//! (FINANSTILSYNET_API_URL): it expects
//! `GET {base}/virksomheter/{orgnr}` returning
//! `{"autorisasjoner": [{"kode": "…", "aktiv": true}]}` with codes
//! containing "regnskap" or "revisj". The URL and field mapping get
//! pinned against the live register during pilot onboarding (see
//! docs/marketplace.md); until then the enforcement point, flow and
//! tests are real, and the adapter is the only thing that may move.
//! **Without a configured URL the check fails closed** — nobody becomes
//! a verified firm because a register was unreachable.

use anyhow::{Context, Result, bail};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct Business {
    #[serde(default)]
    autorisasjoner: Vec<Authorisation>,
}

#[derive(Debug, Deserialize)]
struct Authorisation {
    // Finanstilsynet's own field name; an external schema is not ours
    // to translate, and not a reason to name our fields after it.
    #[serde(rename = "kode")]
    code: String,
    #[serde(default, rename = "aktiv")]
    active: bool,
}

pub struct FinanstilsynetClient {
    base_url: Option<String>,
    http: reqwest::Client,
}

impl FinanstilsynetClient {
    pub fn from_env() -> Self {
        Self {
            base_url: std::env::var("FINANSTILSYNET_API_URL").ok(),
            http: reqwest::Client::new(),
        }
    }

    /// True when the orgnr holds an active autorisasjon of the kind
    /// ('regnskap' or 'revisjon'). Fails closed: no configured register,
    /// unreachable register, or unknown orgnr all mean "not verified".
    pub async fn has_authorisation(&self, orgnumber: &str, kind: &str) -> Result<bool> {
        let Some(base) = &self.base_url else {
            bail!(
                "FINANSTILSYNET_API_URL is not configured — autorisasjon \
                 cannot be verified (see docs/marketplace.md)"
            );
        };
        let url = format!("{}/virksomheter/{orgnumber}", base.trim_end_matches('/'));
        let response = self
            .http
            .get(&url)
            .header("accept", "application/json")
            .send()
            .await
            .context("Finanstilsynets register unreachable")?;
        if response.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(false);
        }
        if !response.status().is_success() {
            bail!("Finanstilsynets register returned {}", response.status());
        }
        let virksomhet: Business = response
            .json()
            .await
            .context("parsing Finanstilsynet response")?;
        let needle = match kind {
            "regnskap" => "regnskap",
            "revisjon" => "revisj",
            other => bail!("unknown autorisasjon kind '{other}'"),
        };
        Ok(virksomhet
            .autorisasjoner
            .iter()
            .any(|a| a.active && a.code.to_lowercase().contains(needle)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_matches_kinds() {
        let virksomhet: Business = serde_json::from_str(
            r#"{"autorisasjoner": [
                {"kode": "Regnskapsforerselskap", "aktiv": true},
                {"kode": "Eiendomsmegling", "aktiv": true},
                {"kode": "Revisjonsselskap", "aktiv": false}
            ]}"#,
        )
        .unwrap();
        let active_accounting = virksomhet
            .autorisasjoner
            .iter()
            .any(|a| a.active && a.code.to_lowercase().contains("regnskap"));
        let active_revisjon = virksomhet
            .autorisasjoner
            .iter()
            .any(|a| a.active && a.code.to_lowercase().contains("revisj"));
        assert!(active_accounting);
        assert!(!active_revisjon, "inactive licenses do not count");
    }
}
