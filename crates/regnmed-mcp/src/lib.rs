//! MCP-server for regnmed (docs/agent.md, #102).
//!
//! **Transport, not authorization.** This is the consequence of #45's
//! decision — «tokenet beviser identitet, regnmed avgjør hva den får
//! gjøre» — and it is why this crate is small: it has no user database,
//! no API keys of its own, and no access model. The agent carries the
//! customer's own `client_credentials` client from regnid, and the
//! `integration_grant` in regnmed's database decides the rest.
//! Revocation stays one `valid_to` away.
//!
//! The protocol is JSON-RPC 2.0 over stdio, one object per line, written
//! by hand. That is the same choice this codebase made for PDF, pain.001,
//! EHF and SAF-T: the wire format is small, it is a specification we must
//! match exactly, and a dependency would not make it more correct — it
//! would only make it harder to see what we send.

pub mod client;
pub mod role;
pub mod tool;

use anyhow::{Context, Result, anyhow, bail};
use serde_json::{Map, Value, json};

use client::{Body, Client};
use tool::{Body as KroppMeny, Tool};

/// The protocol revision we implement. A client that asks for another
/// one is answered with ours, which is what the spec prescribes — the
/// client then decides whether it can live with it.
const PROTOCOL: &str = "2025-06-18";

pub struct Server {
    client: Client,
}

impl Server {
    pub fn new(client: Client) -> Self {
        Self { client }
    }

    /// Handles one JSON-RPC message. `None` means "no reply" — a
    /// notification, which must never be answered.
    pub async fn handle(&self, message: Value) -> Option<Value> {
        let method = message.get("method").and_then(Value::as_str).unwrap_or("");
        // A notification has no id. Per JSON-RPC, answering one is an
        // error on our side, so the shape of the message decides, not a
        // list of method names we would have to keep current.
        let id = message.get("id").cloned()?;
        let params = message
            .get("params")
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default();

        let response = match method {
            "initialize" => Ok(json!({
                "protocolVersion": PROTOCOL,
                "capabilities": { "tools": {}, "prompts": {} },
                "serverInfo": { "name": "regnmed", "version": env!("CARGO_PKG_VERSION") },
                "instructions": INSTRUCTIONS,
            })),
            "ping" => Ok(json!({})),
            "tools/list" => Ok(json!({ "tools": tool_list() })),
            "prompts/list" => Ok(json!({ "prompts": prompt_list() })),
            "prompts/get" => get_prompt(&params),
            "tools/call" => self.call(&params).await,
            annet => Err(ProtocolError::UnknownMethod(annet.to_string())),
        };

        Some(match response {
            Ok(result) => json!({ "jsonrpc": "2.0", "id": id, "result": result }),
            Err(error) => json!({
                "jsonrpc": "2.0", "id": id,
                "error": { "code": error.code(), "message": error.to_string() },
            }),
        })
    }

    /// Runs a tool.
    ///
    /// A failing CALL is not a protocol error: it comes back as tool
    /// content with `isError`, so the model reads regnmed's own message
    /// and can act on it. That is deliberate — the API's errors say what
    /// to do ("hovedboken inneholder N bilag utenfor importjournalen",
    /// "selskapet var ikke mva-registrert …"), and turning them into a
    /// JSON-RPC error code would throw away the only part an agent can
    /// use.
    async fn call(&self, params: &Map<String, Value>) -> Result<Value, ProtocolError> {
        let name = params
            .get("name")
            .and_then(Value::as_str)
            .ok_or_else(|| ProtocolError::UgyldigParam("mangler «name»".into()))?;
        let tool = tool::find(name).ok_or_else(|| ProtocolError::UnknownTool(name.to_string()))?;
        let args = params
            .get("arguments")
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default();

        let (text, failed) = self.execute_tool(tool, args).await;
        Ok(text_response(&text, failed))
    }

    /// Runs a tool and returns its text plus whether it failed. Public
    /// because the hosted agent worker (docs/agent.md §8.3) executes the
    /// same tools over HTTP instead of over stdio: the transport differs,
    /// the dispatch must not, or the two rails would drift apart in
    /// exactly the place where that is hardest to notice.
    pub async fn execute_tool(&self, v: &Tool, args: Map<String, Value>) -> (String, bool) {
        match self.execute(v, args).await {
            Ok(text) => (text, false),
            Err(error) => (format!("{error:#}"), true),
        }
    }

    async fn execute(&self, v: &Tool, mut args: Map<String, Value>) -> Result<String> {
        let path = fill_path(v.path, &mut args, self.client.company.as_deref())?;
        let (body, query) = build_body(v, args)?;
        let (status, text) = self
            .client
            .call(v.method, &path, &query, body)
            .await
            .with_context(|| format!("kallet til {} {path} nådde ikke fram", v.method))?;
        if (200..300).contains(&status) {
            return Ok(if text.is_empty() {
                "{}".to_string()
            } else {
                text
            });
        }
        // The status is part of the message: 404 means "no access or no
        // such thing" (the API never distinguishes, by design), 403 means
        // "you have access but not enough", 429 means slow down.
        bail!("{} {path} svarte {status}: {text}", v.method)
    }
}

/// What a client shows the model before it has called anything. Short on
/// purpose: the real instructions are the role prompts, and a long
/// preamble here competes with them for a small model's attention.
const INSTRUCTIONS: &str = "regnmed er et norsk regnskapssystem. Hovedboken er append-only: \
    det som er bokført kan ikke endres, bare motposteres. Beløp er heltall øre (positivt = debet, \
    negativt = kredit), datoer er YYYY-MM-DD. Kall `selskaper` først for å se hva du har tilgang \
    til, og `oppgaver` for å se hva du er bedt om. Hent prompten for rollen din med prompts/get \
    (regnskapsforer, bokforer, ansatt, migrering) før du gjør noe annet.";

fn text_response(text: &str, error: bool) -> Value {
    json!({
        "content": [{ "type": "text", "text": text }],
        "isError": error,
    })
}

fn tool_list() -> Vec<Value> {
    tool::TOOLS
        .iter()
        .map(|v| {
            json!({
                "name": v.name,
                "description": v.description,
                // Written out in tool.rs and parsed here. If this
                // ever failed the tool would be unusable, so the schemas
                // are unit-tested as JSON rather than guarded at runtime.
                "inputSchema": serde_json::from_str::<Value>(&tool::schema(v))
                    .unwrap_or_else(|_| json!({"type": "object"})),
            })
        })
        .collect()
}

fn prompt_list() -> Vec<Value> {
    role::ROLES
        .iter()
        .map(|r| json!({ "name": r.name, "description": r.description }))
        .collect()
}

fn get_prompt(params: &Map<String, Value>) -> Result<Value, ProtocolError> {
    let name = params
        .get("name")
        .and_then(Value::as_str)
        .ok_or_else(|| ProtocolError::UgyldigParam("mangler «name»".into()))?;
    let text = role::prompt(name).ok_or_else(|| {
        let valg: Vec<&str> = role::ROLES.iter().map(|r| r.name).collect();
        ProtocolError::UgyldigParam(format!(
            "ingen rolle «{name}» — velg en av: {}",
            valg.join(", ")
        ))
    })?;
    Ok(json!({
        "description": role::ROLES.iter().find(|r| r.name == name).map(|r| r.description),
        "messages": [{
            "role": "user",
            "content": { "type": "text", "text": text },
        }],
    }))
}

/// Substitutes `{name}` in the path from the arguments, consuming them.
///
/// `selskap` falls back to the one the server was started for, so an
/// agent working one company never has to carry a uuid around.
fn fill_path(
    mal: &str,
    args: &mut Map<String, Value>,
    standard_selskap: Option<&str>,
) -> Result<String> {
    let mut path = mal.to_string();
    while let Some(start) = path.find('{') {
        let end = path[start..]
            .find('}')
            .ok_or_else(|| anyhow!("ubalansert sti-mal: {mal}"))?
            + start;
        let felt = path[start + 1..end].to_string();
        let value = match args.remove(&felt) {
            Some(Value::String(s)) => s,
            Some(Value::Null) | None if felt == "selskap" => {
                standard_selskap.map(str::to_string).ok_or_else(|| {
                    anyhow!(
                        "selskap mangler — oppgi «selskap» i kallet, eller start serveren med \
                         REGNMED_COMPANY satt"
                    )
                })?
            }
            Some(annet) if !annet.is_null() => annet.to_string(),
            _ => bail!("«{felt}» mangler i kallet"),
        };
        path.replace_range(start..=end, &value);
    }
    Ok(path)
}

/// A request body and the query parameters that go with it.
type Foresporsel = (Option<Body>, Vec<(String, String)>);

/// Turns the remaining arguments into a body and a query string.
fn build_body(v: &Tool, mut args: Map<String, Value>) -> Result<Foresporsel> {
    match v.body {
        KroppMeny::None => Ok((None, query(args))),
        KroppMeny::Json => Ok((Some(Body::Json(Value::Object(args))), Vec::new())),
        KroppMeny::Fil => {
            let path = filepath(&mut args)?;
            let text = std::fs::read_to_string(&path)
                .with_context(|| format!("kunne ikke lese {path}"))?;
            // The one endpoint that takes both a file and structured
            // data: the SAF-T import's `{file, mapping}` envelope.
            match args.remove("mapping") {
                Some(mapping) if !mapping.is_null() => Ok((
                    Some(Body::Json(json!({ "file": text, "mapping": mapping }))),
                    query(args),
                )),
                _ => Ok((
                    Some(Body::Raa {
                        bytes: text.into_bytes(),
                        content_type: content_type(&path).to_string(),
                    }),
                    query(args),
                )),
            }
        }
        KroppMeny::FilBinaer => {
            let path = filepath(&mut args)?;
            let bytes = std::fs::read(&path).with_context(|| format!("kunne ikke lese {path}"))?;
            let mut q = query(args);
            if !q.iter().any(|(k, _)| k == "filename") {
                q.push(("filename".to_string(), filename(&path)));
            }
            // The hash the API verifies and dedupes on: a retry after a
            // lost reply is then a no-op instead of a second bilag.
            use sha2::{Digest, Sha256};
            q.push(("sha256".to_string(), hex::encode(Sha256::digest(&bytes))));
            Ok((
                Some(Body::Raa {
                    bytes,
                    content_type: content_type(&path).to_string(),
                }),
                q,
            ))
        }
    }
}

fn filepath(args: &mut Map<String, Value>) -> Result<String> {
    match args.remove("fil") {
        Some(Value::String(s)) if !s.trim().is_empty() => Ok(s),
        _ => bail!("«fil» mangler — oppgi full sti til filen på denne maskinen"),
    }
}

fn filename(path: &str) -> String {
    std::path::Path::new(path)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "vedlegg".to_string())
}

/// The API stores an inbox document with the type it arrived as, and
/// decides whether an import body is XML or a JSON envelope from the
/// same header — so this is not cosmetic.
fn content_type(path: &str) -> &'static str {
    let lav = path.to_ascii_lowercase();
    match lav.rsplit('.').next() {
        Some("xml") => "application/xml",
        Some("csv") => "text/csv",
        Some("json") => "application/json",
        Some("pdf") => "application/pdf",
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("txt") => "text/plain",
        _ => "application/octet-stream",
    }
}

/// Query parameters from whatever arguments are left. `null` is dropped
/// rather than sent as the string "null" — a model that fills every
/// optional field with null is common, and it should not change the
/// request.
fn query(args: Map<String, Value>) -> Vec<(String, String)> {
    args.into_iter()
        .filter_map(|(k, v)| match v {
            Value::Null => None,
            Value::String(s) => Some((k, s)),
            annet => Some((k, annet.to_string())),
        })
        .collect()
}

enum ProtocolError {
    UnknownMethod(String),
    UnknownTool(String),
    UgyldigParam(String),
}

impl ProtocolError {
    fn code(&self) -> i32 {
        match self {
            // JSON-RPC's own codes: an unknown method is -32601, bad
            // parameters are -32602.
            Self::UnknownMethod(_) => -32601,
            Self::UnknownTool(_) | Self::UgyldigParam(_) => -32602,
        }
    }
}

impl std::fmt::Display for ProtocolError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnknownMethod(m) => write!(f, "ukjent metode «{m}»"),
            Self::UnknownTool(n) => write!(f, "ukjent verktøy «{n}»"),
            Self::UgyldigParam(m) => write!(f, "{m}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(json: Value) -> Map<String, Value> {
        json.as_object().unwrap().clone()
    }

    #[test]
    fn the_path_takes_its_values_from_the_arguments() {
        let mut a = args(json!({"selskap": "abc", "dokument": "doc-1", "status": "ny"}));
        let path = fill_path(
            "/companies/{selskap}/inbox/{dokument}/forslag",
            &mut a,
            None,
        )
        .unwrap();
        assert_eq!(path, "/companies/abc/inbox/doc-1/forslag");
        // What the path consumed is gone; the rest becomes the query.
        assert_eq!(a.len(), 1);
        assert_eq!(query(a), vec![("status".to_string(), "ny".to_string())]);
    }

    #[test]
    fn the_company_falls_back_to_the_one_the_server_was_started_for() {
        let mut a = Map::new();
        let path = fill_path("/companies/{selskap}/vouchers", &mut a, Some("fast-id")).unwrap();
        assert_eq!(path, "/companies/fast-id/vouchers");

        // And says so when there is none, rather than calling
        // /companies//vouchers and reporting a 404 the agent cannot act on.
        let mut a = Map::new();
        let error = fill_path("/companies/{selskap}/vouchers", &mut a, None).unwrap_err();
        assert!(format!("{error}").contains("REGNMED_COMPANY"), "{error}");
    }

    /// A model that fills every optional property with null must not
    /// change the request — `?from=null` is a 400 nobody can diagnose.
    #[test]
    fn nulls_are_dropped_rather_than_sent() {
        let a = args(json!({"from": "2026-01-01", "to": null, "limit": 50}));
        let mut q = query(a);
        q.sort();
        assert_eq!(
            q,
            vec![
                ("from".to_string(), "2026-01-01".to_string()),
                ("limit".to_string(), "50".to_string()),
            ]
        );
    }

    #[tokio::test]
    async fn notifications_are_never_answered() {
        let server = Server::new(probe_client().await);
        assert!(
            server
                .handle(json!({"jsonrpc": "2.0", "method": "notifications/initialized"}))
                .await
                .is_none()
        );
    }

    #[tokio::test]
    async fn initialize_lists_what_we_can_do() {
        let server = Server::new(probe_client().await);
        let response = server
            .handle(json!({"jsonrpc": "2.0", "id": 1, "method": "initialize",
                            "params": {"protocolVersion": "2024-11-05"}}))
            .await
            .unwrap();
        assert_eq!(response["result"]["protocolVersion"], PROTOCOL);
        assert!(response["result"]["capabilities"]["tools"].is_object());
        assert!(response["result"]["capabilities"]["prompts"].is_object());

        let response = server
            .handle(json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"}))
            .await
            .unwrap();
        let tool = response["result"]["tools"].as_array().unwrap();
        assert_eq!(tool.len(), tool::TOOLS.len());
        // Every schema reached the wire as an object, not as the fallback.
        for v in tool {
            assert_eq!(v["inputSchema"]["type"], "object", "{v}");
        }

        let response = server
            .handle(json!({"jsonrpc": "2.0", "id": 3, "method": "prompts/get",
                            "params": {"name": "migrering"}}))
            .await
            .unwrap();
        let text = response["result"]["messages"][0]["content"]["text"]
            .as_str()
            .unwrap();
        assert!(text.contains("GJETT ALDRI ET TALL"));

        // An unknown method is a JSON-RPC error, not a tool error.
        let response = server
            .handle(json!({"jsonrpc": "2.0", "id": 4, "method": "resources/list"}))
            .await
            .unwrap();
        assert_eq!(response["error"]["code"], -32601);

        // An unknown TOOL is a parameter error with the name in it.
        let response = server
            .handle(json!({"jsonrpc": "2.0", "id": 5, "method": "tools/call",
                            "params": {"name": "slett_hovedboken"}}))
            .await
            .unwrap();
        assert_eq!(response["error"]["code"], -32602);
        assert!(
            response["error"]["message"]
                .as_str()
                .unwrap()
                .contains("slett_hovedboken")
        );
    }

    /// Listing tools and prompts spends no credentials, so the test
    /// server is built with a static token it never uses. The port is
    /// one nothing listens on: a test that reached the network would be
    /// a test of somebody's API, not of this dispatcher.
    async fn probe_client() -> Client {
        unsafe {
            std::env::set_var("REGNMED_TOKEN", "test");
            std::env::set_var("REGNMED_API", "http://127.0.0.1:1");
        }
        Client::from_env().await.unwrap()
    }
}
