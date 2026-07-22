//! OIDC relying party: token verification and the authenticated-person
//! extractor.
//!
//! regnmed never sees a password and never bakes in IdP specifics — any
//! OIDC-compliant issuer works (networco-id today). The token proves
//! *identity only*; authorization is resolved from regnmed's own tables
//! (see `regnmed_db::tenancy`).

use anyhow::{Context, Result, bail};
use axum::Json;
use axum::extract::FromRequestParts;
use axum::http::{StatusCode, header, request::Parts};
use axum::response::{IntoResponse, Response};
use jsonwebtoken::jwk::JwkSet;
use jsonwebtoken::{Algorithm, DecodingKey, Validation, decode, decode_header};
use serde::Deserialize;
use serde_json::json;
use tokio::sync::RwLock;
use uuid::Uuid;

use crate::AppState;

/// Claims regnmed reads from an access token — identity only, no roles.
#[derive(Debug, Deserialize)]
pub struct TokenClaims {
    pub sub: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub email: Option<String>,
}

#[derive(Deserialize)]
struct DiscoveryDoc {
    issuer: String,
    jwks_uri: String,
}

pub struct Verifier {
    issuer: String,
    audience: Option<String>,
    jwks: RwLock<JwkSet>,
    /// None when the JWKS was loaded statically (dev/tests) — no refresh.
    jwks_uri: Option<String>,
    http: reqwest::Client,
}

impl Verifier {
    /// Configuration from the environment:
    /// - `OIDC_ISSUER` (required) — standard discovery via
    ///   `/.well-known/openid-configuration`.
    /// - `OIDC_AUDIENCE` (optional) — enforced when set.
    /// - `OIDC_JWKS_FILE` (dev/tests only) — load the JWKS from disk
    ///   instead of discovery. Signatures are still fully validated.
    pub async fn from_env() -> Result<Self> {
        let issuer = std::env::var("OIDC_ISSUER").context("OIDC_ISSUER is not set")?;
        let audience = std::env::var("OIDC_AUDIENCE").ok();
        if let Ok(path) = std::env::var("OIDC_JWKS_FILE") {
            let raw = std::fs::read_to_string(&path)
                .with_context(|| format!("reading OIDC_JWKS_FILE {path}"))?;
            let jwks: JwkSet = serde_json::from_str(&raw).context("parsing OIDC_JWKS_FILE")?;
            return Ok(Self::from_jwks(&issuer, audience, jwks));
        }
        Self::discover(&issuer, audience).await
    }

    pub async fn discover(issuer: &str, audience: Option<String>) -> Result<Self> {
        let http = reqwest::Client::new();
        let url = format!(
            "{}/.well-known/openid-configuration",
            issuer.trim_end_matches('/')
        );
        let doc: DiscoveryDoc = http
            .get(&url)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await
            .with_context(|| format!("fetching OIDC discovery document from {url}"))?;
        if doc.issuer.trim_end_matches('/') != issuer.trim_end_matches('/') {
            bail!(
                "issuer mismatch: configured {issuer}, discovery document says {}",
                doc.issuer
            );
        }
        let jwks: JwkSet = http
            .get(&doc.jwks_uri)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await
            .with_context(|| format!("fetching JWKS from {}", doc.jwks_uri))?;
        Ok(Self {
            // Validate against the discovery document's exact issuer value —
            // that is the string the IdP puts in the `iss` claim.
            issuer: doc.issuer,
            audience,
            jwks: RwLock::new(jwks),
            jwks_uri: Some(doc.jwks_uri),
            http,
        })
    }

    pub fn from_jwks(issuer: &str, audience: Option<String>, jwks: JwkSet) -> Self {
        Self {
            issuer: issuer.to_string(),
            audience,
            jwks: RwLock::new(jwks),
            jwks_uri: None,
            http: reqwest::Client::new(),
        }
    }

    pub async fn verify(&self, token: &str) -> Result<TokenClaims> {
        let header = decode_header(token).context("malformed token header")?;
        // Asymmetric signatures only: an IdP never signs access tokens with
        // a shared secret its relying parties also hold, and accepting HS*
        // here would let anyone with the public JWKS forge tokens.
        if !matches!(
            header.alg,
            Algorithm::RS256
                | Algorithm::RS384
                | Algorithm::RS512
                | Algorithm::ES256
                | Algorithm::ES384
                | Algorithm::EdDSA
        ) {
            bail!("token algorithm {:?} is not accepted", header.alg);
        }
        let kid = header.kid.as_deref().context("token header has no kid")?;

        let key = match self.decoding_key(kid).await {
            Some(key) => key,
            None => {
                // Unknown kid usually means the IdP rotated its keys.
                self.refresh_jwks().await?;
                self.decoding_key(kid)
                    .await
                    .context("token signed with a key not in the issuer's JWKS")?
            }
        };

        let mut validation = Validation::new(header.alg);
        validation.set_issuer(&[&self.issuer]);
        match &self.audience {
            Some(aud) => validation.set_audience(&[aud]),
            None => validation.validate_aud = false,
        }

        let data = decode::<TokenClaims>(token, &key, &validation).context("token rejected")?;
        Ok(data.claims)
    }

    async fn decoding_key(&self, kid: &str) -> Option<DecodingKey> {
        let jwks = self.jwks.read().await;
        jwks.find(kid)
            .and_then(|jwk| DecodingKey::from_jwk(jwk).ok())
    }

    async fn refresh_jwks(&self) -> Result<()> {
        let Some(uri) = &self.jwks_uri else {
            return Ok(());
        };
        let fresh: JwkSet = self
            .http
            .get(uri)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        *self.jwks.write().await = fresh;
        Ok(())
    }
}

/// The authenticated person, provisioned just-in-time on first sight of a
/// new OIDC subject. Add this as a handler argument to protect a route.
#[derive(Debug)]
pub struct AuthPerson {
    pub person_id: Uuid,
    pub sub: String,
    pub name: Option<String>,
    pub email: Option<String>,
}

impl FromRequestParts<AppState> for AuthPerson {
    type Rejection = ApiError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let header_value = parts
            .headers
            .get(header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .ok_or(ApiError::Unauthorized("missing Authorization header"))?;
        let token = header_value
            .strip_prefix("Bearer ")
            .ok_or(ApiError::Unauthorized("expected a Bearer token"))?;

        let claims = state
            .verifier
            .verify(token)
            .await
            .map_err(|_| ApiError::Unauthorized("invalid token"))?;

        let person_id = regnmed_db::ensure_person(
            &state.pool,
            &claims.sub,
            claims.name.as_deref(),
            claims.email.as_deref(),
        )
        .await?;

        Ok(AuthPerson {
            person_id,
            sub: claims.sub,
            name: claims.name,
            email: claims.email,
        })
    }
}

pub enum ApiError {
    Unauthorized(&'static str),
    /// Also covers "exists but you have no access" — a caller without
    /// access must not learn whether the company exists.
    NotFound,
    /// Known company, insufficient access level (e.g. revisor 'les'
    /// calling a mutating endpoint).
    Forbidden(&'static str),
    BadRequest(String),
    Internal(anyhow::Error),
}

impl From<anyhow::Error> for ApiError {
    fn from(err: anyhow::Error) -> Self {
        ApiError::Internal(err)
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        match self {
            ApiError::Unauthorized(msg) => {
                (StatusCode::UNAUTHORIZED, Json(json!({ "error": msg }))).into_response()
            }
            ApiError::NotFound => {
                (StatusCode::NOT_FOUND, Json(json!({ "error": "not found" }))).into_response()
            }
            ApiError::Forbidden(msg) => {
                (StatusCode::FORBIDDEN, Json(json!({ "error": msg }))).into_response()
            }
            ApiError::BadRequest(msg) => {
                (StatusCode::BAD_REQUEST, Json(json!({ "error": msg }))).into_response()
            }
            ApiError::Internal(err) => {
                // Log the detail server-side; never leak it to the client.
                eprintln!("internal error: {err:#}");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({ "error": "internal error" })),
                )
                    .into_response()
            }
        }
    }
}
