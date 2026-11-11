//! Skatteetatens API for the aksjonærregisteroppgave (RF-1086,
//! docs/aksjonaer.md, #182). OpenAPI 1.3.0 on SwaggerHub
//! (`skatteetaten/innrapportering-aksjonaerregister-api`).
//!
//! `regnmed_core::shareholder_return` renders the forms; this module moves
//! them. One filing is three steps, each with its own `idempotencyKey`:
//!
//! 1. `POST {base}/{inntektsaar}/1086H` — the hovedskjema (XML). Answers
//!    `{"hovedskjemaId": uuid}`.
//! 2. `POST {base}/{inntektsaar}/{hovedskjemaId}/1086U` — one underskjema
//!    per shareholder (XML). Answers 200 with no body.
//! 3. `POST {base}/{inntektsaar}/{hovedskjemaId}/bekreft?antall_underskjema=N`
//!    — closes the filing. Answers the `Leveransebekreftelse`
//!    (`dialogId`, `forsendelseId`), the only proof of delivery.
//!
//! Every call carries the **systembruker token for the company** (the
//! oppgavegiver). A key reused with the same body gets the first answer
//! back, so a retry of a filing that never got its receipt resends every
//! step with the keys it was stored with — nothing is filed twice.

use anyhow::{Context, Result, bail};
use serde::Deserialize;
use uuid::Uuid;

use crate::maskinporten::TokenProvider;

pub const SCOPES: &str = "skatteetaten:innrapporteringaksjonaerregisteroppgave";

pub struct ShareholderReturnClient {
    /// `https://api-test.sits.no/api/aksjonaerregister/v1` in test.
    pub base_url: String,
    http: reqwest::Client,
}

/// Skatteetatens `Leveransebekreftelse`. Proof of delivery, not of an
/// approved oppgave — Skatteetaten processes it afterwards.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Receipt {
    pub dialog_id: String,
    pub forsendelse_id: String,
    #[serde(default)]
    pub oppgavegivers_leveranse_referanse: Option<String>,
}

/// One form and the key it is always sent with.
pub struct Form<'a> {
    pub xml: &'a str,
    pub idempotency_key: Uuid,
}

#[derive(Deserialize)]
struct MainFormId {
    // The spec says `hovedskjemaId`, the example in the documentation
    // `hovedskjemaid`; take either.
    #[serde(rename = "hovedskjemaId", alias = "hovedskjemaid")]
    id: String,
}

impl ShareholderReturnClient {
    pub fn new(base_url: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into().trim_end_matches('/').to_string(),
            http: reqwest::Client::new(),
        }
    }

    /// Reads AKSJONAERREGISTER_URL, defaulting to the TEST environment —
    /// production is opted into, never fallen into.
    pub fn from_env() -> Self {
        Self::new(
            std::env::var("AKSJONAERREGISTER_URL")
                .unwrap_or_else(|_| "https://api-test.sits.no/api/aksjonaerregister/v1".into()),
        )
    }

    /// Files one oppgave: hovedskjema, every underskjema, bekreft.
    pub async fn submit(
        &self,
        tokens: &TokenProvider,
        orgnumber: &str,
        income_year: i32,
        main_form: Form<'_>,
        sub_forms: &[Form<'_>],
        confirm_key: Uuid,
    ) -> Result<Receipt> {
        let token = tokens.token_for_systemuser(orgnumber).await?;
        let base = format!("{}/{income_year}", self.base_url);

        let body = self
            .post(&format!("{base}/1086H"), &token, &main_form)
            .await
            .context("sending the hovedskjema")?;
        let main_id = serde_json::from_str::<MainFormId>(&body)
            .with_context(|| format!("reading the hovedskjema id from {body}"))?
            .id;

        for (i, form) in sub_forms.iter().enumerate() {
            self.post(&format!("{base}/{main_id}/1086U"), &token, form)
                .await
                .with_context(|| format!("sending underskjema {} of {}", i + 1, sub_forms.len()))?;
        }

        let body = self
            .post(
                &format!(
                    "{base}/{main_id}/bekreft?antall_underskjema={}",
                    sub_forms.len()
                ),
                &token,
                &Form {
                    xml: "",
                    idempotency_key: confirm_key,
                },
            )
            .await
            .context("confirming the oppgave")?;
        serde_json::from_str(&body).with_context(|| format!("reading the receipt from {body}"))
    }

    async fn post(&self, url: &str, token: &str, form: &Form<'_>) -> Result<String> {
        let mut request = self
            .http
            .post(url)
            .bearer_auth(token)
            .header("idempotencyKey", form.idempotency_key.to_string())
            .header("accept", "application/json");
        if !form.xml.is_empty() {
            request = request
                .header("content-type", "application/xml")
                .body(form.xml.to_string());
        }
        let response = request.send().await?;
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        if !status.is_success() {
            // Skatteetatens `Feil` (kode, melding, spesifisering) verbatim:
            // it names the field and what is wrong with it.
            bail!("{status}: {body}");
        }
        Ok(body)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::extract::{Path, Query, State};
    use axum::http::{HeaderMap, StatusCode};
    use axum::routing::post;
    use axum::{Json, Router};
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};

    #[derive(Default)]
    struct Seen {
        /// (path, idempotencyKey, body) per call, in order.
        calls: Vec<(String, String, String)>,
    }

    type S = State<Arc<Mutex<Seen>>>;

    fn record(seen: &S, path: String, headers: &HeaderMap, body: String) {
        assert_eq!(headers["authorization"], "Bearer sysuser-token");
        let key = headers["idempotencyKey"].to_str().unwrap().to_string();
        seen.0.lock().unwrap().calls.push((path, key, body));
    }

    async fn mock(fail_second_sub: bool) -> (String, Arc<Mutex<Seen>>) {
        let seen = Arc::new(Mutex::new(Seen::default()));
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
                "/v1/{year}/1086H",
                post(
                    |seen: S, Path(year): Path<String>, headers: HeaderMap, body: String| async move {
                        assert_eq!(headers["content-type"], "application/xml");
                        record(&seen, format!("{year}/1086H"), &headers, body);
                        Json(serde_json::json!({"hovedskjemaId": "h-1"}))
                    },
                ),
            )
            .route(
                "/v1/{year}/{main}/1086U",
                post(
                    move |seen: S,
                          Path((year, main)): Path<(String, String)>,
                          headers: HeaderMap,
                          body: String| async move {
                        record(&seen, format!("{year}/{main}/1086U"), &headers, body);
                        let subs = seen.0.lock().unwrap().calls.len() - 1;
                        if fail_second_sub && subs == 2 {
                            return (
                                StatusCode::BAD_REQUEST,
                                Json(serde_json::json!({"kode": "GLD_010", "melding": "feil i post 23"})),
                            );
                        }
                        (StatusCode::OK, Json(serde_json::json!(null)))
                    },
                ),
            )
            .route(
                "/v1/{year}/{main}/bekreft",
                post(
                    |seen: S,
                     Path((year, main)): Path<(String, String)>,
                     Query(q): Query<HashMap<String, String>>,
                     headers: HeaderMap| async move {
                        record(
                            &seen,
                            format!("{year}/{main}/bekreft?{}", q["antall_underskjema"]),
                            &headers,
                            String::new(),
                        );
                        Json(serde_json::json!({
                            "oppgavegiversLeveranseReferanse": "h-1",
                            "dialogId": "d-1",
                            "forsendelseId": "f-1"
                        }))
                    },
                ),
            )
            .with_state(seen.clone());
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        (base, seen)
    }

    #[tokio::test]
    async fn files_main_form_every_sub_form_then_confirms() {
        let (base, seen) = mock(false).await;
        let (config, _) = crate::maskinporten::tests::test_config(&format!("{base}/token"));
        let tokens = TokenProvider::new(config);
        let client = ShareholderReturnClient::new(format!("{base}/v1/"));
        let keys: Vec<Uuid> = (0..4).map(|_| Uuid::new_v4()).collect();
        let form = |xml, i: usize| Form {
            xml,
            idempotency_key: keys[i],
        };

        let receipt = client
            .submit(
                &tokens,
                "311032240",
                2026,
                form("<H/>", 0),
                &[form("<U1/>", 1), form("<U2/>", 2)],
                keys[3],
            )
            .await
            .unwrap();
        assert_eq!(
            receipt,
            Receipt {
                dialog_id: "d-1".into(),
                forsendelse_id: "f-1".into(),
                oppgavegivers_leveranse_referanse: Some("h-1".into()),
            }
        );
        let calls = &seen.lock().unwrap().calls;
        let k = |i: usize| keys[i].to_string();
        assert_eq!(
            *calls,
            vec![
                ("2026/1086H".into(), k(0), "<H/>".into()),
                ("2026/h-1/1086U".into(), k(1), "<U1/>".into()),
                ("2026/h-1/1086U".into(), k(2), "<U2/>".into()),
                ("2026/h-1/bekreft?2".into(), k(3), String::new()),
            ]
        );
    }

    #[tokio::test]
    async fn a_refused_sub_form_stops_before_bekreft_and_says_which() {
        let (base, seen) = mock(true).await;
        let (config, _) = crate::maskinporten::tests::test_config(&format!("{base}/token"));
        let client = ShareholderReturnClient::new(format!("{base}/v1"));
        let f = |xml| Form {
            xml,
            idempotency_key: Uuid::new_v4(),
        };
        let err = client
            .submit(
                &TokenProvider::new(config),
                "311032240",
                2026,
                f("<H/>"),
                &[f("<U1/>"), f("<U2/>"), f("<U3/>")],
                Uuid::new_v4(),
            )
            .await
            .unwrap_err();
        let err = format!("{err:#}");
        assert!(
            err.contains("underskjema 2 of 3") && err.contains("GLD_010"),
            "{err}"
        );
        // Never confirmed: an oppgave with a missing shareholder must not
        // be closed.
        assert_eq!(seen.lock().unwrap().calls.len(), 3);
    }
}
