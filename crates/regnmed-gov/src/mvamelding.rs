//! Skatteetaten's mva-melding validation API.
//!
//! The melding XML (built by `regnmed_core::mvamelding`) is POSTed to the
//! validation endpoint before submission; Skatteetaten replies with a
//! `valideringsresultat` naming every avvik.
//!
//! **This API wants an ID-porten token, not a Maskinporten one** —
//! established empirically 2026-08-10, and the reason this client cannot
//! complete a round-trip yet. Holding the Maskinporten scope
//! `skatteetaten:mvameldingvalidering` is not enough: Digdir mints it
//! happily (Skatteetaten granted it to us), but the API rejects the token
//! type with `IDPORTEN_TOKEN_FEIL_FORMAT` / 403. Skatteetaten's own
//! documentation agrees — validation and submission list the scopes
//! alongside `openid`, which only a user login carries, and Maskinporten
//! support is described as a later ambition. So this endpoint is
//! downstream of ID-porten federation (issue #26), exactly like the
//! skattemelding (docs/skattemelding.md). Details in docs/gov.md.
//!
//! The bearer token below therefore stays as-is: the shape of the call is
//! right and the token source is what has to change, once #26 gives us
//! one. Endpoints and the dead documented host: docs/gov.md.

use anyhow::{Context, Result};

use crate::maskinporten::TokenProvider;

pub struct MvaMeldingClient {
    /// Full URL of the validation endpoint.
    pub validation_url: String,
    http: reqwest::Client,
}

#[derive(Debug)]
pub struct ValidationOutcome {
    /// HTTP-level success and no avvik in the result document.
    pub valid: bool,
    /// The raw `valideringsresultat` XML, stored alongside the melding as
    /// documentation of the control.
    pub result_xml: String,
    /// Extracted `<avvik>`/`<sjekk>` texts, best effort.
    pub findings: Vec<String>,
}

impl MvaMeldingClient {
    pub fn new(validation_url: impl Into<String>) -> Self {
        Self {
            validation_url: validation_url.into(),
            http: reqwest::Client::new(),
        }
    }

    /// Reads MVA_VALIDATION_URL (falls back to the test environment host
    /// that actually answers).
    ///
    /// Not the host Skatteetaten documents: `idporten-api-sbstest.sits.no`
    /// completes a TLS handshake and then resets every request, on every
    /// path, from every network tried (2026-08-10) — it looks
    /// decommissioned. `api-test.sits.no` serves the same path and returns
    /// real, structured API errors, so a misconfiguration now surfaces as
    /// a diagnosable response instead of a bare connection reset.
    pub fn from_env() -> Self {
        Self::new(std::env::var("MVA_VALIDATION_URL").unwrap_or_else(|_| {
            "https://api-test.sits.no/api/mva/grensesnittstoette/mva-melding/valider".to_string()
        }))
    }

    pub async fn validate(
        &self,
        tokens: &TokenProvider,
        melding_xml: &str,
    ) -> Result<ValidationOutcome> {
        let token = tokens.token().await?;
        let response = self
            .http
            .post(&self.validation_url)
            .bearer_auth(token)
            .header("content-type", "application/xml")
            .body(melding_xml.to_string())
            .send()
            .await
            .context("calling mva-melding validation API")?;

        let status = response.status();
        let result_xml = response.text().await.unwrap_or_default();
        if !status.is_success() {
            anyhow::bail!("validation API returned {status}: {result_xml}");
        }

        let findings = extract_findings(&result_xml);
        Ok(ValidationOutcome {
            valid: findings.is_empty(),
            result_xml,
            findings,
        })
    }
}

/// Pulls avvik descriptions out of a valideringsresultat without a full
/// XML parser: robust enough for reporting, while the raw XML is kept.
fn extract_findings(xml: &str) -> Vec<String> {
    let mut findings = Vec::new();
    for tag in ["avvik", "sjekkResultat"] {
        let open = format!("<{tag}>");
        let close = format!("</{tag}>");
        let mut rest = xml;
        while let Some(start) = rest.find(&open) {
            let Some(end) = rest[start..].find(&close) else {
                break;
            };
            let block = &rest[start..start + end];
            // The human-readable text lives in <beskrivelse> or the block itself.
            let text = block
                .split_once("<beskrivelse>")
                .and_then(|(_, tail)| tail.split_once("</beskrivelse>"))
                .map(|(text, _)| text.to_string())
                .unwrap_or_else(|| block[open.len()..].trim().to_string());
            findings.push(text);
            rest = &rest[start + end + close.len()..];
        }
    }
    findings
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_avvik_descriptions() {
        let xml = r#"<valideringsresultat>
            <avvik><avviksnavn>x</avviksnavn><beskrivelse>fastsattMerverdiavgift stemmer ikke</beskrivelse></avvik>
            <avvik><beskrivelse>ugyldig sats</beskrivelse></avvik>
        </valideringsresultat>"#;
        let findings = extract_findings(xml);
        assert_eq!(findings.len(), 2);
        assert!(findings[0].contains("stemmer ikke"));
        assert!(findings[1].contains("ugyldig sats"));
        assert!(extract_findings("<valideringsresultat/>").is_empty());
    }
}
