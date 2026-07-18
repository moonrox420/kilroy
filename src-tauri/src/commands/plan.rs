//! Plan editor / executor commands.
//!
//! Once `agent_send_message` has returned a pending plan for an
//! Autonomous-mode message, the frontend can:
//!
//!   * `update_plan_task` — rename / re-input a single pending task.
//!   * `delete_plan_task` — drop one.
//!   * `insert_plan_task` — append a new one.
//!   * `cancel_plan`      — mark every pending task `cancelled`.
//!   * `execute_plan`     — kick off the executor in the background.
//!
//! Execution returns immediately; results stream via `agent://run/*`
//! events. Errors are emitted as `agent://run/completed` with success=false.

use crate::commands::memory::{require_memory, require_project, require_session};
use crate::db::{activity, agent_runtime, chunks, decisions, messages, projects, sessions, tasks};
use crate::runtime::events::{PlannedTask, RunCompleted};
use crate::runtime::executor::ExecCtx;
use crate::state::AppState;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, State};

#[derive(Deserialize)]
pub struct UpdateTaskPayload {
    pub task_id: i64,
    pub title: Option<String>,
    pub input: Option<String>,
}

#[tauri::command]
pub async fn update_plan_task(
    state: State<'_, AppState>,
    payload: UpdateTaskPayload,
) -> Result<(), String> {
    let memory_conn = require_memory(&state)?;

    // tasks.input is JSON ({ title, input }) — read, modify, write.
    // SCOPE the read-and-parse: acquire the lock, read the row, drop the
    // lock IMMEDIATELY. Then parse the JSON outside the lock. Then take
    // the lock again only to issue the UPDATE. Previously the lock was
    // held across the serde_json::from_str call which could starve other
    // memory-DB consumers if the JSON blob grew.
    let raw: String = {
        let conn = memory_conn.lock();
        conn.query_row(
            "SELECT input FROM tasks WHERE id = ?1 AND status = 'pending'",
            rusqlite::params![payload.task_id],
            |r| r.get::<_, String>(0),
        )
        .map_err(|e| format!("task {} not pending: {}", payload.task_id, e))?
    };
    let mut current: serde_json::Value =
        serde_json::from_str(&raw).map_err(|e| format!("parse input json: {}", e))?;

    if let Some(t) = payload.title {
        current["title"] = serde_json::Value::String(t);
    }
    if let Some(i) = payload.input {
        current["input"] = serde_json::Value::String(i);
    }

    let serialised = current.to_string();
    {
        let conn = memory_conn.lock();
        conn.execute(
            "UPDATE tasks SET input = ?2 WHERE id = ?1",
            rusqlite::params![payload.task_id, serialised],
        )
        .map_err(|e| format!("update task: {}", e))?;
    }
    Ok(())
}

#[tauri::command]
pub async fn delete_plan_task(state: State<'_, AppState>, task_id: i64) -> Result<(), String> {
    let memory_conn = require_memory(&state)?;
    let conn = memory_conn.lock();
    conn.execute(
        "DELETE FROM tasks WHERE id = ?1 AND status = 'pending'",
        rusqlite::params![task_id],
    )
    .map_err(|e| format!("delete task: {}", e))?;
    Ok(())
}

#[derive(Deserialize)]
pub struct InsertTaskPayload {
    pub r#type: String,
    pub agent: String,
    pub title: String,
    pub input: String,
}

#[tauri::command]
pub async fn insert_plan_task(
    state: State<'_, AppState>,
    payload: InsertTaskPayload,
) -> Result<i64, String> {
    let session_id = require_session(&state)?;
    let memory_conn = require_memory(&state)?;
    let conn = memory_conn.lock();
    let id = tasks::insert(
        &conn,
        tasks::NewTask {
            session_id: Some(session_id),
            parent_id: None,
            r#type: &payload.r#type,
            agent: &payload.agent,
            input: &serde_json::json!({ "title": payload.title, "input": payload.input })
                .to_string(),
        },
    )
    .map_err(|e| format!("insert task: {:#}", e))?;
    Ok(id)
}

#[tauri::command]
pub async fn cancel_plan(state: State<'_, AppState>, task_ids: Vec<i64>) -> Result<(), String> {
    let memory_conn = require_memory(&state)?;
    let conn = memory_conn.lock();
    for id in task_ids {
        let _ = conn.execute(
            "UPDATE tasks SET status = 'cancelled', completed_at = strftime('%s', 'now')
             WHERE id = ?1 AND status IN ('pending', 'running')",
            rusqlite::params![id],
        );
    }
    Ok(())
}

#[derive(Deserialize)]
pub struct ExecutePlanPayload {
    pub run_id: String,
    pub task_ids: Vec<i64>,
}

#[derive(Serialize)]
pub struct ExecuteAck {
    pub run_id: String,
    pub task_count: usize,
}

#[tauri::command]
pub async fn execute_plan(
    app: AppHandle,
    state: State<'_, AppState>,
    payload: ExecutePlanPayload,
) -> Result<ExecuteAck, String> {
    let project_id = require_project(&state)?;
    let session_id = require_session(&state)?;
    let memory_conn = require_memory(&state)?;
    let chat = state.chat.clone();

    let project_root = state
        .memory
        .lock()
        .as_ref()
        .map(|m| m.root.clone())
        .ok_or_else(|| "no memory open".to_string())?;

    // Guard: reject if any task is already completed/success/failed — prevents
    // re-running finished work.
    {
        let conn = memory_conn.lock();
        for id in &payload.task_ids {
            let status: String = conn
                .query_row(
                    "SELECT status FROM tasks WHERE id = ?1",
                    rusqlite::params![id],
                    |r| r.get(0),
                )
                .map_err(|e| format!("task {}: {}", id, e))?;
            if status == "success" || status == "failed" || status == "cancelled" {
                return Err(format!(
                    "task {} is already '{}' — cannot re-run completed tasks",
                    id, status
                ));
            }
        }
    }

    // Load tasks + project metadata snapshot up front so the spawned task
    // doesn't need to hold any `State` references (those have lifetimes).
    let plan: Vec<PlannedTask>;
    let project_overview;
    let recent_msgs: Vec<messages::Message>;
    let user_goal: String;
    {
        let conn = memory_conn.lock();
        let p = projects::fetch_by_path(&conn, &project_root.to_string_lossy())
            .map_err(|e| format!("fetch project: {:#}", e))?;
        project_overview = format!("name: {}\nroot: {}", p.name, p.root_path);

        plan = payload
            .task_ids
            .iter()
            .filter_map(|id| {
                conn.query_row(
                    "SELECT id, type, agent, input FROM tasks WHERE id = ?1",
                    rusqlite::params![id],
                    |r| {
                        let raw_input: String = r.get(3)?;
                        let v: serde_json::Value =
                            serde_json::from_str(&raw_input).unwrap_or(serde_json::Value::Null);
                        let title = v
                            .get("title")
                            .and_then(|x| x.as_str())
                            .unwrap_or("(untitled)")
                            .to_string();
                        let input = v
                            .get("input")
                            .and_then(|x| x.as_str())
                            .unwrap_or("")
                            .to_string();
                        Ok(PlannedTask {
                            task_id: r.get(0)?,
                            r#type: r.get(1)?,
                            agent: r.get(2)?,
                            title,
                            input,
                        })
                    },
                )
                .ok()
            })
            .collect();

        recent_msgs = messages::tail(&conn, session_id, 8).unwrap_or_default();
        // Pull the latest user message as the goal — the planner stamped it
        // into recent_msgs already.
        user_goal = recent_msgs
            .iter()
            .rev()
            .find(|m| m.role == "user")
            .map(|m| m.content.clone())
            .unwrap_or_default();

        // Activity log. Named distinctly so it doesn't shadow the
        // `payload: ExecutePlanPayload` function parameter (the shadow
        // was block-scoped and harmless, but a renamed local is one
        // less trap for a future edit that reaches for `payload` here).
        let activity_payload = serde_json::json!({
            "run_id": payload.run_id,
            "task_count": plan.len(),
        });
        let _ = activity::log(
            &conn,
            Some(session_id),
            "run_started",
            &activity_payload.to_string(),
        );
    }

    if plan.is_empty() {
        return Err("no pending tasks for that plan".into());
    }

    // Retrieve context fresh for the execute pass — same embedding the planner used.
    let embedder = state.embedder.clone();
    let chunks_hits;
    let decisions_hits;
    if !user_goal.is_empty() {
        let (k_chunks, k_decisions) = {
            let s = state.settings.read();
            (s.retrieval_chunks_k, s.retrieval_decisions_k)
        };
        match embedder.embed(&user_goal).await {
            Ok(qv) => {
                let conn = memory_conn.lock();
                chunks_hits = chunks::knn(&conn, project_id, &qv, k_chunks).unwrap_or_default();
                decisions_hits =
                    decisions::knn(&conn, project_id, &qv, k_decisions).unwrap_or_default();
            }
            Err(_) => {
                chunks_hits = vec![];
                decisions_hits = vec![];
            }
        }
    } else {
        chunks_hits = vec![];
        decisions_hits = vec![];
    }

    let run_id = payload.run_id.clone();
    let task_count = plan.len();
    let (default_sandbox, ollama_host, chat_model) = {
        let s = state.settings.read();
        (
            s.default_sandbox.clone(),
            s.ollama_url.clone(),
            s.chat_model.clone(),
        )
    };

    let indexed_chunk_count: i64 = {
        let conn = memory_conn.lock();
        conn.query_row(
            "SELECT COUNT(*) FROM chunks c
             JOIN files f ON f.id = c.file_id
             WHERE f.project_id = ?1",
            rusqlite::params![project_id],
            |r| r.get(0),
        )
        .unwrap_or(0)
    };

    {
        let conn = memory_conn.lock();
        agent_runtime::create_run(&conn, &run_id, Some(session_id), "plan_execute", &user_goal)
            .map_err(|error| format!("create durable plan run: {error:#}"))?;
    }

    // Spawn the actual execution so the IPC call can return immediately.
    let app_handle = app.clone();
    let memory_for_spawn = memory_conn.clone();
    tauri::async_runtime::spawn(async move {
        let ctx = ExecCtx {
            app: &app_handle,
            client: &chat,
            run_id: &run_id,
            memory: memory_for_spawn.clone(),
            session_id,
            project_root,
            project_overview: &project_overview,
            user_goal: &user_goal,
            chunks: &chunks_hits,
            decisions: &decisions_hits,
            recent_msgs: &recent_msgs,
            indexed_chunk_count,
            ollama_host: &ollama_host,
            chat_model: &chat_model,
            default_sandbox,
        };
        let result = crate::runtime::executor::execute(&ctx, &plan).await;
        let _ = app_handle.emit(
            "agent://run/completed",
            RunCompleted {
                run_id: run_id.clone(),
                success: !result.any_failed,
                summary: format!(
                    "{} task{} {}.",
                    plan.len(),
                    if plan.len() == 1 { "" } else { "s" },
                    if result.any_failed {
                        "with failures"
                    } else {
                        "completed"
                    }
                ),
            },
        );
        {
            let conn = memory_for_spawn.lock();
            let pending_approvals: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM agent_tool_calls
                     WHERE run_id = ?1 AND status = 'pending_approval'",
                    rusqlite::params![run_id],
                    |row| row.get(0),
                )
                .unwrap_or(0);
            let (status, verification) = if result.any_failed {
                ("failed", "blocked")
            } else if pending_approvals > 0 {
                ("awaiting_approval", "unverified")
            } else {
                ("completed", "unverified")
            };
            let summary = format!(
                "{} task{} processed; {} approval{} pending.",
                plan.len(),
                if plan.len() == 1 { "" } else { "s" },
                pending_approvals,
                if pending_approvals == 1 { "" } else { "s" },
            );
            let _ = agent_runtime::finish_run(
                &conn,
                &run_id,
                status,
                verification,
                Some(&summary),
                if result.any_failed {
                    Some("one or more plan tasks failed")
                } else {
                    None
                },
                plan.len() as i64,
                0,
                0,
            );
            let activity_payload = serde_json::json!({
                "run_id": run_id,
                "success": !result.any_failed,
                "task_count": plan.len(),
            });
            let _ = activity::log(
                &conn,
                Some(session_id),
                "run_completed",
                &activity_payload.to_string(),
            );
        }
    });

    // Mark stale session timestamp so list_sessions reflects the activity.
    // Char-safe short id: skip the "run_" prefix, take 8 chars. run_id is
    // ASCII today so byte-slicing was safe, but chars() keeps it correct
    // if the id format ever gains non-ASCII.
    let short_run: String = payload.run_id.chars().skip(4).take(8).collect();
    let _ = sessions::set_title(
        &memory_conn.lock(),
        session_id,
        &format!("Run {}", short_run),
    );

    Ok(ExecuteAck {
        run_id: payload.run_id,
        task_count,
    })
}
