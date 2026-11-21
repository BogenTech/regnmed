//! Asking a customer to approve regnmed as a system in Altinn (docs/gov.md).
//!
//! regnmed sends a request, and someone with authority at the customer
//! approves it at the `confirmUrl`. Only then can Maskinporten issue a
//! system user token for that company.

use anyhow::{Context, Result, bail};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::maskinporten::TokenProvider;

pub const SCOPES: &str =
    "altinn:authentication/systemuser.request.write altinn:authentication/systemuser.request.read";

/// What a company reporting for itself grants regnmed.
pub const RESOURCES: [&str; 3] = [
    "ske-innrapportering-amelding",
    "ske-skattekort-til-arbeidsgiver",
    "ske-innrapportering-aksjonaerregisteroppgave",
];

const EXISTING_SYSTEM_USER: &str = "AUTH-00004";
const ALREADY_ACCEPTED: &str = "AUTH-00006";
const UNKNOWN_REQUEST: &str = "AUTH-00010";

pub struct SystemUserClient {
    /// `https://platform.tt02.altinn.no` in test.
    pub platform_url: String,
    pub system_id: String,
    http: reqwest::Client,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SystemUserRequest {
    pub id: String,
    pub status: String,
    #[serde(default)]
    pub confirm_url: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CreateOutcome {
    Requested(SystemUserRequest),
    /// The company has already approved this system.
    AlreadyApproved,
}

impl SystemUserClient {
    pub fn new(platform_url: impl Into<String>, system_id: impl Into<String>) -> Self {
        Self {
            platform_url: platform_url.into().trim_end_matches('/').to_string(),
            system_id: system_id.into(),
            http: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(20))
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .expect("a TLS backend"),
        }
    }

    /// Defaults to the TEST environment: production is opted into.
    pub fn from_env() -> Self {
        Self::new(
            std::env::var("ALTINN_PLATFORM_URL")
                .unwrap_or_else(|_| "https://platform.tt02.altinn.no".into()),
            std::env::var("ALTINN_SYSTEM_ID").unwrap_or_else(|_| "935115086_regnmed".into()),
        )
    }

    fn url(&self, suffix: &str) -> String {
        format!(
            "{}/authentication/api/v1/systemuser/request/vendor{suffix}",
            self.platform_url
        )
    }

    pub async fn create(
        &self,
        tokens: &TokenProvider,
        orgnumber: &str,
        redirect_url: Option<&str>,
    ) -> Result<CreateOutcome> {
        anyhow::ensure!(
            regnmed_core::orgnumber::is_valid(orgnumber),
            "invalid org number: {orgnumber}"
        );
        let rights: Vec<Value> = RESOURCES
            .iter()
            .map(|r| json!({ "resource": [{ "id": "urn:altinn:resource", "value": r }] }))
            .collect();
        let mut body = json!({
            "systemId": self.system_id,
            "partyOrgNo": orgnumber,
            "rights": rights,
        });
        if let Some(url) = redirect_url {
            body["redirectUrl"] = url.into();
        }
        let response = self
            .http
            .post(self.url(""))
            .bearer_auth(tokens.token().await?)
            .header("accept", "application/json")
            .json(&body)
            .send()
            .await
            .context("requesting an Altinn system user")?;
        let status = response.status();
        let text = response.text().await.unwrap_or_default();
        if status.is_success() {
            return parse_request(&text).map(CreateOutcome::Requested);
        }
        let error: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
        if error["code"] == EXISTING_SYSTEM_USER || error["code"] == ALREADY_ACCEPTED {
            return Ok(CreateOutcome::AlreadyApproved);
        }
        bail!("Altinn refused the system user request: {status}: {text}");
    }

    /// `None` when Altinn no longer has the request, as after a timeout.
    pub async fn status(
        &self,
        tokens: &TokenProvider,
        id: &str,
    ) -> Result<Option<SystemUserRequest>> {
        let response = self
            .http
            .get(self.url(&format!("/{id}")))
            .bearer_auth(tokens.token().await?)
            .header("accept", "application/json")
            .send()
            .await
            .context("reading the Altinn system user request")?;
        let status = response.status();
        let text = response.text().await.unwrap_or_default();
        if is_unknown_request(status, &text) {
            return Ok(None);
        }
        if !status.is_success() {
            bail!("Altinn could not read the request: {status}: {text}");
        }
        parse_request(&text).map(Some)
    }

    /// Altinn answers a new request with the old one until it is deleted,
    /// and the externalRef must stay the default for tokens to find the user.
    pub async fn delete(&self, tokens: &TokenProvider, id: &str) -> Result<()> {
        let response = self
            .http
            .delete(self.url(&format!("/{id}")))
            .bearer_auth(tokens.token().await?)
            .header("accept", "application/json")
            .send()
            .await
            .context("deleting the Altinn system user request")?;
        let status = response.status();
        let text = response.text().await.unwrap_or_default();
        if status.is_success() || is_unknown_request(status, &text) {
            return Ok(());
        }
        bail!("Altinn could not delete the request: {status}: {text}");
    }
}

fn is_unknown_request(status: reqwest::StatusCode, text: &str) -> bool {
    status == reqwest::StatusCode::NOT_FOUND
        || serde_json::from_str::<Value>(text).is_ok_and(|e| e["code"] == UNKNOWN_REQUEST)
}

fn parse_request(text: &str) -> Result<SystemUserRequest> {
    let request: SystemUserRequest =
        serde_json::from_str(text).context("reading Altinn's system user request")?;
    if let Some(url) = &request.confirm_url {
        anyhow::ensure!(
            is_altinn_url(url),
            "Altinn returned an unexpected approval link"
        );
    }
    Ok(request)
}

/// The link is shown to a company admin, so only Altinn's own hosts pass.
fn is_altinn_url(url: &str) -> bool {
    reqwest::Url::parse(url).is_ok_and(|u| {
        u.scheme() == "https"
            && u.host_str().is_some_and(|host| {
                let host = host.to_ascii_lowercase();
                host == "altinn.no" || host.ends_with(".altinn.no")
            })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_altinn_can_supply_the_approval_link() {
        assert!(is_altinn_url(
            "https://authn.ui.tt02.altinn.no/authfront/ui/x"
        ));
        for url in [
            "http://authn.altinn.no/x",
            "https://altinn.no.evil.example/x",
            "https://evilaltinn.no/x",
            "javascript:alert(1)",
        ] {
            assert!(!is_altinn_url(url), "{url}");
        }
    }

    #[test]
    fn a_request_with_a_foreign_link_is_refused() {
        let body = r#"{"id":"1","status":"New","confirmUrl":"https://evil.example/x"}"#;
        assert!(parse_request(body).is_err());
        let ok = r#"{"id":"1","status":"New","confirmUrl":"https://authn.ui.tt02.altinn.no/a"}"#;
        assert_eq!(parse_request(ok).unwrap().status, "New");
    }
}
