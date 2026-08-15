// File: src-tauri\src\commands\smartcoder.rs
//! SmartCoder bridge — Python CodeAgent subprocess used by Kilroy's SmartCoder
//! mode and autonomous swarm tasks. Grounded on the open project via
//! `--context-file`, not Hugging Face datasets.

use crate::commands::agent_context::{gather_agent_context, KilroyProjectContext};
use crate::smartcoder_runner::{
    build_smartcoder_argv, python_version_at, resolve_python,
    run_smartcoder_ask_blocking, smartcoder_deps_ready, smartcoder_launchable,
    smartcoder_setup_hint, vendored_script, write_context_temp_file, StreamSink,
};
use crate::state::AppState;
use serde::Serialize;
use std::path::Path;
use tauri::{AppHandle, Emitter, State};

const ALLOWED_SUBCOMMANDS: &[&str] = &["ask", "build-index", "list-datasets"];

#[derive(Serialize, Clone, Debug)]
pub struct SmartCoderStatus {
    pub python_version: Option<String>,
    pub vendored_script: bool,
    pub script_path: Option<String>,
    pub ready: bool,
    /// Legacy HF FAISS index — optional, not required for SmartCoder mode.
    pub index_ready: bool,
    pub ollama_reachable: bool,
    pub chat_model: String,
    pub has_chat_model: bool,
    /// `smolagents` imports in the resolved Python interpreter.
    pub deps_ready: bool,
    /// `project_venv` when `.venv`/`venv` is used; otherwise `system`.
    pub python_source: String,
    pub venv_path: Option<String>,
    /// Python + deps + Ollama + chat model.
    pub usable: bool,
    pub hint: String,
}

#[derive(Serialize, Clone)]
struct DoneEvent {
    code: Option<i32>,
    success: bool,
}

fn smartcoder_index_ready(project_root: Option<&Path>) -> bool {
    let Some(dir) = vendored_script(project_root)
        .and_then(|path| path.parent().map(|parent| parent.join("vector_store")))
    else {
        return false;
    };
    dir.join("embeddings.npy").is_file() && dir.join("documents.jsonl").is_file()
}

fn matches_ollama_model(installed: &str, configured: &str) -> bool {
    if installed == configured {
        return true;
    }
    let base = configured.split(':').next().unwrap_or(configured);
    let installed_base = installed.split(':').next().unwrap_or(installed);
    installed == base
        || installed.starts_with(&format!("{base}:"))
        || configured.starts_with(&format!("{installed_base}:"))
}

#[tauri::command]
pub async fn smartcoder_status(state: State<'_, AppState>) -> Result<SmartCoderStatus, String> {
    let project_root = state.memory.lock().as_ref().map(|m| m.root.clone());
    let chat_model = state.settings.read().chat_model.clone();
    let resolved = resolve_python(project_root.as_deref());
    let script = vendored_script(project_root.as_deref());
    let vendored = script.is_some();
    let ready = smartcoder_launchable(project_root.as_deref());
    let deps_ready = smartcoder_deps_ready(project_root.as_deref());
    let index_ready = smartcoder_index_ready(project_root.as_deref());

    let python_source = resolved
        .as_ref()
        .map(|r| r.source.to_string())
        .unwrap_or_else(|| "none".to_string());
    let venv_path = resolved
        .as_ref()
        .and_then(|r| r.venv_root.as_ref())
        .map(|p| p.to_string_lossy().to_string());
    let python_version = resolved
        .as_ref()
        .and_then(|r| python_version_at(&r.executable));

    let (ollama_reachable, has_chat_model) = match state.embedder.health().await {
        Ok(models) => (
            true,
            models.iter().any(|m| matches_ollama_model(m, &chat_model)),
        ),
        Err(_) => (false, false),
    };

    let usable = ready && deps_ready && ollama_reachable && has_chat_model;

    let setup = smartcoder_setup_hint(project_root.as_deref());
    let hint = if resolved.is_none() {
        format!("Python not found. {setup}")
    } else if !ready {
        format!("SmartCoder cannot launch. {setup}")
    } else if !deps_ready {
        format!("SmartCoder dependencies are unavailable. {setup}")
    } else if !ollama_reachable {
        "Start Ollama (`ollama serve`) so SmartCoder can reach your models.".to_string()
    } else if !has_chat_model {
        format!(
            "Pull the configured chat model: `ollama pull {chat_model}` (Settings → Models)."
        )
    } else if usable {
        let venv_note = if python_source == "project_venv" {
            "Using project .venv. "
        } else {
            ""
        };
        format!(
            "{venv_note}SmartCoder ready — grounded on your open project. Model: `{chat_model}`."
        )
    } else {
        "SmartCoder prerequisites incomplete.".to_string()
    };

    Ok(SmartCoderStatus {
        python_version,
        vendored_script: vendored,
        script_path: script.map(|p| p.to_string_lossy().to_string()),
        ready,
        index_ready,
        ollama_reachable,
        chat_model,
        has_chat_model,
        deps_ready,
        python_source,
        venv_path,
        usable,
        hint,
    })
}

#[tauri::command]
pub async fn smartcoder_run(
    app: AppHandle,
    state: State<'_, AppState>,
    subcommand: String,
    args: Vec<String>,
) -> Result<(), String> {
    if subcommand == "chat" {
        return Err(
            "SmartCoder `chat` is interactive — run in the Terminal: \
             python smartcoder/kilroy_smartcoder.py chat"
                .to_string(),
        );
    }
    if !ALLOWED_SUBCOMMANDS.contains(&subcommand.as_str()) {
        return Err(format!(
            "Unknown SmartCoder subcommand '{}'. Allowed: {}.",
            subcommand,
            ALLOWED_SUBCOMMANDS.join(", ")
        ));
    }

    let project_root = state.memory.lock().as_ref().map(|m| m.root.clone());
    let (ollama_host, chat_model) = {
        let settings = state.settings.read();
        (settings.ollama_url.clone(), settings.chat_model.clone())
    };

    let context_path = if subcommand == "ask" {
        let built = gather_agent_context(&app, &state, &args.join(" ")).await;
        let kilroy_ctx: KilroyProjectContext = (&built).into();
        Some(write_context_temp_file(&kilroy_ctx)?)
    } else {
        None
    };

    let project_grounded = subcommand == "ask";
    let launch = build_smartcoder_argv(
        &subcommand,
        &args,
        project_root.as_deref(),
        &ollama_host,
        &chat_model,
        context_path.as_deref(),
        project_root.as_deref(),
        project_grounded,
        None,
        None,
    )?;

    let coordinator = app.clone();
    let argv = launch.argv;
    let workdir = launch.workdir;
    let ctx_cleanup = context_path;

    std::thread::spawn(move || {
        let sink = StreamSink::SmartCoderPanel {
            app: coordinator.clone(),
        };
        let result = run_smartcoder_ask_blocking(&argv, workdir.as_deref(), &sink);
        if let Some(path) = ctx_cleanup {
            let _ = std::fs::remove_file(path);
        }
        let (code, success) = match &result {
            Ok(process) => (process.code, process.code.unwrap_or(1) == 0),
            Err(_) => (None, false),
        };
        let _ = coordinator.emit("smartcoder://done", DoneEvent { code, success });
    });

    Ok(())
}
