//! Skatteetatens API for payments to self-employed people without a fixed
//! place of business (third-party report, formerly RF-1321;
//! docs/selvstendig.md, #363). OpenAPI 1.0.0 on SwaggerHub
//! (`skatteetaten/innrapportering-betalinger-naeringsdrivende-api`),
//! vendored in docs/selvstendig/.
//!
//! `regnmed_core::contractor_payments` renders the `Melding`; this module
//! moves it. One call files the whole year for one payer:
//! `POST {base}/{inntektsaar}` with the JSON and an `idempotencyKey`,
//! answered by a `Leveransebekreftelse` (`dialogId`, `forsendelseId`) —
//! proof of delivery, not of approval.
//!
//! **Not proven against Skatteetaten.** The test scope was ordered
//! 2026-11-27 (SSV-6691) and is not granted. The request and answer below
//! follow the documentation's own example; the answer is expected in the
//! receipt's Dialogporten dialog as for RF-1086 (the documentation says
//! Skatteetaten usually opens one transmission for the delivery and
//! another for its feedback), and that too is unverified for this form.

use anyhow::{Context, Result, bail};
use serde::Deserialize;
use uuid::Uuid;

use crate::maskinporten::TokenProvider;
pub use crate::shareholder_return::Outcome;

/// `digdir:dialogporten` to read the answer, `altinn:events.subscribe` as
/// for the other filings.
pub const SCOPES: &str = "skatteetaten:innrapporteringbetalingerselvstendignaeringsdrivende \
                          digdir:dialogporten altinn:events.subscribe";

/// The Altinn resource the customer's system user must carry.
pub const RESOURCE: &str = "ske-innrapportering-betalinger-selvstendig-naeringsdrivende";

pub struct ContractorPaymentsClient {
    /// `https://innrapporteringbetalingernaeringsdrivende.api.skatteetaten-test.no/v1`
    /// in test (the OpenAPI's «Ekstern test»).
    pub base_url: String,
    pub dialogporten_url: String,
    http: reqwest::Client,
}

/// Skatteetatens `Leveransebekreftelse`.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Receipt {
    pub dialog_id: String,
    pub forsendelse_id: String,
    // The schema spells it with a capital R here, `Leveransereferanse` in
    // the request; the documentation's example agrees.
    #[serde(default, rename = "oppgavegiversLeveranseReferanse")]
    pub reference: Option<String>,
    #[serde(default)]
    pub antall_oppgaver: Option<i64>,
}

impl ContractorPaymentsClient {
    pub fn new(base_url: impl Into<String>, dialogporten_url: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into().trim_end_matches('/').to_string(),
            dialogporten_url: dialogporten_url.into().trim_end_matches('/').to_string(),
            http: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(60))
                .build()
                .expect("a TLS backend"),
        }
    }

    /// Reads CONTRACTOR_PAYMENTS_URL and DIALOGPORTEN_URL, defaulting to
    /// the TEST environment — production is opted into, never fallen into.
    pub fn from_env() -> Self {
        let var =
            |name: &str, default: &str| std::env::var(name).unwrap_or_else(|_| default.into());
        Self::new(
            var(
                "CONTRACTOR_PAYMENTS_URL",
                "https://innrapporteringbetalingernaeringsdrivende.api.skatteetaten-test.no/v1",
            ),
            var(
                "DIALOGPORTEN_URL",
                "https://platform.tt02.altinn.no/dialogporten/api/v1",
            ),
        )
    }

    /// Files one delivery. `json` is sent byte for byte: a retry with the
    /// same key must carry the same body to get the first answer back.
    pub async fn submit(
        &self,
        tokens: &TokenProvider,
        orgnumber: &str,
        income_year: i32,
        json: &str,
        idempotency_key: Uuid,
    ) -> Result<Receipt> {
        let token = tokens.token_for_systemuser(orgnumber).await?;
        let response = self
            .http
            .post(format!("{}/{income_year}", self.base_url))
            .bearer_auth(token)
            .header("idempotencyKey", idempotency_key.to_string())
            .header("content-type", "application/json")
            .header("accept", "application/json")
            .body(json.to_string())
            .send()
            .await
            .context("sending the report")?;
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        if !status.is_success() {
            // Skatteetatens `Feil` verbatim: it names the field.
            bail!("{status}: {body}");
        }
        serde_json::from_str(&body).with_context(|| format!("reading the receipt from {body}"))
    }

    /// Skatteetaten's answer to the delivery, or `None` while there is none.
    pub async fn outcome(
        &self,
        tokens: &TokenProvider,
        orgnumber: &str,
        dialog_id: &str,
        forsendelse_id: &str,
    ) -> Result<Option<Outcome>> {
        let token = tokens.token_for_systemuser(orgnumber).await?;
        crate::shareholder_return::dialog_outcome(
            &self.http,
            &self.dialogporten_url,
            &token,
            dialog_id,
            forsendelse_id,
        )
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::extract::{Path, State};
    use axum::http::{HeaderMap, StatusCode};
    use axum::routing::post;
    use axum::{Json, Router};
    use std::sync::{Arc, Mutex};

    /// (path, idempotencyKey, body) per call.
    type Seen = Arc<Mutex<Vec<(String, String, String)>>>;

    /// Answers like Skatteetaten's documentation: the example
    /// `Leveransebekreftelse`, or a `Feil` for the second call when
    /// `refuse` is set.
    async fn mock(refuse: bool) -> (String, Seen) {
        let seen: Seen = Default::default();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let app = Router::new()
            .route(
                "/token",
                post(|| async {
                    Json(serde_json::json!({"access_token": "sysuser-token", "expires_in": 120}))
                }),
            )
            .route(
                "/v1/{year}",
                post(
                    move |State(seen): State<Seen>,
                          Path(year): Path<String>,
                          headers: HeaderMap,
                          body: String| async move {
                        assert_eq!(headers["authorization"], "Bearer sysuser-token");
                        assert_eq!(headers["content-type"], "application/json");
                        let key = headers["idempotencyKey"].to_str().unwrap().to_string();
                        let mut calls = seen.lock().unwrap();
                        calls.push((year, key, body));
                        if refuse && calls.len() == 2 {
                            return (
                                StatusCode::BAD_REQUEST,
                                Json(
                                    serde_json::from_str::<serde_json::Value>(include_str!(
                                        "../../../docs/selvstendig/feil-eksempel.json"
                                    ))
                                    .unwrap(),
                                ),
                            );
                        }
                        (
                            StatusCode::OK,
                            Json(
                                serde_json::from_str(include_str!(
                                    "../../../docs/selvstendig/leveransebekreftelse-eksempel.json"
                                ))
                                .unwrap(),
                            ),
                        )
                    },
                ),
            )
            .with_state(seen.clone());
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        (base, seen)
    }

    #[tokio::test]
    async fn files_the_year_with_its_key_and_reads_the_receipt() {
        let (base, seen) = mock(false).await;
        let (config, _) = crate::maskinporten::tests::test_config(&format!("{base}/token"));
        let client = ContractorPaymentsClient::new(format!("{base}/v1/"), "unused");
        let key = Uuid::new_v4();
        let receipt = client
            .submit(
                &TokenProvider::new(config),
                "311032240",
                2026,
                "{\"leveranse\":{}}",
                key,
            )
            .await
            .unwrap();
        assert_eq!(
            receipt,
            Receipt {
                dialog_id: "018b3d0f-d57e-7f5c-8a04-76dbc7e2fed2".into(),
                forsendelse_id: "018f521e-5488-79e3-8817-48e94cb75455".into(),
                reference: Some("leveranse-1".into()),
                antall_oppgaver: Some(23),
            }
        );
        assert_eq!(
            *seen.lock().unwrap(),
            vec![("2026".into(), key.to_string(), "{\"leveranse\":{}}".into())]
        );
    }

    #[tokio::test]
    async fn a_refusal_carries_skatteetatens_own_words() {
        let (base, _) = mock(true).await;
        let (config, _) = crate::maskinporten::tests::test_config(&format!("{base}/token"));
        let tokens = TokenProvider::new(config);
        let client = ContractorPaymentsClient::new(format!("{base}/v1"), "unused");
        client
            .submit(&tokens, "311032240", 2026, "{}", Uuid::new_v4())
            .await
            .unwrap();
        let err = client
            .submit(&tokens, "311032240", 2026, "{}", Uuid::new_v4())
            .await
            .unwrap_err();
        let err = format!("{err:#}");
        assert!(err.contains("400") && err.contains("GLD_1052"), "{err}");
    }
}
