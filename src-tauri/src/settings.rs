//! User-editable settings.
//!
//! Persisted to `<app config dir>/settings.json`. On Windows that's
//! `%APPDATA%\com.kilroy.desktop\settings.json`. The struct lives
//! inside `AppState` behind an `Arc<RwLock<_>>` so the embedder, chat
//! client, retrieval pipeline, and sandbox dispatcher all see edits
//! the moment the user clicks Save.
//!
//! Environment variables (`KILROY_OLLAMA_URL`, `KILROY_CHAT_MODEL`,
//! `KILROY_EMBEDDING_MODEL`, `KILROY_SANDBOX_TIMEOUT_SECS`) seed
//! defaults on FIRST run only. After that, `settings.json` is the
//! single source of truth.

use crate::actuator::SandboxKind;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tauri::{AppHandle, Manager};

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Settings {
    pub ollama_url: String,
    pub chat_model: String,
    pub embedding_model: String,
    pub default_sandbox: SandboxKind,
    pub sandbox_timeout_secs: u64,
    pub retrieval_chunks_k: usize,
    pub retrieval_decisions_k: usize,
    pub chunk_window: usize,
    pub chunk_stride: usize,
    /// 768 for nomic-embed-text. Hot pin so the embedding model and the
    /// vec0 table dimension stay aligned.
    pub embedding_dim: usize,
    /// True until the first-run setup wizard completes. Drives the
    /// onboarding modal that walks the user through Ollama detection,
    /// model pull, and project picker. The wizard flips this to false
    /// when the user clicks Finish.
    #[serde(default = "default_true")]
    pub first_run: bool,
}

fn default_true() -> bool {
    true
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            ollama_url: std::env::var("KILROY_OLLAMA_URL")
                .unwrap_or_else(|_| "http://localhost:11434".into()),
            chat_model: std::env::var("KILROY_CHAT_MODEL")
                .unwrap_or_else(|_| "qwen2.5-coder:14b-instruct-q8_0".into()),
            embedding_model: std::env::var("KILROY_EMBEDDING_MODEL")
                .unwrap_or_else(|_| "nomic-embed-text".into()),
            // Platform-aware default: Windows Sandbox only exists on
            // Windows, so on macOS/Linux a fresh install must default to
            // Host (Windows Sandbox would error on every command). Uses
            // cfg! so the choice is baked in per target at compile time.
            default_sandbox: if cfg!(target_os = "windows") {
                SandboxKind::WindowsSandbox
            } else {
                SandboxKind::Host
            },
            sandbox_timeout_secs: std::env::var("KILROY_SANDBOX_TIMEOUT_SECS")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(300),
            retrieval_chunks_k: 5,
            retrieval_decisions_k: 3,
            chunk_window: 30,
            chunk_stride: 22,
            embedding_dim: 768,
            first_run: true,
        }
    }
}

impl Settings {
    /// Resolve the on-disk path for `settings.json` under Tauri's app config dir.
    pub fn path(app: &AppHandle) -> Result<PathBuf> {
        let dir = app
            .path()
            .app_config_dir()
            .context("resolve app config dir")?;
        std::fs::create_dir_all(&dir).with_context(|| format!("create {}", dir.display()))?;
        Ok(dir.join("settings.json"))
    }

    /// Load from disk, applying any missing fields from `Default::default()`.
    /// Returns the freshly-deserialised settings AND a flag indicating
    /// whether the file existed (false → caller may want to write the
    /// generated defaults).
    pub fn load(app: &AppHandle) -> (Self, bool) {
        let Ok(path) = Self::path(app) else {
            return (Self::default(), false);
        };
        match std::fs::read_to_string(&path) {
            Ok(raw) => match serde_json::from_str::<Self>(&raw) {
                Ok(s) => (s, true),
                Err(e) => {
                    tracing::warn!("settings.json parse failed ({}), regenerating defaults", e);
                    (Self::default(), false)
                }
            },
            Err(_) => (Self::default(), false),
        }
    }

    pub fn save(&self, app: &AppHandle) -> Result<()> {
        let path = Self::path(app)?;
        let raw = serde_json::to_string_pretty(self)?;
        std::fs::write(&path, raw).with_context(|| format!("write {}", path.display()))?;
        Ok(())
    }
}

/// Partial update — every field optional. Used by the `update_settings`
/// command so the frontend can patch one field at a time.
#[derive(Deserialize, Default)]
pub struct SettingsUpdate {
    pub ollama_url: Option<String>,
    pub chat_model: Option<String>,
    pub embedding_model: Option<String>,
    pub default_sandbox: Option<SandboxKind>,
    pub sandbox_timeout_secs: Option<u64>,
    pub retrieval_chunks_k: Option<usize>,
    pub retrieval_decisions_k: Option<usize>,
    pub chunk_window: Option<usize>,
    pub chunk_stride: Option<usize>,
    pub first_run: Option<bool>,
}

impl SettingsUpdate {
    pub fn apply(self, s: &mut Settings) {
        if let Some(v) = self.ollama_url {
            s.ollama_url = v;
        }
        if let Some(v) = self.chat_model {
            s.chat_model = v;
        }
        if let Some(v) = self.embedding_model {
            s.embedding_model = v;
        }
        if let Some(v) = self.default_sandbox {
            s.default_sandbox = v;
        }
        if let Some(v) = self.sandbox_timeout_secs {
            s.sandbox_timeout_secs = v.max(10);
        }
        if let Some(v) = self.retrieval_chunks_k {
            s.retrieval_chunks_k = v.clamp(0, 64);
        }
        if let Some(v) = self.retrieval_decisions_k {
            s.retrieval_decisions_k = v.clamp(0, 64);
        }
        if let Some(v) = self.chunk_window {
            s.chunk_window = v.clamp(8, 200);
        }
        if let Some(v) = self.chunk_stride {
            s.chunk_stride = v.clamp(4, 200);
        }
        if let Some(v) = self.first_run {
            s.first_run = v;
        }
    }
}
