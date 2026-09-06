//! Decision log — the "why we did it this way" record.
//!
//! Decisions are short architectural notes the agent (or the user) writes
//! down as the project evolves. They're embedded for semantic retrieval
//! so when the agent is about to make a change, it can recall related
//! rationale and act consistently.

use anyhow::Result;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

use super::projects::unix_now;
use super::schema::EMBEDDING_DIM;

#[derive(Serialize, Deserialize, Clone)]
pub struct Decision {
    pub id: i64,
    pub project_id: i64,
    pub title: String,
    pub summary: String,
    pub rationale: Option<String>,
    pub related_files: Option<String>,
    pub created_at: i64,
}

pub fn insert(
    conn: &Connection,
    project_id: i64,
    title: &str,
    summary: &str,
    rationale: Option<&str>,
    related_files_json: Option<&str>,
) -> Result<i64> {
    let now = unix_now();
    conn.execute(
        "INSERT INTO decisions (project_id, title, summary, rationale, related_files, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            project_id,
            title,
            summary,
            rationale,
            related_files_json,
            now
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn insert_embedding(conn: &Connection, decision_id: i64, embedding: &[f32]) -> Result<()> {
    if embedding.len() != EMBEDDING_DIM {
        anyhow::bail!(
            "embedding length {} != EMBEDDING_DIM {}",
            embedding.len(),
            EMBEDDING_DIM
        );
    }
    let mut bytes = Vec::with_capacity(embedding.len() * 4);
    for f in embedding {
        bytes.extend_from_slice(&f.to_le_bytes());
    }
    conn.execute(
        "INSERT INTO decision_embeddings (decision_id, embedding) VALUES (?1, ?2)",
        params![decision_id, bytes],
    )?;
    Ok(())
}

// Same rationale as `ChunkHit` — we embed Vec<DecisionHit> in the
// AgentContext blob persisted to `messages.metadata` and read it back
// when rehydrating chat history.
#[derive(Serialize, Deserialize, Clone)]
pub struct DecisionHit {
    pub decision_id: i64,
    pub title: String,
    pub summary: String,
    pub rationale: Option<String>,
    pub created_at: i64,
    pub distance: f32,
}

pub fn knn(
    conn: &Connection,
    project_id: i64,
    query_embedding: &[f32],
    k: usize,
) -> Result<Vec<DecisionHit>> {
    let mut bytes = Vec::with_capacity(query_embedding.len() * 4);
    for f in query_embedding {
        bytes.extend_from_slice(&f.to_le_bytes());
    }
    let mut stmt = conn.prepare(
        "SELECT de.decision_id, d.title, d.summary, d.rationale, d.created_at, de.distance
         FROM decision_embeddings de
         JOIN decisions d ON d.id = de.decision_id
         WHERE de.embedding MATCH ?1 AND d.project_id = ?2 AND k = ?3
         ORDER BY de.distance",
    )?;
    let rows = stmt
        .query_map(params![bytes, project_id, k as i64], |r| {
            Ok(DecisionHit {
                decision_id: r.get(0)?,
                title: r.get(1)?,
                summary: r.get(2)?,
                rationale: r.get(3)?,
                created_at: r.get(4)?,
                distance: r.get(5)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

pub fn list(conn: &Connection, project_id: i64, limit: i64) -> Result<Vec<Decision>> {
    let mut stmt = conn.prepare(
        "SELECT id, project_id, title, summary, rationale, related_files, created_at
         FROM decisions WHERE project_id = ?1
         ORDER BY created_at DESC LIMIT ?2",
    )?;
    let rows = stmt
        .query_map(params![project_id, limit], |r| {
            Ok(Decision {
                id: r.get(0)?,
                project_id: r.get(1)?,
                title: r.get(2)?,
                summary: r.get(3)?,
                rationale: r.get(4)?,
                related_files: r.get(5)?,
                created_at: r.get(6)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}
