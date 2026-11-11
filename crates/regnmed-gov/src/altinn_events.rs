//! Altinn Events subscriptions (docs/lonn.md, «Tilbakemelding som hendelse»).
//!
//! Dialogporten publishes an event whenever a transmission is created in a
//! dialog — including Skatteetatens tilbakemelding on an a-melding. A
//! subscription makes Altinn POST that event to our webhook, so nobody has
//! to ask for the feedback (and Altinn's bruksvilkår §1.2.4 prefer events
//! to polling).
//!
//! Unlike Dialogporten and the system register, **the Events API does not
//! take a Maskinporten token**: it answers 401, and accepts the same token
//! after it has been exchanged for an Altinn token at
//! `/authentication/api/v1/exchange/maskinporten` (established against TT02
//! 2026-11-11). Exchanged from a systembruker token, the subscription runs
//! as that systembruker (`consumer: /systemuser/<id>`), which is what gives
//! it access to the customer's events.

use anyhow::{Context, Result, bail};
use serde::Deserialize;

use crate::maskinporten::TokenProvider;

/// Dialogporten's event for a new transmission — the feedback is one.
pub const TRANSMISSION_CREATED: &str = "dialogporten.dialog.transmission.created.v1";
/// The resource the a-melding dialogs belong to.
pub const AMELDING_RESOURCE: &str = "urn:altinn:resource:ske-innrapportering-amelding";
/// The event Altinn sends to a new endpoint before anything else.
pub const VALIDATE_SUBSCRIPTION: &str = "platform.events.validatesubscription";

pub struct EventsClient {
    /// `https://platform.tt02.altinn.no` in test.
    pub platform_url: String,
    http: reqwest::Client,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Subscription {
    pub id: i64,
    pub end_point: String,
    /// False until Altinn has delivered the validation event to the
    /// endpoint and got a 2xx back; no events flow before that.
    #[serde(default)]
    pub validated: bool,
}

impl EventsClient {
    pub fn new(platform_url: impl Into<String>) -> Self {
        Self {
            platform_url: platform_url.into(),
            http: reqwest::Client::new(),
        }
    }

    /// ALTINN_PLATFORM_URL, defaulting to the TEST environment (TT02).
    pub fn from_env() -> Self {
        Self::new(
            std::env::var("ALTINN_PLATFORM_URL")
                .unwrap_or_else(|_| "https://platform.tt02.altinn.no".into()),
        )
    }

    /// The customer's systembruker token, exchanged for an Altinn token.
    async fn altinn_token(&self, tokens: &TokenProvider, orgnumber: &str) -> Result<String> {
        let maskinporten = tokens.token_for_systemuser(orgnumber).await?;
        let response = self
            .http
            .get(format!(
                "{}/authentication/api/v1/exchange/maskinporten",
                self.platform_url
            ))
            .bearer_auth(maskinporten)
            .send()
            .await
            .context("exchanging the Maskinporten token for an Altinn token")?;
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        if !status.is_success() {
            bail!("Altinn token exchange returned {status}: {body}");
        }
        // The token comes back as a bare JWT, sometimes JSON-quoted.
        Ok(body.trim().trim_matches('"').to_string())
    }

    /// Subscribes `endpoint` to new transmissions in the customer's
    /// a-melding dialogs.
    pub async fn subscribe_amelding(
        &self,
        tokens: &TokenProvider,
        orgnumber: &str,
        endpoint: &str,
    ) -> Result<Subscription> {
        let token = self.altinn_token(tokens, orgnumber).await?;
        let response = self
            .http
            .post(format!("{}/events/api/v1/subscriptions", self.platform_url))
            .bearer_auth(token)
            .json(&serde_json::json!({
                "endPoint": endpoint,
                "resourceFilter": AMELDING_RESOURCE,
                "subjectFilter": format!("urn:altinn:organization:identifier-no:{orgnumber}"),
                "typeFilter": TRANSMISSION_CREATED,
            }))
            .send()
            .await
            .context("creating the Altinn Events subscription")?;
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        if !status.is_success() {
            bail!("Altinn Events subscription returned {status}: {body}");
        }
        serde_json::from_str(&body).context("parsing the subscription")
    }

    /// The subscription as Altinn has it — `validated` says whether events
    /// are flowing.
    pub async fn subscription(
        &self,
        tokens: &TokenProvider,
        orgnumber: &str,
        id: i64,
    ) -> Result<Subscription> {
        let token = self.altinn_token(tokens, orgnumber).await?;
        let response = self
            .http
            .get(format!(
                "{}/events/api/v1/subscriptions/{id}",
                self.platform_url
            ))
            .bearer_auth(token)
            .send()
            .await?;
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        if !status.is_success() {
            bail!("Altinn Events subscription {id} returned {status}: {body}");
        }
        Ok(serde_json::from_str(&body)?)
    }

    pub async fn unsubscribe(
        &self,
        tokens: &TokenProvider,
        orgnumber: &str,
        id: i64,
    ) -> Result<()> {
        let token = self.altinn_token(tokens, orgnumber).await?;
        let status = self
            .http
            .delete(format!(
                "{}/events/api/v1/subscriptions/{id}",
                self.platform_url
            ))
            .bearer_auth(token)
            .send()
            .await?
            .status();
        if !status.is_success() {
            bail!("deleting Altinn Events subscription {id} returned {status}");
        }
        Ok(())
    }
}

/// The parts of an incoming CloudEvent the webhook uses. Everything in it
/// is UNTRUSTED — Altinn does not sign events — so it is only ever a hint
/// about which dialog to look at through our own authenticated client.
#[derive(Debug, Deserialize)]
pub struct CloudEvent {
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default)]
    pub resource: Option<String>,
    /// For Dialogporten events: the dialog id.
    #[serde(default)]
    pub resourceinstance: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderMap;
    use axum::routing::{get, post};
    use axum::{Json, Router};

    #[tokio::test]
    async fn subscribes_with_an_exchanged_systemuser_token() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let app = Router::new()
            .route(
                "/token",
                post(|| async {
                    Json(serde_json::json!({"access_token": "maskinporten", "expires_in": 120}))
                }),
            )
            .route(
                "/authentication/api/v1/exchange/maskinporten",
                get(|headers: HeaderMap| async move {
                    assert_eq!(headers["authorization"], "Bearer maskinporten");
                    "\"altinn-token\""
                }),
            )
            .route(
                "/events/api/v1/subscriptions",
                post(
                    |headers: HeaderMap, Json(body): Json<serde_json::Value>| async move {
                        // The Events API takes the EXCHANGED token, never the
                        // Maskinporten one.
                        assert_eq!(headers["authorization"], "Bearer altinn-token");
                        assert_eq!(body["resourceFilter"], AMELDING_RESOURCE);
                        assert_eq!(body["typeFilter"], TRANSMISSION_CREATED);
                        assert_eq!(
                            body["subjectFilter"],
                            "urn:altinn:organization:identifier-no:311032240"
                        );
                        (
                            axum::http::StatusCode::CREATED,
                            Json(serde_json::json!({
                                "id": 126899, "endPoint": body["endPoint"], "validated": false
                            })),
                        )
                    },
                ),
            );
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let (config, _) = crate::maskinporten::tests::test_config(&format!("{base}/token"));
        let subscription = EventsClient::new(base)
            .subscribe_amelding(
                &TokenProvider::new(config),
                "311032240",
                "https://api.test.regnmed.no/webhooks/altinn",
            )
            .await
            .unwrap();
        assert_eq!(
            subscription,
            Subscription {
                id: 126899,
                end_point: "https://api.test.regnmed.no/webhooks/altinn".into(),
                validated: false,
            }
        );
    }

    #[test]
    fn a_dialogporten_event_names_its_dialog() {
        let event: CloudEvent = serde_json::from_str(
            r#"{"specversion":"1.0","id":"e","type":"dialogporten.dialog.transmission.created.v1",
                "resource":"urn:altinn:resource:ske-innrapportering-amelding",
                "resourceinstance":"01a1cae1-7ae4-7431-868a-ff94d80a3e8f",
                "subject":"urn:altinn:organization:identifier-no:311032240"}"#,
        )
        .unwrap();
        assert_eq!(event.kind, TRANSMISSION_CREATED);
        assert_eq!(event.resource.as_deref(), Some(AMELDING_RESOURCE));
        assert_eq!(
            event.resourceinstance.as_deref(),
            Some("01a1cae1-7ae4-7431-868a-ff94d80a3e8f")
        );
        let validation: CloudEvent = serde_json::from_str(
            r#"{"id":"v","source":"https://platform.altinn.no/events/api/v1/subscriptions/1",
                "type":"platform.events.validatesubscription","specversion":"1.0"}"#,
        )
        .unwrap();
        assert_eq!(validation.kind, VALIDATE_SUBSCRIPTION);
    }
}
