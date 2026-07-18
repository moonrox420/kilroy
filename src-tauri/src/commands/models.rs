//! Model lifecycle commands.
//!
//! Pulling a model used to require dropping out to a terminal and
//! running `ollama pull qwen2.5-coder:14b-instruct-q8_0` manually. That's
//! the wrong UX for a consumer app — the user shouldn't need to know
//! `ollama` is an actual CLI binary. `pull_model` here streams the
//! NDJSON progress from `/api/pull` back to the frontend as
//! `ollama://pull/progress` events so Settings can render a progress
//! strip live.
//!
//! Future commands that belong in this module: `delete_model`,
//! `inspect_model` (Modelfile + parameters), `copy_model`.

use crate::state::AppState;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tauri::{AppHandle, Emitter, State};

/// Streamed progress event. One emitted per NDJSON line from
/// `/api/pull` plus a final `success` or `error` line.
#[derive(Serialize, Clone, Debug)]
pub struct PullProgress {
    pub tag: String,
    /// Ollama's own status string: "pulling manifest", "downloading",
    /// "verifying sha256 digest", "writing manifest", "success", "error".
    pub status: String,
    /// Bytes downloaded for the current layer. 0 if not in a downloading
    /// phase.
    pub completed: u64,
    /// Bytes total for the current layer. 0 if unknown.
    pub total: u64,
    /// SHA256 digest of the current layer, when reported.
    pub digest: Option<String>,
    /// Error string when status == "error".
    pub error: Option<String>,
    /// True once the pull is finished (success or error). Lets the UI
    /// flip its "pulling" flag without polling status strings.
    pub done: bool,
}

/// Raw shape we expect from `/api/pull` per line. All fields optional —
/// Ollama only includes what's relevant for the current phase.
#[derive(Deserialize, Debug, Default)]
struct PullLine {
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    digest: Option<String>,
    #[serde(default)]
    total: Option<u64>,
    #[serde(default)]
    completed: Option<u64>,
    #[serde(default)]
    error: Option<String>,
}

/// Trigger an `ollama pull <tag>` from inside the app. Streams
/// progress events on `ollama://pull/progress`. Resolves when the pull
/// completes (success or error). The error is returned to the caller
/// AND emitted as a final event so passive listeners can react.
#[tauri::command]
pub async fn pull_model(
    app: AppHandle,
    state: State<'_, AppState>,
    tag: String,
) -> Result<(), String> {
    let url = state.settings.read().ollama_url.clone();
    let endpoint = format!("{}/api/pull", url.trim_end_matches('/'));

    let emit = |progress: PullProgress| {
        let _ = app.emit("ollama://pull/progress", progress);
    };

    // Initial "starting" event — lets the UI show the strip immediately
    // instead of waiting for the first byte from Ollama (which can take
    // a few seconds on cold cache).
    emit(PullProgress {
        tag: tag.clone(),
        status: "starting".into(),
        completed: 0,
        total: 0,
        digest: None,
        error: None,
        done: false,
    });

    let client = reqwest::Client::builder()
        // Big models can take 10-30+ minutes on slow connections. We're
        // streaming so we just need a long ceiling on the overall request,
        // not per-chunk. A connect_timeout still guards against a dead
        // Ollama daemon — that should fail in seconds, not minutes.
        .timeout(Duration::from_secs(60 * 60))
        .connect_timeout(Duration::from_secs(5))
        .build()
        .map_err(|e| format!("reqwest build: {}", e))?;

    let body = serde_json::json!({ "name": tag, "stream": true });

    let mut resp = match client.post(&endpoint).json(&body).send().await {
        Ok(r) => r,
        Err(e) => {
            let err = format!("POST {}: {}", endpoint, e);
            emit(PullProgress {
                tag: tag.clone(),
                status: "error".into(),
                completed: 0,
                total: 0,
                digest: None,
                error: Some(err.clone()),
                done: true,
            });
            return Err(err);
        }
    };

    if !resp.status().is_success() {
        let st = resp.status();
        let txt = resp.text().await.unwrap_or_default();
        let err = format!(
            "Ollama returned {}: {}",
            st,
            txt.chars().take(400).collect::<String>()
        );
        emit(PullProgress {
            tag: tag.clone(),
            status: "error".into(),
            completed: 0,
            total: 0,
            digest: None,
            error: Some(err.clone()),
            done: true,
        });
        return Err(err);
    }

    // NDJSON streaming — same pattern the chat client uses for chat
    // responses. Read raw byte chunks, split on `\n`, parse each line.
    let mut buf = Vec::<u8>::new();
    let mut final_error: Option<String> = None;
    let mut succeeded = false;

    loop {
        match resp.chunk().await {
            Ok(Some(chunk)) => {
                buf.extend_from_slice(&chunk);
                while let Some(pos) = buf.iter().position(|&b| b == b'\n') {
                    let line: Vec<u8> = buf.drain(..=pos).collect();
                    let line_str = String::from_utf8_lossy(&line);
                    let line_str = line_str.trim();
                    if line_str.is_empty() {
                        continue;
                    }
                    match serde_json::from_str::<PullLine>(line_str) {
                        Ok(p) => {
                            let status = p.status.unwrap_or_else(|| "(unknown)".into());
                            if let Some(e) = &p.error {
                                final_error = Some(e.clone());
                            }
                            if status == "success" {
                                succeeded = true;
                            }
                            emit(PullProgress {
                                tag: tag.clone(),
                                status,
                                completed: p.completed.unwrap_or(0),
                                total: p.total.unwrap_or(0),
                                digest: p.digest,
                                error: p.error,
                                done: false,
                            });
                        }
                        Err(e) => {
                            tracing::debug!("pull NDJSON parse skip: {}: {}", e, line_str);
                        }
                    }
                }
            }
            Ok(None) => break,
            Err(e) => {
                final_error = Some(format!("read stream: {}", e));
                break;
            }
        }
    }

    if let Some(err) = final_error {
        emit(PullProgress {
            tag: tag.clone(),
            status: "error".into(),
            completed: 0,
            total: 0,
            digest: None,
            error: Some(err.clone()),
            done: true,
        });
        return Err(err);
    }

    emit(PullProgress {
        tag: tag.clone(),
        status: if succeeded {
            "success".into()
        } else {
            "complete".into()
        },
        completed: 0,
        total: 0,
        digest: None,
        error: None,
        done: true,
    });
    tracing::info!(tag = %tag, "model pull complete");
    Ok(())
}
