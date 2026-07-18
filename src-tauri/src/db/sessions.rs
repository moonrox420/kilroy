//! Session lifecycle.
//!
//! A session is one continuous conversation. We create one on folder open
//! (or resume the most recent one), close it when the user explicitly
//! starts a new session, and never delete (you want the audit trail).

use anyhow::Result;
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;

use super::projects::unix_now;

#[derive(Serialize, Clone)]
pub struct Session {
    pub id: i64,
    pub project_id: i64,
    pub title: Option<String>,
    pub agent_mode: String,
    pub started_at: i64,
    pub ended_at: Option<i64>,
}

pub fn create(conn: &Connection, project_id: i64, agent_mode: &str) -> Result<Session> {
    let now = unix_now();
    conn.execute(
        "INSERT INTO sessions (project_id, agent_mode, started_at)
         VALUES (?1, ?2, ?3)",
        params![project_id, agent_mode, now],
    )?;
    let id = conn.last_insert_rowid();
    Ok(Session {
        id,
        project_id,
        title: None,
        agent_mode: agent_mode.to_string(),
        started_at: now,
        ended_at: None,
    })
}

/// Find the most recent open session for the project, or create one.
pub fn current_or_create(conn: &Connection, project_id: i64, agent_mode: &str) -> Result<Session> {
    let existing: Option<Session> = conn
        .query_row(
            "SELECT id, project_id, title, agent_mode, started_at, ended_at
             FROM sessions
             WHERE project_id = ?1 AND ended_at IS NULL
             ORDER BY started_at DESC LIMIT 1",
            params![project_id],
            |r| {
                Ok(Session {
                    id: r.get(0)?,
                    project_id: r.get(1)?,
                    title: r.get(2)?,
                    agent_mode: r.get(3)?,
                    started_at: r.get(4)?,
                    ended_at: r.get(5)?,
                })
            },
        )
        .optional()?;

    if let Some(s) = existing {
        return Ok(s);
    }
    create(conn, project_id, agent_mode)
}

pub fn get(conn: &Connection, session_id: i64) -> Result<Session> {
    conn.query_row(
        "SELECT id, project_id, title, agent_mode, started_at, ended_at
         FROM sessions WHERE id = ?1",
        params![session_id],
        |r| {
            Ok(Session {
                id: r.get(0)?,
                project_id: r.get(1)?,
                title: r.get(2)?,
                agent_mode: r.get(3)?,
                started_at: r.get(4)?,
                ended_at: r.get(5)?,
            })
        },
    )
    .map_err(Into::into)
}

pub fn list(conn: &Connection, project_id: i64, limit: i64) -> Result<Vec<Session>> {
    let mut stmt = conn.prepare(
        "SELECT id, project_id, title, agent_mode, started_at, ended_at
         FROM sessions WHERE project_id = ?1
         ORDER BY started_at DESC LIMIT ?2",
    )?;
    let rows = stmt
        .query_map(params![project_id, limit], |r| {
            Ok(Session {
                id: r.get(0)?,
                project_id: r.get(1)?,
                title: r.get(2)?,
                agent_mode: r.get(3)?,
                started_at: r.get(4)?,
                ended_at: r.get(5)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

pub fn set_title(conn: &Connection, session_id: i64, title: &str) -> Result<()> {
    conn.execute(
        "UPDATE sessions SET title = ?1 WHERE id = ?2",
        params![title, session_id],
    )?;
    Ok(())
}

pub fn end(conn: &Connection, session_id: i64) -> Result<()> {
    conn.execute(
        "UPDATE sessions SET ended_at = strftime('%s', 'now') WHERE id = ?1",
        params![session_id],
    )?;
    Ok(())
}
