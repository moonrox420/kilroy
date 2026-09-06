//! Task graph audit log.
//!
//! Every agent action — planning, code edit, test run, review — is a
//! task. Tasks form a DAG via `parent_id`. This module is the
//! persistence layer; the live runtime + scheduler is a separate concern
//! that will land in the next pass.

use anyhow::Result;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

use super::projects::unix_now;

#[derive(Serialize, Deserialize, Clone)]
pub struct Task {
    pub id: i64,
    pub session_id: Option<i64>,
    pub parent_id: Option<i64>,
    pub r#type: String,
    pub agent: String,
    pub status: String,
    pub input: String,
    pub output: Option<String>,
    pub retry_count: i64,
    pub started_at: Option<i64>,
    pub completed_at: Option<i64>,
    pub created_at: i64,
}

#[derive(Deserialize)]
pub struct NewTask<'a> {
    pub session_id: Option<i64>,
    pub parent_id: Option<i64>,
    pub r#type: &'a str,
    pub agent: &'a str,
    pub input: &'a str,
}

pub fn insert(conn: &Connection, t: NewTask) -> Result<i64> {
    let now = unix_now();
    conn.execute(
        "INSERT INTO tasks (session_id, parent_id, type, agent, status, input, retry_count, created_at)
         VALUES (?1, ?2, ?3, ?4, 'pending', ?5, 0, ?6)",
        params![t.session_id, t.parent_id, t.r#type, t.agent, t.input, now],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn mark_running(conn: &Connection, id: i64) -> Result<()> {
    conn.execute(
        "UPDATE tasks SET status = 'running', started_at = strftime('%s', 'now') WHERE id = ?1",
        params![id],
    )?;
    Ok(())
}

pub fn mark_success(conn: &Connection, id: i64, output: &str) -> Result<()> {
    conn.execute(
        "UPDATE tasks
         SET status = 'success', output = ?2, completed_at = strftime('%s', 'now')
         WHERE id = ?1",
        params![id, output],
    )?;
    Ok(())
}

pub fn mark_failed(conn: &Connection, id: i64, error: &str) -> Result<()> {
    conn.execute(
        "UPDATE tasks
         SET status = 'failed', output = ?2, completed_at = strftime('%s', 'now'),
             retry_count = retry_count + 1
         WHERE id = ?1",
        params![id, error],
    )?;
    Ok(())
}

pub fn list_for_session(conn: &Connection, session_id: i64, limit: i64) -> Result<Vec<Task>> {
    let mut stmt = conn.prepare(
        "SELECT id, session_id, parent_id, type, agent, status, input, output,
                retry_count, started_at, completed_at, created_at
         FROM tasks WHERE session_id = ?1
         ORDER BY created_at DESC LIMIT ?2",
    )?;
    let rows = stmt
        .query_map(params![session_id, limit], |r| {
            Ok(Task {
                id: r.get(0)?,
                session_id: r.get(1)?,
                parent_id: r.get(2)?,
                r#type: r.get(3)?,
                agent: r.get(4)?,
                status: r.get(5)?,
                input: r.get(6)?,
                output: r.get(7)?,
                retry_count: r.get(8)?,
                started_at: r.get(9)?,
                completed_at: r.get(10)?,
                created_at: r.get(11)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}
