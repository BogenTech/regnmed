//! Skatteetaten's mva-melding validation API.
//!
//! The melding XML (built by `regnmed_core::mvamelding`) is POSTed to the
//! validation endpoint before submission; Skatteetaten replies with a
//! `valideringsresultat` naming every deviation.
//!
//! **Validation wants the ID-porten token of a logged-in person, and
//! submission an Altinn token exchanged from it.** Maskinporten is not
//! accepted: the VAT rules require knowing which person filed (docs/gov.md).
//! Who signs in, and when, is `regnmed-db::vat_filing`; this file only moves
//! the documents.

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::Deserialize;
use std::time::Duration;

use crate::maskinporten::TokenProvider;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const TOTAL_TIMEOUT: Duration = Duration::from_secs(60);

/// A call that hangs would hold the worker's slot and the filing's lock.
fn filing_http() -> reqwest::Client {
    http_with(CONNECT_TIMEOUT, TOTAL_TIMEOUT)
}

fn http_with(connect: Duration, total: Duration) -> reqwest::Client {
    reqwest::Client::builder()
        .connect_timeout(connect)
        .timeout(total)
        .build()
        .expect("a TLS backend")
}

pub struct VatReturnClient {
    /// Full URL of the validation endpoint.
    pub validation_url: String,
    http: reqwest::Client,
}

#[derive(Debug)]
pub struct ValidationOutcome {
    /// HTTP-level success and no deviation in the result document.
    pub valid: bool,
    /// The raw `valideringsresultat` XML, stored alongside the melding as
    /// documentation of the control.
    pub result_xml: String,
    /// Extracted `<avvik>`/`<sjekk>` texts, best effort.
    pub findings: Vec<String>,
}

impl VatReturnClient {
    pub fn new(validation_url: impl Into<String>) -> Self {
        Self {
            validation_url: validation_url.into(),
            http: filing_http(),
        }
    }

    /// Reads MVA_VALIDATION_URL (falls back to the test environment).
    ///
    /// Two hosts, one letter apart, and only one of them is real:
    /// `idporten-api-TEST.sits.no` is the current test endpoint (from
    /// Skatteetaten's api-dokumentasjon catalogue), while
    /// `idporten-api-SBSTEST.sits.no` — still printed on the older
    /// mva-meldingen documentation site — accepts a TLS handshake and
    /// then resets every request, on every path. Trust the catalogue.
    pub fn from_env() -> Self {
        Self::new(std::env::var("MVA_VALIDATION_URL").unwrap_or_else(|_| {
            "https://idporten-api-test.sits.no/api/mva/grensesnittstoette/mva-melding/valider"
                .to_string()
        }))
    }

    pub async fn validate(
        &self,
        tokens: &TokenProvider,
        melding_xml: &str,
    ) -> Result<ValidationOutcome> {
        self.validate_with_token(&tokens.token().await?, melding_xml)
            .await
    }

    /// Validates with the person's ID-porten token.
    pub async fn validate_with_token(
        &self,
        token: &str,
        melding_xml: &str,
    ) -> Result<ValidationOutcome> {
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
        let result_xml = response
            .text()
            .await
            .context("reading the validation answer")?;
        if !status.is_success() {
            anyhow::bail!("validation API returned {status}: {result_xml}");
        }

        let findings = extract_findings(&result_xml);
        // An answer we cannot read is never a clean melding.
        anyhow::ensure!(
            !findings.is_empty() || is_clean_result(&result_xml),
            "the validation API answered with something that is not a valideringsresultat"
        );
        Ok(ValidationOutcome {
            valid: findings.is_empty(),
            result_xml,
            findings,
        })
    }
}

/// Why a failed call failed, because the two answers permit different things.
#[derive(Debug)]
pub enum FilingError {
    /// Altinn answered and refused: nothing moved.
    Refused { status: u16, body: String },
    /// No clear answer: the call may or may not have taken effect.
    Unresolved(String),
}

impl std::fmt::Display for FilingError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Refused { status, body } => write!(f, "Altinn refused ({status}): {body}"),
            Self::Unresolved(e) => write!(f, "no clear answer from Altinn: {e}"),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Instance {
    pub id: String,
    #[serde(default)]
    pub data: Vec<DataElement>,
    pub process: Option<InstanceProcess>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DataElement {
    pub id: String,
    pub data_type: String,
    #[serde(default)]
    pub content_type: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstanceProcess {
    pub current_task: Option<InstanceTask>,
    #[serde(default)]
    pub ended: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstanceTask {
    pub element_id: String,
    #[serde(default)]
    pub name: Option<String>,
}

impl Instance {
    pub fn task(&self) -> Option<&str> {
        self.process
            .as_ref()?
            .current_task
            .as_ref()
            .map(|t| t.element_id.as_str())
    }
    /// Filed, by the only two states Skatteetaten documents for it: waiting
    /// in `Tilbakemelding`, or a process that has ended.
    pub fn is_submitted(&self) -> bool {
        let Some(process) = &self.process else {
            return false;
        };
        match &process.current_task {
            Some(task) => task.name.as_deref() == Some("Tilbakemelding"),
            None => process.ended.is_some(),
        }
    }
    pub fn element(&self, data_type: &str) -> Option<&DataElement> {
        self.data.iter().find(|d| d.data_type == data_type)
    }
}

pub const ENVELOPE_DATA_TYPE: &str =
    "no.skatteetaten.fastsetting.avgift.mva.mvameldinginnsending.v1.0";
pub const MELDING_DATA_TYPE: &str = "mvamelding";

/// Altinn's mva-melding app, reached with the person's exchanged token.
pub struct VatFilingClient {
    pub validation: VatReturnClient,
    /// `https://platform.tt02.altinn.no` in test.
    pub platform_url: String,
    /// `.../skd/mva-melding-innsending-test/instances` in test.
    pub instances_url: String,
    http: reqwest::Client,
}

impl VatFilingClient {
    pub fn new(
        validation: VatReturnClient,
        platform_url: impl Into<String>,
        instances_url: impl Into<String>,
    ) -> Result<Self> {
        Ok(Self {
            validation,
            platform_url: platform_url.into().trim_end_matches('/').to_string(),
            instances_url: instances_url.into().trim_end_matches('/').to_string(),
            http: filing_http(),
        })
    }

    pub fn instance_url(&self, instance_id: &str) -> String {
        format!("{}/{instance_id}", self.instances_url)
    }

    pub async fn altinn_token(&self, idporten_token: &str) -> Result<String, FilingError> {
        let response = self
            .http
            .get(format!(
                "{}/authentication/api/v1/exchange/id-porten",
                self.platform_url
            ))
            .bearer_auth(idporten_token)
            .send()
            .await
            .map_err(unresolved)?;
        // The token comes back as a bare JWT, sometimes JSON-quoted.
        Ok(read(response).await?.trim().trim_matches('"').to_string())
    }

    pub async fn create_instance(
        &self,
        token: &str,
        orgnumber: &str,
    ) -> Result<Instance, FilingError> {
        let response = self
            .http
            .post(&self.instances_url)
            .bearer_auth(token)
            .json(&serde_json::json!({
                "instanceOwner": { "organisationNumber": orgnumber }
            }))
            .send()
            .await
            .map_err(unresolved)?;
        parse_instance(read(response).await?)
    }

    pub async fn instance(&self, token: &str, url: &str) -> Result<Instance, FilingError> {
        let response = self
            .http
            .get(url)
            .bearer_auth(token)
            .send()
            .await
            .map_err(unresolved)?;
        parse_instance(read(response).await?)
    }

    pub async fn put_envelope(
        &self,
        token: &str,
        instance_url: &str,
        element_id: &str,
        xml: &str,
    ) -> Result<(), FilingError> {
        let response = self
            .http
            .put(format!("{instance_url}/data/{element_id}"))
            .bearer_auth(token)
            .header("content-type", "application/xml")
            .body(xml.to_string())
            .send()
            .await
            .map_err(unresolved)?;
        read(response).await.map(drop)
    }

    pub async fn upload_melding(
        &self,
        token: &str,
        instance_url: &str,
        xml: &str,
    ) -> Result<(), FilingError> {
        let response = self
            .http
            .post(format!("{instance_url}/data?dataType={MELDING_DATA_TYPE}"))
            .bearer_auth(token)
            .header("content-type", "text/xml")
            .header(
                "content-disposition",
                "attachment; filename=mva-melding.xml",
            )
            .body(xml.to_string())
            .send()
            .await
            .map_err(unresolved)?;
        read(response).await.map(drop)
    }

    pub async fn process_next(&self, token: &str, instance_url: &str) -> Result<(), FilingError> {
        let response = self
            .http
            .put(format!("{instance_url}/process/next"))
            .bearer_auth(token)
            .header("content-type", "application/json")
            .send()
            .await
            .map_err(unresolved)?;
        read(response).await.map(drop)
    }

    /// Removes a draft that will never be confirmed.
    pub async fn delete_instance(&self, token: &str, url: &str) -> Result<(), FilingError> {
        let response = self
            .http
            .delete(url)
            .bearer_auth(token)
            .send()
            .await
            .map_err(unresolved)?;
        read(response).await.map(drop)
    }

    pub async fn feedback_ready(
        &self,
        token: &str,
        instance_url: &str,
    ) -> Result<bool, FilingError> {
        let response = self
            .http
            .get(format!("{instance_url}/feedback/status"))
            .bearer_auth(token)
            .header("accept", "application/json")
            .send()
            .await
            .map_err(unresolved)?;
        let body: serde_json::Value = serde_json::from_str(&read(response).await?)
            .map_err(|e| FilingError::Unresolved(format!("unreadable feedback status: {e}")))?;
        Ok(body["isFeedbackProvided"] == true)
    }

    /// The bytes of one data element and their content type.
    pub async fn document(
        &self,
        token: &str,
        instance_url: &str,
        element: &DataElement,
    ) -> Result<(Vec<u8>, String), FilingError> {
        let response = self
            .http
            .get(format!("{instance_url}/data/{}", element.id))
            .bearer_auth(token)
            .send()
            .await
            .map_err(unresolved)?;
        let status = response.status();
        if !status.is_success() {
            return Err(refused(
                status.as_u16(),
                response.text().await.unwrap_or_default(),
            ));
        }
        let content_type = element
            .content_type
            .clone()
            .unwrap_or_else(|| "application/octet-stream".into());
        let bytes = response.bytes().await.map_err(unresolved)?;
        Ok((bytes.to_vec(), content_type))
    }
}

fn unresolved(e: reqwest::Error) -> FilingError {
    // Without the URL: an error text must never carry more than it needs.
    FilingError::Unresolved(e.without_url().to_string())
}

fn refused(status: u16, body: String) -> FilingError {
    let body = body.chars().take(2000).collect();
    // A 408 is a timeout, so it says nothing about whether the call took effect.
    if status >= 500 || status == 408 {
        FilingError::Unresolved(format!("{status}: {body}"))
    } else {
        FilingError::Refused { status, body }
    }
}

async fn read(response: reqwest::Response) -> Result<String, FilingError> {
    let status = response.status();
    let body = response.text().await.map_err(unresolved)?;
    if status.is_success() {
        Ok(body)
    } else {
        Err(refused(status.as_u16(), body))
    }
}

fn parse_instance(body: String) -> Result<Instance, FilingError> {
    serde_json::from_str(&body)
        .map_err(|e| FilingError::Unresolved(format!("unreadable instance: {e}")))
}

/// When a JWT expires, read without verifying it: we hold it as issued
/// and only need to know when to stop trying it.
pub fn token_expiry(token: &str) -> Option<DateTime<Utc>> {
    let mut validation = jsonwebtoken::Validation::new(jsonwebtoken::Algorithm::RS256);
    validation.insecure_disable_signature_validation();
    validation.validate_exp = false;
    validation.validate_aud = false;
    validation.required_spec_claims.clear();
    #[derive(Deserialize)]
    struct Claims {
        exp: i64,
    }
    let data = jsonwebtoken::decode::<Claims>(
        token,
        &jsonwebtoken::DecodingKey::from_secret(&[]),
        &validation,
    )
    .ok()?;
    DateTime::from_timestamp(data.claims.exp, 0)
}

fn tag<'a>(xml: &'a str, name: &str) -> Option<&'a str> {
    let open = format!("<{name}>");
    let close = format!("</{name}>");
    let start = xml.find(&open)? + open.len();
    let end = xml[start..].find(&close)?;
    Some(xml[start..start + end].trim())
}

/// The payment details in Skatteetaten's `betalingsinformasjon`.
pub fn payment_info(xml: &str) -> Option<serde_json::Value> {
    Some(serde_json::json!({
        "kid": tag(xml, "kundeidentifikasjonsnummer")?,
        "beloep": tag(xml, "beloep")?,
        "frist": tag(xml, "betalingsfrist")?,
        "konto": tag(xml, "norskKontonummer"),
    }))
}

/// The published `valideringsresultat` for a melding without deviations.
pub fn is_clean_result(xml: &str) -> bool {
    xml.contains("<valideringsresultat")
        && tag(xml, "avvikVedMeldingslevering") == Some("ingen avvik")
}

/// Whether the overall verdict rejects the melding. A deviation alone
/// does not: Skatteetaten can still accept it.
pub fn is_rejection(xml: &str) -> bool {
    tag(xml, "avvikVedMeldingslevering")
        .or_else(|| tag(xml, "status"))
        .is_some_and(|verdict| {
            let verdict = verdict.to_uppercase();
            verdict.contains("UGYLDIG") || verdict.contains("MANGELFULL")
        })
}

/// What a `valideringsresultat` objects to. Empty only when it says the
/// melding has no deviations: an overall verdict without a line still counts.
pub fn extract_findings(xml: &str) -> Vec<String> {
    let mut findings = Vec::new();
    for name in ["avvik", "sjekkResultat", "valideringsfeil"] {
        let open = format!("<{name}>");
        let close = format!("</{name}>");
        let mut rest = xml;
        while let Some(start) = rest.find(&open) {
            let Some(end) = rest[start..].find(&close) else {
                break;
            };
            let block = &rest[start..start + end];
            let text = ["begrunnelse", "feilmelding", "beskrivelse"]
                .iter()
                .find_map(|t| tag(block, t))
                .unwrap_or_else(|| block[open.len()..].trim());
            findings.push(text.to_string());
            rest = &rest[start + end + close.len()..];
        }
    }
    let verdict = tag(xml, "avvikVedMeldingslevering")
        .filter(|v| *v != "ingen avvik")
        .or_else(|| {
            tag(xml, "status").filter(|s| {
                let s = s.to_uppercase();
                s.contains("UGYLDIG") || s.contains("MANGELFULL") || s.contains("AVVIK")
            })
        });
    if let (true, Some(verdict)) = (findings.is_empty(), verdict) {
        findings.push(verdict.to_string());
    }
    findings
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_deviation_descriptions() {
        let xml = r#"<valideringsresultat>
            <avvik><avviksnavn>x</avviksnavn><description>fastsattMerverdiavgift stemmer ikke</description></avvik>
            <avvik><description>ugyldig sats</description></avvik>
        </valideringsresultat>"#;
        let findings = extract_findings(xml);
        assert_eq!(findings.len(), 2);
        assert!(findings[0].contains("stemmer ikke"));
        assert!(findings[1].contains("ugyldig sats"));
        assert!(extract_findings("<valideringsresultat/>").is_empty());
    }

    /// Skatteetaten's published examples, one per shape.
    #[test]
    fn reads_the_official_result_formats() {
        let clean = r#"<valideringsresultat xmlns="no:skatteetaten:fastsetting:avgift:mva:valideringsresultat:v1">
            <avvikVedMeldingslevering>ingen avvik</avvikVedMeldingslevering>
        </valideringsresultat>"#;
        assert!(extract_findings(clean).is_empty());

        let current = r#"<valideringsresultat>
            <avvikVedMeldingslevering>ugyldig skattemelding</avvikVedMeldingslevering>
            <avvik><stiTilAvvik>//x</stiTilAvvik><avviksinformasjon>
                <begrunnelse>Summen stemmer ikke</begrunnelse><avvikstype>ugyldig skattemelding</avvikstype>
            </avviksinformasjon></avvik>
        </valideringsresultat>"#;
        assert_eq!(extract_findings(current), ["Summen stemmer ikke"]);

        let older = r#"<valideringsresultat><status>UGYLDIG_SKATTEMELDING</status>
            <valideringsfeil><valideringsDetaljer><feilmelding>Må være gyldig</feilmelding>
            </valideringsDetaljer></valideringsfeil></valideringsresultat>"#;
        assert_eq!(extract_findings(older), ["Må være gyldig"]);

        let verdict_only = "<valideringsresultat><avvikVedMeldingslevering>mangelfull skattemelding</avvikVedMeldingslevering></valideringsresultat>";
        assert_eq!(extract_findings(verdict_only), ["mangelfull skattemelding"]);
    }

    #[test]
    fn only_an_error_verdict_is_a_rejection() {
        let verdict = |v: &str| {
            format!(
                "<valideringsresultat><avvikVedMeldingslevering>{v}</avvikVedMeldingslevering></valideringsresultat>"
            )
        };
        assert!(is_rejection(&verdict("ugyldig skattemelding")));
        assert!(is_rejection(&verdict("mangelfull skattemelding")));
        assert!(!is_rejection(&verdict("avvikende skattemelding")));
        assert!(!is_rejection(&verdict("ingen avvik")));
        assert!(is_rejection(
            "<r><status>UGYLDIG_SKATTEMELDING</status></r>"
        ));
        assert!(!is_rejection("<r/>"));
    }

    fn instance(process: serde_json::Value) -> Instance {
        serde_json::from_value(serde_json::json!({ "id": "1/g", "process": process })).unwrap()
    }

    #[test]
    fn only_the_documented_states_count_as_filed() {
        let task = |id: &str, name: &str| serde_json::json!({ "currentTask": { "elementId": id, "name": name } });
        assert!(instance(task("Task_3", "Tilbakemelding")).is_submitted());
        assert!(
            instance(serde_json::json!({ "currentTask": null, "ended": "2021-05-21T11:29:35Z" }))
                .is_submitted()
        );
        assert!(!instance(task("Task_3", "Noe annet")).is_submitted());
        assert!(!instance(serde_json::json!({ "currentTask": null })).is_submitted());
        assert!(!instance(serde_json::Value::Null).is_submitted());
    }

    async fn one_answer(raw: &'static str) -> VatReturnClient {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut seen = [0u8; 2048];
            let _ = socket.read(&mut seen).await;
            socket.write_all(raw.as_bytes()).await.unwrap();
        });
        VatReturnClient::new(format!("http://{address}/valider"))
    }

    #[tokio::test]
    async fn a_cut_off_answer_is_an_error() {
        let client = one_answer("HTTP/1.1 200 OK\r\ncontent-length: 500\r\n\r\n<valid").await;
        let error = client.validate_with_token("t", "<x/>").await.err().unwrap();
        assert!(format!("{error:#}").contains("reading the validation answer"));
    }

    #[tokio::test]
    async fn only_a_recognizable_result_counts_as_clean() {
        for body in [
            "HTTP/1.1 200 OK\r\ncontent-length: 0\r\n\r\n",
            "HTTP/1.1 200 OK\r\ncontent-length: 12\r\n\r\n<html></html",
            "HTTP/1.1 200 OK\r\ncontent-length: 22\r\n\r\n<valideringsresultat/>",
        ] {
            let client = one_answer(body).await;
            assert!(
                client.validate_with_token("t", "<x/>").await.is_err(),
                "{body}"
            );
        }
        assert!(is_clean_result(
            "<valideringsresultat xmlns=\"x\"><avvikVedMeldingslevering>ingen avvik</avvikVedMeldingslevering></valideringsresultat>"
        ));
        assert!(!is_clean_result(
            "<avvikVedMeldingslevering>ingen avvik</avvikVedMeldingslevering>"
        ));
    }

    #[tokio::test]
    async fn validation_gives_up_on_a_silent_server() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let mut held = Vec::new();
            while let Ok((socket, _)) = listener.accept().await {
                held.push(socket);
            }
        });
        let client = VatReturnClient {
            validation_url: format!("http://{address}/valider"),
            http: http_with(Duration::from_millis(100), Duration::from_millis(200)),
        };
        let attempt = tokio::time::timeout(
            Duration::from_secs(5),
            client.validate_with_token("token", "<x/>"),
        )
        .await
        .expect("the client's own timeout ends the call");
        assert!(attempt.is_err());
    }

    #[test]
    fn reads_the_payment_details() {
        let xml = r#"<betalingsinformasjon xmlns="x">
            <kundeidentifikasjonsnummer>99999999999999</kundeidentifikasjonsnummer>
            <beloep>250</beloep><betaleTil>Skatteetaten</betaleTil>
            <betalingsfrist>2021-05-21</betalingsfrist>
            <betalesTilKonto><norskKontonummer>12345678903</norskKontonummer></betalesTilKonto>
        </betalingsinformasjon>"#;
        let info = payment_info(xml).unwrap();
        assert_eq!(info["kid"], "99999999999999");
        assert_eq!(info["beloep"], "250");
        assert_eq!(info["konto"], "12345678903");
        assert!(payment_info("<x/>").is_none());
    }

    #[test]
    fn a_server_error_is_never_read_as_a_refusal() {
        assert!(matches!(
            refused(502, "x".into()),
            FilingError::Unresolved(_)
        ));
        assert!(matches!(
            refused(408, "x".into()),
            FilingError::Unresolved(_)
        ));
        assert!(matches!(
            refused(403, "x".into()),
            FilingError::Refused { status: 403, .. }
        ));
    }
}
