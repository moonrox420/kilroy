//! Pending action commands — list / accept / reject.

use crate::actuator::{self, ActionPayload};
use crate::commands::memory::{require_memory, require_session};
use crate::db::{actions as db_actions, activity, agent_runtime};
use crate::state::AppState;
use rusqlite::OptionalExtension;
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
    pub follow_up_action_ids: Vec<i64>,
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
    let _project_access = state.project_access.read().await;
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

    {
        let conn = memory_conn.lock();
        if !db_actions::claim_pending(&conn, action_id)
            .map_err(|e| format!("claim action: {:#}", e))?
        {
            return Err(format!("action {action_id} is no longer pending"));
        }
    }

    let mut error_msg: Option<String> = None;
    let mut audit_elapsed_ms: Option<u64> = None;
    let mut audit_out_hash: Option<String> = None;
    let mut audit_output: Option<String> = None;
    let mut follow_up_action_ids = Vec::new();

    let new_status = match &parsed {
        ActionPayload::FileChange {
            path,
            before_sha256,
            content,
        } => {
            match actuator::staging::apply_change(
                &project_root,
                path,
                before_sha256.as_deref(),
                content.as_deref(),
            ) {
                Ok(()) => "applied",
                Err(error) => {
                    error_msg = Some(format!("{error:#}"));
                    "failed"
                }
            }
        }
        ActionPayload::FileWrite { path, content, .. } => {
            let result = actuator::validate_file_write_snapshot(
                &project_root,
                path,
                content,
                action.diff.as_deref(),
            )
            .and_then(|()| actuator::apply_file_write(&project_root, path, content));
            match result {
                Ok(_) => "applied",
                Err(e) => {
                    error_msg = Some(format!("{:#}", e));
                    "failed"
                }
            }
        }
        ActionPayload::FilePatch { path, unified_diff } => {
            let diff_to_apply = payload.override_diff.as_deref().unwrap_or(unified_diff);
            match actuator::apply_file_patch(&project_root, path, diff_to_apply) {
                Ok(_) => "applied",
                Err(e) => {
                    error_msg = Some(format!("{:#}", e));
                    "failed"
                }
            }
        }
        ActionPayload::Shell { command, sandbox } => {
            if actuator::shell_targets_protected(command) {
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
                let command_root = project_root.clone();
                let command_for_run = command.clone();
                let sandbox_for_run = sandbox.clone();
                let execution = tauri::async_runtime::spawn_blocking(move || {
                    actuator::sandbox::run(&command_for_run, &command_root, &sandbox_for_run, opts)
                })
                .await
                .map_err(|error| anyhow::anyhow!("command worker failed: {error}"))
                .and_then(|result| result);
                match execution {
                    Ok(out) => {
                        let combined = format!("{}\n{}", out.stdout, out.stderr);
                        audit_out_hash = Some(output_hash(&combined));
                        audit_output = Some(combined.chars().take(64_000).collect());
                        audit_elapsed_ms = Some(t0.elapsed().as_millis() as u64);

                        if out.exit_code == Some(0) {
                            let saved =
                                persist_staged_changes(&memory_conn.lock(), &action, &out.changes);
                            match saved {
                                Ok(ids) => {
                                    follow_up_action_ids = ids;
                                    "applied"
                                }
                                Err(error) => {
                                    error_msg = Some(format!("command finished but staging approvals could not be saved; the project was not changed: {error:#}"));
                                    "failed"
                                }
                            }
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
            "output": audit_output,
            "follow_up_action_ids": follow_up_action_ids,
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
        follow_up_action_ids: follow_up_action_ids.clone(),
    };

    for id in follow_up_action_ids {
        let _ = app.emit(
            "actuator://action_proposed",
            serde_json::json!({
                "run_id": "", "task_id": action.task_id.unwrap_or(0), "action_id": id,
                "kind": "file_change", "target": null, "has_diff": true,
            }),
        );
    }

    let _ = app.emit("actuator://action_resolved", resolved.clone());

    // Post-apply compiler / linter gate for file edits
    if new_status == "applied"
        && matches!(
            parsed,
            ActionPayload::FilePatch { .. } | ActionPayload::FileWrite { .. }
        )
    {
        let app_handle = app.clone();
        let root_for_check = project_root.clone();
        let target_file_opt = action.target.clone();
        let memory_for_repair = memory_conn.clone();
        let chat_client = state.chat.clone();
        let session_for_repair = session_id_opt;

        tauri::async_runtime::spawn(async move {
            match crate::actuator::compiler::run_compiler_check(&root_for_check) {
                Ok(Some(check_result)) => {
                    let _ = app_handle.emit("agent://compiler/check", check_result.clone());
                    if !check_result.success {
                        tracing::warn!(
                            "Post-apply compiler check failed: {}",
                            check_result.command
                        );
                        if let Some(target_file) = target_file_opt {
                            let error_output =
                                format!("{}\n{}", check_result.stdout, check_result.stderr);
                            if let Err(err) = run_auto_repair_pass(
                                &app_handle,
                                &chat_client,
                                &memory_for_repair,
                                &root_for_check,
                                session_for_repair,
                                &target_file,
                                &error_output,
                            )
                            .await
                            {
                                tracing::error!("Auto-repair pass failed: {err:#}");
                            }
                        }
                    } else {
                        tracing::info!(
                            "Post-apply compiler check passed: {}",
                            check_result.command
                        );
                    }
                }
                Ok(None) => {}
                Err(err) => {
                    tracing::warn!("Compiler check could not run: {err:#}");
                }
            }
        });
    }

    Ok(resolved)
}

#[tauri::command]
pub async fn reject_action(
    app: AppHandle,
    state: State<'_, AppState>,
    action_id: i64,
) -> Result<ActionResolved, String> {
    let _project_access = state.project_access.read().await;
    let session_id_opt = *state.current_session_id.lock();
    let memory_conn = require_memory(&state)?;

    {
        let conn = memory_conn.lock();
        if !db_actions::reject_pending(&conn, action_id)
            .map_err(|e| format!("reject action: {:#}", e))?
        {
            return Err(format!("action {action_id} is not pending"));
        }
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
        follow_up_action_ids: Vec::new(),
    };

    let _ = app.emit("actuator://action_resolved", resolved.clone());
    Ok(resolved)
}

fn persist_staged_changes(
    conn: &rusqlite::Connection,
    parent: &db_actions::Action,
    changes: &[actuator::staging::StagedChange],
) -> anyhow::Result<Vec<i64>> {
    let transaction = conn.unchecked_transaction()?;
    let linkage: Option<(String, i64)> = transaction.query_row(
        "SELECT run_id, step_id FROM agent_tool_calls WHERE action_id = ?1 AND step_id IS NOT NULL LIMIT 1",
        [parent.id], |row| Ok((row.get(0)?, row.get(1)?)),
    ).optional()?;
    let mut ids = Vec::with_capacity(changes.len());
    for change in changes {
        let payload = serde_json::to_string(&change.payload)?;
        let id = db_actions::insert(
            &transaction,
            db_actions::NewAction {
                session_id: parent.session_id,
                task_id: parent.task_id,
                kind: "file_change",
                target: Some(&change.path),
                payload: &payload,
                diff: change.diff.as_deref(),
            },
        )?;
        if let Some((run_id, step_id)) = &linkage {
            agent_runtime::record_pending_approval(
                &transaction,
                run_id,
                *step_id,
                id,
                "file_change",
                &payload,
            )?;
        }
        ids.push(id);
    }
    transaction.commit()?;
    Ok(ids)
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
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

#[tauri::command]
pub async fn agent_run_compiler_check(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<Option<crate::actuator::compiler::CompilerCheckResult>, String> {
    let root = state
        .memory
        .lock()
        .as_ref()
        .map(|m| m.root.clone())
        .ok_or_else(|| "no project open".to_string())?;

    let result = crate::actuator::compiler::run_compiler_check(&root)
        .map_err(|e| format!("compiler check error: {e:#}"))?;

    if let Some(ref res) = result {
        let _ = app.emit("agent://compiler/check", res.clone());
    }
    Ok(result)
}

async fn run_auto_repair_pass(
    app: &AppHandle,
    chat: &crate::generation::ChatClient,
    memory: &std::sync::Arc<parking_lot::Mutex<rusqlite::Connection>>,
    root: &std::path::Path,
    session_id: Option<i64>,
    target_file: &str,
    error_output: &str,
) -> anyhow::Result<i64> {
    let abs_path = crate::actuator::resolve_safe(root, target_file)
        .map_err(|e| anyhow::anyhow!("resolve path: {e:#}"))?;
    let current_content =
        std::fs::read_to_string(&abs_path).map_err(|e| anyhow::anyhow!("read file: {e:#}"))?;

    let (lang_name, lang_tag) = crate::runtime::coder::infer_language(target_file);

    let system_prompt = format!(
        r#"You are an automated {lang_upper} Syntax & Compiler Repair Agent.
Your single objective is to fix the exact compilation, linting, or syntax error while preserving 100% of the surrounding operational logic.

===============================================================================
SEARCH / REPLACE BLOCK SPECIFICATION
===============================================================================
```{lang_lower} path={target_file}
<<<<<<< SEARCH
[exact lines from the file to locate]
=======
[replacement lines to insert]
>>>>>>> REPLACE
```

CRITICAL RULES:
1. DO NOT REMOVE FUNCTIONALITY: Do not resolve errors by deleting method logic, commenting out code, or stripping handlers.
2. DO NOT INSERT PLACEHOLDERS: Output complete code with zero '// TODO' or '...'.
3. FIX EXACT ERRORS ONLY: Resolve the exact compiler error specified in the trace."#,
        lang_upper = lang_name.to_uppercase(),
        lang_lower = lang_tag,
        target_file = target_file,
    );

    let user_prompt = format!(
        r#"COMPILER ERROR TRACE:
{error_output}

FAULTY SOURCE CODE:
```{lang_tag}
{current_content}
```

Provide focused SEARCH/REPLACE blocks fixing the compiler error."#
    );

    let repair_reply = chat
        .generate_text_with_images(
            &system_prompt,
            &user_prompt,
            None,
            Some(crate::generation::ChatOptions {
                temperature: Some(0.1),
                num_predict: Some(4096),
                top_p: None,
                num_ctx: Some(16_384),
            }),
        )
        .await
        .map_err(|e| anyhow::anyhow!("chat generation: {e:#}"))?;

    let blocks = crate::actuator::search_replace::extract_search_replace_blocks(&repair_reply);
    if blocks.is_empty() {
        return Err(anyhow::anyhow!("no repair blocks generated"));
    }

    let patch_res = crate::actuator::search_replace::apply_search_replace(
        target_file,
        &current_content,
        &blocks,
    )
    .map_err(|e| anyhow::anyhow!("apply repair: {e:#}"))?;

    let payload = ActionPayload::FilePatch {
        path: target_file.to_string(),
        unified_diff: patch_res.unified_diff.clone(),
    };
    let payload_json =
        serde_json::to_string(&payload).map_err(|e| anyhow::anyhow!("json encode: {e:#}"))?;

    let action_id = {
        let conn = memory.lock();
        crate::db::actions::insert(
            &conn,
            crate::db::actions::NewAction {
                session_id,
                task_id: None,
                kind: "file_patch",
                target: Some(target_file),
                payload: &payload_json,
                diff: Some(&patch_res.unified_diff),
            },
        )
        .map_err(|e| anyhow::anyhow!("insert action: {e:#}"))?
    };

    let _ = app.emit(
        "actuator://action_proposed",
        serde_json::json!({
            "run_id": "",
            "task_id": 0,
            "action_id": action_id,
            "kind": "file_patch",
            "target": target_file,
            "has_diff": true,
        }),
    );

    let _ = app.emit(
        "agent://editor/preview",
        serde_json::json!({
            "run_id": "",
            "action_id": action_id,
            "path": target_file,
            "contents": patch_res.patched_content,
            "diff": patch_res.unified_diff,
        }),
    );

    Ok(action_id)
}
