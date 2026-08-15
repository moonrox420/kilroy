//! Kilroy — local AI agentic software engineering platform.

// Several modules expose helpers that earlier phases used and that future
// phases will use again (chunk_text default-args wrapper, has_command shell
// detection, with_conn DB lambda, session end, file list). They are dead
// today because the current call sites use the configurable / explicit
// variants. We keep them rather than deleting to preserve capability for
// future phases — silencing the warnings crate-wide is cheaper than
// scattering individual #[allow(dead_code)] attributes that future readers
// would have to audit one at a time.
#![allow(dead_code)]

mod actuator;
mod commands;
mod db;
mod embeddings;
mod generation;
mod ollama_bundled;
mod platform;
mod runtime;
mod settings;
mod smartcoder_runner;
mod state;
mod tray;

use tauri::Manager;
use tracing_subscriber::EnvFilter;

pub fn run() {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("info,kilroy_lib=debug")),
        )
        .with_target(false)
        .compact()
        .init();

    tracing::info!("Kilroy starting");

    tauri::Builder::default()
        .plugin(tauri_plugin_window_state::Builder::default().build())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_process::init())
        .setup(|app| {
            // Load settings.json (or generate from env-var / defaults).
            let (settings_loaded, found) = settings::Settings::load(&app.handle().clone());
            if !found {
                if let Err(e) = settings_loaded.save(&app.handle().clone()) {
                    tracing::warn!("could not write initial settings.json: {:#}", e);
                }
            }
            tracing::info!(?settings_loaded, "settings loaded");

            // Build AppState from the settings.
            app.manage(state::AppState::new(settings_loaded));

            tray::install(app)?;

            // Bundled-Ollama orchestration. If a system Ollama daemon is
            // already listening on the configured URL, we defer. Otherwise
            // and if a bundled binary exists at resources/ollama/ollama.exe,
            // we start it. The child handle is leaked into the tokio runtime
            // and dropped at process exit (Ollama gets SIGTERM via the OS).
            let app_handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                let url = {
                    let state: tauri::State<'_, state::AppState> = app_handle.state();
                    // Bind the cloned URL to a local before the block ends so
                    // the RwLockReadGuard temporary drops BEFORE `state` does.
                    // Without this binding the borrow checker rejects the
                    // implicit drop order (state drops first, guard still
                    // holds a borrow into it = E0597).
                    let url = state.settings.read().ollama_url.clone();
                    url
                };
                if let Some(child) = ollama_bundled::ensure_running(&app_handle, &url).await {
                    // Stash the child in AppState so the RunEvent::Exit
                    // handler at the bottom of this function can kill it.
                    // Previously we used std::mem::forget which leaked the
                    // handle and left `ollama serve` running after Kilroy
                    // exited, blocking the port for the next launch.
                    let state: tauri::State<'_, state::AppState> = app_handle.state();
                    *state.bundled_ollama.lock() = Some(child);
                }
            });

            // Background Ollama health-check (runs AFTER the spawn above
            // has had a moment to start the daemon, if applicable).
            // We use std::thread::sleep because tokio isn't in our direct
            // dependency list — it's only transitive through Tauri — and
            // a 2-second blocking sleep inside a tauri::async_runtime
            // spawn is benign (it's its own task, doesn't block anything).
            let app_handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                std::thread::sleep(std::time::Duration::from_millis(2000));
                let state: tauri::State<'_, state::AppState> = app_handle.state();
                let emb = state.embedder.clone();
                match emb.health().await {
                    Ok(models) => {
                        let has = models.iter().any(|m| m.starts_with(emb.model().as_str()));
                        tracing::info!(
                            ollama = "ok",
                            embedding_model = %emb.model(),
                            installed = has,
                            "ollama reachable"
                        );
                    }
                    Err(e) => tracing::warn!("ollama unreachable: {:#}", e),
                }
            });

            #[cfg(debug_assertions)]
            {
                if let Some(window) = app.get_webview_window("main") {
                    window.open_devtools();
                }
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            // Filesystem
            commands::fs::list_dir,
            commands::fs::read_file,
            commands::fs::write_file,
            commands::fs::file_exists,
            commands::fs::pick_folder,
            commands::fs::pick_save_file,
            commands::fs::pick_open_file,
            // Terminal (PTY)
            commands::terminal::terminal_spawn,
            commands::terminal::terminal_attach,
            commands::terminal::terminal_flush,
            commands::terminal::terminal_write,
            commands::terminal::terminal_resize,
            commands::terminal::terminal_kill,
            commands::terminal::list_available_shells,
            // Agent
            commands::agent::agent_send_message,
            commands::agent::agent_set_mode,
            commands::agent::agent_get_status,
            commands::smartcoder::smartcoder_status,
            commands::smartcoder::smartcoder_run,
            // Plan / runtime
            commands::plan::update_plan_task,
            commands::plan::delete_plan_task,
            commands::plan::insert_plan_task,
            commands::plan::cancel_plan,
            commands::plan::execute_plan,
            // Actions (actuator review)
            commands::actions::list_actions,
            commands::actions::list_pending_actions_for_task,
            commands::actions::accept_action,
            commands::actions::reject_action,
            // Activity feed
            commands::activity::list_activity,
            // Memory
            commands::memory::open_project,
            commands::memory::save_message,
            commands::memory::list_sessions,
            commands::memory::start_session,
            commands::memory::switch_session,
            commands::memory::index_project,
            commands::memory::search_memory,
            commands::memory::log_decision,
            commands::memory::list_decisions,
            commands::memory::list_tasks,
            commands::memory::project_index_status,
            commands::memory::clear_project_index,
            // Skills
            commands::skills::list_skills,
            commands::skills::read_skill,
            commands::skills::open_skills_folder,
            commands::skills::write_skill,
            // Settings
            commands::settings::get_settings,
            commands::settings::update_settings,
            commands::settings::ollama_health,
            // Models (pull, etc.)
            commands::models::pull_model,
            // Datasets (inspect + Modelfile composition + training env probe)
            commands::datasets::dataset_inspect,
            commands::datasets::dataset_create_modelfile,
            commands::datasets::training_env_status,
            // Distillation corpus
            commands::corpus::corpus_append,
            commands::corpus::corpus_stats,
            commands::corpus::corpus_open_folder,
            // Background refactor swarm
            commands::refactor::refactor_scan_candidates,
            commands::refactor::refactor_analyze_file,
            commands::refactor::refactor_list_proposals,
            commands::refactor::refactor_dismiss_proposal,
            commands::refactor::refactor_apply_proposal,
            commands::refactor::refactor_scan_run_stats,
            // App
            commands::app::app_info,
            // Platform / OS detector
            platform::platform_info,
        ])
        .build(tauri::generate_context!())
        .expect("error while building Kilroy")
        .run(|app_handle, event| {
            // Lifecycle cleanup: when the app is exiting (window closed
            // or quit-from-tray), kill the bundled Ollama daemon if we
            // started one. RunEvent::ExitRequested fires before Exit;
            // we handle both so cleanup runs once.
            if matches!(
                event,
                tauri::RunEvent::Exit | tauri::RunEvent::ExitRequested { .. }
            ) {
                let state: tauri::State<'_, state::AppState> = app_handle.state();
                state.shutdown();
            }
        });
}
