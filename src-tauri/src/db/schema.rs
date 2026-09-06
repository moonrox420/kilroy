//! Schema + migrations.
//!
//! Migrations are append-only — never rewrite, only add new files. The
//! `_meta` table tracks which versions have run. This keeps an upgrade
//! path simple as the agent evolves.

use anyhow::Result;
use rusqlite::{Connection, OptionalExtension};

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
    include_str!("migrations/005_staged_file_changes.sql"),
    include_str!("migrations/006_embedding_profile.sql"),
];

pub fn migrate(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS _meta (
            version INTEGER PRIMARY KEY,
            applied_at INTEGER NOT NULL
        );",
    )?;

    let current: u32 = conn.query_row("SELECT COALESCE(MAX(version), 0) FROM _meta", [], |r| {
        r.get(0)
    })?;

    for (idx, sql) in MIGRATIONS.iter().enumerate() {
        let v = (idx + 1) as u32;
        if v <= current {
            continue;
        }
        tracing::info!(version = v, "applying migration");
        let foreign_keys: bool = conn.pragma_query_value(None, "foreign_keys", |row| row.get(0))?;
        if v == 5 {
            conn.pragma_update(None, "foreign_keys", false)?;
        }
        // Each migration runs in its own transaction so a half-applied
        // schema cannot leave the DB in a broken state.
        let completion: Result<()> = (|| {
            let transaction = conn.unchecked_transaction()?;
            transaction.execute_batch(sql)?;
            let violation: Option<String> = transaction
                .query_row("PRAGMA foreign_key_check", [], |row| row.get(0))
                .optional()?;
            if let Some(table) = violation {
                anyhow::bail!("foreign-key violation in {table} during migration");
            }
            transaction.execute(
                "INSERT INTO _meta (version, applied_at) VALUES (?, strftime('%s', 'now'))",
                [v],
            )?;
            transaction.commit()?;
            Ok(())
        })();
        if v == 5 {
            conn.pragma_update(None, "foreign_keys", foreign_keys)?;
        }
        completion.map_err(|error| error.context(format!("migration v{v} failed")))?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn staged_action_migration_preserves_existing_approval_links() {
        crate::db::register_vec_extension();
        let conn = Connection::open_in_memory().unwrap();
        conn.pragma_update(None, "foreign_keys", true).unwrap();
        for sql in &MIGRATIONS[..4] {
            conn.execute_batch(sql).unwrap();
        }
        conn.execute_batch("CREATE TABLE _meta(version INTEGER PRIMARY KEY, applied_at INTEGER NOT NULL); INSERT INTO _meta VALUES(4, 0);
            INSERT INTO actions(id,kind,payload,status,created_at) VALUES(42,'shell','{}','pending',0);").unwrap();
        crate::db::agent_runtime::create_run(&conn, "migration-test", None, "code", "test")
            .unwrap();
        let step = crate::db::agent_runtime::create_step(&conn, "migration-test", 1, "test", None)
            .unwrap();
        crate::db::agent_runtime::record_pending_approval(
            &conn,
            "migration-test",
            step,
            42,
            "shell",
            "{}",
        )
        .unwrap();
        migrate(&conn).unwrap();
        let linked: i64 = conn
            .query_row("SELECT action_id FROM agent_tool_calls", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(linked, 42);
        assert!(conn
            .pragma_query_value::<bool, _>(None, "foreign_keys", |row| row.get(0))
            .unwrap());
        conn.execute("INSERT INTO actions(kind,payload,status,created_at) VALUES('file_change','{}','pending',0)", []).unwrap();
        migrate(&conn).unwrap();
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM actions", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 2);
    }
}
