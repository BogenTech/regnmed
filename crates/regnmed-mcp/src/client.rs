//! The HTTP side: the customer's own machine identity, and one call.
//!
//! regnmed issues no API keys (docs/integrations.md). The agent carries a
//! `client_credentials` client the customer registered in the IdP, and
//! what that identity may do is decided by the grant in regnmed's own
//! database — not here, and not by the token.

use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow, bail};
use serde_json::Value;

/// A request body. `Raa` carries its own content type because the API
/// decides what a body IS from that header: XML is a SAF-T file, JSON is
/// an envelope, and an inbox upload is stored with the type it arrived
/// as.
pub enum Body {
    Json(Value),
    Raa {
        bytes: Vec<u8>,
        content_type: String,
    },
}

enum Auth {
    /// A token someone else obtained — for a test run, or an agent that
    /// already holds one.
    Statisk(String),
    Client {
        token_url: String,
        client_id: String,
        secret: String,
        scope: Option<String>,
    },
}

pub struct Client {
    http: reqwest::Client,
    base: String,
    /// The company the tools act for when a call does not name one.
    pub selskap: Option<String>,
    auth: Auth,
    cache: tokio::sync::Mutex<Option<(String, Instant)>>,
}

fn env(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|v| !v.trim().is_empty())
}

impl Client {
    /// Reads the configuration from the environment, the way every MCP
    /// server is configured. Fails LOUDLY and says which variable is
    /// missing: a server that starts without credentials only fails
    /// later, inside a tool call, where the agent cannot tell a
    /// configuration error from a permission one.
    pub async fn from_env() -> Result<Self> {
        let base = env("REGNMED_API")
            .unwrap_or_else(|| "https://api.regnmed.no".to_string())
            .trim_end_matches('/')
            .to_string();
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(60))
            .build()?;

        let auth = match (
            env("REGNMED_TOKEN"),
            env("REGNMED_CLIENT_ID"),
            env("REGNMED_CLIENT_SECRET"),
        ) {
            (Some(token), _, _) => Auth::Statisk(token),
            (None, Some(client_id), Some(secret)) => {
                let token_url = match env("REGNMED_TOKEN_URL") {
                    Some(url) => url,
                    None => {
                        let issuer = env("REGNMED_ISSUER")
                            .unwrap_or_else(|| "https://id.regnmed.no".to_string());
                        discover_token_endpoint(&http, issuer.trim_end_matches('/')).await?
                    }
                };
                Auth::Client {
                    token_url,
                    client_id,
                    secret,
                    scope: env("REGNMED_SCOPE"),
                }
            }
            _ => bail!(
                "mangler legitimasjon: sett REGNMED_CLIENT_ID og REGNMED_CLIENT_SECRET \
                 (klienten kunden registrerte i regnid), eller REGNMED_TOKEN for et \
                 ferdig token"
            ),
        };

        Ok(Self {
            http,
            base,
            selskap: env("REGNMED_COMPANY"),
            auth,
            cache: tokio::sync::Mutex::new(None),
        })
    }

    /// A client for one agent's own credentials — the hosted worker
    /// (docs/agent.md §8.3), which holds many agents' secrets and so
    /// cannot read them from process-global environment variables.
    /// Same `client_credentials` flow as the customer's own MCP server:
    /// one authentication path, used by both rails.
    pub async fn with_client_credentials(
        base: &str,
        selskap: Option<String>,
        issuer: &str,
        client_id: &str,
        secret: &str,
    ) -> Result<Self> {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(60))
            .build()?;
        let token_url = discover_token_endpoint(&http, issuer.trim_end_matches('/')).await?;
        Ok(Self {
            http,
            base: base.trim_end_matches('/').to_string(),
            selskap,
            auth: Auth::Client {
                token_url,
                client_id: client_id.to_string(),
                secret: secret.to_string(),
                scope: None,
            },
            cache: tokio::sync::Mutex::new(None),
        })
    }

    /// An explicitly configured client. Used by the end-to-end test,
    /// which must not go through process-global environment variables:
    /// tests in one binary share them, and a flaky configuration is
    /// worse than no test.
    pub fn new(base: &str, selskap: Option<String>, token: &str) -> Self {
        Self {
            http: reqwest::Client::new(),
            base: base.trim_end_matches('/').to_string(),
            selskap,
            auth: Auth::Statisk(token.to_string()),
            cache: tokio::sync::Mutex::new(None),
        }
    }

    async fn token(&self) -> Result<String> {
        match &self.auth {
            Auth::Statisk(t) => Ok(t.clone()),
            Auth::Client {
                token_url,
                client_id,
                secret,
                scope,
            } => {
                let mut cache = self.cache.lock().await;
                if let Some((token, utloper)) = cache.as_ref()
                    && *utloper > Instant::now()
                {
                    return Ok(token.clone());
                }
                // client_secret_post: the secret travels in the body,
                // which is what regnid accepts today (BogenTech/regnid#2
                // tracks client_secret_basic).
                let mut form = vec![
                    ("grant_type", "client_credentials"),
                    ("client_id", client_id.as_str()),
                    ("client_secret", secret.as_str()),
                ];
                if let Some(scope) = scope {
                    form.push(("scope", scope.as_str()));
                }
                let svar = self.http.post(token_url).form(&form).send().await?;
                let status = svar.status();
                let tekst = svar.text().await?;
                if !status.is_success() {
                    bail!("tokenet ble nektet av {token_url} ({status}): {tekst}");
                }
                let json: Value = serde_json::from_str(&tekst)
                    .with_context(|| format!("uventet svar fra {token_url}: {tekst}"))?;
                let token = json["access_token"]
                    .as_str()
                    .ok_or_else(|| anyhow!("svaret fra {token_url} har ingen access_token"))?
                    .to_string();
                // A minute's margin, so a token never expires in flight.
                let levetid = json["expires_in"]
                    .as_u64()
                    .unwrap_or(300)
                    .saturating_sub(60);
                *cache = Some((
                    token.clone(),
                    Instant::now() + Duration::from_secs(levetid.max(5)),
                ));
                Ok(token)
            }
        }
    }

    /// One call. Returns the body as text whatever the status — the
    /// caller turns a 4xx into something the agent can read, because
    /// regnmed's error messages say what to do and swallowing them would
    /// leave the agent guessing.
    pub async fn call(
        &self,
        method: &str,
        sti: &str,
        query: &[(String, String)],
        kropp: Option<Body>,
    ) -> Result<(u16, String)> {
        let url = format!("{}{sti}", self.base);
        let mut req = match method {
            "GET" => self.http.get(&url),
            "POST" => self.http.post(&url),
            "PUT" => self.http.put(&url),
            "DELETE" => self.http.delete(&url),
            annet => bail!("ukjent metode {annet}"),
        };
        if !query.is_empty() {
            req = req.query(query);
        }
        req = req.bearer_auth(self.token().await?);
        req = match kropp {
            None => req,
            Some(Body::Json(v)) => req.json(&v),
            Some(Body::Raa {
                bytes,
                content_type,
            }) => req
                .header(reqwest::header::CONTENT_TYPE, content_type)
                .body(bytes),
        };
        let svar = req.send().await?;
        let status = svar.status().as_u16();
        Ok((status, svar.text().await?))
    }
}

async fn discover_token_endpoint(http: &reqwest::Client, issuer: &str) -> Result<String> {
    let url = format!("{issuer}/.well-known/openid-configuration");
    let json: Value = http
        .get(&url)
        .send()
        .await
        .with_context(|| format!("fant ikke IdP-en på {url}"))?
        .json()
        .await
        .with_context(|| format!("{url} svarte ikke med OIDC-metadata"))?;
    json["token_endpoint"]
        .as_str()
        .map(str::to_string)
        .ok_or_else(|| anyhow!("{url} oppgir ingen token_endpoint — sett REGNMED_TOKEN_URL"))
}
