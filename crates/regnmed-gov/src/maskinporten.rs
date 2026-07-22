//! Maskinporten: OAuth2 tokens for machine-to-machine access to
//! Norwegian government APIs (Digdir).
//!
//! Flow (RFC 7523 JWT grant): sign a short-lived assertion with the key
//! registered on our Maskinporten client, POST it to the token endpoint,
//! receive an access token scoped to e.g.
//! `skatteetaten:mvameldingvalidering`. Tokens are cached until shortly
//! before expiry.
//!
//! Environments: test `https://test.maskinporten.no`, production
//! `https://maskinporten.no`. Client registration happens in Digdir's
//! Samarbeidsportalen; see docs/gov.md for the operational steps.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use jsonwebtoken::{Algorithm, EncodingKey, Header};
use serde::Deserialize;

#[derive(Clone)]
pub struct MaskinportenConfig {
    /// Token endpoint, e.g. `https://test.maskinporten.no/token`.
    pub token_endpoint: String,
    /// The `aud` of the grant assertion — the Maskinporten issuer,
    /// e.g. `https://test.maskinporten.no/`.
    pub audience: String,
    /// Our client id (UUID from Samarbeidsportalen).
    pub client_id: String,
    /// RS256 private key registered on the client (PEM).
    pub encoding_key: EncodingKey,
    /// Key id, when the client has multiple registered keys.
    pub kid: Option<String>,
    /// Space-separated scopes, e.g. `skatteetaten:mvameldingvalidering`.
    pub scopes: String,
}

impl MaskinportenConfig {
    /// Reads MASKINPORTEN_TOKEN_ENDPOINT, MASKINPORTEN_AUDIENCE,
    /// MASKINPORTEN_CLIENT_ID, MASKINPORTEN_KEY_FILE (PEM path),
    /// MASKINPORTEN_KID (optional) and MASKINPORTEN_SCOPES.
    pub fn from_env() -> Result<Self> {
        let var = |name: &str| {
            std::env::var(name).with_context(|| format!("{name} is not set — see docs/gov.md"))
        };
        let key_file = var("MASKINPORTEN_KEY_FILE")?;
        let pem = std::fs::read(&key_file)
            .with_context(|| format!("reading MASKINPORTEN_KEY_FILE {key_file}"))?;
        Ok(Self {
            token_endpoint: var("MASKINPORTEN_TOKEN_ENDPOINT")?,
            audience: var("MASKINPORTEN_AUDIENCE")?,
            client_id: var("MASKINPORTEN_CLIENT_ID")?,
            encoding_key: EncodingKey::from_rsa_pem(&pem).context("parsing RS256 private key")?,
            kid: std::env::var("MASKINPORTEN_KID").ok(),
            scopes: var("MASKINPORTEN_SCOPES")?,
        })
    }
}

/// Fetches and caches Maskinporten access tokens.
pub struct TokenProvider {
    config: MaskinportenConfig,
    http: reqwest::Client,
    cache: Mutex<Option<CachedToken>>,
}

struct CachedToken {
    access_token: String,
    valid_until: Instant,
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
    expires_in: u64,
}

impl TokenProvider {
    pub fn new(config: MaskinportenConfig) -> Self {
        Self {
            config,
            http: reqwest::Client::new(),
            cache: Mutex::new(None),
        }
    }

    /// A valid access token — cached, or freshly fetched with a signed
    /// JWT grant.
    pub async fn token(&self) -> Result<String> {
        if let Some(cached) = self.cache.lock().unwrap().as_ref()
            && cached.valid_until > Instant::now()
        {
            return Ok(cached.access_token.clone());
        }

        let assertion = self.grant_assertion()?;
        let response = self
            .http
            .post(&self.config.token_endpoint)
            .form(&[
                ("grant_type", "urn:ietf:params:oauth:grant-type:jwt-bearer"),
                ("assertion", assertion.as_str()),
            ])
            .send()
            .await
            .context("requesting Maskinporten token")?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            anyhow::bail!("Maskinporten token request failed ({status}): {body}");
        }
        let token: TokenResponse = response.json().await.context("parsing token response")?;

        // Refresh a bit early so a token never expires mid-request.
        let valid_until = Instant::now() + Duration::from_secs(token.expires_in.saturating_sub(30));
        *self.cache.lock().unwrap() = Some(CachedToken {
            access_token: token.access_token.clone(),
            valid_until,
        });
        Ok(token.access_token)
    }

    /// The signed JWT grant. Maskinporten requires exp − iat ≤ 120 s.
    fn grant_assertion(&self) -> Result<String> {
        let mut header = Header::new(Algorithm::RS256);
        header.kid = self.config.kid.clone();
        let now = chrono::Utc::now().timestamp();
        let claims = serde_json::json!({
            "aud": self.config.audience,
            "iss": self.config.client_id,
            "scope": self.config.scopes,
            "iat": now,
            "exp": now + 120,
            "jti": uuid::Uuid::new_v4().to_string(),
        });
        jsonwebtoken::encode(&header, &claims, &self.config.encoding_key)
            .context("signing Maskinporten grant")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::Engine;
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    use rsa::RsaPrivateKey;
    use rsa::pkcs1::EncodeRsaPrivateKey;
    use rsa::traits::PublicKeyParts;

    fn test_config(token_endpoint: &str) -> (MaskinportenConfig, jsonwebtoken::DecodingKey) {
        let private = RsaPrivateKey::new(&mut rand::thread_rng(), 2048).unwrap();
        let pem = private.to_pkcs1_pem(rsa::pkcs1::LineEnding::LF).unwrap();
        let decoding = jsonwebtoken::DecodingKey::from_rsa_components(
            &URL_SAFE_NO_PAD.encode(private.n().to_bytes_be()),
            &URL_SAFE_NO_PAD.encode(private.e().to_bytes_be()),
        )
        .unwrap();
        let config = MaskinportenConfig {
            token_endpoint: token_endpoint.into(),
            audience: "https://test.maskinporten.no/".into(),
            client_id: "client-123".into(),
            encoding_key: EncodingKey::from_rsa_pem(pem.as_bytes()).unwrap(),
            kid: Some("key-1".into()),
            scopes: "skatteetaten:mvameldingvalidering".into(),
        };
        (config, decoding)
    }

    #[test]
    fn grant_assertion_has_the_required_claims() {
        let (config, decoding) = test_config("http://unused.invalid/token");
        let provider = TokenProvider::new(config);
        let assertion = provider.grant_assertion().unwrap();

        let mut validation = jsonwebtoken::Validation::new(Algorithm::RS256);
        validation.set_audience(&["https://test.maskinporten.no/"]);
        validation.validate_exp = true;
        let token =
            jsonwebtoken::decode::<serde_json::Value>(&assertion, &decoding, &validation).unwrap();

        assert_eq!(token.header.kid.as_deref(), Some("key-1"));
        assert_eq!(token.claims["iss"], "client-123");
        assert_eq!(token.claims["scope"], "skatteetaten:mvameldingvalidering");
        let iat = token.claims["iat"].as_i64().unwrap();
        let exp = token.claims["exp"].as_i64().unwrap();
        assert!(exp - iat <= 120, "Maskinporten rejects grants over 120 s");
        assert!(token.claims["jti"].is_string());
    }

    /// Full token flow against a local mock of the token endpoint,
    /// including the cache: two calls, one upstream request.
    #[tokio::test]
    async fn fetches_and_caches_tokens() {
        use axum::{Form, Json, Router, routing::post};
        use std::sync::Arc;
        use std::sync::atomic::{AtomicU32, Ordering};

        let hits = Arc::new(AtomicU32::new(0));
        let hits_in_handler = hits.clone();
        let app = Router::new().route(
            "/token",
            post(
                move |Form(form): Form<std::collections::HashMap<String, String>>| {
                    let hits = hits_in_handler.clone();
                    async move {
                        assert_eq!(
                            form["grant_type"],
                            "urn:ietf:params:oauth:grant-type:jwt-bearer"
                        );
                        assert!(
                            form["assertion"].split('.').count() == 3,
                            "JWS compact form"
                        );
                        hits.fetch_add(1, Ordering::SeqCst);
                        Json(serde_json::json!({
                            "access_token": "test-access-token",
                            "token_type": "Bearer",
                            "expires_in": 120,
                        }))
                    }
                },
            ),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

        let (config, _) = test_config(&format!("http://{addr}/token"));
        let provider = TokenProvider::new(config);
        assert_eq!(provider.token().await.unwrap(), "test-access-token");
        assert_eq!(provider.token().await.unwrap(), "test-access-token");
        assert_eq!(
            hits.load(Ordering::SeqCst),
            1,
            "second call served from cache"
        );
    }
}
