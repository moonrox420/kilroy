//! File metadata + content-hash dedupe.

use anyhow::Result;
use rusqlite::{params, Connection, OptionalExtension};
use sha2::{Digest, Sha256};

pub fn hash(content: &str) -> String {
    use std::fmt::Write;
    let mut h = Sha256::new();
    h.update(content.as_bytes());
    let digest = h.finalize();
    // sha2 0.11+ migrated its output from `GenericArray` to `hybrid-array::Array`,
    // which no longer implements `LowerHex`. Iterate the bytes explicitly so we
    // don't depend on the digest type's formatting impls.
    let mut out = String::with_capacity(digest.len() * 2);
    for byte in digest.iter() {
        let _ = write!(out, "{:02x}", byte);
    }
    out
}

pub struct FileRecord {
    pub id: i64,
    pub path: String,
    pub content_hash: String,
}

/// Returns Ok((file_id, was_new_hash)).
///
/// If the same file path is already indexed with the same content hash,
/// `was_new_hash` is false and the caller can skip re-chunking. Otherwise
/// the file row is created or updated and old chunks should be cleared.
pub fn upsert(
    conn: &Connection,
    project_id: i64,
    path: &str,
    content_hash: &str,
    size_bytes: i64,
    language: Option<&str>,
) -> Result<(i64, bool)> {
    let existing: Option<FileRecord> = conn
        .query_row(
            "SELECT id, path, content_hash FROM files WHERE project_id = ?1 AND path = ?2",
            params![project_id, path],
            |r| {
                Ok(FileRecord {
                    id: r.get(0)?,
                    path: r.get(1)?,
                    content_hash: r.get(2)?,
                })
            },
        )
        .optional()?;

    if let Some(rec) = existing {
        if rec.content_hash == content_hash {
            return Ok((rec.id, false));
        }
        // Hash changed — bump metadata and clear old chunks so the caller
        // can re-chunk from scratch.
        conn.execute(
            "UPDATE files
             SET content_hash = ?1, size_bytes = ?2, language = ?3,
                 last_indexed_at = strftime('%s', 'now')
             WHERE id = ?4",
            params![content_hash, size_bytes, language, rec.id],
        )?;
        conn.execute(
            "DELETE FROM chunk_embeddings
             WHERE chunk_id IN (SELECT id FROM chunks WHERE file_id = ?1)",
            params![rec.id],
        )?;
        conn.execute("DELETE FROM chunks WHERE file_id = ?1", params![rec.id])?;
        return Ok((rec.id, true));
    }

    conn.execute(
        "INSERT INTO files (project_id, path, content_hash, size_bytes, language, last_indexed_at)
         VALUES (?1, ?2, ?3, ?4, ?5, strftime('%s', 'now'))",
        params![project_id, path, content_hash, size_bytes, language],
    )?;
    Ok((conn.last_insert_rowid(), true))
}

/// Files indexed for a project, ordered by path.
pub fn list(conn: &Connection, project_id: i64) -> Result<Vec<String>> {
    let mut stmt = conn.prepare("SELECT path FROM files WHERE project_id = ?1 ORDER BY path")?;
    let rows = stmt
        .query_map(params![project_id], |r| r.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}
