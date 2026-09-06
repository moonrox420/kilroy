//! Activity feed command — list rows from the `activity` table.

use crate::commands::memory::require_memory;
use crate::db::activity;
use crate::state::AppState;
use serde::Serialize;
use tauri::State;

#[derive(Serialize)]
pub struct ActivityView {
    pub id: i64,
    pub session_id: Option<i64>,
    pub kind: String,
    pub payload: serde_json::Value,
    pub created_at: i64,
}

#[tauri::command]
pub async fn list_activity(
    state: State<'_, AppState>,
    session_only: Option<bool>,
    limit: Option<i64>,
) -> Result<Vec<ActivityView>, String> {
    let memory_conn = require_memory(&state)?;
    let sid = if session_only.unwrap_or(false) {
        *state.current_session_id.lock()
    } else {
        None
    };
    let conn = memory_conn.lock();
    let rows = activity::list(&conn, sid, limit.unwrap_or(200))
        .map_err(|e| format!("list_activity: {:#}", e))?;
    Ok(rows
        .into_iter()
        .map(|r| ActivityView {
            id: r.id,
            session_id: r.session_id,
            kind: r.kind,
            payload: serde_json::from_str(&r.payload).unwrap_or(serde_json::Value::Null),
            created_at: r.created_at,
        })
        .collect())
}
