//! Provisioning a machine client in regnid (docs/agent.md §8.6).
//!
//! The IdP is a separate service in a separate repository and regnmed
//! is a relying party only — that rule is not relaxed here. regnmed
//! still issues no credentials of its own; it ASKS regnid for one, with
//! its own credentials, over regnid's own narrow endpoint, and stores
//! what comes back.
//!
//! Why it exists at all: a hosted agent needs an OIDC identity, and the
//! only way to get one used to be `regnid add-client` on the server. A
//! customer creating a bookkeeper from their own portal cannot be told
//! to do that, and the alternative — one privileged credential the
//! worker uses to act AS any agent — would mean a single leak exposed
//! every agent in every company. So the identity stays per agent and
//! the provisioning step moves behind this call.
//!
//! `base` is configurable so tests can point at a mock, the same
//! pattern as every other client in this crate.

use anyhow::{Context, Result, bail, ensure};

pub struct Regnid {
    http: reqwest::Client,
    base: String,
    client_id: String,
    client_secret: String,
}

/// What regnid hands back, once. The secret has no second chance: only
/// its hash is stored there, so a caller that drops it has to create a
/// new client.
#[derive(Debug)]
pub struct MachineClient {
    pub client_id: String,
    pub client_secret: String,
}

impl Regnid {
    /// `base` is the issuer URL. `client_id`/`client_secret` are
    /// regnmed's own, and must be on regnid's `PROVISION_CLIENTS`
    /// allowlist — otherwise every call is refused with 403, which is
    /// the point of the allowlist.
    pub fn new(base: &str, client_id: &str, client_secret: &str) -> Self {
        Self {
            http: reqwest::Client::new(),
            base: base.trim_end_matches('/').to_string(),
            client_id: client_id.to_string(),
            client_secret: client_secret.to_string(),
        }
    }

    /// Creates a `client_credentials`-only client and returns its
    /// credentials.
    ///
    /// `audience` matters more than it looks: regnmed validates `aud`,
    /// so a client created without it authenticates perfectly and then
    /// gets 401 on every single API call, with nothing in any log to
    /// say why (the trap recorded in CLAUDE.md point 56).
    pub async fn create_machine_client(
        &self,
        client_id: &str,
        name: &str,
        audience: &str,
    ) -> Result<MachineClient> {
        ensure!(!client_id.trim().is_empty(), "client_id mangler");
        ensure!(!name.trim().is_empty(), "navnet mangler");
        let response = self
            .http
            .post(format!("{}/provision/machine-client", self.base))
            .basic_auth(&self.client_id, Some(&self.client_secret))
            .json(&serde_json::json!({
                "client_id": client_id.trim(),
                "name": name.trim(),
                "audience": audience,
            }))
            .send()
            .await
            .context("nådde ikke påloggingstjenesten")?;
        let status = response.status();
        let body: serde_json::Value = response.json().await.unwrap_or(serde_json::Value::Null);
        if !status.is_success() {
            // regnid's own description, verbatim: it is written for
            // whoever has to fix the configuration, and paraphrasing it
            // would throw away the only useful part.
            let hvorfor = body["error_description"]
                .as_str()
                .or_else(|| body["error"].as_str())
                .unwrap_or("ukjent feil");
            bail!("påloggingstjenesten avviste registreringen ({status}): {hvorfor}");
        }
        let hemmelighet = body["client_secret"]
            .as_str()
            .filter(|s| !s.is_empty())
            .context("påloggingstjenesten svarte uten klienthemmelighet")?;
        Ok(MachineClient {
            client_id: body["client_id"].as_str().unwrap_or(client_id).to_string(),
            client_secret: hemmelighet.to_string(),
        })
    }
}
