//! Ollama chat generation, driven by `Settings`.
//!
//! Like the embedder, the chat client reads `ollama_url` and
//! `chat_model` fresh from the shared settings each call so the
//! Settings dialog's Save takes effect immediately.

use crate::settings::Settings;
use anyhow::{anyhow, Context, Result};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Duration;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
    /// Vision-model images, raw base64 (no `data:image/...;base64,` prefix
    /// — Ollama's `/api/chat` rejects the data URL form). Skipped during
    /// serialisation when None so non-vision turns produce the same wire
    /// format as before. Adding this field is forward-compatible with
    /// every Ollama version that supports vision; older daemons silently
    /// ignore it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub images: Option<Vec<String>>,
}

impl ChatMessage {
    /// Convenience for the common text-only case so call sites don't
    /// have to think about `images: None` every time.
    pub fn text(role: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            role: role.into(),
            content: content.into(),
            images: None,
        }
    }
}

#[derive(Serialize, Debug)]
struct ChatRequest<'a> {
    model: &'a str,
    messages: &'a [ChatMessage],
    stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    options: Option<ChatOptions>,
    #[serde(skip_serializing_if = "Option::is_none")]
    format: Option<&'a str>,
}

#[derive(Serialize, Debug, Clone)]
pub struct ChatOptions {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub num_predict: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub num_ctx: Option<i32>,
}

#[derive(Deserialize, Debug)]
struct ChatStreamChunk {
    #[serde(default)]
    message: Option<ChatStreamMessage>,
    #[serde(default)]
    done: bool,
    #[serde(default)]
    eval_count: Option<u64>,
}

#[derive(Deserialize, Debug)]
struct ChatStreamMessage {
    #[serde(default)]
    content: String,
}

#[derive(Clone)]
pub struct ChatClient {
    client: reqwest::Client,
    settings: Arc<RwLock<Settings>>,
}

impl ChatClient {
    pub fn new(settings: Arc<RwLock<Settings>>) -> Self {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(300))
            .connect_timeout(Duration::from_secs(3))
            .build()
            .expect("reqwest client");
        Self { client, settings }
    }

    pub fn model(&self) -> String {
        self.settings.read().chat_model.clone()
    }

    fn endpoint(&self) -> String {
        format!("{}/api/chat", self.settings.read().ollama_url)
    }

    /// Streaming chat. Calls `on_chunk` for each delta, returns the final text + token count.
    pub async fn chat_stream<F>(
        &self,
        messages: &[ChatMessage],
        options: Option<ChatOptions>,
        mut on_chunk: F,
    ) -> Result<ChatResult>
    where
        F: FnMut(&str),
    {
        let url = self.endpoint();
        let model = self.model();
        let body = ChatRequest {
            model: &model,
            messages,
            stream: true,
            options,
            format: None,
        };
        let mut resp = self
            .client
            .post(&url)
            .json(&body)
            .send()
            .await
            .with_context(|| format!("POST {}", url))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(anyhow!(
                "ollama chat {}: {}",
                status,
                body.chars().take(400).collect::<String>()
            ));
        }

        let mut buf = Vec::<u8>::new();
        let mut full = String::new();
        let mut tokens = 0u64;

        while let Some(chunk) = resp.chunk().await? {
            buf.extend_from_slice(&chunk);
            while let Some(pos) = buf.iter().position(|&b| b == b'\n') {
                let line: Vec<u8> = buf.drain(..=pos).collect();
                let line_str = String::from_utf8_lossy(&line);
                let line_str = line_str.trim();
                if line_str.is_empty() {
                    continue;
                }
                match serde_json::from_str::<ChatStreamChunk>(line_str) {
                    Ok(c) => {
                        if let Some(m) = c.message {
                            if !m.content.is_empty() {
                                on_chunk(&m.content);
                                full.push_str(&m.content);
                            }
                        }
                        if c.done {
                            if let Some(n) = c.eval_count {
                                tokens = n;
                            }
                        }
                    }
                    Err(e) => {
                        tracing::debug!("ollama parse skip: {}: {}", e, line_str);
                    }
                }
            }
        }

        Ok(ChatResult {
            content: full,
            tokens,
        })
    }

    /// One-shot JSON-mode completion. Asks Ollama to return strict JSON;
    /// caller deserialises into their own type via `serde_json`.
    pub async fn generate_json<T: serde::de::DeserializeOwned>(
        &self,
        system: &str,
        user: &str,
        options: Option<ChatOptions>,
    ) -> Result<T> {
        let url = self.endpoint();
        let model = self.model();
        let msgs = vec![
            ChatMessage::text("system", system),
            ChatMessage::text("user", user),
        ];
        let body = ChatRequest {
            model: &model,
            messages: &msgs,
            stream: false,
            options,
            format: Some("json"),
        };
        let resp = self
            .client
            .post(&url)
            .json(&body)
            .send()
            .await
            .with_context(|| format!("POST {}", url))?;
        if !resp.status().is_success() {
            let s = resp.status();
            let b = resp.text().await.unwrap_or_default();
            return Err(anyhow!(
                "ollama chat (json) {}: {}",
                s,
                b.chars().take(400).collect::<String>()
            ));
        }

        #[derive(Deserialize)]
        struct OneShot {
            message: OneShotMsg,
        }
        #[derive(Deserialize)]
        struct OneShotMsg {
            content: String,
        }
        let raw: OneShot = resp.json().await?;
        let parsed: T = serde_json::from_str(&raw.message.content).with_context(|| {
            format!(
                "ollama returned invalid JSON: {}",
                raw.message.content.chars().take(400).collect::<String>()
            )
        })?;
        Ok(parsed)
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ChatResult {
    pub content: String,
    pub tokens: u64,
}
