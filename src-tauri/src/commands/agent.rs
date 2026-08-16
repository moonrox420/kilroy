//! Agent send-message loop.
//!
//! Behaviour by mode:
//!
//! * CodeAgent (SmartCoder) — default; Python Smart Coder supplies project-grounded
//!   analysis to the approval-gated Rust executor.
//! * Copilot — quick-reply Ollama stream only; no execution loop.
//! * Governance — analysis-only single-shot, low temperature, no edits.
//! * Autonomous — planner produces a task DAG, persists it `pending`, then returns immediately; the frontend reviews and calls `execute_plan` to run it.
//! * Multi-Agent — same as Autonomous; role-specific prompts per task.
//!
//! Every send/reply pair is persisted to `messages` and recorded in `activity`.

pub use super::agent_context::AgentContext;
use super::agent_context::{gather_agent_context, BuiltAgentContext, KilroyProjectContext};
use crate::commands::memory::{require_memory, require_session};
use crate::db::{activity, agent_runtime, messages, tasks};
use crate::generation::{ChatMessage as LlmMessage, ChatOptions};
use crate::runtime::agent::{self as rust_agent, AgentRequest, RuntimeMode};
use crate::runtime::events::{PlanReady, PlannedTask, RunStarted};
use crate::runtime::planner;
use crate::smartcoder_runner::{
    build_smartcoder_argv, run_smartcoder_ask_blocking, write_context_temp_file, StreamSink,
};
use crate::state::{AgentMode, AppState};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, State};

// ─── Wire types ─────────────────────────────────────────────────────────────

#[derive(Serialize)]
pub struct AgentMessage {
    pub id: String,
    pub role: String,
    pub content: String,
    pub context: AgentContext,
    pub run_id: Option<String>,
    pub tasks: Vec<TaskRow>,
    /// True when the message carries a pending plan awaiting user execute.
    pub plan_pending: bool,
}

#[derive(Serialize)]
pub struct TaskRow {
    pub id: i64,
    pub r#type: String,
    pub agent: String,
    pub title: String,
    pub input: String,
    pub status: String,
    pub output_preview: String,
}

#[derive(Serialize)]
pub struct AgentStatus {
    pub mode: AgentMode,
    pub active_agents: u32,
    pub queued_tasks: u32,
    pub model: String,
    pub ready: bool,
}

#[derive(Deserialize)]
pub struct SendMessagePayload {
    pub message: String,
    /// Optional raw-base64 images attached to this turn. Forwarded to
    /// Ollama on the final user `ChatMessage.images` array — vision
    /// models read them, text-only models ignore them. The frontend is
    /// responsible for resizing / re-encoding to keep prompts small;
    /// Ollama itself has no inline image size cap but the daemon will
    /// OOM on multi-megabyte payloads if the user pastes a 4K
    /// screenshot raw.
    #[serde(default)]
    pub images: Option<Vec<String>>,
}

#[tauri::command]
pub async fn agent_send_message(
    app: AppHandle,
    state: State<'_, AppState>,
    payload: SendMessagePayload,
) -> Result<AgentMessage, String> {
    let mode = *state.agent_mode.lock();
    let mut user_msg = payload.message.trim().to_string();
    if user_msg.is_empty() && payload.images.as_ref().is_none_or(Vec::is_empty) {
        return Err("empty message".into());
    }
    if user_msg.is_empty() {
        user_msg = "Analyze the attached image and respond to what it contains.".to_string();
    }

    let project_id_opt = *state.current_project_id.lock();
    let session_id_opt = *state.current_session_id.lock();

    // 1. Persist user message + log activity.
    if let (Some(_pid), Some(sid)) = (project_id_opt, session_id_opt) {
        if let Ok(memory_conn) = require_memory(&state) {
            let conn = memory_conn.lock();
            let _ = messages::insert(&conn, sid, "user", &user_msg, None, None);
            let _ = activity::log(
                &conn,
                Some(sid),
                "message_sent",
                &serde_json::json!({ "preview": short(&user_msg, 120) }).to_string(),
            );
        }
    }

    // 2. Retrieve context (shared with SmartCoder bridge and executor).
    let built = gather_agent_context(&app, &state, &user_msg).await;
    let ctx = built.ctx.clone();
    let recent_msgs = built.recent_msgs.clone();
    let overview_for_prompt = built.overview_for_prompt.clone();

    let reply_id = uuid::Uuid::new_v4().to_string();
    let run_id = format!("run_{}", &reply_id[..8]);

    // 3. Dispatch by mode.
    match mode {
        AgentMode::CodeAgent => {
            let smartcoder = run_smartcoder_analysis(&app, &state, &user_msg, &built).await;
            let content = match smartcoder {
                Ok(analysis)
                    if is_conversational_query(&user_msg)
                        && payload.images.as_ref().is_none_or(Vec::is_empty) =>
                {
                    analysis
                }
                Ok(analysis) => {
                    run_tool_agent(
                        &app,
                        &state,
                        &user_msg,
                        &built,
                        &run_id,
                        session_id_opt,
                        RuntimeMode::Code,
                        Some(analysis),
                        payload.images.clone(),
                    )
                    .await
                }
                Err(error) => format!("Smart Coder backend failed: {error}"),
            };
            persist_reply(&state, session_id_opt, &content, &ctx);
            Ok(AgentMessage {
                id: reply_id,
                role: "agent".into(),
                content,
                context: ctx,
                run_id: None,
                tasks: vec![],
                plan_pending: false,
            })
        }
        AgentMode::Copilot | AgentMode::Governance => {
            let content = run_single_shot(
                &app,
                &state,
                mode,
                &user_msg,
                &ctx,
                &recent_msgs,
                &overview_for_prompt,
                payload.images.clone(),
            )
            .await;
            persist_reply(&state, session_id_opt, &content, &ctx);
            Ok(AgentMessage {
                id: reply_id,
                role: "agent".into(),
                content,
                context: ctx,
                run_id: None,
                tasks: vec![],
                plan_pending: false,
            })
        }
        AgentMode::Council => {
            let content = run_council(
                &app,
                &state,
                &user_msg,
                &ctx,
                &recent_msgs,
                &overview_for_prompt,
                payload.images.clone(),
            )
            .await;
            persist_reply(&state, session_id_opt, &content, &ctx);
            Ok(AgentMessage {
                id: reply_id,
                role: "agent".into(),
                content,
                context: ctx,
                run_id: None,
                tasks: vec![],
                plan_pending: false,
            })
        }
        AgentMode::Debug => {
            let content = run_tool_agent(
                &app,
                &state,
                &user_msg,
                &built,
                &run_id,
                session_id_opt,
                RuntimeMode::ReviewDebug,
                None,
                payload.images.clone(),
            )
            .await;
            persist_reply(&state, session_id_opt, &content, &ctx);
            Ok(AgentMessage {
                id: reply_id,
                role: "agent".into(),
                content,
                context: ctx,
                run_id: None,
                tasks: vec![],
                plan_pending: false,
            })
        }
        AgentMode::Autonomous | AgentMode::MultiAgent | AgentMode::TestFirst => {
            match plan_only(
                &app,
                &state,
                mode,
                &user_msg,
                &ctx,
                &recent_msgs,
                &overview_for_prompt,
                &run_id,
            )
            .await
            {
                Ok((overview, tasks_for_reply)) => {
                    let content = compose_plan_summary(&overview, &tasks_for_reply);
                    // Persist the run linkage (run_id + the task ids it created)
                    // into the message metadata so the run's task-stream card
                    // can be rehydrated after an app restart — without it, the
                    // live run state is lost on reload and history shows only
                    // the plain summary text.
                    let task_ids: Vec<i64> = tasks_for_reply.iter().map(|t| t.id).collect();
                    persist_reply_with_run(
                        &state,
                        session_id_opt,
                        &content,
                        &ctx,
                        &run_id,
                        &task_ids,
                    );
                    Ok(AgentMessage {
                        id: reply_id,
                        role: "agent".into(),
                        content,
                        context: ctx,
                        run_id: Some(run_id),
                        tasks: tasks_for_reply,
                        plan_pending: true,
                    })
                }
                Err(e) => {
                    let content = format!(
                        "Planner failed: {}\n\nFalling back to single-shot reply:\n\n{}",
                        e,
                        run_single_shot(
                            &app,
                            &state,
                            mode,
                            &user_msg,
                            &ctx,
                            &recent_msgs,
                            &overview_for_prompt,
                            payload.images.clone(),
                        )
                        .await,
                    );
                    persist_reply(&state, session_id_opt, &content, &ctx);
                    Ok(AgentMessage {
                        id: reply_id,
                        role: "agent".into(),
                        content,
                        context: ctx,
                        run_id: None,
                        tasks: vec![],
                        plan_pending: false,
                    })
                }
            }
        }
    }
}

fn persist_reply(
    state: &State<'_, AppState>,
    session_id: Option<i64>,
    content: &str,
    ctx: &AgentContext,
) {
    let Some(sid) = session_id else { return };
    let Ok(memory_conn) = require_memory(state) else {
        return;
    };
    let meta = serde_json::to_string(ctx).ok();
    let conn = memory_conn.lock();
    let _ = messages::insert(&conn, sid, "agent", content, meta.as_deref(), None);
    let _ = activity::log(
        &conn,
        Some(sid),
        "message_received",
        &serde_json::json!({ "preview": short(content, 120) }).to_string(),
    );
}

/// Like `persist_reply`, but stamps the autonomous-run linkage into the
/// message metadata. The metadata stays a strict SUPERSET of `AgentContext`
/// (the frontend still parses it as that for the context popover) with two
/// extra keys: `run_id` and `run_task_ids`. On reload, `loadHistory` reads
/// those to re-attach the run's task-stream card to this message.
fn persist_reply_with_run(
    state: &State<'_, AppState>,
    session_id: Option<i64>,
    content: &str,
    ctx: &AgentContext,
    run_id: &str,
    task_ids: &[i64],
) {
    let Some(sid) = session_id else { return };
    let Ok(memory_conn) = require_memory(state) else {
        return;
    };

    // Serialize the context to a JSON object, then inject the run linkage.
    // Falls back to the plain context string if anything about the value
    // shape is unexpected, so a serialization quirk can never drop the reply.
    let meta = match serde_json::to_value(ctx) {
        Ok(mut value) => {
            if let Some(obj) = value.as_object_mut() {
                obj.insert(
                    "run_id".to_string(),
                    serde_json::Value::String(run_id.to_string()),
                );
                obj.insert("run_task_ids".to_string(), serde_json::json!(task_ids));
            }
            serde_json::to_string(&value).ok()
        }
        Err(_) => serde_json::to_string(ctx).ok(),
    };

    let conn = memory_conn.lock();
    let _ = messages::insert(&conn, sid, "agent", content, meta.as_deref(), None);
    let _ = activity::log(
        &conn,
        Some(sid),
        "message_received",
        &serde_json::json!({ "preview": short(content, 120) }).to_string(),
    );
}

// ─── Code Agent (SmartCoder subprocess, project-grounded) ─────────────────────

async fn run_tool_agent(
    app: &AppHandle,
    state: &State<'_, AppState>,
    user_msg: &str,
    built: &BuiltAgentContext,
    run_id: &str,
    session_id: Option<i64>,
    mode: RuntimeMode,
    smartcoder_context: Option<String>,
    images: Option<Vec<String>>,
) -> String {
    let request = AgentRequest {
        run_id: run_id.to_string(),
        session_id,
        mode,
        message: user_msg.to_string(),
        smartcoder_context,
        images,
    };
    match rust_agent::run_code(app, state.inner(), request, built).await {
        Ok(run) => run.summary,
        Err(error) => format!("Code runtime failed: {error:#}"),
    }
}

async fn run_smartcoder_analysis(
    app: &AppHandle,
    state: &State<'_, AppState>,
    user_msg: &str,
    built: &BuiltAgentContext,
) -> Result<String, String> {
    let project_root = built.project_root.clone();
    let context: KilroyProjectContext = built.into();
    let context_path = write_context_temp_file(&context)?;
    let (ollama_host, chat_model) = {
        let settings = state.settings.read();
        (settings.ollama_url.clone(), settings.chat_model.clone())
    };
    let launch_result = build_smartcoder_argv(
        "ask",
        &[user_msg.to_string()],
        project_root.as_deref(),
        &ollama_host,
        &chat_model,
        Some(&context_path),
        project_root.as_deref(),
        true,
        Some("architect"),
        Some("analysis"),
    );
    let launch = match launch_result {
        Ok(launch) => launch,
        Err(error) => {
            let _ = std::fs::remove_file(context_path);
            return Err(error);
        }
    };
    let sink = StreamSink::SmartCoderPanel { app: app.clone() };
    let argv = launch.argv;
    let workdir = launch.workdir;
    let result = tauri::async_runtime::spawn_blocking(move || {
        let result = run_smartcoder_ask_blocking(
            &argv,
            workdir.as_deref(),
            &sink,
            Some(300),
        );
        let _ = std::fs::remove_file(context_path);
        result
    })
    .await
    .map_err(|error| format!("Smart Coder worker join failed: {error}"))??;

    if result.code.unwrap_or(1) != 0 {
        let stderr = result.stderr.trim();
        let stdout = result.stdout.trim();
        let detail = match (stderr.is_empty(), stdout.is_empty()) {
            (false, false) => format!(
                "Smart Coder stderr:\n{stderr}\n\nSmart Coder stdout:\n{stdout}"
            ),
            (false, true) => format!("Smart Coder stderr:\n{stderr}"),
            (true, false) => format!("Smart Coder stdout:\n{stdout}"),
            (true, true) => format!("process exited with code {:?}", result.code),
        };
        tracing::error!(error = %detail, "Smart Coder backend process failed");
        return Err(detail);
    }
    let output = result.stdout.trim();
    if output.is_empty() {
        return Err("process completed without a response".to_string());
    }
    Ok(output.to_string())
}

fn is_simple_greeting(message: &str) -> bool {
    let normalized = message
        .trim()
        .trim_matches(|character: char| character.is_ascii_punctuation())
        .to_ascii_lowercase();
    matches!(
        normalized.as_str(),
        "hi" | "hello" | "hey" | "howdy" | "good morning" | "good afternoon" | "good evening"
    )
}

fn is_conversational_query(message: &str) -> bool {
    if is_simple_greeting(message) || is_explain_query(message) {
        return true;
    }

    let normalized = message.trim().to_ascii_lowercase();
    const PLANNING: &[&str] = &[
        "help me plan",
        "i want to plan",
        "let's plan",
        "lets plan",
        "plan a ",
        "plan an ",
        "plan the ",
        "plan out",
        "brainstorm",
        "talk through",
        "discuss",
        "thinking about",
        "how should we",
        "how would we",
        "what would be",
    ];
    if !PLANNING.iter().any(|phrase| normalized.contains(phrase)) {
        return false;
    }

    const EXECUTE_NOW: &[&str] = &[
        "implement it",
        "build it",
        "make the changes",
        "apply the changes",
        "go ahead and implement",
        "execute the plan",
        "start coding",
    ];
    !EXECUTE_NOW.iter().any(|phrase| normalized.contains(phrase))
}

// ─── Single-shot streamed reply ─────────────────────────────────────────────

#[allow(clippy::too_many_arguments)]
async fn run_single_shot(
    app: &AppHandle,
    state: &State<'_, AppState>,
    mode: AgentMode,
    user_msg: &str,
    ctx: &AgentContext,
    recent_msgs: &[messages::Message],
    project_overview: &str,
    user_images: Option<Vec<String>>,
) -> String {
    let durable_run_id = format!("run_{}", uuid::Uuid::new_v4().simple());
    let durable = state
        .memory
        .lock()
        .as_ref()
        .map(|memory| memory.conn.clone());
    let durable_step_id = durable.as_ref().and_then(|memory| {
        let conn = memory.lock();
        let session_id = *state.current_session_id.lock();
        if let Err(error) = agent_runtime::create_run(
            &conn,
            &durable_run_id,
            session_id,
            RuntimeMode::Chat.as_str(),
            user_msg,
        ) {
            tracing::warn!("persist chat run: {error:#}");
            return None;
        }
        match agent_runtime::create_step(&conn, &durable_run_id, 1, "model_stream", None) {
            Ok(step_id) => Some(step_id),
            Err(error) => {
                tracing::warn!("persist chat step: {error:#}");
                None
            }
        }
    });
    let chat = state.chat.clone();
    let msgs = build_chat_messages(
        mode,
        user_msg,
        ctx,
        recent_msgs,
        project_overview,
        user_images,
    );
    let app_for_chunks = app.clone();
    let mut accumulated = String::new();
    let res = chat
        .chat_stream(
            &msgs,
            Some(ChatOptions {
                temperature: Some(match mode {
                    AgentMode::Governance => 0.1,
                    _ => 0.4,
                }),
                num_predict: Some(2048),
                top_p: None,
                num_ctx: Some(8192),
            }),
            |delta| {
                accumulated.push_str(delta);
                let _ = app_for_chunks.emit(
                    "agent://stream",
                    StreamChunk {
                        delta: delta.to_string(),
                    },
                );
            },
        )
        .await;
    match res {
        Ok(result) => {
            if let (Some(memory), Some(step_id)) = (&durable, durable_step_id) {
                let conn = memory.lock();
                let _ = agent_runtime::finish_step(
                    &conn,
                    step_id,
                    "completed",
                    Some(&accumulated),
                    None,
                    1,
                    0,
                );
                let _ = agent_runtime::finish_run(
                    &conn,
                    &durable_run_id,
                    "completed",
                    "unverified",
                    Some(&accumulated),
                    None,
                    1,
                    0,
                    result.tokens as i64,
                );
            }
            accumulated
        }
        Err(e) => {
            let failure = format!(
                "(model unavailable — {})\n\nUser message: {}\n\nRetrieved {} chunks / {} decisions.",
                e,
                user_msg,
                ctx.chunks.len(),
                ctx.decisions.len()
            );
            if let (Some(memory), Some(step_id)) = (&durable, durable_step_id) {
                let conn = memory.lock();
                let _ = agent_runtime::finish_step(
                    &conn,
                    step_id,
                    "failed",
                    None,
                    Some(&failure),
                    1,
                    0,
                );
                let _ = agent_runtime::finish_run(
                    &conn,
                    &durable_run_id,
                    "failed",
                    "blocked",
                    None,
                    Some(&failure),
                    1,
                    0,
                    0,
                );
            }
            failure
        }
    }
}

#[derive(Serialize, Clone)]
pub struct StreamChunk {
    pub delta: String,
}

// ─── Council mode — adversarial debate ──────────────────────────────────────
//
// Council mode fans a single user message out to FOUR parallel agents
// with deliberately opposing priorities, then runs a fifth synthesiser
// pass that surfaces tradeoffs and picks a recommended path.
//
// Why this exists: single-model replies average across viewpoints
// silently — the user sees one answer and doesn't know which concerns
// were dropped on the floor. Council renders the disagreement
// explicitly, which is exactly what you want for stakes-shaped
// questions ("should I migrate to X?", "is this design good?", "where
// will this break?"). The four voices are picked to cover the
// dimensions a senior engineer would naturally weigh.

#[derive(Serialize, Clone)]
struct CouncilVoiceChunk {
    voice: &'static str,
    delta: String,
}

#[derive(Serialize, Clone)]
struct CouncilVoiceDone {
    voice: &'static str,
    content: String,
}

#[derive(Serialize, Clone)]
struct CouncilSynthesisChunk {
    delta: String,
}

#[derive(Serialize, Clone)]
struct CouncilDone {
    synthesis: String,
}

/// The four standing Council voices. Order is deliberate: we render
/// them in this sequence in the final Markdown, and the UI columns
/// mirror the same order. Each prompt is ~80 words — enough to set
/// the voice's priorities and prohibitions without bloating the
/// system prompt.
const COUNCIL_VOICES: &[(&str, &str, &str)] = &[
    (
        "velocity",
        "⚡ Velocity",
        "You are the VELOCITY voice on Kilroy's agent council. Your job is to \
         optimise for shipping fast and accepting cheap tech debt. Identify \
         the minimum-viable path. Call out where the user is overthinking. \
         Recommend pragmatic shortcuts that are explicitly time-bound (e.g. \
         'hardcode for v1, generalise in v2'). Stay under 250 words. PROHIBITIONS: \
         do not advocate refactors, abstractions, or speculative work; that's \
         another voice's job. Be direct, terse, no hedging.",
    ),
    (
        "maintainability",
        "🔧 Maintainability",
        "You are the MAINTAINABILITY voice. Optimise for code that survives a \
         year of edits by other people. Flag abstractions worth keeping, \
         naming worth fixing, magic worth eliminating, coupling worth \
         decoupling. Stay under 250 words. PROHIBITIONS: do not optimise for \
         speed of shipping (velocity's job) or for adversarial threats \
         (security's job). Focus on the year-from-now reader of the code.",
    ),
    (
        "security",
        "🛡️ Security / Paranoia",
        "You are the SECURITY / PARANOIA voice. Think threat models — input \
         validation, secrets handling, auth boundaries, supply-chain, \
         injection vectors, principle of least privilege. If the question \
         seems benign, find the angle where it isn't (parsed user input? \
         outbound traffic? credential lifetime?). Stay under 250 words. \
         PROHIBITIONS: do not lecture on hypotheticals unrelated to the \
         actual question. Anchor every concern to a concrete attack path.",
    ),
    (
        "correctness",
        "🎯 Correctness",
        "You are the CORRECTNESS voice. Edge cases, invariants, race \
         conditions, off-by-ones, error paths, boundary values, null/empty/\
         unicode handling. For each concern, describe the test that catches \
         the bug. Stay under 250 words. PROHIBITIONS: do not opine on style, \
         performance, or architecture — only on whether the thing computes \
         the right answer for every input it'll see.",
    ),
];

const COUNCIL_SYNTHESIZER: &str =
    "You are the council synthesizer. You've just received four perspectives \
     on the same question — Velocity, Maintainability, Security, Correctness. \
     Output, in this exact structure:\n\n\
     ## Where the voices agree\n\
     Bullet list. The shared ground is what the user can act on with the \
     least uncertainty.\n\n\
     ## Where they disagree (and what's at stake)\n\
     For each material disagreement, one line: which voices, the disagreement \
     in one sentence, the cost of getting it wrong in one sentence.\n\n\
     ## Recommended path\n\
     Pick ONE path. Be opinionated. Explicitly say which concerns from the \
     disagreement section you're accepting and which you're deferring. End \
     with a one-sentence test for whether the recommended path actually \
     worked. Stay under 350 words total.";

// ─── Debug mode — failure-analysis swarm ────────────────────────────────────
//
// Debug mode is Council's twin: four parallel agents, then a synthesizer.
// The voices are tuned for diagnosing a stack trace / failing test /
// unexpected behaviour. Splitting the work across four specialised
// perspectives works WAY better than asking a single agent to "debug
// this" because the reasoning paths conflict: reading the stack trace
// pulls toward proximate causes, while reviewing recent diffs pulls
// toward root causes. Letting both run in parallel and then
// synthesising surfaces the actual bug far more reliably.

const DEBUG_VOICES: &[(&str, &str, &str)] = &[
    (
        "error_reader",
        "🔍 Error reader",
        "You are the ERROR READER. You receive a stack trace, panic, \
         test failure, or unexpected output as the user's message. Your \
         job: identify exactly what failed, where in the call stack the \
         failure originates, and which line of user code is implicated. \
         Quote the most informative frames verbatim. Stay under 250 \
         words. PROHIBITIONS: do not propose fixes (that's the fix \
         author's job). Do not speculate beyond what the trace shows. \
         If the trace is too truncated to be useful, say so explicitly \
         and name the one missing piece you'd need.",
    ),
    (
        "recent_changes",
        "📜 Recent changes",
        "You are the RECENT CHANGES reviewer. Look at the retrieved \
         code and (if visible) recent task outputs in the project \
         context. Identify which file(s) and code paths were modified \
         most recently that COULD plausibly cause the failure under \
         investigation. Cite specific file:line ranges. Stay under 250 \
         words. PROHIBITIONS: do not analyse the error message itself \
         (that's the error reader's job). Focus on the diff-shaped \
         question: 'what changed lately that touches this?'.",
    ),
    (
        "hypothesis",
        "💭 Hypotheses",
        "You are the HYPOTHESIS generator. Propose 2–4 candidate root \
         causes for the failure, ordered by likelihood. For each, \
         explain the mechanism in one sentence ('A causes B because C') \
         and name a one-line check the user could run RIGHT NOW to \
         confirm or refute it (a console.log, a debugger breakpoint, \
         a unit test, a grep). Stay under 250 words. PROHIBITIONS: do \
         not propose the fix yet — your job is to enumerate the \
         possibility space, not collapse it.",
    ),
    (
        "fix_author",
        "🛠️ Fix author",
        "You are the FIX AUTHOR. Assume the most-likely hypothesis from \
         the user's framing is correct. Produce: (1) a minimal \
         reproduction case in code if the bug is reproducible from the \
         context, OR (2) the smallest possible fix in unified-diff form \
         that addresses the proximate cause. Show the file:line. Stay \
         under 250 words. PROHIBITIONS: do not produce a sweeping \
         refactor; the fix's job is to make the immediate failure go \
         away, not to redesign the surrounding code.",
    ),
];

const DEBUG_SYNTHESIZER: &str = "You are the debug-swarm synthesizer. Four voices have analysed a \
     failure: an error reader, a recent-changes reviewer, a hypothesis \
     generator, and a fix author. Output, in this exact structure:\n\n\
     ## Most-likely root cause\n\
     One paragraph: pick ONE hypothesis as most-likely. Cite the \
     evidence from the error trace AND the recent-changes review that \
     points at it. If you're under 60% confident, say so — \"low \
     confidence\" is a useful signal.\n\n\
     ## Confirming check\n\
     The single command, breakpoint, or test the user should run RIGHT \
     NOW to confirm the root cause. Be specific (paste the exact code / \
     command). The check should take <30 seconds.\n\n\
     ## Proposed fix\n\
     A unified diff or minimal code snippet that addresses the cause. \
     File:line specified. If the fix author's proposal looks wrong \
     given what the other voices found, write your own.\n\n\
     ## If the fix is wrong\n\
     One sentence: which of the OTHER hypotheses would you try next, \
     and why. This gives the user a fallback if the first attempt \
     misses. Stay under 400 words total.";

async fn run_council(
    app: &AppHandle,
    state: &State<'_, AppState>,
    user_msg: &str,
    ctx: &AgentContext,
    recent_msgs: &[messages::Message],
    project_overview: &str,
    user_images: Option<Vec<String>>,
) -> String {
    run_swarm(
        app,
        state,
        COUNCIL_VOICES,
        COUNCIL_SYNTHESIZER,
        "🧭 Synthesis",
        user_msg,
        ctx,
        recent_msgs,
        project_overview,
        user_images,
    )
    .await
}

async fn run_debug(
    app: &AppHandle,
    state: &State<'_, AppState>,
    user_msg: &str,
    ctx: &AgentContext,
    recent_msgs: &[messages::Message],
    project_overview: &str,
    user_images: Option<Vec<String>>,
) -> String {
    run_swarm(
        app,
        state,
        DEBUG_VOICES,
        DEBUG_SYNTHESIZER,
        "🧭 Diagnosis & fix",
        user_msg,
        ctx,
        recent_msgs,
        project_overview,
        user_images,
    )
    .await
}

/// Public-to-the-crate swarm driver. Other modules (commands::refactor)
/// can build their own swarms by passing their voices, synthesizer, and
/// event prefix. The `event_prefix` discriminates which frontend surface
/// the live progress events go to — Council/Debug both share
/// `"agent://council"` (chat panel CouncilLive view), Refactor uses
/// `"agent://refactor"` (Refactor panel view), so background scans
/// don't visually contaminate active chat turns.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn swarm_with_context(
    app: &AppHandle,
    chat: &crate::generation::ChatClient,
    voices: &'static [(&'static str, &'static str, &'static str)],
    synthesizer_system: &'static str,
    synthesis_label: &'static str,
    event_prefix: &'static str,
    shared_context: &str,
    user_input: &str,
    user_images: Option<Vec<String>>,
) -> String {
    // Channel names derived from the prefix. Building once and cloning
    // would also work but a String concat per call is negligible.
    let voice_ch = format!("{}/voice", event_prefix);
    let voice_done_ch = format!("{}/voice_done", event_prefix);
    let synthesis_ch = format!("{}/synthesis", event_prefix);
    let done_ch = format!("{}/done", event_prefix);

    let mut handles = Vec::new();
    for (voice_id, _label, voice_prompt) in voices {
        let chat = chat.clone();
        let app = app.clone();
        let msgs = build_council_voice_messages(
            voice_prompt,
            shared_context,
            user_input,
            user_images.clone(),
        );
        let voice_id = *voice_id;
        let voice_ch = voice_ch.clone();
        let voice_done_ch = voice_done_ch.clone();
        let handle = tauri::async_runtime::spawn(async move {
            let mut buf = String::new();
            let res = chat
                .chat_stream(
                    &msgs,
                    Some(ChatOptions {
                        temperature: Some(0.6),
                        num_predict: Some(600),
                        top_p: None,
                        num_ctx: Some(8192),
                    }),
                    |delta| {
                        buf.push_str(delta);
                        let _ = app.emit(
                            &voice_ch,
                            CouncilVoiceChunk {
                                voice: voice_id,
                                delta: delta.to_string(),
                            },
                        );
                    },
                )
                .await;
            if let Err(e) = &res {
                tracing::warn!(voice = voice_id, "swarm voice failed: {:#}", e);
                buf.push_str(&format!("\n\n_({} voice failed: {})_", voice_id, e));
            }
            let _ = app.emit(
                &voice_done_ch,
                CouncilVoiceDone {
                    voice: voice_id,
                    content: buf.clone(),
                },
            );
            (voice_id, buf)
        });
        handles.push(handle);
    }

    let mut outputs: Vec<(&'static str, String)> = Vec::new();
    for h in handles {
        match h.await {
            Ok(pair) => outputs.push(pair),
            Err(e) => {
                tracing::warn!("swarm join error: {:#}", e);
            }
        }
    }

    let synth_user = format_synth_user_prompt(user_input, &outputs, voices);
    let synth_msgs = vec![
        LlmMessage::text("system", synthesizer_system),
        LlmMessage::text("user", synth_user),
    ];
    let app_for_synth = app.clone();
    let mut synthesis = String::new();
    let _ = chat
        .chat_stream(
            &synth_msgs,
            Some(ChatOptions {
                temperature: Some(0.3),
                num_predict: Some(1000),
                top_p: None,
                num_ctx: Some(8192),
            }),
            |delta| {
                synthesis.push_str(delta);
                let _ = app_for_synth.emit(
                    &synthesis_ch,
                    CouncilSynthesisChunk {
                        delta: delta.to_string(),
                    },
                );
            },
        )
        .await;
    let _ = app.emit(
        &done_ch,
        CouncilDone {
            synthesis: synthesis.clone(),
        },
    );

    let mut full = String::new();
    for (voice_id, content) in &outputs {
        let label = voices
            .iter()
            .find(|(id, _, _)| id == voice_id)
            .map(|(_, l, _)| *l)
            .unwrap_or(voice_id);
        full.push_str(&format!("## {}\n\n{}\n\n", label, content.trim()));
    }
    full.push_str(&format!("---\n\n## {}\n\n", synthesis_label));
    full.push_str(synthesis.trim());
    full
}

/// Chat-aware swarm driver. Builds shared_context from the chat session
/// (project, retrieved chunks, decisions, recent history) and forwards
/// to `swarm_with_context` on the Council event channels. Used by
/// Council mode and Debug mode in agent_send_message.
#[allow(clippy::too_many_arguments)]
async fn run_swarm(
    app: &AppHandle,
    state: &State<'_, AppState>,
    voices: &'static [(&'static str, &'static str, &'static str)],
    synthesizer_system: &'static str,
    synthesis_label: &'static str,
    user_msg: &str,
    ctx: &AgentContext,
    recent_msgs: &[messages::Message],
    project_overview: &str,
    user_images: Option<Vec<String>>,
) -> String {
    let chat = state.chat.clone();
    let shared_context = build_council_shared_context(user_msg, ctx, recent_msgs, project_overview);
    swarm_with_context(
        app,
        &chat,
        voices,
        synthesizer_system,
        synthesis_label,
        "agent://council",
        &shared_context,
        user_msg,
        user_images,
    )
    .await
}

fn build_council_shared_context(
    user_msg: &str,
    ctx: &AgentContext,
    recent_msgs: &[messages::Message],
    project_overview: &str,
) -> String {
    let mut s = String::new();
    if !project_overview.is_empty() {
        s.push_str("# Active project\n");
        s.push_str(project_overview);
        s.push_str("\n\n");
    }
    if !ctx.chunks.is_empty() {
        s.push_str("# Retrieved code\n");
        for c in &ctx.chunks {
            s.push_str(&format!(
                "\n## {}:{}-{}\n```\n{}\n```\n",
                c.file_path,
                c.start_line,
                c.end_line,
                truncate(&c.content, 800),
            ));
        }
        s.push('\n');
    }
    if !ctx.decisions.is_empty() {
        s.push_str("# Prior decisions\n");
        for d in &ctx.decisions {
            s.push_str(&format!("- {} — {}\n", d.title, d.summary));
        }
        s.push('\n');
    }
    if !recent_msgs.is_empty() {
        s.push_str("# Recent conversation\n");
        for m in recent_msgs.iter().rev().take(6).rev() {
            s.push_str(&format!("**{}:** {}\n", m.role, truncate(&m.content, 400)));
        }
        s.push('\n');
    }
    s.push_str("# Current user question\n");
    s.push_str(user_msg);
    s
}

fn build_council_voice_messages(
    voice_prompt: &str,
    shared_context: &str,
    user_msg: &str,
    user_images: Option<Vec<String>>,
) -> Vec<LlmMessage> {
    let mut final_user = LlmMessage::text("user", user_msg);
    if let Some(imgs) = user_images {
        if !imgs.is_empty() {
            final_user.images = Some(imgs);
        }
    }
    vec![
        LlmMessage::text(
            "system",
            format!(
                "{}\n\n# Project context\n\n{}",
                voice_prompt, shared_context
            ),
        ),
        final_user,
    ]
}

fn format_synth_user_prompt(
    user_msg: &str,
    outputs: &[(&'static str, String)],
    voice_registry: &'static [(&'static str, &'static str, &'static str)],
) -> String {
    let mut s = String::new();
    s.push_str("Original user input:\n\n");
    s.push_str(user_msg);
    s.push_str("\n\n# Voices' analyses\n\n");
    for (voice_id, content) in outputs {
        let label = voice_registry
            .iter()
            .find(|(id, _, _)| id == voice_id)
            .map(|(_, l, _)| *l)
            .unwrap_or(voice_id);
        s.push_str(&format!("## {}\n{}\n\n", label, content.trim()));
    }
    s.push_str("\nSynthesize per the structure in your system prompt. Be opinionated.");
    s
}

// ─── Planner only (no execute) ──────────────────────────────────────────────

#[allow(clippy::too_many_arguments)]
async fn plan_only(
    app: &AppHandle,
    state: &State<'_, AppState>,
    mode: AgentMode,
    user_msg: &str,
    ctx: &AgentContext,
    recent_msgs: &[messages::Message],
    project_overview: &str,
    run_id: &str,
) -> Result<(String, Vec<TaskRow>), String> {
    let session_id = require_session(state)?;
    let memory_conn = require_memory(state)?;
    let chat = state.chat.clone();

    let mode_str = match mode {
        AgentMode::TestFirst => "test_first",
        AgentMode::MultiAgent => "multi_agent",
        _ => "autonomous",
    };

    let _ = app.emit(
        "agent://run/started",
        RunStarted {
            run_id: run_id.to_string(),
            session_id,
            mode: mode_str.into(),
            user_message: user_msg.to_string(),
        },
    );

    let context_str = build_retrieval_context(ctx, recent_msgs, 8);
    let mut combined_overview = String::new();
    combined_overview.push_str("Project overview:\n");
    combined_overview.push_str(project_overview);
    combined_overview.push_str("\n\nRetrieval context:\n");
    combined_overview.push_str(&context_str);

    let raw_plan = planner::plan_with_mode(
        &chat,
        user_msg,
        &combined_overview,
        matches!(mode, AgentMode::TestFirst),
    )
    .await
    .map_err(|e| format!("{:#}", e))?;
    if raw_plan.tasks.is_empty() {
        return Err("planner returned an empty plan".into());
    }

    let mut planned: Vec<PlannedTask> = Vec::with_capacity(raw_plan.tasks.len());
    let mut tasks_for_reply: Vec<TaskRow> = Vec::with_capacity(raw_plan.tasks.len());
    {
        let conn = memory_conn.lock();
        for t in &raw_plan.tasks {
            let id = tasks::insert(
                &conn,
                tasks::NewTask {
                    session_id: Some(session_id),
                    parent_id: None,
                    r#type: &t.r#type,
                    agent: &t.agent,
                    input: &serde_json::json!({ "title": t.title, "input": t.input }).to_string(),
                },
            )
            .map_err(|e| format!("persist task: {:#}", e))?;
            planned.push(PlannedTask {
                task_id: id,
                r#type: t.r#type.clone(),
                agent: t.agent.clone(),
                title: t.title.clone(),
                input: t.input.clone(),
            });
            tasks_for_reply.push(TaskRow {
                id,
                r#type: t.r#type.clone(),
                agent: t.agent.clone(),
                title: t.title.clone(),
                input: t.input.clone(),
                status: "pending".into(),
                output_preview: String::new(),
            });
        }
        let _ = activity::log(
            &conn,
            Some(session_id),
            "plan_ready",
            &serde_json::json!({
                "run_id": run_id,
                "tasks": planned.iter().map(|p| &p.title).collect::<Vec<_>>(),
            })
            .to_string(),
        );
    }

    let _ = app.emit(
        "agent://run/plan_ready",
        PlanReady {
            run_id: run_id.to_string(),
            tasks: planned,
        },
    );

    let overview = raw_plan
        .overview
        .unwrap_or_else(|| "Plan ready for your review.".to_string());
    Ok((overview, tasks_for_reply))
}

fn compose_plan_summary(overview: &str, tasks: &[TaskRow]) -> String {
    let mut s = String::new();
    s.push_str(overview);
    s.push_str("\n\nReview and edit the plan, then hit Execute.\n");
    for (i, t) in tasks.iter().enumerate() {
        s.push_str(&format!(
            "{}. [{}/{}] {}\n",
            i + 1,
            t.r#type,
            t.agent,
            t.title
        ));
    }
    s
}

// ─── Prompt helpers ─────────────────────────────────────────────────────────

/// True for "what does X do?", "how does Y work?", etc. — answer from context, no code execution.
fn is_explain_query(msg: &str) -> bool {
    let t = msg.trim().to_lowercase();
    if t.is_empty() {
        return false;
    }
    const WORK: &[&str] = &[
        "fix",
        "implement",
        "add ",
        "create ",
        "build ",
        "write ",
        "refactor",
        "delete",
        "update ",
        "change ",
        "run ",
        "deploy",
        "make ",
        "convert ",
        "migrate",
        "patch ",
        "install ",
    ];
    if WORK.iter().any(|w| t.contains(w)) {
        return false;
    }
    const EXPLAIN: &[&str] = &[
        "what does",
        "what do ",
        "what is ",
        "what's ",
        "what are ",
        "how does",
        "how do ",
        "how is ",
        "explain ",
        "describe ",
        "tell me about",
        "tell me what",
        "purpose of",
        "who is ",
        "why does",
    ];
    EXPLAIN.iter().any(|p| t.contains(p)) || (t.ends_with('?') && t.len() < 200)
}

fn build_chat_messages(
    mode: AgentMode,
    user_msg: &str,
    ctx: &AgentContext,
    recent_msgs: &[messages::Message],
    project_overview: &str,
    user_images: Option<Vec<String>>,
) -> Vec<LlmMessage> {
    let mut system = String::new();
    match mode {
        AgentMode::CodeAgent => system.push_str(
            "You are Kilroy's conversational SmartCoder. Talk naturally with the user and use the \
             project overview and retrieved code when relevant. Help brainstorm and plan new projects, \
             features, or refactors; ask focused questions when requirements are unclear. You may propose \
             concrete architecture and implementation plans, but do not execute edits or commands until \
             the user explicitly asks you to implement. Never claim you lack file access.\n\n",
        ),
        AgentMode::Copilot => system.push_str(
            "You are Kilroy in Copilot mode. The user drives, you advise. \
             Be concise. Use code blocks for code. Reference filenames and line ranges \
             from the retrieved context when relevant.\n\n",
        ),
        AgentMode::Governance => system.push_str(
            "You are Kilroy in Governance mode. Audit and analyze the codebase or proposal under \
             review — do NOT propose edits, do NOT emit fenced `file_write` / `diff` blocks. \
             OUTPUT FORMAT (required, in this exact order):\n\n\
             ## Summary\n\
             One paragraph: what was reviewed, your overall assessment in one sentence.\n\n\
             ## Findings\n\
             A Markdown table with columns: `Area | Severity | Finding | Recommendation`. \
             `Severity` must be one of `critical`, `high`, `medium`, `low`. Each row's `Finding` \
             cites a specific file:line range from the retrieved code when applicable. Keep each \
             cell under ~120 chars; deeper context goes in the next section.\n\n\
             ## Risks\n\
             Bullet list of risks the findings expose if left unaddressed — security, correctness, \
             maintainability, performance. Each bullet ties back to a Finding by short title.\n\n\
             ## Recommendations\n\
             Numbered list, highest-impact first. Each item is one sentence with a clear action verb \
             (`Add`, `Remove`, `Refactor`, `Document`, `Test`). The user will turn the high-priority \
             items into tasks for a developer agent in a follow-up run.\n\n\
             If the retrieved context is insufficient to audit confidently, lead the response with a \
             `## Context gaps` section listing what you'd need (specific files, dependency versions, \
             test output) and STOP there — do not fabricate findings.\n\n",
        ),
        _ => {}
    }

    // CRITICAL: the agent's first response would otherwise say "I don't have
    // access to your file system" because LLMs default-refuse without
    // grounding. Tell it WHICH PROJECT is open up front, so even before any
    // semantic retrieval kicks in (e.g., user hasn't run Index Project yet)
    // it knows the folder exists and what to call it.
    if !project_overview.is_empty() {
        system.push_str("# Active project\n");
        system.push_str(
            "Kilroy is running locally on the user's machine and has live access to the open \
             project's filesystem. The user's currently-open folder is:\n",
        );
        system.push_str(project_overview);
        system.push_str(
            "\n\nDo NOT refuse on the grounds of \"I'm an AI without file access\" — Kilroy has \
             read/write access through its tools. If you need to see a specific file's contents and \
             it isn't in the retrieved code below, say so explicitly so the user can run \
             `Memory → Index Project` or open the file.\n\n",
        );
    }

    if !ctx.chunks.is_empty() {
        system.push_str("# Retrieved code\n");
        for c in &ctx.chunks {
            system.push_str(&format!(
                "\n## {}:{}-{}\n```\n{}\n```\n",
                c.file_path,
                c.start_line,
                c.end_line,
                truncate(&c.content, 1200)
            ));
        }
    }
    if !ctx.decisions.is_empty() {
        system.push_str("\n# Prior decisions\n");
        for d in &ctx.decisions {
            system.push_str(&format!("- {}: {}\n", d.title, d.summary));
        }
    }

    let mut out = vec![LlmMessage::text("system", system)];
    for m in recent_msgs {
        let role = match m.role.as_str() {
            "user" => "user",
            "agent" => "assistant",
            _ => continue,
        };
        out.push(LlmMessage::text(role, m.content.clone()));
    }
    // Final user turn — attach any images so vision-capable models
    // (LLaVA, bakllava, llava-phi3, llama3.2-vision, qwen2-vl, etc.)
    // can see them. Ollama accepts the field unconditionally; older /
    // non-vision daemons silently ignore it, so we don't need to
    // sniff the model first.
    let mut final_user = LlmMessage::text("user", user_msg);
    if let Some(imgs) = user_images {
        if !imgs.is_empty() {
            final_user.images = Some(imgs);
        }
    }
    out.push(final_user);
    out
}

fn build_retrieval_context(
    ctx: &AgentContext,
    recent_msgs: &[messages::Message],
    max_recent: usize,
) -> String {
    let mut s = String::new();
    if !ctx.chunks.is_empty() {
        s.push_str("Code matches:\n");
        for c in &ctx.chunks {
            let snippet = c.content.lines().next().unwrap_or("");
            s.push_str(&format!(
                "- {}:{}-{}  {}\n",
                c.file_path,
                c.start_line,
                c.end_line,
                snippet.chars().take(80).collect::<String>()
            ));
        }
    }
    if !ctx.decisions.is_empty() {
        s.push_str("Prior decisions:\n");
        for d in &ctx.decisions {
            s.push_str(&format!("- {}: {}\n", d.title, d.summary));
        }
    }
    if !recent_msgs.is_empty() {
        s.push_str("Recent conversation:\n");
        for m in recent_msgs.iter().take(max_recent) {
            let line = m.content.lines().next().unwrap_or("");
            s.push_str(&format!(
                "- {}: {}\n",
                m.role,
                line.chars().take(100).collect::<String>()
            ));
        }
    }
    if s.is_empty() {
        s.push_str("(empty)\n");
    }
    s
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

fn short(s: &str, max: usize) -> String {
    let trimmed = s.trim();
    if trimmed.chars().count() <= max {
        trimmed.to_string()
    } else {
        trimmed.chars().take(max).collect::<String>() + "…"
    }
}

// ─── Mode commands ──────────────────────────────────────────────────────────

#[tauri::command]
pub async fn agent_set_mode(state: State<'_, AppState>, mode: AgentMode) -> Result<(), String> {
    *state.agent_mode.lock() = mode;
    Ok(())
}

#[tauri::command]
pub async fn agent_get_status(state: State<'_, AppState>) -> Result<AgentStatus, String> {
    let mode = *state.agent_mode.lock();
    let ready = state.memory.lock().is_some();
    let model = state.chat.model().to_string();

    // Count pending tasks from the memory DB as a proxy for queued work.
    let queued_tasks: u32 = if let Ok(memory_conn) = super::memory::require_memory(&state) {
        let conn = memory_conn.lock();
        conn.query_row(
            "SELECT COUNT(*) FROM tasks WHERE status = 'pending'",
            [],
            |r| r.get::<_, u32>(0),
        )
        .unwrap_or(0)
    } else {
        0
    };

    // active_agents: 1 if the mode is one that runs agents (not copilot/governance).
    let active_agents: u32 = if matches!(
        mode,
        AgentMode::CodeAgent
            | AgentMode::Autonomous
            | AgentMode::MultiAgent
            | AgentMode::TestFirst
            | AgentMode::Council
            | AgentMode::Debug
    ) && ready
    {
        1
    } else {
        0
    };

    Ok(AgentStatus {
        mode,
        active_agents,
        queued_tasks,
        model,
        ready,
    })
}

#[cfg(test)]
mod tests {
    use super::{is_conversational_query, is_simple_greeting};

    #[test]
    fn greeting_fast_path_is_narrow_and_case_insensitive() {
        assert!(is_simple_greeting("hello"));
        assert!(is_simple_greeting("Hi!"));
        assert!(is_simple_greeting("  GOOD MORNING  "));
        assert!(!is_simple_greeting("hello, fix the failing test"));
        assert!(!is_simple_greeting("implement a greeting handler"));
    }

    #[test]
    fn planning_discussion_uses_conversational_path_until_execution_is_requested() {
        assert!(is_conversational_query("hello"));
        assert!(is_conversational_query("Help me plan a new Rust project"));
        assert!(is_conversational_query("Can you plan a refactor with me?"));
        assert!(is_conversational_query("Let's brainstorm a refactor"));
        assert!(!is_conversational_query("Implement the parser refactor"));
        assert!(!is_conversational_query(
            "We finished planning; go ahead and implement it"
        ));
    }
}
