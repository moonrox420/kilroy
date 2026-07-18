//! Durable, approval-gated Rust agent runtime.
//!
//! The model may inspect project files through typed read-only tools. Any
//! mutation is persisted as a pending actuator action and is never executed
//! here. This keeps model claims separate from observable evidence.

use crate::actuator::{self, ActionPayload, SandboxKind};
use crate::commands::agent_context::BuiltAgentContext;
use crate::db::{actions, agent_runtime};
use crate::generation::ChatOptions;
use crate::runtime::tools::{self, ToolCall, ToolResult};
use crate::state::AppState;
use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use smartcoder_core::ArchitectureGuardian;
use std::path::Path;
use std::time::Instant;
use tauri::{AppHandle, Emitter};

const MAX_AGENT_STEPS: usize = 6;
const MAX_TOOLS_PER_STEP: usize = 4;
const MAX_EVIDENCE_CHARS: usize = 24_000;

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeMode {
    Chat,
    Code,
    PlanExecute,
    ReviewDebug,
}

impl RuntimeMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Chat => "chat",
            Self::Code => "code",
            Self::PlanExecute => "plan_execute",
            Self::ReviewDebug => "review_debug",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VerificationStatus {
    Verified,
    PartiallyVerified,
    Unverified,
    Blocked,
}

impl VerificationStatus {
    fn as_str(self) -> &'static str {
        match self {
            Self::Verified => "verified",
            Self::PartiallyVerified => "partially_verified",
            Self::Unverified => "unverified",
            Self::Blocked => "blocked",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentRequest {
    pub run_id: String,
    pub session_id: Option<i64>,
    pub mode: RuntimeMode,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentRun {
    pub run_id: String,
    pub status: String,
    pub verification_status: VerificationStatus,
    pub summary: String,
    pub pending_action_ids: Vec<i64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AgentEvent {
    pub run_id: String,
    pub step: usize,
    pub kind: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ApprovalRequest {
    pub run_id: String,
    pub action_id: i64,
    pub kind: String,
    pub target: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
struct ActionProposedEvent {
    run_id: String,
    task_id: i64,
    action_id: i64,
    kind: String,
    target: Option<String>,
    has_diff: bool,
}

#[derive(Debug, Clone, Serialize)]
struct EditorOpenEvent {
    run_id: String,
    path: String,
    line: Option<usize>,
    reason: String,
}

#[derive(Debug, Clone, Serialize)]
struct EditorPreviewEvent {
    run_id: String,
    action_id: i64,
    path: String,
    contents: String,
    diff: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "decision", rename_all = "snake_case")]
enum AgentDecision {
    Inspect {
        rationale: String,
        tools: Vec<ToolCall>,
    },
    Propose {
        summary: String,
        actions: Vec<ActionPayload>,
    },
    Finish {
        summary: String,
        verification_status: VerificationStatus,
    },
    Blocked {
        reason: String,
    },
}

pub async fn run_code(
    app: &AppHandle,
    state: &AppState,
    request: AgentRequest,
    context: &BuiltAgentContext,
) -> Result<AgentRun> {
    let root = context
        .project_root
        .as_deref()
        .ok_or_else(|| anyhow!("open a project before running Code mode"))?;
    let memory = state
        .memory
        .lock()
        .as_ref()
        .map(|memory| memory.conn.clone())
        .ok_or_else(|| anyhow!("project memory is not initialized"))?;

    {
        let conn = memory.lock();
        agent_runtime::create_run(
            &conn,
            &request.run_id,
            request.session_id,
            request.mode.as_str(),
            &request.message,
        )?;
    }

    emit_progress(
        app,
        &request.run_id,
        0,
        "started",
        "Investigating the project",
    );
    let mut evidence = initial_context(context);
    let mut pending_action_ids = Vec::new();
    let mut model_calls = 0_i64;

    for ordinal in 1..=MAX_AGENT_STEPS {
        let step_started = Instant::now();
        let step_id = {
            let conn = memory.lock();
            agent_runtime::create_step(
                &conn,
                &request.run_id,
                ordinal as i64,
                "model_decision",
                Some(&bounded(&evidence, MAX_EVIDENCE_CHARS)),
            )?
        };
        emit_progress(
            app,
            &request.run_id,
            ordinal,
            "thinking",
            "Evaluating evidence",
        );

        let decision = state
            .chat
            .generate_json::<AgentDecision>(
                system_prompt(),
                &decision_prompt(&request.message, &evidence, ordinal),
                Some(ChatOptions {
                    temperature: Some(0.1),
                    num_predict: Some(3072),
                    top_p: None,
                    num_ctx: Some(16_384),
                }),
            )
            .await;
        model_calls += 1;

        let decision = match decision {
            Ok(decision) => decision,
            Err(error) => {
                let message = format!("structured model decision failed: {error:#}");
                let conn = memory.lock();
                agent_runtime::finish_step(
                    &conn,
                    step_id,
                    "failed",
                    None,
                    Some(&message),
                    1,
                    step_started.elapsed().as_millis() as i64,
                )?;
                agent_runtime::finish_run(
                    &conn,
                    &request.run_id,
                    "failed",
                    "blocked",
                    None,
                    Some(&message),
                    model_calls,
                    0,
                    0,
                )?;
                return Err(anyhow!(message));
            }
        };

        match decision {
            AgentDecision::Inspect {
                rationale,
                tools: calls,
            } => {
                if calls.is_empty() {
                    return finish_blocked(
                        &memory,
                        &request.run_id,
                        step_id,
                        model_calls,
                        step_started,
                        "the model requested inspection without any tools",
                    );
                }
                evidence.push_str(&format!("\n\nMODEL RATIONALE: {rationale}"));
                for call in calls.into_iter().take(MAX_TOOLS_PER_STEP) {
                    let result = execute_tool(app, root, &request.run_id, step_id, &memory, &call)?;
                    evidence.push_str("\n\n");
                    evidence.push_str(&tool_evidence(&call, &result));
                }
                let conn = memory.lock();
                agent_runtime::finish_step(
                    &conn,
                    step_id,
                    "completed",
                    Some(&rationale),
                    None,
                    1,
                    step_started.elapsed().as_millis() as i64,
                )?;
            }
            AgentDecision::Propose {
                summary,
                actions: proposed,
            } => {
                if proposed.is_empty() {
                    return finish_blocked(
                        &memory,
                        &request.run_id,
                        step_id,
                        model_calls,
                        step_started,
                        "the model proposed no concrete actions",
                    );
                }
                for action in proposed {
                    let id = persist_proposal(app, root, &memory, &request, step_id, action)?;
                    pending_action_ids.push(id);
                }
                let conn = memory.lock();
                agent_runtime::finish_step(
                    &conn,
                    step_id,
                    "completed",
                    Some(&summary),
                    None,
                    1,
                    step_started.elapsed().as_millis() as i64,
                )?;
                agent_runtime::finish_run(
                    &conn,
                    &request.run_id,
                    "awaiting_approval",
                    "unverified",
                    Some(&summary),
                    None,
                    model_calls,
                    0,
                    0,
                )?;
                emit_progress(app, &request.run_id, ordinal, "awaiting_approval", &summary);
                return Ok(AgentRun {
                    run_id: request.run_id,
                    status: "awaiting_approval".into(),
                    verification_status: VerificationStatus::Unverified,
                    summary,
                    pending_action_ids,
                });
            }
            AgentDecision::Finish {
                summary,
                verification_status,
            } => {
                // Read/search evidence supports investigation, not successful
                // compilation or tests. Never elevate it to verified.
                let status = match verification_status {
                    VerificationStatus::Verified => VerificationStatus::PartiallyVerified,
                    other => other,
                };
                let conn = memory.lock();
                agent_runtime::finish_step(
                    &conn,
                    step_id,
                    "completed",
                    Some(&summary),
                    None,
                    1,
                    step_started.elapsed().as_millis() as i64,
                )?;
                agent_runtime::finish_run(
                    &conn,
                    &request.run_id,
                    "completed",
                    status.as_str(),
                    Some(&summary),
                    None,
                    model_calls,
                    0,
                    0,
                )?;
                return Ok(AgentRun {
                    run_id: request.run_id,
                    status: "completed".into(),
                    verification_status: status,
                    summary,
                    pending_action_ids,
                });
            }
            AgentDecision::Blocked { reason } => {
                return finish_blocked(
                    &memory,
                    &request.run_id,
                    step_id,
                    model_calls,
                    step_started,
                    &reason,
                );
            }
        }
    }

    let reason = format!("agent reached the bounded limit of {MAX_AGENT_STEPS} steps");
    let conn = memory.lock();
    agent_runtime::finish_run(
        &conn,
        &request.run_id,
        "blocked",
        "blocked",
        Some(&reason),
        Some(&reason),
        model_calls,
        0,
        0,
    )?;
    Ok(AgentRun {
        run_id: request.run_id,
        status: "blocked".into(),
        verification_status: VerificationStatus::Blocked,
        summary: reason,
        pending_action_ids,
    })
}

fn execute_tool(
    app: &AppHandle,
    root: &Path,
    run_id: &str,
    step_id: i64,
    memory: &std::sync::Arc<parking_lot::Mutex<rusqlite::Connection>>,
    call: &ToolCall,
) -> Result<ToolResult> {
    let args = serde_json::to_string(call)?;
    let tool_id = {
        let conn = memory.lock();
        agent_runtime::start_tool_call(
            &conn,
            run_id,
            Some(step_id),
            call.name(),
            "read_only",
            &args,
        )?
    };
    emit_progress(app, run_id, step_id as usize, "tool", call.name());
    if let Some(path) = call.editor_path() {
        if let Ok(abs) = actuator::resolve_safe(root, path) {
            let line = match call {
                ToolCall::ReadFile { start_line, .. } => *start_line,
                _ => None,
            };
            let _ = app.emit(
                "agent://editor/open",
                EditorOpenEvent {
                    run_id: run_id.to_string(),
                    path: abs.to_string_lossy().to_string(),
                    line,
                    reason: "Agent is reading this file".into(),
                },
            );
        }
    }
    let result = tools::execute(root, call);
    let conn = memory.lock();
    agent_runtime::finish_tool_call(
        &conn,
        tool_id,
        if result.success {
            "completed"
        } else {
            "failed"
        },
        if result.success {
            Some(result.output.as_str())
        } else {
            None
        },
        result.error.as_deref(),
        result.duration_ms as i64,
    )?;
    agent_runtime::add_evidence(
        &conn,
        run_id,
        Some(step_id),
        call.name(),
        "project",
        if result.success {
            &result.output
        } else {
            result.error.as_deref().unwrap_or("tool failed")
        },
        result.success,
        Some(&args),
    )?;
    Ok(result)
}

fn persist_proposal(
    app: &AppHandle,
    root: &Path,
    memory: &std::sync::Arc<parking_lot::Mutex<rusqlite::Connection>>,
    request: &AgentRequest,
    step_id: i64,
    mut payload: ActionPayload,
) -> Result<i64> {
    if let ActionPayload::Shell { sandbox, .. } = &mut payload {
        *sandbox = SandboxKind::default();
    }
    let policy_input = match &payload {
        ActionPayload::FileWrite { content, .. } => content.as_str(),
        ActionPayload::FilePatch { unified_diff, .. } => unified_diff.as_str(),
        ActionPayload::Shell { command, .. } => command.as_str(),
    };
    let policy = ArchitectureGuardian::new().evaluate(policy_input, None);
    if !policy.passed {
        let reasons = policy
            .vetoes
            .iter()
            .map(|veto| veto.reason.as_str())
            .collect::<Vec<_>>()
            .join("; ");
        return Err(anyhow!("proposal rejected by native policy: {reasons}"));
    }
    let (kind, target, diff, preview) = match &payload {
        ActionPayload::FileWrite { path, content, .. } => (
            "file_write",
            Some(path.clone()),
            actuator::diff_against_disk(root, path, content),
            Some(content.clone()),
        ),
        ActionPayload::FilePatch { path, unified_diff } => (
            "file_patch",
            Some(path.clone()),
            Some(unified_diff.clone()),
            Some(
                actuator::preview_file_patch(root, path, unified_diff)
                    .with_context(|| format!("preview patch for {path}"))?,
            ),
        ),
        ActionPayload::Shell { command, .. } => (
            "shell",
            Some(command.chars().take(80).collect()),
            None,
            None,
        ),
    };
    let payload_json = serde_json::to_string(&payload)?;
    let action_id = {
        let conn = memory.lock();
        let action_id = actions::insert(
            &conn,
            actions::NewAction {
                session_id: request.session_id,
                task_id: None,
                kind,
                target: target.as_deref(),
                payload: &payload_json,
                diff: diff.as_deref(),
            },
        )?;
        agent_runtime::record_pending_approval(
            &conn,
            &request.run_id,
            step_id,
            action_id,
            kind,
            &payload_json,
        )?;
        action_id
    };
    let _ = app.emit(
        "actuator://action_proposed",
        ActionProposedEvent {
            run_id: request.run_id.clone(),
            task_id: 0,
            action_id,
            kind: kind.into(),
            target: target.clone(),
            has_diff: diff.is_some(),
        },
    );
    let _ = app.emit(
        "agent://approval/requested",
        ApprovalRequest {
            run_id: request.run_id.clone(),
            action_id,
            kind: kind.into(),
            target: target.clone(),
        },
    );
    if let (Some(path), Some(contents)) = (target, preview) {
        let _ = app.emit(
            "agent://editor/preview",
            EditorPreviewEvent {
                run_id: request.run_id.clone(),
                action_id,
                path,
                contents,
                diff,
            },
        );
    }
    Ok(action_id)
}

fn finish_blocked(
    memory: &std::sync::Arc<parking_lot::Mutex<rusqlite::Connection>>,
    run_id: &str,
    step_id: i64,
    model_calls: i64,
    started: Instant,
    reason: &str,
) -> Result<AgentRun> {
    let conn = memory.lock();
    agent_runtime::finish_step(
        &conn,
        step_id,
        "blocked",
        Some(reason),
        Some(reason),
        1,
        started.elapsed().as_millis() as i64,
    )?;
    agent_runtime::finish_run(
        &conn,
        run_id,
        "blocked",
        "blocked",
        Some(reason),
        Some(reason),
        model_calls,
        0,
        0,
    )?;
    Ok(AgentRun {
        run_id: run_id.to_string(),
        status: "blocked".into(),
        verification_status: VerificationStatus::Blocked,
        summary: reason.to_string(),
        pending_action_ids: Vec::new(),
    })
}

fn emit_progress(app: &AppHandle, run_id: &str, step: usize, kind: &str, message: &str) {
    let _ = app.emit(
        "agent://runtime/event",
        AgentEvent {
            run_id: run_id.to_string(),
            step,
            kind: kind.to_string(),
            message: message.to_string(),
        },
    );
}

fn initial_context(context: &BuiltAgentContext) -> String {
    let mut output = String::from("PROJECT OVERVIEW\n");
    output.push_str(&context.overview_for_prompt);
    output.push_str("\n\nKNOWN PROJECT FILES\n");
    output.push_str(
        &context
            .project_files
            .iter()
            .take(250)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n"),
    );
    if !context.ctx.chunks.is_empty() {
        output.push_str("\n\nRETRIEVED CODE CONTEXT\n");
        for chunk in context.ctx.chunks.iter().take(8) {
            output.push_str(&format!(
                "\n{}\n",
                serde_json::to_string(chunk).unwrap_or_default()
            ));
        }
    }
    bounded(&output, MAX_EVIDENCE_CHARS)
}

fn decision_prompt(goal: &str, evidence: &str, step: usize) -> String {
    format!(
        "USER GOAL:\n{goal}\n\nSTEP: {step}/{MAX_AGENT_STEPS}\n\nEVIDENCE:\n{}\n\nReturn one JSON decision.",
        bounded(evidence, MAX_EVIDENCE_CHARS)
    )
}

fn system_prompt() -> &'static str {
    r#"You are Kilroy's Rust code runtime. Investigate before proposing changes.
Return strict JSON matching exactly one of these shapes:
{"decision":"inspect","rationale":"why","tools":[{"tool":"read_file","path":"relative/path","start_line":1,"end_line":200}]}
{"decision":"inspect","rationale":"why","tools":[{"tool":"search_files","query":"literal text","path":"optional/relative/dir"}]}
{"decision":"propose","summary":"what the approval-gated changes do","actions":[{"kind":"file_write","path":"relative/path","content":"complete contents","language":"rs"},{"kind":"file_patch","path":"relative/path","unified_diff":"complete unified diff"},{"kind":"shell","command":"command","sandbox":"windows_sandbox"}]}
{"decision":"finish","summary":"answer supported by evidence","verification_status":"partially_verified"}
{"decision":"blocked","reason":"specific missing information"}
Never claim a write, command, build, or test happened unless corresponding tool evidence says it did. Read/search tools are automatic. All file and shell actions require user approval. Prefer unified diffs for existing files. Produce complete, applicable content with no placeholders or TODOs."#
}

fn tool_evidence(call: &ToolCall, result: &ToolResult) -> String {
    format!(
        "TOOL {} {}\n{}",
        call.name(),
        if result.success {
            "SUCCEEDED"
        } else {
            "FAILED"
        },
        if result.success {
            &result.output
        } else {
            result.error.as_deref().unwrap_or("unknown failure")
        }
    )
}

fn bounded(value: &str, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        return value.to_string();
    }
    let tail: String = value
        .chars()
        .rev()
        .take(max_chars)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    format!("[older evidence truncated]\n{tail}")
}
