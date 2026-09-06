//! Activity feed.
//!
//! Append-only timeline of "things that happened" — messages sent,
//! tasks started, decisions logged, actions accepted, indexes built.
//! Drives the Activity drawer; lighter-weight than the per-table lists
//! because each row is a {kind, payload-json, created_at} tuple.

use anyhow::Result;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

use super::projects::unix_now;

#[derive(Serialize, Deserialize, Clone)]
pub struct ActivityRow {
    pub id: i64,
    pub session_id: Option<i64>,
    pub kind: String,
    pub payload: String,
    pub created_at: i64,
}

pub fn log(
    conn: &Connection,
    session_id: Option<i64>,
    kind: &str,
    payload_json: &str,
) -> Result<i64> {
    let now = unix_now();
    conn.execute(
        "INSERT INTO activity (session_id, kind, payload, created_at)
         VALUES (?1, ?2, ?3, ?4)",
        params![session_id, kind, payload_json, now],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn list(conn: &Connection, session_id: Option<i64>, limit: i64) -> Result<Vec<ActivityRow>> {
    let mut rows = Vec::new();
    if let Some(sid) = session_id {
        let mut stmt = conn.prepare(
            "SELECT id, session_id, kind, payload, created_at
             FROM activity WHERE session_id = ?1
             ORDER BY created_at DESC, id DESC LIMIT ?2",
        )?;
        let it = stmt
            .query_map(params![sid, limit], map_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        rows.extend(it);
    } else {
        let mut stmt = conn.prepare(
            "SELECT id, session_id, kind, payload, created_at
             FROM activity ORDER BY created_at DESC, id DESC LIMIT ?1",
        )?;
        let it = stmt
            .query_map(params![limit], map_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        rows.extend(it);
    }
    Ok(rows)
}

fn map_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<ActivityRow> {
    Ok(ActivityRow {
        id: r.get(0)?,
        session_id: r.get(1)?,
        kind: r.get(2)?,
        payload: r.get(3)?,
        created_at: r.get(4)?,
    })
}
