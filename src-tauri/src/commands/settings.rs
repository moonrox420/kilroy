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
    let original = state.settings.read().clone();
    let mut candidate = original.clone();
    payload.apply(&mut candidate);
    candidate
        .validate()
        .map_err(|error| format!("invalid settings: {error:#}"))?;
    if candidate.embedding_model != original.embedding_model
        || candidate.ollama_url != original.ollama_url
    {
        state
            .embedder
            .validate_configuration(&candidate)
            .await
            .map_err(|error| {
                format!("embedding model validation failed; settings were not changed: {error:#}")
            })?;
    }
    let mut settings = state.settings.write();
    if *settings != original {
        return Err("Settings changed during validation. Reload and retry your update.".into());
    }
    candidate
        .save(&app)
        .map_err(|e| format!("save settings: {:#}", e))?;
    *settings = candidate.clone();
    Ok(candidate)
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

pub(crate) fn matches_model(installed: &str, configured: &str) -> bool {
    // An omitted tag means :latest, not any model sharing the same base name.
    if installed == configured {
        return true;
    }
    installed.strip_suffix(":latest").unwrap_or(installed)
        == configured.strip_suffix(":latest").unwrap_or(configured)
}

#[cfg(test)]
mod tests {
    use super::matches_model;

    #[test]
    fn model_readiness_requires_the_configured_tag() {
        assert!(matches_model("nomic-embed-text:latest", "nomic-embed-text"));
        assert!(!matches_model(
            "qwen2.5-coder:7b",
            "qwen2.5-coder:14b-instruct-q8_0"
        ));
    }
}
