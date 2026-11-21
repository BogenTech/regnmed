//! Recommand, the Peppol access point (docs/efaktura.md §2.3).
//!
//! Only `regnmed work` calls this client. The one exception is
//! [`verify_webhook`], which the API needs to accept an event.
//!
//! Recommand has no idempotency key on send, so [`SendOutcome`] is the
//! safety boundary: anything that does not prove the document stayed here
//! is `Unresolved`, and an unresolved send is never retried automatically.

use std::time::Duration;

use anyhow::{Context, Result, bail};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::stripe::{hex, hmac_sha256};

pub const INVOICE_DOCTYPE: &str = "busdox-docid-qns::urn:oasis:names:specification:ubl:schema:xsd:Invoice-2::Invoice##urn:cen.eu:en16931:2017#compliant#urn:fdc:peppol.eu:2017:poacc:billing:3.0::2.1";
pub const CREDIT_NOTE_DOCTYPE: &str = "busdox-docid-qns::urn:oasis:names:specification:ubl:schema:xsd:CreditNote-2::CreditNote##urn:cen.eu:en16931:2017#compliant#urn:fdc:peppol.eu:2017:poacc:billing:3.0::2.1";
pub const BILLING_PROCESS: &str = "cenbii-procid-ubl::urn:fdc:peppol.eu:2017:poacc:billing:01:1.0";

const DEFAULT_BASE: &str = "https://app.recommand.eu";
const PAGE_SIZE: u32 = 100;
const CLOCK_SKEW_MINUTES: i64 = 5;

pub struct Recommand {
    http: reqwest::Client,
    base: String,
    key: String,
    secret: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sender {
    pub provider_company_id: String,
    pub is_verified: bool,
    pub verification_url: Option<String>,
}

/// The Peppol delivery as the provider reports it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DeliveryReport {
    pub status: Option<String>,
    pub failure_category: Option<String>,
    pub failure_message: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SendOutcome {
    Accepted {
        document_id: String,
        delivery: DeliveryReport,
    },
    /// The provider refused the document, so nothing was transmitted.
    Rejected(String),
    /// The request provably never reached the provider's handler.
    NotSent(String),
    /// Nobody knows whether the provider took the document.
    Unresolved(String),
}

#[derive(Debug, Clone)]
pub struct ProviderDocument {
    pub id: String,
    pub xml: Option<String>,
    pub delivery: DeliveryReport,
}

impl Recommand {
    pub fn new(key: &str, secret: &str, base_url: Option<&str>) -> Result<Self> {
        Ok(Self {
            http: reqwest::Client::builder()
                .connect_timeout(Duration::from_secs(10))
                .timeout(Duration::from_secs(60))
                // A redirected POST could be replayed somewhere else.
                .redirect(reqwest::redirect::Policy::none())
                .build()?,
            base: base_url
                .unwrap_or(DEFAULT_BASE)
                .trim_end_matches('/')
                .to_string(),
            key: key.to_string(),
            secret: secret.to_string(),
        })
    }

    /// Both credentials or none: half a configuration is an operator error.
    pub fn from_env() -> Result<Option<Self>> {
        let key = std::env::var("RECOMMAND_API_KEY").ok();
        let secret = std::env::var("RECOMMAND_API_SECRET").ok();
        match (key, secret) {
            (Some(key), Some(secret)) => Ok(Some(Self::new(
                &key,
                &secret,
                std::env::var("RECOMMAND_API_URL").ok().as_deref(),
            )?)),
            (None, None) => Ok(None),
            _ => bail!("RECOMMAND_API_KEY and RECOMMAND_API_SECRET must be set together"),
        }
    }

    fn url(&self, path: &str) -> String {
        format!("{}/api/v1{path}", self.base)
    }

    async fn read(response: reqwest::Response, what: &str) -> Result<Value> {
        let status = response.status();
        let body: Value = response
            .json()
            .await
            .with_context(|| format!("recommand {what}: invalid JSON ({status})"))?;
        if !status.is_success() || body["success"] != true {
            bail!("recommand {what}: {status}: {}", error_text(&body));
        }
        Ok(body)
    }

    /// Always sender-only: the API's default would publish the company as
    /// a receiver in ELMA, which obliges its suppliers to send EHF there.
    pub async fn create_sender(
        &self,
        name: &str,
        address: &str,
        postal_code: &str,
        city: &str,
        orgnumber: &str,
        vat_number: Option<&str>,
    ) -> Result<Sender> {
        let response = self
            .http
            .post(self.url("/companies"))
            .basic_auth(&self.key, Some(&self.secret))
            .json(&json!({
                "name": name,
                "address": address,
                "postalCode": postal_code,
                "city": city,
                "country": "NO",
                "enterpriseNumberScheme": "0192",
                "enterpriseNumber": orgnumber,
                "vatNumber": vat_number,
                "isSmpRecipient": false,
            }))
            .send()
            .await
            .context("recommand create company")?;
        let body = Self::read(response, "create company").await?;
        Ok(Sender {
            provider_company_id: body["company"]["id"]
                .as_str()
                .context("recommand: company without id")?
                .to_string(),
            is_verified: body["company"]["isVerified"] == true,
            verification_url: body["verificationUrl"].as_str().map(str::to_string),
        })
    }

    /// The existing company for an org number, so a setup that crashed
    /// after creating it can continue. A company published as a receiver
    /// is refused: receiving is a separate, explicit decision.
    pub async fn find_sender(&self, orgnumber: &str) -> Result<Option<Sender>> {
        let response = self
            .http
            .get(self.url("/companies"))
            .basic_auth(&self.key, Some(&self.secret))
            .send()
            .await
            .context("recommand list companies")?;
        let body = Self::read(response, "list companies").await?;
        let companies = body["companies"]
            .as_array()
            .context("recommand: list without companies")?;
        let Some(company) = companies
            .iter()
            .find(|c| c["enterpriseNumberScheme"] == "0192" && c["enterpriseNumber"] == orgnumber)
        else {
            return Ok(None);
        };
        if company["isSmpRecipient"] != false {
            bail!("recommand: company {orgnumber} is published as a Peppol receiver");
        }
        Ok(Some(Sender {
            provider_company_id: company["id"]
                .as_str()
                .context("recommand: company without id")?
                .to_string(),
            is_verified: company["isVerified"] == true,
            verification_url: None,
        }))
    }

    pub async fn is_verified(&self, provider_company_id: &str) -> Result<bool> {
        let response = self
            .http
            .get(self.url(&format!("/companies/{provider_company_id}")))
            .basic_auth(&self.key, Some(&self.secret))
            .send()
            .await
            .context("recommand get company")?;
        Ok(Self::read(response, "get company").await?["company"]["isVerified"] == true)
    }

    /// Safe to repeat: an open verification session keeps its URL.
    pub async fn verification_url(&self, provider_company_id: &str) -> Result<String> {
        let response = self
            .http
            .post(self.url(&format!("/companies/{provider_company_id}/verify")))
            .basic_auth(&self.key, Some(&self.secret))
            .send()
            .await
            .context("recommand verify company")?;
        Self::read(response, "verify company").await?["verificationUrl"]
            .as_str()
            .map(str::to_string)
            .context("recommand: verification without url")
    }

    /// `false` does not prove absence: Recommand also answers it when its
    /// SML or SMP lookup failed. See `peppol_directory`.
    pub async fn supports_document(&self, receiver: &str, doctype: &str) -> Result<bool> {
        let response = self
            .http
            .post(self.url("/verify-document-support"))
            .basic_auth(&self.key, Some(&self.secret))
            .json(&json!({
                "peppolAddress": receiver,
                "documentType": doctype,
                "processId": BILLING_PROCESS,
            }))
            .send()
            .await
            .context("recommand document support")?;
        Ok(Self::read(response, "document support").await?["isValid"] == true)
    }

    /// One POST and never a retry. The caller has already committed the
    /// attempt, and decides what the outcome permits.
    pub async fn send_xml(
        &self,
        provider_company_id: &str,
        receiver: &str,
        xml: &str,
        doctype: &str,
    ) -> SendOutcome {
        let response = self
            .http
            .post(self.url(&format!("/{provider_company_id}/send")))
            .basic_auth(&self.key, Some(&self.secret))
            .json(&json!({
                "recipient": receiver,
                "documentType": "xml",
                "document": xml,
                "doctypeId": doctype,
                "processId": BILLING_PROCESS,
            }))
            .send()
            .await;
        let response = match response {
            Ok(response) => response,
            Err(e) if e.is_connect() || e.is_builder() => {
                return SendOutcome::NotSent(format!("{e:#}"));
            }
            Err(e) => return SendOutcome::Unresolved(format!("{e:#}")),
        };
        let status = response.status().as_u16();
        match response.text().await {
            Ok(text) => classify_send(status, &text),
            // The status arrived, but a 2xx body we cannot read still means
            // the provider may hold the document.
            Err(e) if status < 400 => SendOutcome::Unresolved(format!("{e:#}")),
            Err(e) => classify_send(status, &format!("{{\"message\":\"{e}\"}}")),
        }
    }

    /// Outgoing documents since `from` whose XML hashes to `sha256`. Reads
    /// every page, because a missed match can become a duplicate invoice.
    pub async fn find_outgoing_by_sha256(
        &self,
        provider_company_id: &str,
        from: chrono::DateTime<chrono::Utc>,
        sha256: &[u8; 32],
    ) -> Result<Vec<String>> {
        // `from` is our clock and `createdAt` is theirs.
        let from = from - chrono::Duration::minutes(CLOCK_SKEW_MINUTES);
        let mut matches = Vec::new();
        let mut page = 1u32;
        loop {
            let response = self
                .http
                .get(self.url("/documents"))
                .basic_auth(&self.key, Some(&self.secret))
                .query(&[
                    ("companyId", provider_company_id.to_string()),
                    ("direction", "outgoing".to_string()),
                    ("from", from.to_rfc3339()),
                    ("limit", PAGE_SIZE.to_string()),
                    ("page", page.to_string()),
                ])
                .send()
                .await
                .context("recommand list documents")?;
            let body = Self::read(response, "list documents").await?;
            let documents = body["documents"]
                .as_array()
                .context("recommand: list without documents")?;
            for document in documents {
                let id = document["id"]
                    .as_str()
                    .context("recommand: document without id")?;
                let full = self.document(id).await?;
                if full
                    .xml
                    .is_some_and(|xml| Sha256::digest(xml.as_bytes()).as_slice() == sha256)
                {
                    matches.push(id.to_string());
                }
            }
            let total_pages = body["pagination"]["totalPages"]
                .as_u64()
                .context("recommand: list without totalPages")?;
            if u64::from(page) >= total_pages {
                return Ok(matches);
            }
            page += 1;
        }
    }

    pub async fn document(&self, id: &str) -> Result<ProviderDocument> {
        let response = self
            .http
            .get(self.url(&format!("/documents/{id}")))
            .basic_auth(&self.key, Some(&self.secret))
            .send()
            .await
            .context("recommand get document")?;
        let body = Self::read(response, "get document").await?;
        let document = &body["document"];
        Ok(ProviderDocument {
            id: document["id"]
                .as_str()
                .context("recommand: document without id")?
                .to_string(),
            xml: document["xml"].as_str().map(str::to_string),
            delivery: delivery_report(document),
        })
    }
}

/// A document can have an e-mail delivery too; only Peppol counts here.
fn delivery_report(document: &Value) -> DeliveryReport {
    let peppol = document["deliveries"]
        .as_array()
        .and_then(|all| all.iter().find(|d| d["channel"] == "peppol"));
    let source = peppol.unwrap_or(document);
    let status = source["status"]
        .as_str()
        .or_else(|| document["deliveryStatus"].as_str());
    DeliveryReport {
        status: status.map(str::to_string),
        failure_category: source["failure"]["category"].as_str().map(str::to_string),
        failure_message: source["failure"]["message"]
            .as_str()
            .unwrap_or_default()
            .to_string(),
    }
}

fn error_text(body: &Value) -> String {
    if let Some(errors) = body["errors"].as_object() {
        let mut parts = Vec::new();
        for (field, messages) in errors {
            for message in messages.as_array().into_iter().flatten() {
                let message = message.as_str().unwrap_or_default();
                parts.push(if field == "root" {
                    message.to_string()
                } else {
                    format!("{field}: {message}")
                });
            }
        }
        if !parts.is_empty() {
            return parts.join("; ");
        }
    }
    body["message"]
        .as_str()
        .unwrap_or("unknown error")
        .to_string()
}

/// What an HTTP answer to a send proves.
fn classify_send(status: u16, text: &str) -> SendOutcome {
    let body: Value = serde_json::from_str(text).unwrap_or(Value::Null);
    let detail = error_text(&body);
    match status {
        200..=299 => match body["id"].as_str() {
            Some(id) if body["success"] == true => SendOutcome::Accepted {
                document_id: id.to_string(),
                delivery: delivery_report(&body),
            },
            _ => SendOutcome::Unresolved(format!("HTTP {status} without a document id")),
        },
        // Rate limiting happens before the document is handled.
        429 => SendOutcome::NotSent(detail),
        422 => match body["deliveryFailure"]["category"].as_str() {
            Some(
                "recipient_not_found"
                | "document_not_supported"
                | "validation"
                | "recipient_rejected",
            ) => SendOutcome::Rejected(detail),
            // A transport failure may have happened after transmission began.
            _ => SendOutcome::Unresolved(detail),
        },
        400 | 401 | 403 | 404 => SendOutcome::Rejected(detail),
        _ => SendOutcome::Unresolved(format!("HTTP {status}: {detail}")),
    }
}

/// Checks `X-Signature: sha256=<hex>` over the exact raw request body.
pub fn verify_webhook(body: &[u8], signature: &str, secret: &str) -> bool {
    let Some(received) = signature.strip_prefix("sha256=") else {
        return false;
    };
    let expected = hex(&hmac_sha256(secret.as_bytes(), body));
    received.len() == expected.len()
        && received
            .bytes()
            .zip(expected.bytes())
            .fold(0u8, |diff, (a, b)| diff | (a ^ b))
            == 0
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::StatusCode;
    use axum::{Json, Router, routing::post};

    #[test]
    fn only_proof_of_non_delivery_is_retryable() {
        assert!(matches!(
            classify_send(
                200,
                r#"{"success":true,"id":"doc_1","deliveryStatus":"delivered"}"#
            ),
            SendOutcome::Accepted { .. }
        ));
        assert!(matches!(
            classify_send(200, "not json"),
            SendOutcome::Unresolved(_)
        ));
        assert!(matches!(
            classify_send(503, "{}"),
            SendOutcome::Unresolved(_)
        ));
        assert!(matches!(classify_send(429, "{}"), SendOutcome::NotSent(_)));
        assert!(matches!(
            classify_send(
                422,
                r#"{"errors":{"root":["no"]},"deliveryFailure":{"category":"transport"}}"#
            ),
            SendOutcome::Unresolved(_)
        ));
    }

    #[test]
    fn provider_validation_errors_are_readable_rejections() {
        let body = r#"{"success":false,"errors":{"root":["Document validation failed."],"/:Invoice[1]":["BR-CO-16: wrong"]}}"#;
        let SendOutcome::Rejected(detail) = classify_send(400, body) else {
            panic!("a validation error is a rejection");
        };
        assert!(detail.contains("BR-CO-16"), "{detail}");
        assert!(matches!(
            classify_send(
                422,
                r#"{"errors":{"root":["missing"]},"deliveryFailure":{"category":"recipient_not_found"}}"#
            ),
            SendOutcome::Rejected(_)
        ));
    }

    #[test]
    fn the_peppol_delivery_decides_the_report() {
        let report = delivery_report(&json!({
            "deliveryStatus": "delivered",
            "deliveries": [
                { "channel": "email", "status": "delivered", "failure": null },
                { "channel": "peppol", "status": "failed",
                  "failure": { "category": "transport", "message": "lost" } },
            ],
        }));
        assert_eq!(report.status.as_deref(), Some("failed"));
        assert_eq!(report.failure_category.as_deref(), Some("transport"));
        assert_eq!(report.failure_message, "lost");
    }

    #[test]
    fn webhook_signature_covers_the_exact_body() {
        let body = br#"{"eventType":"document.sent"}"#;
        let signature = format!("sha256={}", hex(&hmac_sha256(b"secret", body)));
        assert!(verify_webhook(body, &signature, "secret"));
        assert!(!verify_webhook(b"{ }", &signature, "secret"));
        assert!(!verify_webhook(body, &signature, "other"));
        assert!(!verify_webhook(body, &signature[7..], "secret"));
    }

    async fn serve(app: Router) -> String {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        format!("http://{address}")
    }

    #[tokio::test]
    async fn a_refused_connection_is_not_sent() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        drop(listener);
        let client = Recommand::new("k", "s", Some(&format!("http://{address}"))).unwrap();
        assert!(matches!(
            client
                .send_xml("c_1", "0192:1", "<x/>", INVOICE_DOCTYPE)
                .await,
            SendOutcome::NotSent(_)
        ));
    }

    #[tokio::test]
    async fn a_server_error_after_the_post_is_unresolved() {
        let base = serve(Router::new().route(
            "/api/v1/{company}/send",
            post(|| async { (StatusCode::BAD_GATEWAY, "gateway") }),
        ))
        .await;
        let client = Recommand::new("k", "s", Some(&base)).unwrap();
        assert!(matches!(
            client
                .send_xml("c_1", "0192:1", "<x/>", INVOICE_DOCTYPE)
                .await,
            SendOutcome::Unresolved(_)
        ));
    }

    #[tokio::test]
    async fn a_new_sender_is_never_published_as_a_receiver() {
        let base = serve(Router::new().route(
            "/api/v1/companies",
            post(|Json(body): Json<Value>| async move {
                assert_eq!(body["isSmpRecipient"], false);
                assert_eq!(body["enterpriseNumberScheme"], "0192");
                Json(json!({
                    "success": true,
                    "company": { "id": "c_1", "isVerified": false },
                    "verificationUrl": "https://verify.example/1",
                }))
            }),
        ))
        .await;
        let client = Recommand::new("k", "s", Some(&base)).unwrap();
        let sender = client
            .create_sender("A AS", "Vei 1", "0150", "Oslo", "123456785", None)
            .await
            .unwrap();
        assert_eq!(sender.provider_company_id, "c_1");
        assert!(!sender.is_verified);
    }

    #[tokio::test]
    async fn an_interrupted_setup_finds_its_sender_but_never_a_receiver() {
        let app = Router::new().route(
            "/api/v1/companies",
            axum::routing::get(|| async {
                Json(json!({ "success": true, "companies": [
                    { "id": "c_send", "enterpriseNumberScheme": "0192",
                      "enterpriseNumber": "111111111", "isSmpRecipient": false,
                      "isVerified": true },
                    { "id": "c_recv", "enterpriseNumberScheme": "0192",
                      "enterpriseNumber": "222222222", "isSmpRecipient": true,
                      "isVerified": true },
                ]}))
            }),
        );
        let client = Recommand::new("k", "s", Some(&serve(app).await)).unwrap();
        let found = client.find_sender("111111111").await.unwrap().unwrap();
        assert_eq!(found.provider_company_id, "c_send");
        assert!(client.find_sender("333333333").await.unwrap().is_none());
        assert!(client.find_sender("222222222").await.is_err());
    }

    #[tokio::test]
    async fn hash_search_reads_every_page() {
        let wanted = "<wanted/>";
        let app = Router::new()
            .route(
                "/api/v1/documents",
                axum::routing::get(
                    |axum::extract::Query(query): axum::extract::Query<
                        std::collections::HashMap<String, String>,
                    >| async move {
                        let page = query["page"].as_str();
                        let id = if page == "1" { "doc_a" } else { "doc_b" };
                        Json(json!({
                            "success": true,
                            "documents": [{ "id": id }],
                            "pagination": { "totalPages": 2 },
                        }))
                    },
                ),
            )
            .route(
                "/api/v1/documents/{id}",
                axum::routing::get(
                    |axum::extract::Path(id): axum::extract::Path<String>| async move {
                        let xml = if id == "doc_b" {
                            "<wanted/>"
                        } else {
                            "<other/>"
                        };
                        Json(json!({
                            "success": true,
                            "document": { "id": id, "xml": xml, "deliveryStatus": "delivered" },
                        }))
                    },
                ),
            );
        let client = Recommand::new("k", "s", Some(&serve(app).await)).unwrap();
        let hash: [u8; 32] = Sha256::digest(wanted.as_bytes()).into();
        let found = client
            .find_outgoing_by_sha256("c_1", chrono::Utc::now(), &hash)
            .await
            .unwrap();
        assert_eq!(found, ["doc_b"]);
    }
}
