//! Skatteetaten's mva-melding validation API.
//!
//! The melding XML (built by `regnmed_core::mvamelding`) is POSTed to the
//! validation endpoint before submission; Skatteetaten replies with a
//! `valideringsresultat` naming every avvik. Validation requires a
//! Maskinporten token with scope `skatteetaten:mvameldingvalidering`.
//!
//! Endpoints (see docs/gov.md): test environment
//! `https://idporten-api-sbstest.sits.no/api/mva/grensesnittstoette/mva-melding/valider`.
//! The Altinn3 submission (instance) flow is implemented once real test
//! credentials exist — validation is the required first gate anyway.

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

    /// Reads MVA_VALIDATION_URL (falls back to Skatteetaten's documented
    /// test environment endpoint).
    pub fn from_env() -> Self {
        Self::new(std::env::var("MVA_VALIDATION_URL").unwrap_or_else(|_| {
            "https://idporten-api-sbstest.sits.no/api/mva/grensesnittstoette/mva-melding/valider"
                .to_string()
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
