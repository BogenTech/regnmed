//! Enable Banking (enablebanking.com): PSD2 account information through an
//! aggregator, the transport for docs/bank.md tier 2 (#70).
//!
//! The aggregator holds the eIDAS certificates; we hold an application id
//! and an RSA key, sign a short JWT per request, and never see what the
//! user types at the bank. The flow is: `start_auth` gives a URL at the
//! bank, the bank sends the browser back with a `code`, `create_session`
//! turns it into a session with the accounts consented to, and
//! `transactions` reads one of them until the consent runs out.
//!
//! Field names follow the published API reference (read 2026-11-19). They
//! have NOT been checked against live traffic yet: there is no application
//! registered. The Stripe lesson (CLAUDE.md, item 56) applies — the first
//! real session must be captured and put into the tests.

use anyhow::{Context, Result, anyhow};
use chrono::{DateTime, NaiveDate, Utc};
use jsonwebtoken::{Algorithm, EncodingKey, Header};
use regnmed_core::bankfeed::FeedTx;

use crate::bankfeed::{AuthRequest, Bank, BankFeed, FeedAccount, FeedError, Fut, Session};
use serde_json::{Value, json};

const DEFAULT_BASE: &str = "https://api.enablebanking.com";
/// A runaway stop for paging, not a size limit: the provider's page size is
/// not documented, and a first fetch asks for weeks of a busy account.
const MAX_PAGES: usize = 1000;

pub struct Client {
    app_id: String,
    key: EncodingKey,
    base: String,
    http: reqwest::Client,
}

impl Client {
    /// `key_pem` is the application's RSA private key (PKCS#1 or PKCS#8 PEM).
    pub fn new(app_id: &str, key_pem: &[u8], base: Option<&str>) -> Result<Self> {
        Ok(Self {
            app_id: app_id.to_string(),
            key: EncodingKey::from_rsa_pem(key_pem)
                .context("Enable Banking key is not an RSA PEM")?,
            base: base
                .unwrap_or(DEFAULT_BASE)
                .trim_end_matches('/')
                .to_string(),
            http: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(30))
                .build()?,
        })
    }

    /// The application token: identifies us, never the end user.
    fn token(&self, now: i64) -> Result<String> {
        let mut header = Header::new(Algorithm::RS256);
        header.kid = Some(self.app_id.clone());
        let claims = json!({
            "iss": "enablebanking.com",
            "aud": "api.enablebanking.com",
            "iat": now,
            "exp": now + 3600,
        });
        Ok(jsonwebtoken::encode(&header, &claims, &self.key)?)
    }

    async fn call(
        &self,
        method: reqwest::Method,
        path: &str,
        query: &[(&str, String)],
        body: Option<Value>,
    ) -> Result<Value, FeedError> {
        let mut request = self
            .http
            .request(method, format!("{}{path}", self.base))
            .bearer_auth(self.token(Utc::now().timestamp())?)
            .query(query);
        if let Some(body) = body {
            request = request.json(&body);
        }
        let response = request.send().await.map_err(|e| anyhow!(e))?;
        let status = response.status();
        let text = response.text().await.map_err(|e| anyhow!(e))?;
        let json: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
        if status.is_success() {
            return Ok(json);
        }
        let code = json["error"].as_str().unwrap_or("");
        let message = json["message"].as_str().unwrap_or(&text).to_string();
        Err(match code {
            "EXPIRED_SESSION" | "CLOSED_SESSION" | "REVOKED_SESSION" => {
                FeedError::ConsentGone(message)
            }
            "ASPSP_RATE_LIMIT_EXCEEDED" => FeedError::RateLimited,
            _ if status.as_u16() == 429 => FeedError::RateLimited,
            _ => FeedError::Other(anyhow!("Enable Banking {status}: {code} {message}")),
        })
    }

    /// Banks that offer business accounts in `country`.
    pub async fn banks(&self, country: &str) -> Result<Vec<Bank>, FeedError> {
        let json = self
            .call(
                reqwest::Method::GET,
                "/aspsps",
                &[
                    ("country", country.to_string()),
                    ("psu_type", "business".into()),
                ],
                None,
            )
            .await?;
        Ok(json["aspsps"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|b| {
                        Some(Bank {
                            name: b["name"].as_str()?.to_string(),
                            country: b["country"].as_str()?.to_string(),
                            maximum_consent_validity: b["maximum_consent_validity"]
                                .as_i64()
                                .unwrap_or(0),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default())
    }

    /// Starts the consent at the bank; returns the URL to send the user to.
    /// `state` comes back untouched on our redirect URL.
    pub async fn start_auth(
        &self,
        bank: &str,
        country: &str,
        state: &str,
        redirect_url: &str,
        valid_until: DateTime<Utc>,
    ) -> Result<String, FeedError> {
        let json = self
            .call(
                reqwest::Method::POST,
                "/auth",
                &[],
                Some(json!({
                    "access": { "valid_until": valid_until.to_rfc3339_opts(chrono::SecondsFormat::Secs, true) },
                    "aspsp": { "name": bank, "country": country },
                    "state": state,
                    "redirect_url": redirect_url,
                    "psu_type": "business",
                })),
            )
            .await?;
        json["url"]
            .as_str()
            .map(str::to_string)
            .ok_or_else(|| FeedError::Other(anyhow!("Enable Banking /auth answered without a url")))
    }

    /// Turns the `code` from the redirect into a session.
    pub async fn create_session(&self, code: &str) -> Result<Session, FeedError> {
        let json = self
            .call(
                reqwest::Method::POST,
                "/sessions",
                &[],
                Some(json!({ "code": code })),
            )
            .await?;
        let session_id = json["session_id"]
            .as_str()
            .ok_or_else(|| anyhow!("Enable Banking /sessions answered without a session_id"))?
            .to_string();
        let valid_until = json["access"]["valid_until"]
            .as_str()
            .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
            .map(|d| d.with_timezone(&Utc))
            .ok_or_else(|| {
                anyhow!("Enable Banking /sessions answered without access.valid_until")
            })?;
        let accounts = json["accounts"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|x| {
                        Some(FeedAccount {
                            uid: x["uid"].as_str()?.to_string(),
                            iban: x["account_id"]["iban"].as_str().map(str::to_string),
                            other: x["account_id"]["other"]["identification"]
                                .as_str()
                                .map(str::to_string),
                            currency: x["currency"].as_str().unwrap_or("").to_string(),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();
        Ok(Session {
            session_id,
            valid_until,
            accounts,
        })
    }

    /// Booked transactions from `from` (inclusive) until today. Pending and
    /// other non-booked entries are skipped, like PDNG in camt.053.
    pub async fn transactions(
        &self,
        account_uid: &str,
        from: NaiveDate,
    ) -> Result<Vec<FeedTx>, FeedError> {
        let mut out = Vec::new();
        let mut continuation: Option<String> = None;
        for _ in 0..MAX_PAGES {
            let mut query = vec![("date_from", from.to_string())];
            if let Some(key) = &continuation {
                query.push(("continuation_key", key.clone()));
            }
            let json = self
                .call(
                    reqwest::Method::GET,
                    &format!("/accounts/{account_uid}/transactions"),
                    &query,
                    None,
                )
                .await?;
            for t in json["transactions"].as_array().into_iter().flatten() {
                if t["status"].as_str() != Some("BOOK") {
                    continue;
                }
                out.push(feed_tx(t)?);
            }
            continuation = json["continuation_key"]
                .as_str()
                .filter(|k| !k.is_empty())
                .map(str::to_string);
            if continuation.is_none() {
                return Ok(out);
            }
        }
        Err(FeedError::Other(anyhow!(
            "Enable Banking kept paging past {MAX_PAGES} pages; refusing a partial import"
        )))
    }

    /// Ends the session at the provider. Best effort for the caller: our
    /// own row is ended regardless.
    pub async fn end_session(&self, session_id: &str) -> Result<(), FeedError> {
        self.call(
            reqwest::Method::DELETE,
            &format!("/sessions/{session_id}"),
            &[],
            None,
        )
        .await
        .map(|_| ())
    }
}

impl BankFeed for Client {
    fn id(&self) -> &'static str {
        "enablebanking"
    }
    fn banks<'a>(&'a self, country: &'a str) -> Fut<'a, Vec<Bank>> {
        Box::pin(Client::banks(self, country))
    }
    fn start_auth<'a>(&'a self, r: &'a AuthRequest<'a>) -> Fut<'a, String> {
        Box::pin(Client::start_auth(
            self,
            r.bank,
            r.country,
            r.state,
            r.redirect_url,
            r.valid_until,
        ))
    }
    fn create_session<'a>(&'a self, code: &'a str) -> Fut<'a, Session> {
        Box::pin(Client::create_session(self, code))
    }
    fn transactions<'a>(&'a self, account_uid: &'a str, from: NaiveDate) -> Fut<'a, Vec<FeedTx>> {
        Box::pin(Client::transactions(self, account_uid, from))
    }
    fn end_session<'a>(&'a self, session_id: &'a str) -> Fut<'a, ()> {
        Box::pin(Client::end_session(self, session_id))
    }
}

/// One provider transaction -> the neutral shape. A transaction we cannot
/// read fails the whole fetch: a half import would hide a missing line.
fn feed_tx(t: &Value) -> Result<FeedTx> {
    let booking_date = t["booking_date"]
        .as_str()
        .or_else(|| t["value_date"].as_str())
        .and_then(|d| NaiveDate::parse_from_str(d, "%Y-%m-%d").ok())
        .context("transaction without a booking date")?;
    let credit = match t["credit_debit_indicator"].as_str() {
        Some("CRDT") => true,
        Some("DBIT") => false,
        other => anyhow::bail!("transaction with unknown direction {other:?}"),
    };
    let remittance: Vec<&str> = t["remittance_information"]
        .as_array()
        .map(|a| a.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    // Who it was to or from, then what it said: the same line a statement shows.
    let counterparty = if credit {
        &t["debtor"]["name"]
    } else {
        &t["creditor"]["name"]
    };
    let description = [counterparty.as_str().unwrap_or(""), &remittance.join(" ")]
        .iter()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(" · ");
    Ok(FeedTx {
        entry_reference: t["entry_reference"].as_str().map(str::to_string),
        booking_date,
        amount: t["transaction_amount"]["amount"]
            .as_str()
            .context("transaction without an amount")?
            .to_string(),
        credit,
        description,
        reference: t["reference_number"]
            .as_str()
            .filter(|r| !r.trim().is_empty())
            .map(str::to_string),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_documented_transaction_becomes_a_feed_line() {
        // Shape from the API reference's own example fields.
        let t = json!({
            "entry_reference": "5561990681",
            "transaction_amount": { "currency": "NOK", "amount": "1250.00" },
            "creditor": { "name": "Nordlys Kontor AS" },
            "debtor": { "name": "Fjordvik Konsult AS" },
            "credit_debit_indicator": "DBIT",
            "status": "BOOK",
            "booking_date": "2026-11-03",
            "value_date": "2026-11-03",
            "remittance_information": ["Faktura 1042", "KID 10420"],
            "reference_number": "10420"
        });
        let tx = feed_tx(&t).unwrap();
        assert_eq!(tx.entry_reference.as_deref(), Some("5561990681"));
        assert!(!tx.credit);
        assert_eq!(tx.amount, "1250.00");
        assert_eq!(tx.description, "Nordlys Kontor AS · Faktura 1042 KID 10420");
        assert_eq!(tx.reference.as_deref(), Some("10420"));
    }

    #[test]
    fn an_unreadable_transaction_fails_instead_of_being_skipped() {
        assert!(feed_tx(&json!({ "status": "BOOK", "booking_date": "2026-11-03" })).is_err());
        assert!(
            feed_tx(&json!({
                "booking_date": "2026-11-03", "credit_debit_indicator": "CRDT",
                "transaction_amount": {}
            }))
            .is_err()
        );
    }
}
