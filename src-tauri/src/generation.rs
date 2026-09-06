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
    error: Option<String>,
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

#[derive(Default)]
struct ChatStreamParser {
    buffer: Vec<u8>,
    content: String,
    tokens: u64,
    done: bool,
}

impl ChatStreamParser {
    fn push(&mut self, bytes: &[u8], on_chunk: &mut impl FnMut(&str)) -> Result<()> {
        anyhow::ensure!(
            self.buffer.len() + bytes.len() <= 4 * 1024 * 1024,
            "Ollama stream frame exceeds 4 MiB"
        );
        self.buffer.extend_from_slice(bytes);
        while let Some(end) = self.buffer.iter().position(|byte| *byte == b'\n') {
            let line: Vec<u8> = self.buffer.drain(..=end).collect();
            self.line(&line, on_chunk)?;
        }
        Ok(())
    }

    fn line(&mut self, bytes: &[u8], on_chunk: &mut impl FnMut(&str)) -> Result<()> {
        if bytes.iter().all(u8::is_ascii_whitespace) {
            return Ok(());
        }
        anyhow::ensure!(!self.done, "Ollama sent data after the completion marker");
        let chunk: ChatStreamChunk =
            serde_json::from_slice(bytes).context("invalid Ollama stream frame")?;
        if let Some(error) = chunk.error {
            return Err(anyhow!("Ollama stream failed: {error}"));
        }
        if let Some(message) = chunk.message {
            anyhow::ensure!(
                self.content.len() + message.content.len() <= 8 * 1024 * 1024,
                "Ollama response exceeds 8 MiB"
            );
            if !message.content.is_empty() {
                on_chunk(&message.content);
                self.content.push_str(&message.content);
            }
        }
        if chunk.done {
            self.done = true;
            self.tokens = chunk.eval_count.unwrap_or(0);
        }
        Ok(())
    }

    fn finish(mut self, on_chunk: &mut impl FnMut(&str)) -> Result<ChatResult> {
        let remainder = std::mem::take(&mut self.buffer);
        self.line(&remainder, on_chunk)?;
        anyhow::ensure!(
            self.done,
            "Ollama stream ended before its completion marker"
        );
        Ok(ChatResult {
            content: self.content,
            tokens: self.tokens,
        })
    }
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

    fn request_configuration(&self) -> (String, String) {
        let settings = self.settings.read();
        (
            format!("{}/api/chat", settings.ollama_url.trim_end_matches('/')),
            settings.chat_model.clone(),
        )
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
        let (url, model) = self.request_configuration();
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

        let mut parser = ChatStreamParser::default();
        while let Some(chunk) = resp.chunk().await? {
            parser.push(&chunk, &mut on_chunk)?;
        }
        parser.finish(&mut on_chunk)
    }

    /// One-shot plain text completion with optional vision input.
    pub async fn generate_text_with_images(
        &self,
        system: &str,
        user: &str,
        images: Option<Vec<String>>,
        options: Option<ChatOptions>,
    ) -> Result<String> {
        let (url, model) = self.request_configuration();
        let msgs = vec![
            ChatMessage::text("system", system),
            ChatMessage {
                role: "user".to_string(),
                content: user.to_string(),
                images,
            },
        ];
        let body = ChatRequest {
            model: &model,
            messages: &msgs,
            stream: false,
            options,
            format: None,
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
                "ollama chat {}: {}",
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
        Ok(raw.message.content)
    }

    /// One-shot plain text completion without images.
    #[allow(dead_code)]
    pub async fn generate_text(
        &self,
        system: &str,
        user: &str,
        options: Option<ChatOptions>,
    ) -> Result<String> {
        self.generate_text_with_images(system, user, None, options)
            .await
    }

    /// One-shot JSON-mode completion. Asks Ollama to return strict JSON;
    /// caller deserialises into their own type via `serde_json`.
    pub async fn generate_json<T: serde::de::DeserializeOwned>(
        &self,
        system: &str,
        user: &str,
        options: Option<ChatOptions>,
    ) -> Result<T> {
        self.generate_json_with_images(system, user, None, options)
            .await
    }

    /// One-shot JSON-mode completion with optional vision input.
    pub async fn generate_json_with_images<T: serde::de::DeserializeOwned>(
        &self,
        system: &str,
        user: &str,
        images: Option<Vec<String>>,
        options: Option<ChatOptions>,
    ) -> Result<T> {
        let (url, model) = self.request_configuration();
        let msgs = vec![
            ChatMessage::text("system", system),
            ChatMessage {
                role: "user".to_string(),
                content: user.to_string(),
                images,
            },
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "requires local Ollama and KILROY_LIVE_CHAT_MODEL naming an installed local model"]
    fn live_ollama_chat_and_embeddings() {
        let settings = Settings {
            ollama_url: "http://localhost:11434".into(),
            chat_model: std::env::var("KILROY_LIVE_CHAT_MODEL")
                .expect("set KILROY_LIVE_CHAT_MODEL to an installed local model"),
            embedding_model: "nomic-embed-text".into(),
            ..Settings::default()
        };
        let shared = Arc::new(RwLock::new(settings));
        let client = ChatClient::new(shared.clone());
        let embedder = crate::embeddings::Embedder::new(shared);
        tauri::async_runtime::block_on(async {
            let vectors = embedder
                .embed_many(&["source indexing".into(), "approval gating".into()])
                .await
                .unwrap();
            assert_eq!(vectors.len(), 2);
            assert!(vectors.iter().all(|vector| vector.len() == 768));
            let mut deltas = String::new();
            let result = client
                .chat_stream(
                    &[ChatMessage::text("user", "Reply with the single word OK.")],
                    Some(ChatOptions {
                        temperature: Some(0.0),
                        num_predict: Some(16),
                        top_p: None,
                        num_ctx: Some(2048),
                    }),
                    |text| deltas.push_str(text),
                )
                .await
                .unwrap();
            assert!(!result.content.trim().is_empty());
            assert_eq!(deltas, result.content);
            assert!(result.tokens > 0);
        });
    }

    #[test]
    fn streaming_handles_fragmented_unicode_and_final_line_without_newline() {
        let bytes =
            "{\"message\":{\"content\":\"café\"},\"done\":false}\n{\"done\":true,\"eval_count\":2}"
                .as_bytes();
        let mut parser = ChatStreamParser::default();
        let mut received = String::new();
        let mut callback = |text: &str| received.push_str(text);
        for byte in bytes {
            parser.push(&[*byte], &mut callback).unwrap();
        }
        let result = parser.finish(&mut callback).unwrap();
        assert_eq!(result.content, "café");
        assert_eq!(result.tokens, 2);
        assert_eq!(received, "café");
    }

    #[test]
    fn streaming_errors_and_incomplete_replies_are_not_success() {
        for input in [
            b"{broken}\n".as_slice(),
            b"{\"error\":\"model unavailable\"}\n".as_slice(),
        ] {
            assert!(ChatStreamParser::default()
                .push(input, &mut |_| {})
                .is_err());
        }
        let mut parser = ChatStreamParser::default();
        parser
            .push(b"{\"message\":{\"content\":\"partial\"}}\n", &mut |_| {})
            .unwrap();
        assert!(parser.finish(&mut |_| {}).is_err());
    }
}
