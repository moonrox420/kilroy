//! Shared agent context — used by Kilroy chat, plan executor, and the
//! SmartCoder Python bridge so every path grounds on the same project memory.

use super::memory::list_project_files_sync;
use super::memory::require_memory;
use super::skills::{inject_skills_prompt, list_skills_sync};
use crate::db::{chunks, decisions, messages, projects};
use crate::state::AppState;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tauri::{AppHandle, State};

#[derive(Serialize, Deserialize, Default, Clone)]
pub struct AgentContext {
    pub chunks: Vec<chunks::ChunkHit>,
    pub decisions: Vec<decisions::DecisionHit>,
    pub recent_messages: u32,
    pub ollama_used: bool,
    pub note: Option<String>,
}

/// Everything `agent_send_message` needs after retrieval.
pub struct BuiltAgentContext {
    pub ctx: AgentContext,
    pub recent_msgs: Vec<messages::Message>,
    pub overview_for_prompt: String,
    pub indexed_chunk_count: i64,
    pub project_root: Option<PathBuf>,
    pub project_files: Vec<String>,
}

/// JSON payload written for `kilroy_smartcoder.py --context-file`.
#[derive(Serialize, Clone)]
pub struct KilroyProjectContext {
    pub project_root: String,
    pub project_overview: String,
    pub project_files: Vec<String>,
    pub chunks: Vec<chunks::ChunkHit>,
    pub decisions: Vec<decisions::DecisionHit>,
    pub indexed_chunk_count: i64,
    pub note: Option<String>,
    pub recent_messages: Vec<RecentMessageSnippet>,
}

#[derive(Serialize, Clone)]
pub struct RecentMessageSnippet {
    pub role: String,
    pub content: String,
}

impl From<&BuiltAgentContext> for KilroyProjectContext {
    fn from(built: &BuiltAgentContext) -> Self {
        Self {
            project_root: built
                .project_root
                .as_ref()
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_default(),
            project_overview: built.overview_for_prompt.clone(),
            project_files: built.project_files.clone(),
            chunks: built.ctx.chunks.clone(),
            decisions: built.ctx.decisions.clone(),
            indexed_chunk_count: built.indexed_chunk_count,
            note: built.ctx.note.clone(),
            recent_messages: built
                .recent_msgs
                .iter()
                .map(|m| RecentMessageSnippet {
                    role: m.role.clone(),
                    content: m.content.clone(),
                })
                .collect(),
        }
    }
}

/// Build Kilroy project context from executor fields (plan execute path).
pub fn kilroy_context_from_executor(
    project_root: &std::path::Path,
    project_overview: &str,
    chunks_hits: &[chunks::ChunkHit],
    decisions_hits: &[decisions::DecisionHit],
    recent_msgs: &[messages::Message],
    indexed_chunk_count: i64,
) -> KilroyProjectContext {
    let mut overview = project_overview.to_string();
    let project_files = list_project_files_sync(project_root, 200);
    if !project_files.is_empty() {
        overview.push_str("\n\nfile list (truncated to first 200, sorted):\n");
        for p in &project_files {
            overview.push_str("  - ");
            overview.push_str(p);
            overview.push('\n');
        }
    }
    if indexed_chunk_count == 0 {
        overview.push_str(
            "\nNOTE: Project not indexed — file paths visible but not contents. \
             Run Memory → Index Project (Ctrl+Shift+I) for semantic search.\n",
        );
    }
    KilroyProjectContext {
        project_root: project_root.to_string_lossy().to_string(),
        project_overview: overview,
        project_files,
        chunks: chunks_hits.to_vec(),
        decisions: decisions_hits.to_vec(),
        indexed_chunk_count,
        note: None,
        recent_messages: recent_msgs
            .iter()
            .map(|m| RecentMessageSnippet {
                role: m.role.clone(),
                content: m.content.clone(),
            })
            .collect(),
    }
}

/// Gather retrieval context for a user message — same logic as chat.
pub async fn gather_agent_context(
    app: &AppHandle,
    state: &State<'_, AppState>,
    user_msg: &str,
) -> BuiltAgentContext {
    let mut ctx = AgentContext::default();
    let mut recent_msgs: Vec<messages::Message> = Vec::new();
    let mut project_overview = String::new();
    let mut project_files: Vec<String> = Vec::new();
    let mut indexed_chunk_count: i64 = 0;
    let mut project_root: Option<PathBuf> = None;

    let project_id_opt = *state.current_project_id.lock();
    let session_id_opt = *state.current_session_id.lock();

    if let (Some(pid), Some(sid)) = (project_id_opt, session_id_opt) {
        let embedder = state.embedder.clone();
        project_root = state.memory.lock().as_ref().map(|m| m.root.clone());
        if let Some(ref root) = project_root {
            project_files = list_project_files_sync(root, 200);
        }

        match embedder.embed(user_msg).await {
            Ok(query_vec) => {
                ctx.ollama_used = true;
                if let Ok(memory_conn) = require_memory(state) {
                    let (k_chunks, k_decisions) = {
                        let s = state.settings.read();
                        (s.retrieval_chunks_k, s.retrieval_decisions_k)
                    };
                    let root_str = state
                        .memory
                        .lock()
                        .as_ref()
                        .map(|m| m.root.to_string_lossy().to_string())
                        .unwrap_or_default();
                    let conn = memory_conn.lock();
                    ctx.chunks = chunks::knn(&conn, pid, &query_vec, k_chunks).unwrap_or_default();
                    ctx.decisions =
                        decisions::knn(&conn, pid, &query_vec, k_decisions).unwrap_or_default();
                    recent_msgs = messages::tail(&conn, sid, 8).unwrap_or_default();
                    ctx.recent_messages = recent_msgs.len() as u32;
                    indexed_chunk_count = conn
                        .query_row(
                            "SELECT COUNT(*) FROM chunks c
                             JOIN files f ON f.id = c.file_id
                             WHERE f.project_id = ?1",
                            rusqlite::params![pid],
                            |r| r.get(0),
                        )
                        .unwrap_or(0);
                    if let Ok(p) = projects::fetch_by_path(&conn, &root_str) {
                        project_overview = format!("name: {}\nroot: {}", p.name, p.root_path);
                    }
                }
            }
            Err(e) => {
                ctx.note = Some(format!("embedding skipped: {}", e));
            }
        }
    } else {
        ctx.note = Some("no project open — open a folder to enable memory".into());
    }

    let mut overview_for_prompt = project_overview.clone();
    if !project_files.is_empty() {
        overview_for_prompt.push_str("\n\nfile list (truncated to first 200, sorted):\n");
        for p in &project_files {
            overview_for_prompt.push_str("  - ");
            overview_for_prompt.push_str(p);
            overview_for_prompt.push('\n');
        }
    }
    let skills = list_skills_sync(app, state.memory.lock().as_ref());
    if !skills.is_empty() {
        overview_for_prompt.push_str(&inject_skills_prompt(&skills));
    }
    if project_id_opt.is_some() && indexed_chunk_count == 0 {
        overview_for_prompt.push_str(
            "\nNOTE: This project has NOT been indexed yet — no file CONTENTS have \
             been embedded into Kilroy's memory DB. You can see the file *paths* above \
             but not what's inside them. If the user's request requires looking at \
             specific files, you MUST do one of:\n\
             1. Ask the user to run `Memory → Index Project` (Ctrl+Shift+I) so semantic \
                search starts returning real chunks.\n\
             2. Ask the user to paste the specific file(s) you need.\n\
             3. Refuse to fabricate file contents. NEVER guess at code that lives in \
                a path you haven't actually been shown.\n\n",
        );
    }

    BuiltAgentContext {
        ctx,
        recent_msgs,
        overview_for_prompt,
        indexed_chunk_count,
        project_root,
        project_files,
    }
}
