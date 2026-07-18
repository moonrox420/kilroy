//! Executor — walks the planned task graph one task at a time.
//!
//! For each task it composes a role-specific system prompt, streams the
//! reply from Ollama, emits live events to the frontend, and persists
//! the start/end state to the `tasks` table. Outputs from earlier tasks
//! are appended to the system context of later ones so the executor
//! produces a coherent chain of reasoning rather than independent shots.
//!
//! After a task succeeds we parse its output for fenced blocks. Each
//! parseable block becomes a pending row in the `actions` table and an
//! `actuator://action_proposed` event so the UI can render an
//! Accept/Reject card inline. We never touch disk here — the
//! `commands::actions::accept_action` command is the only writer.

use crate::actuator::{self, parser, ActionPayload, SandboxKind};
use crate::db::{
    actions as db_actions, agent_runtime, chunks::ChunkHit, decisions::DecisionHit, messages, tasks,
};
use crate::generation::{ChatClient, ChatMessage, ChatOptions};
use crate::runtime::events::{PlannedTask, TaskChunk, TaskCompleted, TaskStarted};
use parking_lot::Mutex;
use rusqlite::Connection;
use serde::Serialize;
use std::path::PathBuf;
use std::sync::Arc;
use tauri::{AppHandle, Emitter};

pub struct ExecCtx<'a> {
    pub app: &'a AppHandle,
    pub client: &'a ChatClient,
    pub run_id: &'a str,
    pub memory: Arc<Mutex<Connection>>,
    pub session_id: i64,
    pub project_root: PathBuf,
    pub project_overview: &'a str,
    pub user_goal: &'a str,
    pub chunks: &'a [ChunkHit],
    pub decisions: &'a [DecisionHit],
    pub recent_msgs: &'a [messages::Message],
    pub indexed_chunk_count: i64,
    pub ollama_host: &'a str,
    pub chat_model: &'a str,
    /// Default sandbox kind from settings. Stamped onto shell actions the
    /// parser proposes so the user's preference is respected.
    pub default_sandbox: SandboxKind,
}

pub struct ExecResult {
    pub task_outputs: Vec<(i64, String)>,
    pub any_failed: bool,
}

#[derive(Serialize, Clone)]
struct ActionProposedEvent {
    run_id: String,
    task_id: i64,
    action_id: i64,
    kind: String,
    target: Option<String>,
    has_diff: bool,
}

#[derive(Serialize, Clone)]
struct EditorPreviewEvent {
    run_id: String,
    action_id: i64,
    path: String,
    contents: String,
    diff: Option<String>,
}

pub async fn execute(ctx: &ExecCtx<'_>, plan: &[PlannedTask]) -> ExecResult {
    let mut task_outputs: Vec<(i64, String)> = Vec::new();
    let mut any_failed = false;

    for (ordinal, task) in plan.iter().enumerate() {
        let runtime_step_id = {
            let conn = ctx.memory.lock();
            agent_runtime::create_step(
                &conn,
                ctx.run_id,
                (ordinal + 1) as i64,
                "plan_task",
                Some(&task.input),
            )
            .ok()
        };
        let _ = ctx.app.emit(
            "agent://run/task_started",
            TaskStarted {
                run_id: ctx.run_id.to_string(),
                task_id: task.task_id,
            },
        );
        {
            let conn = ctx.memory.lock();
            let _ = tasks::mark_running(&conn, task.task_id);
        }

        let accumulated: String;
        let app = ctx.app.clone();
        let run_id = ctx.run_id.to_string();
        let task_id = task.task_id;

        let stream_result: Result<(), anyhow::Error> = {
            let messages = build_messages(ctx, task, &task_outputs);
            let mut acc = String::new();
            let app2 = app.clone();
            let run_id2 = run_id.clone();
            let res = ctx
                .client
                .chat_stream(
                    &messages,
                    Some(ChatOptions {
                        temperature: Some(0.3),
                        num_predict: Some(2048),
                        top_p: None,
                        num_ctx: Some(8192),
                    }),
                    |delta| {
                        acc.push_str(delta);
                        let _ = app2.emit(
                            "agent://run/task_chunk",
                            TaskChunk {
                                run_id: run_id2.clone(),
                                task_id,
                                delta: delta.to_string(),
                            },
                        );
                    },
                )
                .await;
            accumulated = acc;
            res.map(|_| ())
        };

        match stream_result {
            Ok(_result) => {
                {
                    let conn = ctx.memory.lock();
                    let _ = tasks::mark_success(&conn, task.task_id, &accumulated);
                    if let Some(step_id) = runtime_step_id {
                        let _ = agent_runtime::finish_step(
                            &conn,
                            step_id,
                            "completed",
                            Some(&accumulated),
                            None,
                            1,
                            0,
                        );
                    }
                }
                let proposed =
                    extract_and_persist_actions(ctx, task.task_id, runtime_step_id, &accumulated);
                let preview = preview_of(&accumulated);
                let _ = ctx.app.emit(
                    "agent://run/task_completed",
                    TaskCompleted {
                        run_id: ctx.run_id.to_string(),
                        task_id: task.task_id,
                        success: true,
                        output_preview: preview,
                    },
                );
                for evt in proposed {
                    let _ = ctx.app.emit("actuator://action_proposed", evt);
                }
                task_outputs.push((task.task_id, accumulated));
            }
            Err(e) => {
                any_failed = true;
                let err_msg = format!("error: {:#}", e);
                {
                    let conn = ctx.memory.lock();
                    let _ = tasks::mark_failed(&conn, task.task_id, &err_msg);
                    if let Some(step_id) = runtime_step_id {
                        let _ = agent_runtime::finish_step(
                            &conn,
                            step_id,
                            "failed",
                            None,
                            Some(&err_msg),
                            1,
                            0,
                        );
                    }
                }
                let _ = ctx.app.emit(
                    "agent://run/task_completed",
                    TaskCompleted {
                        run_id: ctx.run_id.to_string(),
                        task_id: task.task_id,
                        success: false,
                        output_preview: err_msg.clone(),
                    },
                );
                task_outputs.push((task.task_id, err_msg));
                break;
            }
        }
    }

    ExecResult {
        task_outputs,
        any_failed,
    }
}

fn extract_and_persist_actions(
    ctx: &ExecCtx<'_>,
    task_id: i64,
    runtime_step_id: Option<i64>,
    output: &str,
) -> Vec<ActionProposedEvent> {
    let blocks = parser::extract_blocks(output);
    let mut events = Vec::new();

    let conn = ctx.memory.lock();
    for block in &blocks {
        let Some(mut payload) = actuator::payload_from_block(block) else {
            continue;
        };
        // Stamp the user's preferred sandbox on shell actions.
        if let ActionPayload::Shell { sandbox, .. } = &mut payload {
            *sandbox = ctx.default_sandbox.clone();
        }

        let (kind, target, diff, preview) = match &payload {
            ActionPayload::FileWrite { path, content, .. } => {
                let diff = actuator::diff_against_disk(&ctx.project_root, path, content);
                (
                    "file_write",
                    Some(path.clone()),
                    diff,
                    Some(content.clone()),
                )
            }
            ActionPayload::FilePatch { path, unified_diff } => {
                let preview =
                    actuator::preview_file_patch(&ctx.project_root, path, unified_diff).ok();
                (
                    "file_patch",
                    Some(path.clone()),
                    Some(unified_diff.clone()),
                    preview,
                )
            }
            ActionPayload::Shell { command, .. } => {
                let snippet: String = command.chars().take(80).collect();
                ("shell", Some(snippet), None, None)
            }
        };

        let payload_json = serde_json::to_string(&payload).unwrap_or_else(|_| "{}".into());

        let inserted = db_actions::insert(
            &conn,
            db_actions::NewAction {
                session_id: Some(ctx.session_id),
                task_id: Some(task_id),
                kind,
                target: target.as_deref(),
                payload: &payload_json,
                diff: diff.as_deref(),
            },
        );
        match inserted {
            Ok(id) => {
                if let Some(step_id) = runtime_step_id {
                    let _ = agent_runtime::record_pending_approval(
                        &conn,
                        ctx.run_id,
                        step_id,
                        id,
                        kind,
                        &payload_json,
                    );
                }
                events.push(ActionProposedEvent {
                    run_id: ctx.run_id.to_string(),
                    task_id,
                    action_id: id,
                    kind: kind.to_string(),
                    target: target.clone(),
                    has_diff: diff.is_some(),
                });
                if let (Some(path), Some(contents)) = (target.clone(), preview) {
                    let _ = ctx.app.emit(
                        "agent://editor/preview",
                        EditorPreviewEvent {
                            run_id: ctx.run_id.to_string(),
                            action_id: id,
                            path,
                            contents,
                            diff: diff.clone(),
                        },
                    );
                }
            }
            Err(e) => {
                tracing::warn!("persist action: {:#}", e);
            }
        }
    }
    events
}

/// Return the role-specific system-prompt prelude for a task agent.
///
/// The plan-and-execute runtime is multi-agent: planner, architect,
/// developer, qa, reviewer, orchestrator. Without per-role prompts every
/// role gets the same instructions and produces the same kind of output
/// — defeating the point of having different agents. Each role here
/// gets distinct instructions on what to PRODUCE and what to AVOID, so a
/// reviewer doesn't accidentally write implementation code and a qa
/// agent doesn't propose architecture changes.
fn role_prompt(agent: &str) -> &'static str {
    match agent.to_lowercase().as_str() {
        "architect" => {
            "You are an architect. PRODUCE: a structured design — \
             component diagram described in prose, data-flow, key \
             interfaces, persistence boundaries, tradeoffs explicit. AVOID: \
             implementation code (the developer agent will write that). \
             End with an explicit 'open questions' list if any decisions \
             are deferred. Output budget: aim for 600-1200 words of analysis."
        }
        "developer" | "coder" | "engineer" => {
            "You are a senior developer. PRODUCE: the minimum-viable code \
             changes that satisfy the task. Prefer small unified diffs over \
             whole-file rewrites. Match existing style and conventions in \
             the retrieved code. If you need to add a dependency, call it \
             out explicitly. AVOID: unrelated refactors, speculative \
             abstractions, or 'while we're here' changes."
        }
        "qa" | "tester" => {
            "You are a QA engineer. PRODUCE: test cases that exercise the \
             changed paths — happy path, edge cases, regression scenarios. \
             Use the project's existing test framework (look at retrieved \
             code to identify it). For each test, state what it's verifying \
             in one sentence above the code. AVOID: implementation code, \
             non-test refactors, or proposing changes to production code."
        }
        "reviewer" => {
            "You are a code reviewer. PRODUCE: a structured review of \
             the diff or output from prior tasks — sections: SUMMARY (one \
             paragraph), BLOCKING ISSUES (must-fix before merge, with \
             file:line references), SUGGESTIONS (nice-to-haves), POSITIVES \
             (call out genuinely good choices). AVOID: rewriting the code \
             yourself; only flag what should change and why."
        }
        "orchestrator" => {
            "You are an orchestrator. PRODUCE: a synthesis of prior task \
             outputs — what's done, what's blocked, what the next step \
             should be. Keep it short (under 300 words). AVOID: redoing \
             work that prior tasks already completed."
        }
        "planner" => {
            "You are a planner clarifying scope. PRODUCE: a tight \
             definition of done in 3-5 bullet points, followed by any \
             clarifying questions you need before execution can start. \
             AVOID: writing code, designing components, or committing to \
             implementation details."
        }
        _ => {
            "You are an autonomous agent. PRODUCE: a concrete answer to \
             the task. Match the format the user's request implies. AVOID: \
             scope creep beyond the task input."
        }
    }
}

/// Universal recovery clause — appended to every role prompt so the
/// model has a consistent escape hatch when context is insufficient,
/// rather than silently fabricating output. The executor currently
/// can't act on this signal, but surfacing it in the task output gives
/// the user a usable trail.
const STUCK_CLAUSE: &str = "If the task is under-specified or the retrieved context is \
     insufficient to complete it confidently, STOP and emit a single \
     line `BLOCKED: <reason>` followed by what you'd need to proceed. \
     Do not fabricate file contents you have not been shown.";

fn build_messages(
    ctx: &ExecCtx<'_>,
    task: &PlannedTask,
    prior_outputs: &[(i64, String)],
) -> Vec<ChatMessage> {
    let mut sys = String::new();
    sys.push_str(&format!(
        "You are Kilroy's {} agent ({} task). Be concrete. Use fenced code blocks for code.\n\n\
         # Role brief\n{}\n\n{}\n\n\
         FILE EDITING OUTPUT FORMAT:\n\
         * For EDITS to an existing file, emit a unified diff in a fenced block whose info string \
           is `diff` followed by the path, e.g. ```diff src/lib.rs```. Include `--- a/<path>` and \
           `+++ b/<path>` headers plus standard `@@` hunks. Smaller is better — Kilroy lets the user \
           cherry-pick hunks on accept.\n\
         * For BRAND NEW files (the path does not yet exist), emit the complete contents in a fenced \
           block whose info string is the language followed by the path, e.g. ```rust src/new.rs```.\n\
         * For shell commands, use ```powershell or ```bash. By default they run inside Windows \
           Sandbox; mark trivial read-only commands explicitly if you want host execution.\n\
         Do not invent file paths.\n\n",
        task.agent, task.r#type, role_prompt(&task.agent), STUCK_CLAUSE,
    ));
    sys.push_str("# Project overview\n");
    sys.push_str(ctx.project_overview);
    sys.push_str("\n\n# User goal\n");
    sys.push_str(ctx.user_goal);
    sys.push('\n');

    if !ctx.chunks.is_empty() {
        sys.push_str("\n# Retrieved code (top matches)\n");
        for c in ctx.chunks {
            sys.push_str(&format!(
                "\n## {}:{}-{}\n```\n{}\n```\n",
                c.file_path,
                c.start_line,
                c.end_line,
                truncate(&c.content, 1200)
            ));
        }
    }
    if !ctx.decisions.is_empty() {
        sys.push_str("\n# Prior decisions\n");
        for d in ctx.decisions {
            sys.push_str(&format!("- {} — {}\n", d.title, d.summary));
        }
    }
    if !prior_outputs.is_empty() {
        sys.push_str("\n# Prior task outputs in this run\n");
        for (id, out) in prior_outputs {
            sys.push_str(&format!("\n## task #{}\n{}\n", id, truncate(out, 1500)));
        }
    }

    vec![
        ChatMessage::text("system", sys),
        ChatMessage::text("user", task.input.clone()),
    ]
}

fn truncate(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        s.to_string()
    } else {
        let mut out: String = s.chars().take(max_chars).collect();
        out.push_str("\n…");
        out
    }
}

fn preview_of(s: &str) -> String {
    let trimmed = s.trim();
    let first_line = trimmed.lines().next().unwrap_or("").trim();
    let preview: String = first_line.chars().take(200).collect();
    if preview.is_empty() {
        "(empty)".into()
    } else {
        preview
    }
}
