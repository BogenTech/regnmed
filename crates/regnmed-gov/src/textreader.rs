//! The client for `regnmed-ocr`, the optional text-recognition sidecar
//! (docs/bilagstolkning.md). Configured by `OCR_URL`; without it there is
//! no client, and a photo simply yields no suggestion -- exactly as before
//! the sidecar existed.

use anyhow::{Context, Result, bail};

pub struct TextReader {
    base_url: String,
    http: reqwest::Client,
}

impl TextReader {
    /// None when `OCR_URL` is unset.
    pub fn from_env() -> Option<Self> {
        let base_url = std::env::var("OCR_URL")
            .ok()
            .filter(|u| !u.trim().is_empty())?;
        Some(Self {
            base_url,
            http: crate::http(),
        })
    }

    /// The text of a photo, one printed line per line.
    pub async fn read(&self, image: Vec<u8>) -> Result<String> {
        let response = self
            .http
            .post(format!("{}/read", self.base_url.trim_end_matches('/')))
            // One picture at a time in the sidecar: a queue of a few is
            // fine, a stuck one must not hold the request forever.
            .timeout(std::time::Duration::from_secs(30))
            .body(image)
            .send()
            .await
            .context("the text reader is unreachable")?;
        if !response.status().is_success() {
            bail!(
                "the text reader returned {}: {}",
                response.status(),
                response.text().await.unwrap_or_default()
            );
        }
        let body: serde_json::Value = response.json().await?;
        body["text"]
            .as_str()
            .map(str::to_string)
            .context("the text reader's answer has no text")
    }
}
