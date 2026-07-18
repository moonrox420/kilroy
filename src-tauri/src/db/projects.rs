//! Project root CRUD.

use anyhow::Result;
use rusqlite::{params, Connection};
use serde::Serialize;
use std::path::Path;

#[derive(Serialize, Clone)]
pub struct Project {
    pub id: i64,
    pub name: String,
    pub root_path: String,
    pub created_at: i64,
    pub last_opened_at: i64,
}

/// Insert-or-touch: returns the existing row if one exists for the given
/// root path, otherwise creates a new project.
pub fn upsert(conn: &Connection, root: &Path) -> Result<Project> {
    let now = unix_now();
    let root_path = root.to_string_lossy().to_string();
    let name = root
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| root_path.clone());

    conn.execute(
        "INSERT INTO projects (name, root_path, created_at, last_opened_at)
         VALUES (?1, ?2, ?3, ?3)
         ON CONFLICT(root_path) DO UPDATE SET last_opened_at = excluded.last_opened_at",
        params![name, root_path, now],
    )?;

    fetch_by_path(conn, &root_path)
}

pub fn fetch_by_path(conn: &Connection, root_path: &str) -> Result<Project> {
    let p = conn.query_row(
        "SELECT id, name, root_path, created_at, last_opened_at
         FROM projects WHERE root_path = ?1",
        params![root_path],
        |r| {
            Ok(Project {
                id: r.get(0)?,
                name: r.get(1)?,
                root_path: r.get(2)?,
                created_at: r.get(3)?,
                last_opened_at: r.get(4)?,
            })
        },
    )?;
    Ok(p)
}

pub fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}
