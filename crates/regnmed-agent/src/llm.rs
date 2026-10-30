//! An OpenAI-compatible chat-completions client (docs/agent.md §8.3).
//!
//! "OpenAI-compatible" is the whole reason this is small: OpenAI,
//! OpenRouter and most self-hosted gateways speak the same
//! `/chat/completions` with the same `tools` and `tool_calls`, so one
//! client covers the providers a customer is likely to bring, and
//! `annet` covers the rest without a release from us.
//!
//! Only what the loop needs is modelled. Fields we do not use are
//! ignored rather than rejected: a provider adding something must not
//! stop an agent working, and this is a wire we do not control.

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

/// One message in the conversation we send to the model.
#[derive(Debug, Clone, Serialize)]
pub struct Message {
    pub role: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    /// Set on an assistant message that asked for tools.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Value>,
    /// Set on a tool result, naming the call it answers.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
}

impl Message {
    pub fn system(text: impl Into<String>) -> Self {
        Self::enkel("system", text)
    }
    pub fn bruker(text: impl Into<String>) -> Self {
        Self::enkel("user", text)
    }
    /// A turn the agent itself took earlier, replayed from the trail.
    pub fn assistant(text: impl Into<String>) -> Self {
        Self::enkel("assistant", text)
    }
    fn enkel(role: &'static str, text: impl Into<String>) -> Self {
        Message {
            role,
            content: Some(text.into()),
            tool_calls: None,
            tool_call_id: None,
        }
    }
    /// The assistant turn that asked for tools, echoed back verbatim.
    /// Providers validate that every tool result answers a call in the
    /// preceding assistant message, so this must be what they sent --
    /// not something we rebuilt from it.
    pub fn assistant_tools(tool_calls: Value) -> Self {
        Message {
            role: "assistant",
            content: None,
            tool_calls: Some(tool_calls),
            tool_call_id: None,
        }
    }
    pub fn tool_result(tool_call_id: String, text: String) -> Self {
        Message {
            role: "tool",
            content: Some(text),
            tool_calls: None,
            tool_call_id: Some(tool_call_id),
        }
    }
}

/// One tool the model asked us to run.
#[derive(Debug, Clone)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: Map<String, Value>,
}

/// What came back from one turn.
#[derive(Debug)]
pub struct Svar {
    pub text: Option<String>,
    pub calls: Vec<ToolCall>,
    /// The assistant message exactly as the provider sent it.
    pub raw_tool_calls: Option<Value>,
}

#[derive(Deserialize)]
struct Respons {
    choices: Vec<Valg>,
}

#[derive(Deserialize)]
struct Valg {
    message: Beskjed,
}

#[derive(Deserialize)]
struct Beskjed {
    content: Option<String>,
    tool_calls: Option<Vec<RawCall>>,
}

#[derive(Deserialize)]
struct RawCall {
    id: String,
    function: RaaFunksjon,
}

#[derive(Deserialize)]
struct RaaFunksjon {
    name: String,
    /// The provider sends the arguments as a JSON *string*, not an
    /// object. A model can produce an invalid one, so parsing failure is
    /// handled where the loop can tell the model about it.
    arguments: String,
}

pub struct Llm {
    http: reqwest::Client,
    base_url: String,
    model: String,
    key: String,
}

impl Llm {
    pub fn new(base_url: &str, model: &str, key: &str) -> Result<Self> {
        Ok(Self {
            http: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(180))
                .build()?,
            base_url: base_url.trim_end_matches('/').to_string(),
            model: model.to_string(),
            key: key.to_string(),
        })
    }

    /// One turn. `tools` is the tool catalogue in the provider's
    /// format, built once per task by [`crate::tool_catalogue`].
    pub async fn tur(&self, messages: &[Message], tools: &Value) -> Result<Svar> {
        let reply = self
            .http
            .post(format!("{}/chat/completions", self.base_url))
            .bearer_auth(&self.key)
            .json(&json!({
                "model": self.model,
                "messages": messages,
                "tools": tools,
                // Let the model decide when it is done. Forcing a tool
                // would stop it ever answering, and forcing none would
                // stop it ever working.
                "tool_choice": "auto",
            }))
            .send()
            .await
            .context("nådde ikke modelleverandøren")?;

        let status = reply.status();
        let body = reply.text().await.unwrap_or_default();
        if !status.is_success() {
            // The provider's own message is the useful part: a wrong key,
            // a model the account cannot reach, a spent budget. Passing
            // it through means the task's trail says WHY.
            bail!(
                "modelleverandøren svarte {status}: {}",
                truncate(&body, 400)
            );
        }

        let respons: Respons = serde_json::from_str(&body)
            .with_context(|| format!("uventet reply fra leverandøren: {}", truncate(&body, 400)))?;
        let Some(valg) = respons.choices.into_iter().next() else {
            bail!("leverandøren svarte uten noe valg");
        };

        let raa: Option<Value> = serde_json::from_str::<Value>(&body)
            .ok()
            .and_then(|v| v["choices"][0]["message"]["tool_calls"].as_array().cloned())
            .map(Value::Array);

        let mut calls = Vec::new();
        for rå in valg.message.tool_calls.unwrap_or_default() {
            // An unparseable argument list is the model's mistake, and
            // the loop hands it back as a tool error so it can correct
            // itself — exactly what a human tool user would get.
            let arguments = serde_json::from_str::<Map<String, Value>>(&rå.function.arguments)
                .unwrap_or_default();
            calls.push(ToolCall {
                id: rå.id,
                name: rå.function.name,
                arguments,
            });
        }
        Ok(Svar {
            text: valg.message.content,
            calls,
            raw_tool_calls: raa,
        })
    }
}

/// Keeps a provider's error readable in a task note without pasting a
/// whole HTML error page into the ledger's trail.
pub fn truncate(s: &str, maks: usize) -> String {
    let s = s.trim();
    if s.chars().count() <= maks {
        return s.to_string();
    }
    s.chars().take(maks).collect::<String>() + "…"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_tool_call_is_read_out_of_a_providers_answer() {
        let body = json!({
            "choices": [{
                "message": {
                    "content": null,
                    "tool_calls": [{
                        "id": "call_1",
                        "type": "function",
                        "function": {
                            "name": "saldobalanse",
                            "arguments": "{\"year\":2026}"
                        }
                    }]
                }
            }]
        })
        .to_string();
        let respons: Respons = serde_json::from_str(&body).unwrap();
        let calls = &respons.choices[0].message.tool_calls.as_ref().unwrap()[0];
        assert_eq!(calls.function.name, "saldobalanse");
        let args: Map<String, Value> = serde_json::from_str(&calls.function.arguments).unwrap();
        assert_eq!(args["year"], 2026);
    }

    #[test]
    fn fields_we_do_not_model_do_not_break_it() {
        // A provider adding usage, finish_reason or a new message field
        // must not stop an agent working: this is a wire we do not own.
        let body = json!({
            "id": "x", "object": "chat.completion", "usage": {"total_tokens": 9},
            "choices": [{
                "index": 0, "finish_reason": "stop", "logprobs": null,
                "message": {"role": "assistant", "content": "ferdig", "refusal": null}
            }]
        })
        .to_string();
        let respons: Respons = serde_json::from_str(&body).unwrap();
        assert_eq!(
            respons.choices[0].message.content.as_deref(),
            Some("ferdig")
        );
    }

    #[test]
    fn a_long_provider_error_is_cut_before_it_reaches_the_trail() {
        let lang = "x".repeat(900);
        assert_eq!(truncate(&lang, 400).chars().count(), 401);
        assert_eq!(truncate("kort", 400), "kort");
    }
}
