//! Pending action commands — list / accept / reject.

use crate::actuator::{self, ActionPayload};
use crate::commands::memory::{require_memory, require_session};
use crate::db::{actions as db_actions, activity, agent_runtime};
use crate::state::AppState;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::time::Instant;
use tauri::{AppHandle, Emitter, State};

#[derive(Serialize)]
pub struct ActionView {
    pub id: i64,
    pub session_id: Option<i64>,
    pub task_id: Option<i64>,
    pub kind: String,
    pub target: Option<String>,
    pub payload: serde_json::Value,
    pub diff: Option<String>,
    pub status: String,
    pub error: Option<String>,
    pub created_at: i64,
    pub resolved_at: Option<i64>,
}

#[tauri::command]
pub async fn list_actions(
    state: State<'_, AppState>,
    limit: Option<i64>,
) -> Result<Vec<ActionView>, String> {
    let session_id = require_session(&state)?;
    let memory_conn = require_memory(&state)?;
    let conn = memory_conn.lock();
    let rows = db_actions::list_for_session(&conn, session_id, limit.unwrap_or(100))
        .map_err(|e| format!("list_actions: {:#}", e))?;
    Ok(rows.into_iter().map(to_view).collect())
}

#[tauri::command]
pub async fn list_pending_actions_for_task(
    state: State<'_, AppState>,
    task_id: i64,
) -> Result<Vec<ActionView>, String> {
    let memory_conn = require_memory(&state)?;
    let conn = memory_conn.lock();
    let rows = db_actions::pending_for_task(&conn, task_id)
        .map_err(|e| format!("list_pending_actions: {:#}", e))?;
    Ok(rows.into_iter().map(to_view).collect())
}

#[derive(Serialize, Clone)]
pub struct ActionResolved {
    pub action_id: i64,
    pub status: String,
    pub error: Option<String>,
}

#[derive(Deserialize)]
pub struct AcceptPayload {
    pub action_id: i64,
    /// For `file_patch`: a unified diff containing only the hunks the user
    /// wants to apply. If `None`, the action's stored diff is applied verbatim.
    pub override_diff: Option<String>,
}

#[tauri::command]
pub async fn accept_action(
    app: AppHandle,
    state: State<'_, AppState>,
    payload: AcceptPayload,
) -> Result<ActionResolved, String> {
    let action_id = payload.action_id;
    let session_id_opt = *state.current_session_id.lock();
    let memory_conn = require_memory(&state)?;

    let (action, project_root) = {
        let conn = memory_conn.lock();
        let action = db_actions::fetch(&conn, action_id).map_err(|e| format!("fetch: {:#}", e))?;
        let root = state
            .memory
            .lock()
            .as_ref()
            .map(|m| m.root.clone())
            .ok_or_else(|| "no memory open".to_string())?;
        (action, root)
    };

    if action.status != "pending" {
        return Err(format!(
            "action {} is not pending ({})",
            action_id, action.status
        ));
    }

    let parsed: ActionPayload =
        serde_json::from_str(&action.payload).map_err(|e| format!("parse payload: {}", e))?;

    let mut error_msg: Option<String> = None;
    let mut audit_elapsed_ms: Option<u64> = None;
    let mut audit_out_hash: Option<String> = None;

    let new_status = match parsed {
        ActionPayload::FileWrite { path, content, .. } => {
            match actuator::apply_file_write(&project_root, &path, &content) {
                Ok(_) => "applied",
                Err(e) => {
                    error_msg = Some(format!("{:#}", e));
                    "failed"
                }
            }
        }
        ActionPayload::FilePatch { path, unified_diff } => {
            let diff_to_apply = payload.override_diff.as_deref().unwrap_or(&unified_diff);
            match actuator::apply_file_patch(&project_root, &path, diff_to_apply) {
                Ok(_) => "applied",
                Err(e) => {
                    error_msg = Some(format!("{:#}", e));
                    "failed"
                }
            }
        }
        ActionPayload::Shell { command, sandbox } => {
            if actuator::shell_targets_protected(&command) {
                error_msg = Some(
                    "Rejected: shell command targets the protected project virtualenv (.venv/venv)."
                        .to_string(),
                );
                "failed"
            } else {
                let t0 = Instant::now();
                let opts = actuator::sandbox::RunOpts {
                    timeout_secs: state.settings.read().sandbox_timeout_secs,
                };
                match actuator::sandbox::run(&command, &project_root, &sandbox, opts) {
                    Ok(out) => {
                        let combined = format!("{}\n{}", out.stdout, out.stderr);
                        audit_out_hash = Some(output_hash(&combined));
                        audit_elapsed_ms = Some(t0.elapsed().as_millis() as u64);

                        if out.exit_code.unwrap_or(0) == 0 {
                            "applied"
                        } else {
                            let mut msg = format!("exit {}", out.exit_code.unwrap_or(-1));
                            if !out.stderr.is_empty() {
                                msg.push('\n');
                                msg.push_str(out.stderr.trim());
                            }
                            if !out.stdout.is_empty() {
                                msg.push_str("\n---stdout---\n");
                                msg.push_str(out.stdout.trim());
                            }
                            error_msg = Some(msg);
                            "failed"
                        }
                    }
                    Err(e) => {
                        error_msg = Some(format!("{:#}", e));
                        "failed"
                    }
                }
            }
        }
    };

    {
        let conn = memory_conn.lock();
        db_actions::set_status(&conn, action_id, new_status, error_msg.as_deref())
            .map_err(|e| format!("set_status: {:#}", e))?;
        agent_runtime::resolve_approval(&conn, action_id, new_status, error_msg.as_deref())
            .map_err(|e| format!("resolve agent approval: {:#}", e))?;

        let mut activity_payload = serde_json::json!({
            "action_id": action_id,
            "kind": action.kind,
            "target": action.target,
            "status": new_status,
            "error": error_msg,
            "session_id": session_id_opt,
        });

        if let (Some(ms), Some(hash)) = (audit_elapsed_ms, audit_out_hash) {
            if let Some(obj) = activity_payload.as_object_mut() {
                obj.insert("elapsed_ms".into(), serde_json::json!(ms));
                obj.insert("out_sha256".into(), serde_json::json!(hash));
            }
        }

        activity::log(
            &conn,
            session_id_opt,
            if new_status == "applied" {
                "action_applied"
            } else {
                "action_failed"
            },
            &activity_payload.to_string(),
        )
        .map_err(|e| format!("activity_log: {:#}", e))?;
    }

    let resolved = ActionResolved {
        action_id,
        status: new_status.into(),
        error: error_msg,
    };

    let _ = app.emit("actuator://action_resolved", resolved.clone());
    Ok(resolved)
}

#[tauri::command]
pub async fn reject_action(
    app: AppHandle,
    state: State<'_, AppState>,
    action_id: i64,
) -> Result<ActionResolved, String> {
    let session_id_opt = *state.current_session_id.lock();
    let memory_conn = require_memory(&state)?;

    {
        let conn = memory_conn.lock();
        db_actions::set_status(&conn, action_id, "rejected", None)
            .map_err(|e| format!("set_status: {:#}", e))?;
        agent_runtime::resolve_approval(&conn, action_id, "rejected", None)
            .map_err(|e| format!("resolve agent approval: {:#}", e))?;

        let payload = serde_json::json!({ "action_id": action_id });
        activity::log(
            &conn,
            session_id_opt,
            "action_rejected",
            &payload.to_string(),
        )
        .map_err(|e| format!("activity_log: {:#}", e))?;
    }

    let resolved = ActionResolved {
        action_id,
        status: "rejected".into(),
        error: None,
    };

    let _ = app.emit("actuator://action_resolved", resolved.clone());
    Ok(resolved)
}

fn to_view(a: db_actions::Action) -> ActionView {
    let payload =
        serde_json::from_str::<serde_json::Value>(&a.payload).unwrap_or(serde_json::Value::Null);
    ActionView {
        id: a.id,
        session_id: a.session_id,
        task_id: a.task_id,
        kind: a.kind,
        target: a.target,
        payload,
        diff: a.diff,
        status: a.status,
        error: a.error,
        created_at: a.created_at,
        resolved_at: a.resolved_at,
    }
}

fn output_hash(s: &str) -> String {
    let mut h = Sha256::new();
    h.update(s.as_bytes());
    let digest = h.finalize();
    digest[..8].iter().map(|b| format!("{b:02x}")).collect()
}
