//! Pending actuator actions.
//!
//! Every code block or shell command the agent emits in a task becomes
//! a row here with status='pending'. The user reviews and either
//! accepts (→ 'accepted' → executed → 'applied'/'failed') or rejects
//! (→ 'rejected'). Nothing touches disk without a row transition.

use anyhow::Result;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

use super::projects::unix_now;

#[derive(Serialize, Deserialize, Clone)]
pub struct Action {
    pub id: i64,
    pub session_id: Option<i64>,
    pub task_id: Option<i64>,
    pub kind: String,
    pub target: Option<String>,
    pub payload: String,
    pub diff: Option<String>,
    pub status: String,
    pub error: Option<String>,
    pub created_at: i64,
    pub resolved_at: Option<i64>,
}

pub struct NewAction<'a> {
    pub session_id: Option<i64>,
    pub task_id: Option<i64>,
    pub kind: &'a str,
    pub target: Option<&'a str>,
    pub payload: &'a str,
    pub diff: Option<&'a str>,
}

pub fn insert(conn: &Connection, a: NewAction) -> Result<i64> {
    let now = unix_now();
    conn.execute(
        "INSERT INTO actions (session_id, task_id, kind, target, payload, diff, status, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'pending', ?7)",
        params![
            a.session_id,
            a.task_id,
            a.kind,
            a.target,
            a.payload,
            a.diff,
            now
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn fetch(conn: &Connection, id: i64) -> Result<Action> {
    let row = conn.query_row(
        "SELECT id, session_id, task_id, kind, target, payload, diff, status, error, created_at, resolved_at
         FROM actions WHERE id = ?1",
        params![id],
        map_row,
    )?;
    Ok(row)
}

pub fn list_for_session(conn: &Connection, session_id: i64, limit: i64) -> Result<Vec<Action>> {
    let mut stmt = conn.prepare(
        "SELECT id, session_id, task_id, kind, target, payload, diff, status, error, created_at, resolved_at
         FROM actions WHERE session_id = ?1
         ORDER BY created_at DESC LIMIT ?2",
    )?;
    let rows = stmt
        .query_map(params![session_id, limit], map_row)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

pub fn pending_for_task(conn: &Connection, task_id: i64) -> Result<Vec<Action>> {
    let mut stmt = conn.prepare(
        "SELECT id, session_id, task_id, kind, target, payload, diff, status, error, created_at, resolved_at
         FROM actions WHERE task_id = ?1 AND status = 'pending'
         ORDER BY created_at ASC",
    )?;
    let rows = stmt
        .query_map(params![task_id], map_row)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

pub fn set_status(conn: &Connection, id: i64, status: &str, error: Option<&str>) -> Result<()> {
    conn.execute(
        "UPDATE actions
         SET status = ?2, error = ?3, resolved_at = strftime('%s', 'now')
         WHERE id = ?1",
        params![id, status, error],
    )?;
    Ok(())
}

fn map_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<Action> {
    Ok(Action {
        id: r.get(0)?,
        session_id: r.get(1)?,
        task_id: r.get(2)?,
        kind: r.get(3)?,
        target: r.get(4)?,
        payload: r.get(5)?,
        diff: r.get(6)?,
        status: r.get(7)?,
        error: r.get(8)?,
        created_at: r.get(9)?,
        resolved_at: r.get(10)?,
    })
}
