//! Embeddings via Ollama, driven by `Settings`.
//!
//! The Embedder reads `ollama_url` and `embedding_model` from the live
//! settings on every call so the UI's Save in the Settings dialog is
//! reflected on the next embed without restarting the app.

use crate::settings::Settings;
use anyhow::{anyhow, Context, Result};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Duration;

#[derive(Clone)]
pub struct Embedder {
    client: reqwest::Client,
    settings: Arc<RwLock<Settings>>,
}

impl Embedder {
    pub fn new(settings: Arc<RwLock<Settings>>) -> Self {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(60))
            .connect_timeout(Duration::from_secs(2))
            .build()
            .expect("reqwest client");
        Self { client, settings }
    }

    pub fn model(&self) -> String {
        self.settings.read().embedding_model.clone()
    }

    fn endpoints(&self) -> (String, String) {
        let s = self.settings.read();
        (
            format!("{}/api/tags", s.ollama_url),
            format!("{}/api/embed", s.ollama_url),
        )
    }

    /// Reachability check + list of installed Ollama models.
    pub async fn health(&self) -> Result<Vec<String>> {
        #[derive(Deserialize)]
        struct TagsResponse {
            models: Vec<TagModel>,
        }
        #[derive(Deserialize)]
        struct TagModel {
            name: String,
        }
        let (tags_url, _) = self.endpoints();
        let resp = self
            .client
            .get(&tags_url)
            .send()
            .await
            .with_context(|| format!("GET {}", tags_url))?;
        if !resp.status().is_success() {
            return Err(anyhow!("ollama /api/tags returned {}", resp.status()));
        }
        let tags: TagsResponse = resp.json().await?;
        Ok(tags.models.into_iter().map(|m| m.name).collect())
    }

    /// Embed a single text. Reads `embedding_model` and `ollama_url` fresh
    /// from settings on each call.
    pub async fn embed(&self, text: &str) -> Result<Vec<f32>> {
        #[derive(Serialize)]
        struct Req<'a> {
            model: &'a str,
            input: &'a str,
        }
        #[derive(Deserialize)]
        struct Resp {
            embeddings: Vec<Vec<f32>>,
        }

        let model;
        let url;
        {
            let s = self.settings.read();
            model = s.embedding_model.clone();
            url = format!("{}/api/embed", s.ollama_url);
        }

        let resp = self
            .client
            .post(&url)
            .json(&Req {
                model: &model,
                input: text,
            })
            .send()
            .await
            .with_context(|| format!("POST {}", url))?;
        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(anyhow!(
                "ollama embeddings returned {}: {}",
                status,
                body.chars().take(400).collect::<String>()
            ));
        }
        let parsed: Resp = resp.json().await?;
        parsed
            .embeddings
            .into_iter()
            .next()
            .ok_or_else(|| anyhow!("ollama returned no embeddings"))
    }

    /// Embed many texts, batching them into `/api/embed` requests with an
    /// array `input`. This turns indexing from N HTTP round-trips into
    /// N/`EMBED_BATCH_SIZE` — the dominant cost when embedding a whole repo.
    ///
    /// Order is preserved: the returned Vec lines up 1:1 with `texts`, which
    /// the indexer relies on to pair each embedding with its chunk. Any batch
    /// failure fails the whole call — identical to the previous per-text `?`
    /// behaviour, and the indexer already treats that as a per-file error and
    /// moves on, so error granularity at the file level is unchanged.
    pub async fn embed_many(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }
        let mut out = Vec::with_capacity(texts.len());
        for batch in texts.chunks(EMBED_BATCH_SIZE) {
            let mut vectors = self.embed_batch(batch).await?;
            out.append(&mut vectors);
        }
        Ok(out)
    }

    /// One batched `/api/embed` call. Sends `input` as a JSON array and
    /// returns the embeddings in request order.
    ///
    /// Guards that the server returned exactly one vector per input. A count
    /// mismatch would silently misalign embeddings with their chunks and
    /// corrupt the index (chunk N would get chunk N+1's vector), so we fail
    /// loudly instead of writing a scrambled index.
    async fn embed_batch(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        #[derive(Serialize)]
        struct Req<'a> {
            model: &'a str,
            input: &'a [String],
        }
        #[derive(Deserialize)]
        struct Resp {
            embeddings: Vec<Vec<f32>>,
        }

        let model;
        let url;
        {
            let s = self.settings.read();
            model = s.embedding_model.clone();
            url = format!("{}/api/embed", s.ollama_url);
        }

        let resp = self
            .client
            .post(&url)
            .json(&Req {
                model: &model,
                input: texts,
            })
            .send()
            .await
            .with_context(|| format!("POST {}", url))?;
        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(anyhow!(
                "ollama embeddings returned {}: {}",
                status,
                body.chars().take(400).collect::<String>()
            ));
        }
        let parsed: Resp = resp.json().await?;
        if parsed.embeddings.len() != texts.len() {
            return Err(anyhow!(
                "ollama returned {} embeddings for {} inputs — refusing to write a misaligned index",
                parsed.embeddings.len(),
                texts.len()
            ));
        }
        Ok(parsed.embeddings)
    }
}

/// Max texts per `/api/embed` request. Capped so a file with thousands of
/// chunks doesn't post a multi-MB body or spike the embedding server's
/// per-request memory; large enough that the HTTP round-trip stops being the
/// bottleneck during indexing.
const EMBED_BATCH_SIZE: usize = 32;
