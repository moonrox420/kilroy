//! Code chunking + vector storage.
//!
//! Naive but reliable chunker: a sliding window of `WINDOW` lines with
//! `STRIDE` overlap. We skip files that are too large, look obviously
//! binary, or in standard ignore directories. Symbol detection is left
//! to a future pass — currently the chunk's first non-blank line acts
//! as a stand-in label.

use anyhow::Result;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

use super::schema::EMBEDDING_DIM;

pub const DEFAULT_WINDOW: usize = 30;
pub const DEFAULT_STRIDE: usize = 22; // = DEFAULT_WINDOW - 8 lines of overlap

#[derive(Debug, Clone)]
pub struct Chunk {
    pub start_line: usize,
    pub end_line: usize,
    pub content: String,
    pub symbol: Option<String>,
}

pub fn chunk_text(text: &str) -> Vec<Chunk> {
    chunk_text_with(text, DEFAULT_WINDOW, DEFAULT_STRIDE)
}

pub fn chunk_text_with(text: &str, window: usize, stride: usize) -> Vec<Chunk> {
    let window = window.max(2);
    let stride = stride.max(1).min(window);
    let lines: Vec<&str> = text.lines().collect();
    if lines.is_empty() {
        return vec![];
    }
    let mut out = Vec::new();
    let mut start = 0usize;
    while start < lines.len() {
        let end = (start + window).min(lines.len());
        let slice = &lines[start..end];
        let content = slice.join("\n");
        let symbol = slice
            .iter()
            .find(|l| !l.trim().is_empty())
            .map(|l| l.trim().chars().take(120).collect::<String>());
        out.push(Chunk {
            start_line: start + 1,
            end_line: end,
            content,
            symbol,
        });
        if end == lines.len() {
            break;
        }
        start += stride;
    }
    out
}

pub fn insert_chunks(conn: &Connection, file_id: i64, chunks: &[Chunk]) -> Result<Vec<i64>> {
    let mut stmt = conn.prepare(
        "INSERT INTO chunks (file_id, start_line, end_line, symbol, content)
         VALUES (?1, ?2, ?3, ?4, ?5)",
    )?;
    let mut ids = Vec::with_capacity(chunks.len());
    for c in chunks {
        stmt.execute(params![
            file_id,
            c.start_line as i64,
            c.end_line as i64,
            c.symbol,
            c.content
        ])?;
        ids.push(conn.last_insert_rowid());
    }
    Ok(ids)
}

/// Store a vector. sqlite-vec wants a length-prefixed little-endian float
/// blob; we encode that here.
pub fn insert_embedding(conn: &Connection, chunk_id: i64, embedding: &[f32]) -> Result<()> {
    if embedding.len() != EMBEDDING_DIM {
        anyhow::bail!(
            "embedding length {} != EMBEDDING_DIM {}",
            embedding.len(),
            EMBEDDING_DIM
        );
    }
    let bytes = floats_to_bytes(embedding);
    conn.execute(
        "INSERT INTO chunk_embeddings (chunk_id, embedding) VALUES (?1, ?2)",
        params![chunk_id, bytes],
    )?;
    Ok(())
}

fn floats_to_bytes(v: &[f32]) -> Vec<u8> {
    let mut out = Vec::with_capacity(v.len() * 4);
    for f in v {
        out.extend_from_slice(&f.to_le_bytes());
    }
    out
}

// `Deserialize` is needed because we embed `Vec<ChunkHit>` inside
// `AgentContext`, which is the JSON metadata blob stored on each
// agent message — we read it back when rehydrating chat history.
#[derive(Serialize, Deserialize, Clone)]
pub struct ChunkHit {
    pub chunk_id: i64,
    pub file_id: i64,
    pub file_path: String,
    pub start_line: i64,
    pub end_line: i64,
    pub symbol: Option<String>,
    pub content: String,
    pub distance: f32,
}

/// k-NN search over chunk embeddings, scoped to a single project.
pub fn knn(
    conn: &Connection,
    project_id: i64,
    query_embedding: &[f32],
    k: usize,
) -> Result<Vec<ChunkHit>> {
    let bytes = floats_to_bytes(query_embedding);
    let mut stmt = conn.prepare(
        "SELECT ce.chunk_id, c.file_id, f.path, c.start_line, c.end_line, c.symbol, c.content, ce.distance
         FROM chunk_embeddings ce
         JOIN chunks c ON c.id = ce.chunk_id
         JOIN files f ON f.id = c.file_id
         WHERE ce.embedding MATCH ?1 AND f.project_id = ?2 AND k = ?3
         ORDER BY ce.distance",
    )?;
    let rows = stmt
        .query_map(params![bytes, project_id, k as i64], |r| {
            Ok(ChunkHit {
                chunk_id: r.get(0)?,
                file_id: r.get(1)?,
                file_path: r.get(2)?,
                start_line: r.get(3)?,
                end_line: r.get(4)?,
                symbol: r.get(5)?,
                content: r.get(6)?,
                distance: r.get(7)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}
