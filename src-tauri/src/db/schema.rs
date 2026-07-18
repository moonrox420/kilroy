//! Schema + migrations.
//!
//! Migrations are append-only — never rewrite, only add new files. The
//! `_meta` table tracks which versions have run. This keeps an upgrade
//! path simple as the agent evolves.

use anyhow::Result;
use rusqlite::Connection;

/// Embedding dimension. Matches `nomic-embed-text` from Ollama.
/// Swap this if you wire a different embedding model.
pub const EMBEDDING_DIM: usize = 768;

const MIGRATIONS: &[&str] = &[
    // v1 — core schema.
    include_str!("migrations/001_initial.sql"),
    // v2 — actuator actions + richer activity feed.
    include_str!("migrations/002_actions_and_activity.sql"),
    // v3 — refactor proposals (background-refactor swarm inbox).
    include_str!("migrations/003_refactor_proposals.sql"),
    // v4 — durable evidence-driven Rust agent runtime.
    include_str!("migrations/004_agent_runtime.sql"),
];

pub fn migrate(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS _meta (
            version INTEGER PRIMARY KEY,
            applied_at INTEGER NOT NULL
        );",
    )?;

    let current: u32 = conn
        .query_row("SELECT COALESCE(MAX(version), 0) FROM _meta", [], |r| {
            r.get(0)
        })
        .unwrap_or(0);

    for (idx, sql) in MIGRATIONS.iter().enumerate() {
        let v = (idx + 1) as u32;
        if v <= current {
            continue;
        }
        tracing::info!(version = v, "applying migration");
        // Each migration runs in its own transaction so a half-applied
        // schema cannot leave the DB in a broken state.
        conn.execute_batch("BEGIN;")?;
        let res: Result<()> = (|| {
            conn.execute_batch(sql)?;
            conn.execute(
                "INSERT INTO _meta (version, applied_at) VALUES (?, strftime('%s', 'now'))",
                [v],
            )?;
            Ok(())
        })();
        match res {
            Ok(()) => conn.execute_batch("COMMIT;")?,
            Err(e) => {
                conn.execute_batch("ROLLBACK;").ok();
                return Err(e.context(format!("migration v{} failed", v)));
            }
        }
    }

    Ok(())
}
