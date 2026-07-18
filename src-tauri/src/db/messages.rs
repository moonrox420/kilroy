//! Chat messages within a session.

use anyhow::Result;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

use super::projects::unix_now;

#[derive(Serialize, Deserialize, Clone)]
pub struct Message {
    pub id: i64,
    pub session_id: i64,
    pub role: String,
    pub content: String,
    pub metadata: Option<String>,
    pub parent_id: Option<i64>,
    pub created_at: i64,
}

pub fn insert(
    conn: &Connection,
    session_id: i64,
    role: &str,
    content: &str,
    metadata: Option<&str>,
    parent_id: Option<i64>,
) -> Result<Message> {
    let now = unix_now();
    conn.execute(
        "INSERT INTO messages (session_id, role, content, metadata, parent_id, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![session_id, role, content, metadata, parent_id, now],
    )?;
    let id = conn.last_insert_rowid();
    Ok(Message {
        id,
        session_id,
        role: role.to_string(),
        content: content.to_string(),
        metadata: metadata.map(str::to_owned),
        parent_id,
        created_at: now,
    })
}

pub fn list(conn: &Connection, session_id: i64, limit: i64) -> Result<Vec<Message>> {
    let mut stmt = conn.prepare(
        "SELECT id, session_id, role, content, metadata, parent_id, created_at
         FROM messages WHERE session_id = ?1
         ORDER BY created_at ASC, id ASC
         LIMIT ?2",
    )?;
    let rows = stmt
        .query_map(params![session_id, limit], |r| {
            Ok(Message {
                id: r.get(0)?,
                session_id: r.get(1)?,
                role: r.get(2)?,
                content: r.get(3)?,
                metadata: r.get(4)?,
                parent_id: r.get(5)?,
                created_at: r.get(6)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

/// Pull the last `n` messages, oldest-first, for prompt context.
pub fn tail(conn: &Connection, session_id: i64, n: i64) -> Result<Vec<Message>> {
    let mut stmt = conn.prepare(
        "SELECT id, session_id, role, content, metadata, parent_id, created_at
         FROM messages WHERE session_id = ?1
         ORDER BY created_at DESC, id DESC LIMIT ?2",
    )?;
    let mut rows = stmt
        .query_map(params![session_id, n], |r| {
            Ok(Message {
                id: r.get(0)?,
                session_id: r.get(1)?,
                role: r.get(2)?,
                content: r.get(3)?,
                metadata: r.get(4)?,
                parent_id: r.get(5)?,
                created_at: r.get(6)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    rows.reverse();
    Ok(rows)
}
