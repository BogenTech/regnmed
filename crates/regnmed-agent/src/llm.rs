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
pub struct Melding {
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

impl Melding {
    pub fn system(tekst: impl Into<String>) -> Self {
        Self::enkel("system", tekst)
    }
    pub fn bruker(tekst: impl Into<String>) -> Self {
        Self::enkel("user", tekst)
    }
    fn enkel(role: &'static str, tekst: impl Into<String>) -> Self {
        Melding {
            role,
            content: Some(tekst.into()),
            tool_calls: None,
            tool_call_id: None,
        }
    }
    /// The assistant turn that asked for tools, echoed back verbatim.
    /// Providers validate that every tool result answers a call in the
    /// preceding assistant message, so this must be what they sent --
    /// not something we rebuilt from it.
    pub fn assistent_verktoy(tool_calls: Value) -> Self {
        Melding {
            role: "assistant",
            content: None,
            tool_calls: Some(tool_calls),
            tool_call_id: None,
        }
    }
    pub fn verktoysvar(tool_call_id: String, tekst: String) -> Self {
        Melding {
            role: "tool",
            content: Some(tekst),
            tool_calls: None,
            tool_call_id: Some(tool_call_id),
        }
    }
}

/// One tool the model asked us to run.
#[derive(Debug, Clone)]
pub struct Verktoykall {
    pub id: String,
    pub navn: String,
    pub argumenter: Map<String, Value>,
}

/// What came back from one turn.
#[derive(Debug)]
pub struct Svar {
    pub tekst: Option<String>,
    pub kall: Vec<Verktoykall>,
    /// The assistant message exactly as the provider sent it.
    pub raa_tool_calls: Option<Value>,
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
    tool_calls: Option<Vec<RaaKall>>,
}

#[derive(Deserialize)]
struct RaaKall {
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
    modell: String,
    nokkel: String,
}

impl Llm {
    pub fn ny(base_url: &str, modell: &str, nokkel: &str) -> Result<Self> {
        Ok(Self {
            http: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(180))
                .build()?,
            base_url: base_url.trim_end_matches('/').to_string(),
            modell: modell.to_string(),
            nokkel: nokkel.to_string(),
        })
    }

    /// One turn. `verktoy` is the tool catalogue in the provider's
    /// format, built once per task by [`crate::verktoykatalog`].
    pub async fn tur(&self, meldinger: &[Melding], verktoy: &Value) -> Result<Svar> {
        let svar = self
            .http
            .post(format!("{}/chat/completions", self.base_url))
            .bearer_auth(&self.nokkel)
            .json(&json!({
                "model": self.modell,
                "messages": meldinger,
                "tools": verktoy,
                // Let the model decide when it is done. Forcing a tool
                // would stop it ever answering, and forcing none would
                // stop it ever working.
                "tool_choice": "auto",
            }))
            .send()
            .await
            .context("nådde ikke modelleverandøren")?;

        let status = svar.status();
        let kropp = svar.text().await.unwrap_or_default();
        if !status.is_success() {
            // The provider's own message is the useful part: a wrong key,
            // a model the account cannot reach, a spent budget. Passing
            // it through means the task's trail says WHY.
            bail!("modelleverandøren svarte {status}: {}", kutt(&kropp, 400));
        }

        let respons: Respons = serde_json::from_str(&kropp)
            .with_context(|| format!("uventet svar fra leverandøren: {}", kutt(&kropp, 400)))?;
        let Some(valg) = respons.choices.into_iter().next() else {
            bail!("leverandøren svarte uten noe valg");
        };

        let raa: Option<Value> = serde_json::from_str::<Value>(&kropp)
            .ok()
            .and_then(|v| v["choices"][0]["message"]["tool_calls"].as_array().cloned())
            .map(Value::Array);

        let mut kall = Vec::new();
        for rå in valg.message.tool_calls.unwrap_or_default() {
            // An unparseable argument list is the model's mistake, and
            // the loop hands it back as a tool error so it can correct
            // itself — exactly what a human tool user would get.
            let argumenter = serde_json::from_str::<Map<String, Value>>(&rå.function.arguments)
                .unwrap_or_default();
            kall.push(Verktoykall {
                id: rå.id,
                navn: rå.function.name,
                argumenter,
            });
        }
        Ok(Svar {
            tekst: valg.message.content,
            kall,
            raa_tool_calls: raa,
        })
    }
}

/// Keeps a provider's error readable in a task note without pasting a
/// whole HTML error page into the ledger's trail.
pub fn kutt(s: &str, maks: usize) -> String {
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
        let kropp = json!({
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
        let respons: Respons = serde_json::from_str(&kropp).unwrap();
        let kall = &respons.choices[0].message.tool_calls.as_ref().unwrap()[0];
        assert_eq!(kall.function.name, "saldobalanse");
        let args: Map<String, Value> = serde_json::from_str(&kall.function.arguments).unwrap();
        assert_eq!(args["year"], 2026);
    }

    #[test]
    fn fields_we_do_not_model_do_not_break_it() {
        // A provider adding usage, finish_reason or a new message field
        // must not stop an agent working: this is a wire we do not own.
        let kropp = json!({
            "id": "x", "object": "chat.completion", "usage": {"total_tokens": 9},
            "choices": [{
                "index": 0, "finish_reason": "stop", "logprobs": null,
                "message": {"role": "assistant", "content": "ferdig", "refusal": null}
            }]
        })
        .to_string();
        let respons: Respons = serde_json::from_str(&kropp).unwrap();
        assert_eq!(
            respons.choices[0].message.content.as_deref(),
            Some("ferdig")
        );
    }

    #[test]
    fn a_long_provider_error_is_cut_before_it_reaches_the_trail() {
        let lang = "x".repeat(900);
        assert_eq!(kutt(&lang, 400).chars().count(), 401);
        assert_eq!(kutt("kort", 400), "kort");
    }
}
