//! Settings commands.
//!
//! `get_settings` / `update_settings` read and patch the live Settings.
//! `available_models` shells out to Ollama to list installed models so
//! the UI can present a dropdown. `ollama_health` returns a richer
//! status object than the `open_project` one — used by the Settings
//! dialog's connection-test button.

use crate::settings::{Settings, SettingsUpdate};
use crate::state::AppState;
use serde::Serialize;
use tauri::{AppHandle, State};

#[tauri::command]
pub async fn get_settings(state: State<'_, AppState>) -> Result<Settings, String> {
    Ok(state.settings.read().clone())
}

#[tauri::command]
pub async fn update_settings(
    app: AppHandle,
    state: State<'_, AppState>,
    payload: SettingsUpdate,
) -> Result<Settings, String> {
    {
        let mut s = state.settings.write();
        payload.apply(&mut s);
    }
    let snapshot = state.settings.read().clone();
    snapshot
        .save(&app)
        .map_err(|e| format!("save settings: {:#}", e))?;
    Ok(snapshot)
}

#[derive(Serialize)]
pub struct OllamaHealthFull {
    pub reachable: bool,
    pub models: Vec<String>,
    pub chat_model: String,
    pub embedding_model: String,
    pub has_chat_model: bool,
    pub has_embedding_model: bool,
    pub error: Option<String>,
}

#[tauri::command]
pub async fn ollama_health(state: State<'_, AppState>) -> Result<OllamaHealthFull, String> {
    let (chat_model, embedding_model) = {
        let s = state.settings.read();
        (s.chat_model.clone(), s.embedding_model.clone())
    };
    let emb = state.embedder.clone();
    match emb.health().await {
        Ok(models) => Ok(OllamaHealthFull {
            reachable: true,
            has_chat_model: models.iter().any(|m| matches_model(m, &chat_model)),
            has_embedding_model: models.iter().any(|m| matches_model(m, &embedding_model)),
            chat_model,
            embedding_model,
            models,
            error: None,
        }),
        Err(e) => Ok(OllamaHealthFull {
            reachable: false,
            models: vec![],
            chat_model,
            embedding_model,
            has_chat_model: false,
            has_embedding_model: false,
            error: Some(format!("{:#}", e)),
        }),
    }
}

fn matches_model(installed: &str, configured: &str) -> bool {
    // Ollama tags often include the digest suffix (e.g. `nomic-embed-text:latest`).
    // We accept either an exact match or a prefix-up-to-':' match so the
    // user can configure `nomic-embed-text` and we still find `nomic-embed-text:latest`.
    if installed == configured {
        return true;
    }
    let inst_base = installed.split(':').next().unwrap_or(installed);
    let conf_base = configured.split(':').next().unwrap_or(configured);
    inst_base == conf_base
}
