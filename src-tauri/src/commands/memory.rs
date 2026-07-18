//! Memory commands — the Tauri surface for the brain.
//!
//! Open / list / index a project; persist chat; semantic search the
//! codebase; log decisions. The agent runtime composes these into the
//! actual retrieval-augmented chat loop in `commands/agent.rs`.

use crate::db::{activity, chunks, decisions, files, messages, projects, sessions, tasks, Memory};
use crate::state::AppState;
use parking_lot::Mutex;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tauri::{AppHandle, Emitter, State};
use walkdir_lite::WalkDir;

// ─── Open / current project ─────────────────────────────────────────────────

#[derive(Serialize, Clone)]
pub struct ProjectOpened {
    pub project: projects::Project,
    pub session: sessions::Session,
    pub messages: Vec<messages::Message>,
    pub ollama_status: OllamaStatus,
}

#[derive(Serialize, Clone)]
pub struct OllamaStatus {
    pub reachable: bool,
    pub models: Vec<String>,
    pub embedding_model: String,
    pub has_embedding_model: bool,
}

#[tauri::command]
pub async fn open_project(
    state: State<'_, AppState>,
    path: String,
) -> Result<ProjectOpened, String> {
    let p = PathBuf::from(&path);
    if !p.exists() {
        return Err(format!("path does not exist: {}", path));
    }

    let memory = Memory::open(&p).map_err(|e| format!("open memory: {:#}", e))?;

    let mode_str = mode_as_str(*state.agent_mode.lock());

    let project_record;
    let session_record;
    let history;
    {
        let conn = memory.conn.lock();
        project_record = projects::upsert(&conn, &p).map_err(stringify)?;
        session_record =
            sessions::current_or_create(&conn, project_record.id, mode_str).map_err(stringify)?;
        history = messages::list(&conn, session_record.id, 500).map_err(stringify)?;
    }

    *state.memory.lock() = Some(memory);
    *state.current_project_id.lock() = Some(project_record.id);
    *state.current_session_id.lock() = Some(session_record.id);

    // Activity log — project opened.
    if let Ok(memory_conn) = require_memory(&state) {
        let conn = memory_conn.lock();
        let _ = activity::log(
            &conn,
            Some(session_record.id),
            "project_opened",
            &serde_json::json!({
                "project_id": project_record.id,
                "name": project_record.name,
                "root": project_record.root_path,
            })
            .to_string(),
        );
    }

    let ollama_status = check_ollama(&state).await;

    Ok(ProjectOpened {
        project: project_record,
        session: session_record,
        messages: history,
        ollama_status,
    })
}

async fn check_ollama(state: &State<'_, AppState>) -> OllamaStatus {
    let emb = state.embedder.clone();
    let embedding_model = emb.model().to_string();
    match emb.health().await {
        Ok(models) => {
            let has = models.iter().any(|m| m.starts_with(&embedding_model));
            OllamaStatus {
                reachable: true,
                models,
                embedding_model,
                has_embedding_model: has,
            }
        }
        Err(e) => {
            tracing::warn!("ollama unreachable: {:#}", e);
            OllamaStatus {
                reachable: false,
                models: vec![],
                embedding_model,
                has_embedding_model: false,
            }
        }
    }
}

// ─── Chat persistence ───────────────────────────────────────────────────────

#[tauri::command]
pub async fn save_message(
    state: State<'_, AppState>,
    role: String,
    content: String,
    metadata: Option<String>,
) -> Result<messages::Message, String> {
    let session_id = require_session(&state)?;
    let memory_conn = require_memory(&state)?;
    let conn = memory_conn.lock();
    messages::insert(
        &conn,
        session_id,
        &role,
        &content,
        metadata.as_deref(),
        None,
    )
    .map_err(stringify)
}

#[tauri::command]
pub async fn list_sessions(
    state: State<'_, AppState>,
    limit: Option<i64>,
) -> Result<Vec<sessions::Session>, String> {
    let project_id = require_project(&state)?;
    let memory_conn = require_memory(&state)?;
    let conn = memory_conn.lock();
    sessions::list(&conn, project_id, limit.unwrap_or(20)).map_err(stringify)
}

#[tauri::command]
pub async fn start_session(state: State<'_, AppState>) -> Result<sessions::Session, String> {
    let project_id = require_project(&state)?;
    let memory_conn = require_memory(&state)?;
    let mode_str = mode_as_str(*state.agent_mode.lock());
    let session = {
        let conn = memory_conn.lock();
        sessions::create(&conn, project_id, mode_str).map_err(stringify)?
    };
    *state.current_session_id.lock() = Some(session.id);
    Ok(session)
}

#[derive(Serialize, Clone)]
pub struct SessionSwitched {
    pub session: sessions::Session,
    pub messages: Vec<messages::Message>,
}

/// Switch the active session to a different one, loading its message history.
/// Returns the session record and its messages so the frontend can hydrate
/// the chat store.
#[tauri::command]
pub async fn switch_session(
    state: State<'_, AppState>,
    session_id: i64,
) -> Result<SessionSwitched, String> {
    let project_id = require_project(&state)?;
    let memory_conn = require_memory(&state)?;
    let conn = memory_conn.lock();

    // Verify the session belongs to the current project
    let session =
        sessions::get(&conn, session_id).map_err(|e| format!("session not found: {:#}", e))?;
    if session.project_id != project_id {
        return Err("session does not belong to the current project".to_string());
    }

    let history = messages::list(&conn, session_id, 500).map_err(stringify)?;

    *state.current_session_id.lock() = Some(session_id);

    Ok(SessionSwitched {
        session,
        messages: history,
    })
}

// ─── Project indexing ───────────────────────────────────────────────────────

#[derive(Serialize, Clone)]
pub struct IndexProgress {
    pub phase: String,
    pub current: u32,
    pub total: u32,
    pub message: String,
}

#[derive(Serialize, Clone)]
pub struct IndexResult {
    pub files_seen: u32,
    pub files_indexed: u32,
    pub chunks_inserted: u32,
    pub skipped_too_large: u32,
    pub skipped_binary: u32,
    pub errors: u32,
    pub duration_ms: u64,
}

const MAX_FILE_BYTES: u64 = 1_000_000; // 1 MB cap per file

#[tauri::command]
pub async fn index_project(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<IndexResult, String> {
    let started = std::time::Instant::now();
    let project_id = require_project(&state)?;
    let memory_conn = require_memory(&state)?;
    let root = state
        .memory
        .lock()
        .as_ref()
        .map(|m| m.root.clone())
        .ok_or_else(|| "no memory open".to_string())?;
    let embedder = state.embedder.clone();

    // Collect candidate files first so we have a stable total for progress UI.
    let candidates: Vec<PathBuf> = WalkDir::new(&root)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
        .map(|e| e.into_path())
        .filter(|p| !is_ignored(p, &root))
        .collect();

    let total = candidates.len() as u32;
    emit_progress(
        &app,
        IndexProgress {
            phase: "walking".into(),
            current: 0,
            total,
            message: format!("found {} candidate files", total),
        },
    );

    let mut result = IndexResult {
        files_seen: total,
        files_indexed: 0,
        chunks_inserted: 0,
        skipped_too_large: 0,
        skipped_binary: 0,
        errors: 0,
        duration_ms: 0,
    };

    for (i, path) in candidates.iter().enumerate() {
        let rel = path.strip_prefix(&root).unwrap_or(path).to_path_buf();
        emit_progress(
            &app,
            IndexProgress {
                phase: "indexing".into(),
                current: (i + 1) as u32,
                total,
                message: rel.display().to_string(),
            },
        );

        let meta = match std::fs::metadata(path) {
            Ok(m) => m,
            Err(_) => {
                result.errors += 1;
                continue;
            }
        };
        if meta.len() > MAX_FILE_BYTES {
            result.skipped_too_large += 1;
            continue;
        }

        let bytes = match std::fs::read(path) {
            Ok(b) => b,
            Err(_) => {
                result.errors += 1;
                continue;
            }
        };
        if looks_binary(&bytes) {
            result.skipped_binary += 1;
            continue;
        }
        let text = String::from_utf8_lossy(&bytes).to_string();
        let hash = files::hash(&text);
        let language = language_for_path(path);

        // upsert + check whether re-chunking is needed
        let (file_id, needs_reindex) = {
            let conn = memory_conn.lock();
            match files::upsert(
                &conn,
                project_id,
                &rel.to_string_lossy(),
                &hash,
                bytes.len() as i64,
                language.as_deref(),
            ) {
                Ok(t) => t,
                Err(e) => {
                    tracing::warn!(file = %rel.display(), "upsert: {:#}", e);
                    result.errors += 1;
                    continue;
                }
            }
        };
        if !needs_reindex {
            continue;
        }

        // Chunk + embed. Window + stride come from live settings so the
        // user can tune chunking without recompiling.
        let (window, stride) = {
            let s = state.settings.read();
            (s.chunk_window, s.chunk_stride)
        };
        let pieces = chunks::chunk_text_with(&text, window, stride);
        let texts: Vec<String> = pieces.iter().map(|c| c.content.clone()).collect();
        let vectors = match embedder.embed_many(&texts).await {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!(file = %rel.display(), "embed: {:#}", e);
                result.errors += 1;
                continue;
            }
        };

        // Single transaction per file keeps writes durable + atomic.
        let conn = memory_conn.lock();
        if let Err(e) = (|| -> anyhow::Result<()> {
            conn.execute_batch("BEGIN;")?;
            let ids = chunks::insert_chunks(&conn, file_id, &pieces)?;
            for (chunk_id, vec) in ids.iter().zip(vectors.iter()) {
                chunks::insert_embedding(&conn, *chunk_id, vec)?;
            }
            conn.execute_batch("COMMIT;")?;
            Ok(())
        })() {
            conn.execute_batch("ROLLBACK;").ok();
            tracing::warn!(file = %rel.display(), "insert: {:#}", e);
            result.errors += 1;
            continue;
        }
        result.files_indexed += 1;
        result.chunks_inserted += pieces.len() as u32;
    }

    result.duration_ms = started.elapsed().as_millis() as u64;
    emit_progress(
        &app,
        IndexProgress {
            phase: "done".into(),
            current: total,
            total,
            message: format!(
                "{} files, {} chunks in {}ms",
                result.files_indexed, result.chunks_inserted, result.duration_ms
            ),
        },
    );

    // Activity log — index completed.
    {
        let conn = memory_conn.lock();
        let _ = activity::log(
            &conn,
            *state.current_session_id.lock(),
            "index_completed",
            &serde_json::json!({
                "files_indexed": result.files_indexed,
                "chunks_inserted": result.chunks_inserted,
                "duration_ms": result.duration_ms,
            })
            .to_string(),
        );
    }
    Ok(result)
}

fn emit_progress(app: &AppHandle, p: IndexProgress) {
    let _ = app.emit("memory://index/progress", p);
}

// ─── Semantic search ────────────────────────────────────────────────────────

#[derive(Serialize)]
pub struct SearchResult {
    pub chunks: Vec<chunks::ChunkHit>,
    pub decisions: Vec<decisions::DecisionHit>,
}

#[tauri::command]
pub async fn search_memory(
    state: State<'_, AppState>,
    query: String,
    k: Option<usize>,
) -> Result<SearchResult, String> {
    let project_id = require_project(&state)?;
    let memory_conn = require_memory(&state)?;
    let embedder = state.embedder.clone();

    let query_vec = embedder.embed(&query).await.map_err(stringify)?;
    let k = k.unwrap_or(8);

    let (chunks_hits, decision_hits) = {
        let conn = memory_conn.lock();
        let c = chunks::knn(&conn, project_id, &query_vec, k).map_err(stringify)?;
        let d = decisions::knn(&conn, project_id, &query_vec, k.min(4)).map_err(stringify)?;
        (c, d)
    };

    Ok(SearchResult {
        chunks: chunks_hits,
        decisions: decision_hits,
    })
}

// ─── Decisions ──────────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct LogDecisionPayload {
    pub title: String,
    pub summary: String,
    pub rationale: Option<String>,
    pub related_files: Option<Vec<String>>,
}

#[tauri::command]
pub async fn log_decision(
    state: State<'_, AppState>,
    payload: LogDecisionPayload,
) -> Result<i64, String> {
    let project_id = require_project(&state)?;
    let memory_conn = require_memory(&state)?;
    let embedder = state.embedder.clone();

    let combined = format!(
        "{}\n{}\n{}",
        payload.title,
        payload.summary,
        payload.rationale.clone().unwrap_or_default()
    );
    let related_json = payload
        .related_files
        .as_ref()
        .map(|v| serde_json::to_string(v).unwrap_or_else(|_| "[]".into()));
    let vec = embedder.embed(&combined).await.map_err(stringify)?;

    let conn = memory_conn.lock();
    let id = decisions::insert(
        &conn,
        project_id,
        &payload.title,
        &payload.summary,
        payload.rationale.as_deref(),
        related_json.as_deref(),
    )
    .map_err(stringify)?;
    decisions::insert_embedding(&conn, id, &vec).map_err(stringify)?;
    let _ = activity::log(
        &conn,
        *state.current_session_id.lock(),
        "decision_logged",
        &serde_json::json!({ "decision_id": id, "title": payload.title }).to_string(),
    );
    Ok(id)
}

#[tauri::command]
pub async fn list_decisions(
    state: State<'_, AppState>,
    limit: Option<i64>,
) -> Result<Vec<decisions::Decision>, String> {
    let project_id = require_project(&state)?;
    let memory_conn = require_memory(&state)?;
    let conn = memory_conn.lock();
    decisions::list(&conn, project_id, limit.unwrap_or(50)).map_err(stringify)
}

// ─── Task history ───────────────────────────────────────────────────────────

#[tauri::command]
pub async fn list_tasks(
    state: State<'_, AppState>,
    limit: Option<i64>,
) -> Result<Vec<tasks::Task>, String> {
    let session_id = require_session(&state)?;
    let memory_conn = require_memory(&state)?;
    let conn = memory_conn.lock();
    tasks::list_for_session(&conn, session_id, limit.unwrap_or(50)).map_err(stringify)
}

// ─── State accessor helpers ─────────────────────────────────────────────────

pub(crate) fn require_session(state: &State<'_, AppState>) -> Result<i64, String> {
    (*state.current_session_id.lock()).ok_or_else(|| "no active session".to_string())
}

pub(crate) fn require_project(state: &State<'_, AppState>) -> Result<i64, String> {
    (*state.current_project_id.lock()).ok_or_else(|| "no active project".to_string())
}

pub(crate) fn require_memory(
    state: &State<'_, AppState>,
) -> Result<Arc<Mutex<Connection>>, String> {
    state
        .memory
        .lock()
        .as_ref()
        .map(|m| m.conn.clone())
        .ok_or_else(|| "no memory open".to_string())
}

/// Quick directory walk that the agent's prompt builder uses to inject
/// a flat list of project paths into the system prompt. We honour the
/// same `is_ignored` rules as the indexer so the model sees a clean
/// picture (no `node_modules`, no `target/debug`, etc).
///
/// `max_paths` caps the output — for a 100k-file repo we don't want to
/// stuff 100k paths into the prompt window. The default caller passes
/// 200 which is plenty to give the model a structural overview.
pub fn list_project_files_sync(root: &Path, max_paths: usize) -> Vec<String> {
    let mut out: Vec<String> = Vec::with_capacity(max_paths.min(256));
    let walker = walkdir_lite::WalkDir::new(root).into_iter();
    for entry in walker.filter_map(|e| e.ok()) {
        if !entry.file_type().is_file() {
            continue;
        }
        let path = entry.into_path();
        if is_ignored(&path, root) {
            continue;
        }
        let rel = path
            .strip_prefix(root)
            .unwrap_or(&path)
            .to_string_lossy()
            .to_string();
        out.push(rel);
        if out.len() >= max_paths {
            break;
        }
    }
    out.sort();
    out
}

/// True iff the indexer has populated any chunks for the active project —
/// used by the chat banner to decide whether to nag the user.
#[tauri::command]
pub async fn project_index_status(
    state: State<'_, AppState>,
) -> Result<ProjectIndexStatus, String> {
    let Some(project_id) = *state.current_project_id.lock() else {
        return Ok(ProjectIndexStatus::default());
    };
    let memory_conn = require_memory(&state)?;
    let conn = memory_conn.lock();
    let files: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM files WHERE project_id = ?1",
            rusqlite::params![project_id],
            |r| r.get(0),
        )
        .unwrap_or(0);
    let chunks: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM chunks c
             JOIN files f ON f.id = c.file_id
             WHERE f.project_id = ?1",
            rusqlite::params![project_id],
            |r| r.get(0),
        )
        .unwrap_or(0);
    Ok(ProjectIndexStatus {
        files_indexed: files as u32,
        chunks_indexed: chunks as u32,
        is_indexed: chunks > 0,
    })
}

/// Wipe indexed files/chunks/embeddings for the active project. Chat history,
/// decisions, and tasks are preserved so you can re-index a narrower tree.
#[tauri::command]
pub async fn clear_project_index(state: State<'_, AppState>) -> Result<ClearIndexResult, String> {
    let Some(project_id) = *state.current_project_id.lock() else {
        return Err("no project open".into());
    };
    let memory_conn = require_memory(&state)?;
    let conn = memory_conn.lock();

    let files: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM files WHERE project_id = ?1",
            rusqlite::params![project_id],
            |r| r.get(0),
        )
        .unwrap_or(0);
    let chunks: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM chunks c
             JOIN files f ON f.id = c.file_id
             WHERE f.project_id = ?1",
            rusqlite::params![project_id],
            |r| r.get(0),
        )
        .unwrap_or(0);

    if files == 0 && chunks == 0 {
        return Ok(ClearIndexResult {
            files_removed: 0,
            chunks_removed: 0,
        });
    }

    conn.execute_batch("BEGIN;")
        .map_err(|e| format!("clear index begin: {:#}", e))?;
    let wipe = (|| -> Result<(), rusqlite::Error> {
        conn.execute(
            "DELETE FROM chunk_embeddings
             WHERE chunk_id IN (
               SELECT c.id FROM chunks c
               JOIN files f ON f.id = c.file_id
               WHERE f.project_id = ?1
             )",
            rusqlite::params![project_id],
        )?;
        conn.execute(
            "DELETE FROM chunks
             WHERE file_id IN (SELECT id FROM files WHERE project_id = ?1)",
            rusqlite::params![project_id],
        )?;
        conn.execute(
            "DELETE FROM files WHERE project_id = ?1",
            rusqlite::params![project_id],
        )?;
        Ok(())
    })();
    match wipe {
        Ok(()) => conn
            .execute_batch("COMMIT;")
            .map_err(|e| format!("clear index commit: {:#}", e))?,
        Err(e) => {
            conn.execute_batch("ROLLBACK;").ok();
            return Err(format!("clear index: {:#}", e));
        }
    }

    Ok(ClearIndexResult {
        files_removed: files as u32,
        chunks_removed: chunks as u32,
    })
}

#[derive(Serialize, Default, Clone)]
pub struct ProjectIndexStatus {
    pub files_indexed: u32,
    pub chunks_indexed: u32,
    pub is_indexed: bool,
}

#[derive(Serialize, Clone)]
pub struct ClearIndexResult {
    pub files_removed: u32,
    pub chunks_removed: u32,
}

fn stringify<E: std::fmt::Display>(e: E) -> String {
    format!("{:#}", e)
}

fn mode_as_str(mode: crate::state::AgentMode) -> &'static str {
    match mode {
        crate::state::AgentMode::Copilot => "copilot",
        crate::state::AgentMode::Autonomous => "autonomous",
        crate::state::AgentMode::MultiAgent => "multi_agent",
        crate::state::AgentMode::Governance => "governance",
        crate::state::AgentMode::Council => "council",
        crate::state::AgentMode::Debug => "debug",
        crate::state::AgentMode::TestFirst => "test_first",
        crate::state::AgentMode::CodeAgent => "code_agent",
    }
}

fn looks_binary(bytes: &[u8]) -> bool {
    // Sample the first 8KB. If we see a NUL byte or too many high control
    // chars, treat as binary.
    let sample = &bytes[..bytes.len().min(8192)];
    let mut weird = 0usize;
    for &b in sample {
        if b == 0 {
            return true;
        }
        if b < 9 || (b > 13 && b < 32) {
            weird += 1;
        }
    }
    weird * 100 > sample.len() * 30
}

fn is_ignored(path: &Path, root: &Path) -> bool {
    let rel = path.strip_prefix(root).unwrap_or(path);
    for component in rel.components() {
        let s = component.as_os_str().to_string_lossy();
        match s.as_ref() {
            ".git" | "node_modules" | "target" | "dist" | "build" | ".next" | ".vite"
            | ".turbo" | ".cache" | ".kilroy" | "__pycache__" | ".venv" | "venv" | ".idea"
            | ".vscode" | "gen" => return true,
            _ => {}
        }
    }
    // Skip dotfiles at any depth EXCEPT well-known config files.
    if let Some(name) = path.file_name() {
        let n = name.to_string_lossy();
        if n.starts_with('.')
            && !matches!(
                n.as_ref(),
                ".env.example" | ".eslintrc" | ".prettierrc" | ".editorconfig" | ".gitignore"
            )
        {
            return true;
        }
        // Common binaries / lockfiles we don't want chunked.
        let lower = n.to_lowercase();
        if lower.ends_with(".lock")
            || lower.ends_with(".pdf")
            || lower.ends_with(".zip")
            || lower.ends_with(".tar")
            || lower.ends_with(".gz")
            || lower.ends_with(".png")
            || lower.ends_with(".jpg")
            || lower.ends_with(".jpeg")
            || lower.ends_with(".gif")
            || lower.ends_with(".ico")
            || lower.ends_with(".icns")
            || lower.ends_with(".webp")
            || lower.ends_with(".mp4")
            || lower.ends_with(".mp3")
            || lower.ends_with(".wav")
            || lower.ends_with(".exe")
            || lower.ends_with(".dll")
            || lower.ends_with(".so")
            || lower.ends_with(".dylib")
            || lower.ends_with(".class")
            || lower.ends_with(".jar")
        {
            return true;
        }
    }
    false
}

fn language_for_path(path: &Path) -> Option<String> {
    let ext = path.extension()?.to_str()?.to_lowercase();
    let lang = match ext.as_str() {
        "ts" | "tsx" => "typescript",
        "js" | "jsx" | "mjs" | "cjs" => "javascript",
        "rs" => "rust",
        "py" => "python",
        "go" => "go",
        "java" => "java",
        "kt" | "kts" => "kotlin",
        "rb" => "ruby",
        "swift" => "swift",
        "c" | "h" => "c",
        "cpp" | "cc" | "hpp" => "cpp",
        "cs" => "csharp",
        "php" => "php",
        "lua" => "lua",
        "sh" | "bash" | "zsh" => "shell",
        "ps1" | "psm1" => "powershell",
        "sql" => "sql",
        "html" | "htm" => "html",
        "css" | "scss" | "sass" | "less" => "css",
        "json" => "json",
        "yml" | "yaml" => "yaml",
        "toml" => "toml",
        "md" | "markdown" => "markdown",
        _ => return None,
    };
    Some(lang.to_string())
}

// ─── A minimal walkdir to avoid pulling another crate ───────────────────────

mod walkdir_lite {
    use std::path::PathBuf;

    pub struct WalkDir {
        root: PathBuf,
    }

    impl WalkDir {
        pub fn new(root: impl Into<PathBuf>) -> Self {
            Self { root: root.into() }
        }
        pub fn into_iter(self) -> Iter {
            Iter {
                stack: vec![self.root],
            }
        }
    }

    pub struct Entry {
        path: PathBuf,
        ft: std::fs::FileType,
    }

    impl Entry {
        pub fn file_type(&self) -> &std::fs::FileType {
            &self.ft
        }
        pub fn into_path(self) -> PathBuf {
            self.path
        }
    }

    pub struct Iter {
        stack: Vec<PathBuf>,
    }

    impl Iterator for Iter {
        type Item = std::io::Result<Entry>;
        fn next(&mut self) -> Option<Self::Item> {
            while let Some(p) = self.stack.pop() {
                let meta = std::fs::symlink_metadata(&p);
                let ft = match meta {
                    Ok(m) => m.file_type(),
                    Err(e) => return Some(Err(e)),
                };
                // Note on symlinks: symlink_metadata reports the link's
                // own type, NOT the target's. So a symlink to a directory
                // has ft.is_dir() == false and gets yielded as a single
                // entry (indexed once, no recursion into the target),
                // which means we never follow symlinks into directories
                // and therefore never cycle. Symlinked FILES still get
                // indexed via the entry we yield — preserves indexing
                // function the user expects.
                if ft.is_dir() {
                    match std::fs::read_dir(&p) {
                        Ok(rd) => {
                            for child in rd.flatten() {
                                self.stack.push(child.path());
                            }
                        }
                        Err(_) => continue,
                    }
                    continue;
                }
                return Some(Ok(Entry { path: p, ft }));
            }
            None
        }
    }
}
