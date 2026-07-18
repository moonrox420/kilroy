-- Durable evidence-driven agent runtime.

CREATE TABLE IF NOT EXISTS agent_runs (
    run_id TEXT PRIMARY KEY,
    session_id INTEGER REFERENCES sessions(id) ON DELETE SET NULL,
    mode TEXT NOT NULL CHECK(mode IN ('chat', 'code', 'plan_execute', 'review_debug')),
    user_message TEXT NOT NULL,
    status TEXT NOT NULL CHECK(status IN (
        'running', 'awaiting_approval', 'completed', 'failed', 'blocked', 'cancelled', 'interrupted'
    )),
    verification_status TEXT NOT NULL CHECK(verification_status IN (
        'verified', 'partially_verified', 'unverified', 'blocked'
    )) DEFAULT 'unverified',
    summary TEXT,
    error TEXT,
    model_calls INTEGER NOT NULL DEFAULT 0,
    input_tokens INTEGER NOT NULL DEFAULT 0,
    output_tokens INTEGER NOT NULL DEFAULT 0,
    started_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    completed_at INTEGER
);

CREATE INDEX IF NOT EXISTS idx_agent_runs_session
    ON agent_runs(session_id, started_at DESC);
CREATE INDEX IF NOT EXISTS idx_agent_runs_status
    ON agent_runs(status, updated_at);

CREATE TABLE IF NOT EXISTS agent_steps (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    run_id TEXT NOT NULL REFERENCES agent_runs(run_id) ON DELETE CASCADE,
    ordinal INTEGER NOT NULL,
    kind TEXT NOT NULL,
    status TEXT NOT NULL CHECK(status IN ('running', 'completed', 'failed', 'blocked')),
    input TEXT,
    output TEXT,
    error TEXT,
    model_calls INTEGER NOT NULL DEFAULT 0,
    duration_ms INTEGER,
    started_at INTEGER NOT NULL,
    completed_at INTEGER,
    UNIQUE(run_id, ordinal)
);

CREATE INDEX IF NOT EXISTS idx_agent_steps_run
    ON agent_steps(run_id, ordinal);

CREATE TABLE IF NOT EXISTS agent_tool_calls (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    run_id TEXT NOT NULL REFERENCES agent_runs(run_id) ON DELETE CASCADE,
    step_id INTEGER REFERENCES agent_steps(id) ON DELETE SET NULL,
    action_id INTEGER REFERENCES actions(id) ON DELETE SET NULL,
    tool TEXT NOT NULL,
    risk TEXT NOT NULL CHECK(risk IN ('read_only', 'approval_required')),
    status TEXT NOT NULL CHECK(status IN ('running', 'completed', 'failed', 'pending_approval')),
    args TEXT NOT NULL,
    result TEXT,
    error TEXT,
    duration_ms INTEGER,
    created_at INTEGER NOT NULL,
    completed_at INTEGER
);

CREATE INDEX IF NOT EXISTS idx_agent_tool_calls_run
    ON agent_tool_calls(run_id, created_at);
CREATE INDEX IF NOT EXISTS idx_agent_tool_calls_action
    ON agent_tool_calls(action_id);

CREATE TABLE IF NOT EXISTS agent_evidence (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    run_id TEXT NOT NULL REFERENCES agent_runs(run_id) ON DELETE CASCADE,
    step_id INTEGER REFERENCES agent_steps(id) ON DELETE SET NULL,
    kind TEXT NOT NULL,
    source TEXT NOT NULL,
    summary TEXT NOT NULL,
    passed INTEGER NOT NULL CHECK(passed IN (0, 1)),
    metadata TEXT,
    created_at INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_agent_evidence_run
    ON agent_evidence(run_id, created_at);
