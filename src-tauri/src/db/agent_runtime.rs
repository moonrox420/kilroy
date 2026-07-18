//! Persistence for the durable Rust agent runtime.

use anyhow::Result;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

use super::projects::unix_now;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentRunRecord {
    pub run_id: String,
    pub session_id: Option<i64>,
    pub mode: String,
    pub user_message: String,
    pub status: String,
    pub verification_status: String,
    pub summary: Option<String>,
    pub error: Option<String>,
    pub model_calls: i64,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub started_at: i64,
    pub updated_at: i64,
    pub completed_at: Option<i64>,
}

pub fn recover_interrupted(conn: &Connection) -> Result<usize> {
    let changed = conn.execute(
        "UPDATE agent_runs
         SET status = 'interrupted',
             verification_status = 'blocked',
             error = COALESCE(error, 'Application stopped before the run completed.'),
             updated_at = strftime('%s', 'now'),
             completed_at = strftime('%s', 'now')
         WHERE status = 'running'",
        [],
    )?;
    conn.execute(
        "UPDATE agent_steps
         SET status = 'failed',
             error = COALESCE(error, 'Application stopped before the step completed.'),
             completed_at = strftime('%s', 'now')
         WHERE status = 'running'",
        [],
    )?;
    conn.execute(
        "UPDATE agent_tool_calls
         SET status = 'failed',
             error = COALESCE(error, 'Application stopped before the tool completed.'),
             completed_at = strftime('%s', 'now')
         WHERE status = 'running'",
        [],
    )?;
    Ok(changed)
}

pub fn create_run(
    conn: &Connection,
    run_id: &str,
    session_id: Option<i64>,
    mode: &str,
    user_message: &str,
) -> Result<()> {
    let now = unix_now();
    conn.execute(
        "INSERT INTO agent_runs (
            run_id, session_id, mode, user_message, status,
            verification_status, started_at, updated_at
         ) VALUES (?1, ?2, ?3, ?4, 'running', 'unverified', ?5, ?5)",
        params![run_id, session_id, mode, user_message, now],
    )?;
    Ok(())
}

pub fn create_step(
    conn: &Connection,
    run_id: &str,
    ordinal: i64,
    kind: &str,
    input: Option<&str>,
) -> Result<i64> {
    let now = unix_now();
    conn.execute(
        "INSERT INTO agent_steps (run_id, ordinal, kind, status, input, started_at)
         VALUES (?1, ?2, ?3, 'running', ?4, ?5)",
        params![run_id, ordinal, kind, input, now],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn finish_step(
    conn: &Connection,
    step_id: i64,
    status: &str,
    output: Option<&str>,
    error: Option<&str>,
    model_calls: i64,
    duration_ms: i64,
) -> Result<()> {
    conn.execute(
        "UPDATE agent_steps
         SET status = ?2, output = ?3, error = ?4, model_calls = ?5,
             duration_ms = ?6, completed_at = strftime('%s', 'now')
         WHERE id = ?1",
        params![step_id, status, output, error, model_calls, duration_ms],
    )?;
    Ok(())
}

pub fn start_tool_call(
    conn: &Connection,
    run_id: &str,
    step_id: Option<i64>,
    tool: &str,
    risk: &str,
    args: &str,
) -> Result<i64> {
    let now = unix_now();
    conn.execute(
        "INSERT INTO agent_tool_calls (
            run_id, step_id, tool, risk, status, args, created_at
         ) VALUES (?1, ?2, ?3, ?4, 'running', ?5, ?6)",
        params![run_id, step_id, tool, risk, args, now],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn finish_tool_call(
    conn: &Connection,
    id: i64,
    status: &str,
    result: Option<&str>,
    error: Option<&str>,
    duration_ms: i64,
) -> Result<()> {
    conn.execute(
        "UPDATE agent_tool_calls
         SET status = ?2, result = ?3, error = ?4, duration_ms = ?5,
             completed_at = strftime('%s', 'now')
         WHERE id = ?1",
        params![id, status, result, error, duration_ms],
    )?;
    Ok(())
}

pub fn record_pending_approval(
    conn: &Connection,
    run_id: &str,
    step_id: i64,
    action_id: i64,
    tool: &str,
    args: &str,
) -> Result<i64> {
    let now = unix_now();
    conn.execute(
        "INSERT INTO agent_tool_calls (
            run_id, step_id, action_id, tool, risk, status, args, created_at
         ) VALUES (?1, ?2, ?3, ?4, 'approval_required', 'pending_approval', ?5, ?6)",
        params![run_id, step_id, action_id, tool, args, now],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn resolve_approval(
    conn: &Connection,
    action_id: i64,
    action_status: &str,
    error: Option<&str>,
) -> Result<Option<String>> {
    let run_id = conn
        .query_row(
            "SELECT run_id FROM agent_tool_calls WHERE action_id = ?1",
            params![action_id],
            |row| row.get::<_, String>(0),
        )
        .optional()?;
    let Some(run_id) = run_id else {
        return Ok(None);
    };
    conn.execute(
        "UPDATE agent_tool_calls
         SET status = ?2, result = ?3, error = ?4, completed_at = strftime('%s', 'now')
         WHERE action_id = ?1 AND status = 'pending_approval'",
        params![
            action_id,
            if action_status == "failed" {
                "failed"
            } else {
                "completed"
            },
            action_status,
            error
        ],
    )?;
    let pending: i64 = conn.query_row(
        "SELECT COUNT(*) FROM agent_tool_calls
         WHERE run_id = ?1 AND status = 'pending_approval'",
        params![run_id],
        |row| row.get(0),
    )?;
    if pending == 0 {
        let failed: i64 = conn.query_row(
            "SELECT COUNT(*) FROM agent_tool_calls
             WHERE run_id = ?1 AND risk = 'approval_required' AND status = 'failed'",
            params![run_id],
            |row| row.get(0),
        )?;
        conn.execute(
            "UPDATE agent_runs
             SET status = ?2, verification_status = ?3, updated_at = strftime('%s', 'now'),
                 completed_at = strftime('%s', 'now'), error = COALESCE(?4, error)
             WHERE run_id = ?1",
            params![
                run_id,
                if failed > 0 { "failed" } else { "completed" },
                if failed > 0 { "blocked" } else { "unverified" },
                error
            ],
        )?;
    }
    Ok(Some(run_id))
}

pub fn add_evidence(
    conn: &Connection,
    run_id: &str,
    step_id: Option<i64>,
    kind: &str,
    source: &str,
    summary: &str,
    passed: bool,
    metadata: Option<&str>,
) -> Result<i64> {
    let now = unix_now();
    conn.execute(
        "INSERT INTO agent_evidence (
            run_id, step_id, kind, source, summary, passed, metadata, created_at
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            run_id,
            step_id,
            kind,
            source,
            summary,
            if passed { 1 } else { 0 },
            metadata,
            now
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

#[allow(clippy::too_many_arguments)]
pub fn finish_run(
    conn: &Connection,
    run_id: &str,
    status: &str,
    verification_status: &str,
    summary: Option<&str>,
    error: Option<&str>,
    model_calls: i64,
    input_tokens: i64,
    output_tokens: i64,
) -> Result<()> {
    conn.execute(
        "UPDATE agent_runs
         SET status = ?2, verification_status = ?3, summary = ?4, error = ?5,
             model_calls = ?6, input_tokens = ?7, output_tokens = ?8,
             updated_at = strftime('%s', 'now'),
             completed_at = CASE WHEN ?2 = 'awaiting_approval' THEN NULL ELSE strftime('%s', 'now') END
         WHERE run_id = ?1",
        params![
            run_id,
            status,
            verification_status,
            summary,
            error,
            model_calls,
            input_tokens,
            output_tokens
        ],
    )?;
    Ok(())
}

pub fn fetch_run(conn: &Connection, run_id: &str) -> Result<AgentRunRecord> {
    Ok(conn.query_row(
        "SELECT run_id, session_id, mode, user_message, status,
                verification_status, summary, error, model_calls,
                input_tokens, output_tokens, started_at, updated_at, completed_at
         FROM agent_runs WHERE run_id = ?1",
        params![run_id],
        |row| {
            Ok(AgentRunRecord {
                run_id: row.get(0)?,
                session_id: row.get(1)?,
                mode: row.get(2)?,
                user_message: row.get(3)?,
                status: row.get(4)?,
                verification_status: row.get(5)?,
                summary: row.get(6)?,
                error: row.get(7)?,
                model_calls: row.get(8)?,
                input_tokens: row.get(9)?,
                output_tokens: row.get(10)?,
                started_at: row.get(11)?,
                updated_at: row.get(12)?,
                completed_at: row.get(13)?,
            })
        },
    )?)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn database() -> Connection {
        let conn = Connection::open_in_memory().expect("open in-memory database");
        conn.execute_batch(
            "CREATE TABLE sessions (id INTEGER PRIMARY KEY);
             CREATE TABLE actions (id INTEGER PRIMARY KEY);",
        )
        .expect("create referenced sessions table");
        conn.execute_batch(include_str!("migrations/004_agent_runtime.sql"))
            .expect("apply agent runtime migration");
        conn
    }

    #[test]
    fn persists_completed_tool_evidence_and_approval_state() {
        let conn = database();
        create_run(&conn, "run_test", None, "code", "inspect and edit").unwrap();
        let step = create_step(&conn, "run_test", 1, "model_decision", None).unwrap();
        let tool = start_tool_call(
            &conn,
            "run_test",
            Some(step),
            "read_file",
            "read_only",
            r#"{"path":"src/lib.rs"}"#,
        )
        .unwrap();
        finish_tool_call(&conn, tool, "completed", Some("file contents"), None, 4).unwrap();
        add_evidence(
            &conn,
            "run_test",
            Some(step),
            "read_file",
            "project",
            "read src/lib.rs",
            true,
            None,
        )
        .unwrap();
        finish_step(&conn, step, "completed", Some("proposal"), None, 1, 10).unwrap();
        finish_run(
            &conn,
            "run_test",
            "awaiting_approval",
            "unverified",
            Some("proposal ready"),
            None,
            1,
            0,
            0,
        )
        .unwrap();

        let run = fetch_run(&conn, "run_test").unwrap();
        assert_eq!(run.status, "awaiting_approval");
        assert_eq!(run.verification_status, "unverified");
        assert!(run.completed_at.is_none());
    }

    #[test]
    fn recovers_interrupted_running_records() {
        let conn = database();
        create_run(&conn, "run_interrupted", None, "review_debug", "review").unwrap();
        create_step(&conn, "run_interrupted", 1, "model_decision", None).unwrap();
        assert_eq!(recover_interrupted(&conn).unwrap(), 1);
        let run = fetch_run(&conn, "run_interrupted").unwrap();
        assert_eq!(run.status, "interrupted");
        assert_eq!(run.verification_status, "blocked");
    }
}
