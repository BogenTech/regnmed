//! ID-porten sign-in for filings that must name a person (docs/mva.md).
//!
//! This is not how anyone logs in to regnmed. It is the moment a person with
//! authority approves one mva-melding, and it yields their access token.
//! The national ID (`pid`) in the id_token is deliberately never parsed.

use anyhow::{Context, Result, bail, ensure};
use chrono::{DateTime, Utc};
use jsonwebtoken::jwk::JwkSet;
use jsonwebtoken::{Algorithm, DecodingKey, Validation, decode, decode_header};
use serde::Deserialize;

pub const SCOPES: &str =
    "openid skatteetaten:mvameldingvalidering altinn:instances.read altinn:instances.write";
pub const REQUIRED_ACR: &str = "idporten-loa-high";
/// How far ID-porten's clock may be behind ours when judging `auth_time`.
const AUTH_TIME_SKEW_SECS: i64 = 60;

#[derive(Debug, Clone, Deserialize)]
struct Discovery {
    issuer: String,
    authorization_endpoint: String,
    token_endpoint: String,
    jwks_uri: String,
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
    id_token: String,
    expires_in: i64,
}

/// Only what we keep. Fields left out, `pid` first of all, are dropped by
/// the parser.
#[derive(Debug, Deserialize)]
struct IdClaims {
    sub: String,
    acr: String,
    auth_time: i64,
    nonce: String,
}

/// Who approved, and the token that acts for them.
pub struct Approval {
    pub access_token: String,
    pub expires_in: i64,
    pub sub: String,
    pub acr: String,
    pub auth_time: DateTime<Utc>,
}

impl std::fmt::Debug for Approval {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Approval")
            .field("access_token", &"redacted")
            .field("sub", &self.sub)
            .field("acr", &self.acr)
            .finish()
    }
}

pub struct IdportenClient {
    issuer: String,
    client_id: String,
    client_secret: String,
    redirect_uri: String,
    http: reqwest::Client,
}

impl IdportenClient {
    pub fn new(
        issuer: impl Into<String>,
        client_id: impl Into<String>,
        client_secret: impl Into<String>,
        redirect_uri: impl Into<String>,
    ) -> Result<Self> {
        Ok(Self {
            issuer: issuer.into().trim_end_matches('/').to_string(),
            client_id: client_id.into(),
            client_secret: client_secret.into(),
            redirect_uri: redirect_uri.into(),
            http: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(20))
                .build()?,
        })
    }

    pub fn issuer(&self) -> &str {
        &self.issuer
    }

    pub fn client_id(&self) -> &str {
        &self.client_id
    }

    // shortcut: discovery on every sign-in, cache it if sign-ins ever get frequent.
    async fn discover(&self) -> Result<Discovery> {
        let url = format!("{}/.well-known/openid-configuration", self.issuer);
        let discovery: Discovery = self
            .http
            .get(url)
            .send()
            .await
            .context("ID-porten discovery")?
            .error_for_status()
            .context("ID-porten discovery")?
            .json()
            .await
            .context("parsing ID-porten discovery")?;
        ensure!(
            discovery.issuer.trim_end_matches('/') == self.issuer,
            "ID-porten discovery names another issuer"
        );
        Ok(discovery)
    }

    /// Where to send the person to sign in.
    pub async fn authorize_url(
        &self,
        state: &str,
        nonce: &str,
        code_challenge: &str,
    ) -> Result<String> {
        let discovery = self.discover().await?;
        let url = reqwest::Url::parse_with_params(
            &discovery.authorization_endpoint,
            &[
                ("response_type", "code"),
                ("client_id", &self.client_id),
                ("redirect_uri", &self.redirect_uri),
                ("scope", SCOPES),
                ("state", state),
                ("nonce", nonce),
                ("acr_values", REQUIRED_ACR),
                // An existing ID-porten session is not an approval of this filing.
                ("prompt", "login"),
                ("code_challenge", code_challenge),
                ("code_challenge_method", "S256"),
                ("ui_locales", "nb"),
            ],
        )
        .context("ID-porten authorization endpoint")?;
        Ok(url.into())
    }

    /// Redeems the one-time code and verifies who signed in.
    /// `started_at` is when this attempt began: an older sign-in is refused.
    pub async fn redeem(
        &self,
        code: &str,
        verifier: &str,
        nonce: &str,
        started_at: DateTime<Utc>,
    ) -> Result<Approval> {
        let discovery = self.discover().await?;
        let response = self
            .http
            .post(&discovery.token_endpoint)
            .form(&[
                ("grant_type", "authorization_code"),
                ("code", code),
                ("redirect_uri", &self.redirect_uri),
                ("client_id", &self.client_id),
                ("client_secret", &self.client_secret),
                ("code_verifier", verifier),
            ])
            .send()
            .await
            .context("ID-porten token request")?;
        let status = response.status();
        if !status.is_success() {
            // The error code only: a body is not ours to copy into a log.
            let error = response
                .json::<serde_json::Value>()
                .await
                .ok()
                .and_then(|b| b["error"].as_str().map(str::to_string))
                .unwrap_or_default();
            bail!("ID-porten refused the code ({status}): {error}");
        }
        let tokens: TokenResponse = response
            .json()
            .await
            .context("parsing the ID-porten token response")?;
        let jwks: JwkSet = self
            .http
            .get(&discovery.jwks_uri)
            .send()
            .await
            .context("fetching ID-porten's keys")?
            .error_for_status()
            .context("fetching ID-porten's keys")?
            .json()
            .await
            .context("parsing ID-porten's keys")?;
        let claims = verify_id_token(
            &tokens.id_token,
            &jwks,
            &self.issuer,
            &self.client_id,
            nonce,
            started_at,
        )?;
        Ok(Approval {
            access_token: tokens.access_token,
            expires_in: tokens.expires_in,
            sub: claims.sub,
            acr: claims.acr,
            auth_time: DateTime::from_timestamp(claims.auth_time, 0)
                .context("id_token auth_time out of range")?,
        })
    }
}

fn verify_id_token(
    token: &str,
    jwks: &JwkSet,
    issuer: &str,
    client_id: &str,
    nonce: &str,
    started_at: DateTime<Utc>,
) -> Result<IdClaims> {
    let header = decode_header(token).context("unreadable id_token")?;
    ensure!(header.alg == Algorithm::RS256, "id_token is not RS256");
    let kid = header.kid.context("id_token has no key id")?;
    let jwk = jwks
        .find(&kid)
        .context("id_token signed with an unknown key")?;
    let key = DecodingKey::from_jwk(jwk).context("unusable ID-porten key")?;
    let mut validation = Validation::new(Algorithm::RS256);
    validation.set_issuer(&[issuer]);
    validation.set_audience(&[client_id]);
    let claims = decode::<IdClaims>(token, &key, &validation)
        .context("id_token failed verification")?
        .claims;
    ensure!(claims.nonce == nonce, "id_token nonce does not match");
    ensure!(
        claims.auth_time >= started_at.timestamp() - AUTH_TIME_SKEW_SECS,
        "the sign-in is older than this approval"
    );
    ensure!(
        claims.acr == REQUIRED_ACR,
        "id_token security level is {}, not {REQUIRED_ACR}",
        claims.acr
    );
    Ok(claims)
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::Engine;
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    use jsonwebtoken::{EncodingKey, Header, encode};
    use rsa::RsaPrivateKey;
    use rsa::pkcs1::EncodeRsaPrivateKey;
    use rsa::traits::PublicKeyParts;
    use serde_json::json;

    const ISSUER: &str = "https://test.idporten.invalid";

    fn keys() -> (EncodingKey, JwkSet) {
        let private = RsaPrivateKey::new(&mut rand::thread_rng(), 2048).unwrap();
        let pem = private.to_pkcs1_pem(rsa::pkcs1::LineEnding::LF).unwrap();
        let jwks = serde_json::from_value(json!({ "keys": [{
            "kty": "RSA", "use": "sig", "alg": "RS256", "kid": "k1",
            "n": URL_SAFE_NO_PAD.encode(private.n().to_bytes_be()),
            "e": URL_SAFE_NO_PAD.encode(private.e().to_bytes_be()),
        }]}))
        .unwrap();
        (EncodingKey::from_rsa_pem(pem.as_bytes()).unwrap(), jwks)
    }

    fn sign(key: &EncodingKey, overrides: serde_json::Value) -> String {
        let now = Utc::now().timestamp();
        let mut claims = json!({
            "iss": ISSUER, "aud": "client", "sub": "pairwise", "acr": REQUIRED_ACR,
            "auth_time": now, "exp": now + 60, "nonce": "n", "pid": "01010112345",
        });
        for (k, v) in overrides.as_object().unwrap() {
            claims[k] = v.clone();
        }
        let mut header = Header::new(Algorithm::RS256);
        header.kid = Some("k1".into());
        encode(&header, &claims, key).unwrap()
    }

    #[test]
    fn an_id_token_is_held_to_every_check() {
        let (key, jwks) = keys();
        let started = Utc::now();
        let check = |t: &str| verify_id_token(t, &jwks, ISSUER, "client", "n", started);

        let good = check(&sign(&key, json!({}))).unwrap();
        assert_eq!(good.sub, "pairwise");
        assert!(
            !format!("{good:?}").contains("01010112345"),
            "pid is never parsed"
        );

        for bad in [
            json!({ "nonce": "other" }),
            json!({ "aud": "someone-else" }),
            json!({ "iss": "https://evil.invalid" }),
            json!({ "acr": "idporten-loa-substantial" }),
            json!({ "exp": Utc::now().timestamp() - 3600 }),
            json!({ "auth_time": started.timestamp() - AUTH_TIME_SKEW_SECS - 1 }),
        ] {
            assert!(check(&sign(&key, bad.clone())).is_err(), "accepted {bad}");
        }

        check(&sign(
            &key,
            json!({ "auth_time": started.timestamp() - AUTH_TIME_SKEW_SECS }),
        ))
        .expect("a small clock skew is allowed");

        let (other_key, _) = keys();
        assert!(
            check(&sign(&other_key, json!({}))).is_err(),
            "wrong signature"
        );
    }
}
