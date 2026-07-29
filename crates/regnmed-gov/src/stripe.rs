//! Stripe client for the card rail (#74, docs/abonnement.md §5).
//!
//! Minimal and hand-rolled, like the rest of the house's clients: three
//! calls (customer, checkout session in setup mode, off-session charge)
//! plus webhook verification. No Stripe Billing/Subscriptions — the
//! abonnement state lives in OUR hovedbok, Stripe is only a faster route
//! to "paid", and that is what keeps the provider replaceable.
//!
//! Card data never touches us: the customer types it at Stripe (hosted
//! checkout) and we store only references plus last4/brand for display.
//!
//! `base` is configurable so tests can point the client at a local mock —
//! the same pattern as the BRREG and Finanstilsynet clients.

use anyhow::{Context, Result, bail, ensure};
use sha2::{Digest, Sha256};

pub struct Stripe {
    http: reqwest::Client,
    base: String,
    secret: String,
}

impl Stripe {
    pub fn new(secret_key: &str, base_url: Option<&str>) -> Self {
        Self {
            http: reqwest::Client::new(),
            base: base_url.unwrap_or("https://api.stripe.com").to_string(),
            secret: secret_key.to_string(),
        }
    }

    async fn post_form(
        &self,
        path: &str,
        form: &[(&str, &str)],
        idempotency_key: Option<&str>,
    ) -> Result<serde_json::Value> {
        let mut req = self
            .http
            .post(format!("{}{path}", self.base))
            .basic_auth(&self.secret, None::<&str>)
            .form(form);
        if let Some(key) = idempotency_key {
            req = req.header("Idempotency-Key", key);
        }
        let resp = req.send().await.context("stripe: forespørselen feilet")?;
        let status = resp.status();
        let body: serde_json::Value = resp.json().await.context("stripe: ugyldig JSON")?;
        if !status.is_success() {
            bail!(
                "stripe {path}: {status} — {}",
                body["error"]["message"].as_str().unwrap_or("ukjent feil")
            );
        }
        Ok(body)
    }

    async fn get(&self, path: &str) -> Result<serde_json::Value> {
        let resp = self
            .http
            .get(format!("{}{path}", self.base))
            .basic_auth(&self.secret, None::<&str>)
            .send()
            .await
            .context("stripe: forespørselen feilet")?;
        let status = resp.status();
        let body: serde_json::Value = resp.json().await.context("stripe: ugyldig JSON")?;
        if !status.is_success() {
            bail!(
                "stripe {path}: {status} — {}",
                body["error"]["message"].as_str().unwrap_or("ukjent feil")
            );
        }
        Ok(body)
    }

    /// Creates a Stripe customer for a company — never reuses one, the
    /// caller checks first. The metadata binds the customer to our
    /// company, not the other way around.
    pub async fn create_customer(
        &self,
        company_id: &str,
        name: &str,
        orgnr: &str,
    ) -> Result<String> {
        let body = self
            .post_form(
                "/v1/customers",
                &[
                    ("name", name),
                    ("metadata[regnmed_company_id]", company_id),
                    ("metadata[orgnr]", orgnr),
                ],
                None,
            )
            .await?;
        body["id"]
            .as_str()
            .map(str::to_string)
            .context("stripe: kunde uten id")
    }

    /// Checkout session in SETUP mode: the customer stores a card with
    /// Stripe, without anything being charged. The result is a URL the
    /// portal sends the user to; the card comes back via webhook.
    pub async fn create_setup_session(
        &self,
        customer: &str,
        company_id: &str,
        success_url: &str,
        cancel_url: &str,
    ) -> Result<String> {
        let body = self
            .post_form(
                "/v1/checkout/sessions",
                &[
                    ("mode", "setup"),
                    ("customer", customer),
                    ("client_reference_id", company_id),
                    ("success_url", success_url),
                    ("cancel_url", cancel_url),
                ],
                None,
            )
            .await?;
        body["url"]
            .as_str()
            .map(str::to_string)
            .context("stripe: sesjon uten url")
    }

    /// Fetches the payment method a completed setup session stored.
    pub async fn setup_intent_payment_method(&self, setup_intent_id: &str) -> Result<String> {
        let body = self
            .get(&format!("/v1/setup_intents/{setup_intent_id}"))
            .await?;
        body["payment_method"]
            .as_str()
            .map(str::to_string)
            .context("stripe: setup intent uten payment_method")
    }

    /// The card's display info (brand + last4) — never the card number.
    pub async fn payment_method_card(&self, payment_method_id: &str) -> Result<(String, String)> {
        let body = self
            .get(&format!("/v1/payment_methods/{payment_method_id}"))
            .await?;
        Ok((
            body["card"]["brand"].as_str().unwrap_or("").to_string(),
            body["card"]["last4"].as_str().unwrap_or("").to_string(),
        ))
    }

    /// Off-session charge for an issued abonnement faktura.
    ///
    /// The idempotency key IS the faktura's id: the same faktura can
    /// never be charged twice, however many times the job runs. The
    /// amount is GROSS incl. mva, in øre (Stripe's smallest unit for NOK).
    pub async fn charge_invoice(
        &self,
        amount_ore: i64,
        customer: &str,
        payment_method: &str,
        invoice_id: &str,
        company_id: &str,
        description: &str,
    ) -> Result<(String, String)> {
        ensure!(amount_ore > 0, "trekk må være positivt");
        let amount = amount_ore.to_string();
        let body = self
            .post_form(
                "/v1/payment_intents",
                &[
                    ("amount", amount.as_str()),
                    ("currency", "nok"),
                    ("customer", customer),
                    ("payment_method", payment_method),
                    ("off_session", "true"),
                    ("confirm", "true"),
                    ("description", description),
                    ("metadata[invoice_id]", invoice_id),
                    ("metadata[regnmed_company_id]", company_id),
                ],
                Some(invoice_id),
            )
            .await?;
        Ok((
            body["id"].as_str().unwrap_or("").to_string(),
            body["status"].as_str().unwrap_or("").to_string(),
        ))
    }
}

/// HMAC-SHA256, hand-rolled over sha2 (RFC 2104; blokkstørrelse 64).
/// Testet mot RFC 4231-vektor under.
fn hmac_sha256(key: &[u8], msg: &[u8]) -> [u8; 32] {
    let mut k = [0u8; 64];
    if key.len() > 64 {
        k[..32].copy_from_slice(&Sha256::digest(key));
    } else {
        k[..key.len()].copy_from_slice(key);
    }
    let mut inner = Sha256::new();
    inner.update(k.map(|b| b ^ 0x36));
    inner.update(msg);
    let inner = inner.finalize();
    let mut outer = Sha256::new();
    outer.update(k.map(|b| b ^ 0x5c));
    outer.update(inner);
    outer.finalize().into()
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Verifies Stripe's webhook signature (the `Stripe-Signature` header:
/// `t=<unix>,v1=<hex>`, signed over `"{t}.{payload}"`).
///
/// `now_unix` is passed in by the caller — the clock is a dependency,
/// not a side effect, so the tolerance window can be tested.
pub fn verify_webhook(
    payload: &[u8],
    signature_header: &str,
    secret: &str,
    tolerance_secs: i64,
    now_unix: i64,
) -> Result<()> {
    let mut t: Option<i64> = None;
    let mut v1s: Vec<&str> = Vec::new();
    for part in signature_header.split(',') {
        match part.trim().split_once('=') {
            Some(("t", v)) => t = v.parse().ok(),
            Some(("v1", v)) => v1s.push(v),
            _ => {}
        }
    }
    let t = t.context("webhook: mangler t i signaturen")?;
    ensure!(!v1s.is_empty(), "webhook: mangler v1 i signaturen");
    ensure!(
        (now_unix - t).abs() <= tolerance_secs,
        "webhook: tidsstempelet er utenfor toleransen (replay?)"
    );

    let mut signed = t.to_string().into_bytes();
    signed.push(b'.');
    signed.extend_from_slice(payload);
    let expected = hex(&hmac_sha256(secret.as_bytes(), &signed));

    // Constant-time comparison across every candidate.
    let ok = v1s.iter().any(|v| {
        v.len() == expected.len()
            && v.bytes()
                .zip(expected.bytes())
                .fold(0u8, |acc, (a, b)| acc | (a ^ b))
                == 0
    });
    ensure!(ok, "webhook: signaturen stemmer ikke");
    Ok(())
}

/// Signs a payload the way Stripe would — for tests, and as
/// documentation of the format. Having both directions here is what
/// makes verification testable without Stripe.
pub fn sign_webhook(payload: &[u8], secret: &str, t_unix: i64) -> String {
    let mut signed = t_unix.to_string().into_bytes();
    signed.push(b'.');
    signed.extend_from_slice(payload);
    format!(
        "t={t_unix},v1={}",
        hex(&hmac_sha256(secret.as_bytes(), &signed))
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// RFC 4231, test case 2: key "Jefe", data "what do ya want for
    /// nothing?".
    #[test]
    fn hmac_sha256_rfc4231() {
        let mac = hmac_sha256(b"Jefe", b"what do ya want for nothing?");
        assert_eq!(
            hex(&mac),
            "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843"
        );
    }

    #[test]
    fn webhook_round_trip_and_rejections() {
        let payload = br#"{"type":"payment_intent.succeeded"}"#;
        let header = sign_webhook(payload, "whsec_test", 1_700_000_000);
        verify_webhook(payload, &header, "whsec_test", 300, 1_700_000_100).unwrap();

        // Wrong secret.
        assert!(verify_webhook(payload, &header, "whsec_annen", 300, 1_700_000_100).is_err());
        // Tampered payload.
        assert!(verify_webhook(b"{}", &header, "whsec_test", 300, 1_700_000_100).is_err());
        // Outside the tolerance window (replay).
        assert!(verify_webhook(payload, &header, "whsec_test", 300, 1_700_009_999).is_err());
    }
}
