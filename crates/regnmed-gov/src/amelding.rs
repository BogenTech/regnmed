//! Skatteetatens a-melding API: submission and feedback (docs/gov.md, #46).
//!
//! `regnmed_core::amelding::render_json` builds the body; this module moves
//! it. Every call carries a **systembruker token for the customer** (the
//! opplysningspliktig) — `TokenProvider::token_for_systemuser` — so the
//! provider must be configured with both scopes this needs:
//! `skatteetaten:innrapporteringamelding digdir:dialogporten`.
//!
//! - **Submit**: `POST {base}/v1/innsending/{periode}/{opplysningspliktig}`
//!   with a fresh `idempotencyKey` header. The answer is only a receipt
//!   (`dialogId`, `forsendelseId`, `meldingsId`); Skatteetaten checks the
//!   content afterwards.
//! - **Feedback** is NOT at `forsendelser/{forsendelseId}` — that answers
//!   404 `AMLD_1050`. It is a separate `Information` transmission in the
//!   receipt's Dialogporten dialog, related to the submission's
//!   transmission, whose `Api` attachment URL points at the tilbakemelding.
//!   Established against the test environment 2026-11-11.

use anyhow::{Context, Result, bail};
use serde::Deserialize;
use uuid::Uuid;

use crate::maskinporten::TokenProvider;

/// The scopes a provider for this module must request.
pub const SCOPES: &str = "skatteetaten:innrapporteringamelding digdir:dialogporten";

pub struct AmeldingClient {
    /// `https://innrapporteringamelding.api.skatteetaten-test.no` in test.
    pub base_url: String,
    /// `https://platform.tt02.altinn.no/dialogporten/api/v1` in test.
    pub dialogporten_url: String,
    http: reqwest::Client,
}

/// Skatteetatens `Leveransebekreftelse`. Proof of delivery, not of a
/// valid melding — that is the feedback.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Receipt {
    pub dialog_id: String,
    pub forsendelse_id: String,
    pub meldings_id: String,
}

impl AmeldingClient {
    pub fn new(base_url: impl Into<String>, dialogporten_url: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into(),
            dialogporten_url: dialogporten_url.into(),
            http: reqwest::Client::new(),
        }
    }

    /// Reads AMELDING_URL and DIALOGPORTEN_URL, defaulting to the TEST
    /// environment — production is opted into, never fallen into.
    pub fn from_env() -> Self {
        let var =
            |name: &str, default: &str| std::env::var(name).unwrap_or_else(|_| default.into());
        Self::new(
            var(
                "AMELDING_URL",
                "https://innrapporteringamelding.api.skatteetaten-test.no",
            ),
            var(
                "DIALOGPORTEN_URL",
                "https://platform.tt02.altinn.no/dialogporten/api/v1",
            ),
        )
    }

    /// Submits one melding. `idempotency_key` must be new for new content
    /// and REUSED for a retry of the same content: Skatteetaten then
    /// answers with the first receipt instead of taking a second melding.
    pub async fn submit(
        &self,
        tokens: &TokenProvider,
        opplysningspliktig: &str,
        period: &str,
        json: String,
        idempotency_key: Uuid,
    ) -> Result<Receipt> {
        let token = tokens.token_for_systemuser(opplysningspliktig).await?;
        let response = self
            .http
            .post(format!(
                "{}/v1/innsending/{period}/{opplysningspliktig}",
                self.base_url
            ))
            .bearer_auth(token)
            .header("idempotencyKey", idempotency_key.to_string())
            .header("content-type", "application/json")
            .header("accept", "application/json")
            .body(json)
            .send()
            .await
            .context("calling the a-melding submission API")?;
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        if !status.is_success() {
            // Skatteetatens `Feil` (kode, melding, spesifisering) verbatim:
            // it says what to fix.
            bail!("a-melding submission returned {status}: {body}");
        }
        serde_json::from_str(&body).context("parsing the a-melding receipt")
    }

    /// The tilbakemelding for a submission, or `None` while Skatteetaten
    /// has not produced it yet (it came 21 s after submission in test).
    /// Returned verbatim: `mottakstatus`, `avvik`, the sums received, and
    /// the KID numbers for paying them.
    pub async fn feedback(
        &self,
        tokens: &TokenProvider,
        opplysningspliktig: &str,
        receipt: &Receipt,
    ) -> Result<Option<serde_json::Value>> {
        let token = tokens.token_for_systemuser(opplysningspliktig).await?;
        let dialog: Dialog = self
            .get_json(
                &format!(
                    "{}/enduser/dialogs/{}",
                    self.dialogporten_url, receipt.dialog_id
                ),
                &token,
            )
            .await
            .context("reading the a-melding dialog in Dialogporten")?;
        let Some(url) = feedback_url(&dialog, &receipt.forsendelse_id) else {
            return Ok(None);
        };
        self.get_json(&url, &token)
            .await
            .context("reading the a-melding tilbakemelding")
            .map(Some)
    }

    async fn get_json<T: serde::de::DeserializeOwned>(&self, url: &str, token: &str) -> Result<T> {
        let response = self
            .http
            .get(url)
            .bearer_auth(token)
            .header("accept", "application/json")
            .send()
            .await?;
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        if !status.is_success() {
            bail!("{url} returned {status}: {body}");
        }
        Ok(serde_json::from_str(&body)?)
    }
}

#[derive(Deserialize)]
struct Dialog {
    #[serde(default)]
    transmissions: Vec<Transmission>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Transmission {
    #[serde(rename = "type")]
    kind: String,
    related_transmission_id: Option<String>,
    #[serde(default)]
    attachments: Vec<Attachment>,
}

#[derive(Deserialize)]
struct Attachment {
    #[serde(default)]
    urls: Vec<AttachmentUrl>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct AttachmentUrl {
    url: String,
    consumer_type: String,
}

/// The `Api` URL of the Information transmission answering this submission.
fn feedback_url(dialog: &Dialog, forsendelse_id: &str) -> Option<String> {
    dialog
        .transmissions
        .iter()
        .filter(|t| t.kind == "Information")
        .filter(|t| t.related_transmission_id.as_deref() == Some(forsendelse_id))
        .flat_map(|t| &t.attachments)
        .flat_map(|a| &a.urls)
        .find(|u| u.consumer_type == "Api")
        .map(|u| u.url.clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::extract::{Path, State};
    use axum::http::HeaderMap;
    use axum::routing::{get, post};
    use axum::{Json, Router};
    use std::sync::{Arc, Mutex};

    #[derive(Default)]
    struct Seen {
        idempotency: Option<String>,
        path: Option<String>,
        body: Option<serde_json::Value>,
        feedback_ready: bool,
    }

    /// Token endpoint, submission API, Dialogporten and the feedback API in
    /// one mock — the shapes copied from the real test environment.
    async fn mock() -> (String, Arc<Mutex<Seen>>) {
        let seen = Arc::new(Mutex::new(Seen::default()));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let feedback = format!("{base}/tilbakemelding/v1/forsendelser/t-info");
        let app = Router::new()
            .route(
                "/token",
                post(|| async {
                    Json(serde_json::json!({"access_token": "sysuser-token", "expires_in": 120}))
                }),
            )
            .route(
                "/v1/innsending/{period}/{org}",
                post(
                    |State(seen): State<Arc<Mutex<Seen>>>,
                     Path((period, org)): Path<(String, String)>,
                     headers: HeaderMap,
                     Json(body): Json<serde_json::Value>| async move {
                        assert_eq!(headers["authorization"], "Bearer sysuser-token");
                        let mut s = seen.lock().unwrap();
                        s.idempotency = Some(headers["idempotencyKey"].to_str().unwrap().into());
                        s.path = Some(format!("{period}/{org}"));
                        s.body = Some(body);
                        Json(serde_json::json!({
                            "dialogId": "d-1", "forsendelseId": "t-sub", "meldingsId": "m-1"
                        }))
                    },
                ),
            )
            .route(
                "/dp/enduser/dialogs/{id}",
                get(move |State(seen): State<Arc<Mutex<Seen>>>| {
                    let feedback = feedback.clone();
                    async move {
                        let mut transmissions = vec![serde_json::json!({
                            "id": "t-sub", "type": "Submission",
                            "attachments": [{"urls": [{"url": "http://x/doc", "consumerType": "Api"}]}]
                        })];
                        if seen.lock().unwrap().feedback_ready {
                            transmissions.push(serde_json::json!({
                                "id": "t-info", "type": "Information", "relatedTransmissionId": "t-sub",
                                "attachments": [{"urls": [
                                    {"url": "http://x/gui", "consumerType": "Gui"},
                                    {"url": feedback, "consumerType": "Api"}
                                ]}]
                            }));
                        }
                        Json(serde_json::json!({"id": "d-1", "transmissions": transmissions}))
                    }
                }),
            )
            .route(
                "/tilbakemelding/v1/forsendelser/t-info",
                get(|| async {
                    Json(serde_json::json!({"mottak": [{"mottattLeveranse": [
                        {"mottakstatus": "mottatt", "avvik": []}
                    ]}]}))
                }),
            )
            .with_state(seen.clone());
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        (base, seen)
    }

    #[tokio::test]
    async fn submits_with_a_systemuser_token_and_finds_the_feedback() {
        let (base, seen) = mock().await;
        let (config, _) = crate::maskinporten::tests::test_config(&format!("{base}/token"));
        let tokens = TokenProvider::new(config);
        let client = AmeldingClient::new(base.clone(), format!("{base}/dp"));
        let key = Uuid::new_v4();

        let receipt = client
            .submit(
                &tokens,
                "311032240",
                "2026-10",
                r#"{"leveranse":{}}"#.into(),
                key,
            )
            .await
            .unwrap();
        assert_eq!(
            receipt,
            Receipt {
                dialog_id: "d-1".into(),
                forsendelse_id: "t-sub".into(),
                meldings_id: "m-1".into(),
            }
        );
        {
            let s = seen.lock().unwrap();
            assert_eq!(s.path.as_deref(), Some("2026-10/311032240"));
            assert_eq!(s.idempotency.as_deref(), Some(key.to_string().as_str()));
            assert_eq!(s.body, Some(serde_json::json!({"leveranse": {}})));
        }

        // Not produced yet: the submission's own transmission is NOT it.
        assert_eq!(
            client
                .feedback(&tokens, "311032240", &receipt)
                .await
                .unwrap(),
            None
        );
        seen.lock().unwrap().feedback_ready = true;
        let feedback = client
            .feedback(&tokens, "311032240", &receipt)
            .await
            .unwrap()
            .expect("feedback once the Information transmission exists");
        assert_eq!(
            feedback["mottak"][0]["mottattLeveranse"][0]["mottakstatus"],
            "mottatt"
        );
    }

    #[tokio::test]
    async fn an_error_from_skatteetaten_is_passed_on_verbatim() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let app = Router::new()
            .route(
                "/token",
                post(|| async {
                    Json(serde_json::json!({"access_token": "t", "expires_in": 120}))
                }),
            )
            .route(
                "/v1/innsending/{period}/{org}",
                post(|| async {
                    (
                        axum::http::StatusCode::FORBIDDEN,
                        Json(serde_json::json!({"kode": "AMLD_1063", "melding": "mangler systembruker"})),
                    )
                }),
            );
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let (config, _) = crate::maskinporten::tests::test_config(&format!("{base}/token"));
        let err = AmeldingClient::new(base.clone(), base)
            .submit(
                &TokenProvider::new(config),
                "311032240",
                "2026-10",
                "{}".into(),
                Uuid::new_v4(),
            )
            .await
            .unwrap_err()
            .to_string();
        assert!(err.contains("403") && err.contains("AMLD_1063"), "{err}");
    }
}
