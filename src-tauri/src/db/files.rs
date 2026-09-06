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
    pub content_hash: String,
}

/// Check without changing metadata; embedding failure must remain retryable.
pub fn needs_index(
    conn: &Connection,
    project_id: i64,
    path: &str,
    content_hash: &str,
) -> Result<bool> {
    let existing: Option<String> = conn
        .query_row(
            "SELECT content_hash FROM files WHERE project_id = ?1 AND path = ?2",
            params![project_id, path],
            |row| row.get(0),
        )
        .optional()?;
    Ok(existing.as_deref() != Some(content_hash))
}

/// Commit file metadata, chunks and vectors together, retaining the old index on error.
pub struct IndexedFile<'a> {
    pub project_id: i64,
    pub path: &'a str,
    pub content_hash: &'a str,
    pub size_bytes: i64,
    pub language: Option<&'a str>,
}

pub fn replace_index(
    conn: &Connection,
    file: IndexedFile<'_>,
    pieces: &[super::chunks::Chunk],
    vectors: &[Vec<f32>],
) -> Result<()> {
    anyhow::ensure!(pieces.len() == vectors.len(), "chunk/vector count mismatch");
    let transaction = conn.unchecked_transaction()?;
    let (id, changed) = upsert(
        &transaction,
        file.project_id,
        file.path,
        file.content_hash,
        file.size_bytes,
        file.language,
    )?;
    if changed {
        let ids = super::chunks::insert_chunks(&transaction, id, pieces)?;
        for (chunk_id, vector) in ids.iter().zip(vectors) {
            super::chunks::insert_embedding(&transaction, *chunk_id, vector)?;
        }
    }
    transaction.commit()?;
    Ok(())
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
            "SELECT id, content_hash FROM files WHERE project_id = ?1 AND path = ?2",
            params![project_id, path],
            |r| {
                Ok(FileRecord {
                    id: r.get(0)?,
                    content_hash: r.get(1)?,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failed_vector_write_keeps_old_hash_and_chunks_retryable() {
        crate::db::register_vec_extension();
        let conn = Connection::open_in_memory().unwrap();
        crate::db::schema::migrate(&conn).unwrap();
        let project =
            crate::db::projects::upsert(&conn, std::path::Path::new("index-test")).unwrap();
        let pieces = crate::db::chunks::chunk_text_with("old content", 80, 60);
        let metadata = |content_hash| IndexedFile {
            project_id: project.id,
            path: "source.rs",
            content_hash,
            size_bytes: 11,
            language: Some("rust"),
        };
        replace_index(&conn, metadata("old"), &pieces, &[vec![0.0; 768]]).unwrap();
        let changed = crate::db::chunks::chunk_text_with("new content", 80, 60);
        assert!(replace_index(&conn, metadata("new"), &changed, &[vec![0.0; 384]]).is_err());
        assert!(needs_index(&conn, project.id, "source.rs", "new").unwrap());
        assert!(!needs_index(&conn, project.id, "source.rs", "old").unwrap());
        assert_eq!(
            conn.query_row("SELECT content FROM chunks", [], |row| row
                .get::<_, String>(0))
                .unwrap(),
            "old content"
        );
        replace_index(&conn, metadata("new"), &changed, &[vec![0.0; 768]]).unwrap();
        assert!(!needs_index(&conn, project.id, "source.rs", "new").unwrap());
        assert_eq!(
            conn.query_row("SELECT content FROM chunks", [], |row| row
                .get::<_, String>(0))
                .unwrap(),
            "new content"
        );
    }
}
