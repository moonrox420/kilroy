//! Prevent vectors from different models or servers sharing a search space.

use anyhow::{bail, Result};
use rusqlite::{Connection, OptionalExtension};

fn current(conn: &Connection) -> Result<Option<String>> {
    Ok(conn
        .query_row(
            "SELECT profile FROM embedding_profile WHERE id = 1",
            [],
            |row| row.get(0),
        )
        .optional()?)
}

/// Bind an empty database, or refuse retrieval/writes against a different model.
pub fn ensure(conn: &Connection, profile: &str) -> Result<()> {
    match current(conn)? {
        Some(existing) if existing == profile => Ok(()),
        None => {
            let populated: bool = conn.query_row(
                "SELECT EXISTS(SELECT 1 FROM chunk_embeddings) OR EXISTS(SELECT 1 FROM decision_embeddings)",
                [], |row| row.get(0),
            )?;
            if populated {
                bail!("The existing vector index has no model provenance. Run Index Project to rebuild it before semantic retrieval.");
            }
            conn.execute("INSERT INTO embedding_profile(id, profile) VALUES(1, ?1)", [profile])?;
            Ok(())
        }
        Some(_) => bail!("The embedding model or server changed. Run Index Project to rebuild derived vectors; conversations and decisions are preserved."),
    }
}

/// Invalidate derived vectors only after the caller has probed the new model.
pub fn prepare_index(conn: &Connection, profile: &str) -> Result<()> {
    if current(conn)?.as_deref() == Some(profile) {
        return Ok(());
    }
    let transaction = conn.unchecked_transaction()?;
    transaction.execute_batch("DELETE FROM chunk_embeddings; DELETE FROM chunks; UPDATE files SET content_hash = ''; DELETE FROM decision_embeddings;")?;
    transaction.execute(
        "INSERT INTO embedding_profile(id, profile) VALUES(1, ?1) ON CONFLICT(id) DO UPDATE SET profile = excluded.profile",
        [profile],
    )?;
    transaction.commit()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn models_cannot_mix_and_rebuild_preserves_decision_text() {
        crate::db::register_vec_extension();
        let conn = Connection::open_in_memory().unwrap();
        crate::db::schema::migrate(&conn).unwrap();
        ensure(&conn, "model-a").unwrap();
        ensure(&conn, "model-a").unwrap();
        assert!(ensure(&conn, "model-b").is_err());
        conn.execute("INSERT INTO projects(id, root_path, name, created_at, last_opened_at) VALUES(1, 'test', 'test', 0, 0)", []).unwrap();
        let id = crate::db::decisions::insert(&conn, 1, "Choice", "Keep this decision", None, None)
            .unwrap();
        crate::db::decisions::insert_embedding(&conn, id, &vec![0.0; 768]).unwrap();
        prepare_index(&conn, "model-b").unwrap();
        ensure(&conn, "model-b").unwrap();
        assert!(ensure(&conn, "model-a").is_err());
        assert_eq!(crate::db::decisions::list(&conn, 1, 10).unwrap().len(), 1);
        assert_eq!(
            conn.query_row("SELECT count(*) FROM decision_embeddings", [], |row| row
                .get::<_, i64>(0))
                .unwrap(),
            0
        );
    }
}
