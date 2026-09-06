-- Kilroy memory schema v1.
--
-- Tables are grouped: project root → sessions → messages, plus a separate
-- code-indexing path (files → chunks → chunk_embeddings) and decision log.
-- Every timestamp is unix seconds (INTEGER) for portability.

-- ─── Project root ───────────────────────────────────────────────────────────

CREATE TABLE IF NOT EXISTS projects (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL,
    root_path TEXT NOT NULL UNIQUE,
    created_at INTEGER NOT NULL,
    last_opened_at INTEGER NOT NULL
);

-- ─── Conversation history ───────────────────────────────────────────────────

CREATE TABLE IF NOT EXISTS sessions (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    project_id INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    title TEXT,
    agent_mode TEXT NOT NULL,
    started_at INTEGER NOT NULL,
    ended_at INTEGER
);

CREATE INDEX IF NOT EXISTS idx_sessions_project ON sessions(project_id, started_at DESC);

CREATE TABLE IF NOT EXISTS messages (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id INTEGER NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    role TEXT NOT NULL CHECK(role IN ('user', 'agent', 'system', 'tool')),
    content TEXT NOT NULL,
    metadata TEXT,
    parent_id INTEGER REFERENCES messages(id) ON DELETE SET NULL,
    created_at INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_messages_session ON messages(session_id, created_at);

-- ─── Code indexing for semantic search ──────────────────────────────────────

CREATE TABLE IF NOT EXISTS files (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    project_id INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    path TEXT NOT NULL,
    content_hash TEXT NOT NULL,
    size_bytes INTEGER NOT NULL,
    language TEXT,
    last_indexed_at INTEGER NOT NULL,
    UNIQUE(project_id, path)
);

CREATE INDEX IF NOT EXISTS idx_files_project ON files(project_id);

CREATE TABLE IF NOT EXISTS chunks (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    file_id INTEGER NOT NULL REFERENCES files(id) ON DELETE CASCADE,
    start_line INTEGER NOT NULL,
    end_line INTEGER NOT NULL,
    symbol TEXT,
    content TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_chunks_file ON chunks(file_id);

-- sqlite-vec virtual table for the actual vector index.
-- vec0(...) is the API from the sqlite-vec extension.
CREATE VIRTUAL TABLE IF NOT EXISTS chunk_embeddings USING vec0(
    chunk_id INTEGER PRIMARY KEY,
    embedding FLOAT[768]
);

-- ─── Task graph history (audit trail for agent runs) ────────────────────────

CREATE TABLE IF NOT EXISTS tasks (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id INTEGER REFERENCES sessions(id) ON DELETE SET NULL,
    parent_id INTEGER REFERENCES tasks(id) ON DELETE SET NULL,
    type TEXT NOT NULL,
    agent TEXT NOT NULL,
    status TEXT NOT NULL CHECK(status IN ('pending', 'running', 'success', 'failed', 'cancelled')),
    input TEXT NOT NULL,
    output TEXT,
    retry_count INTEGER NOT NULL DEFAULT 0,
    started_at INTEGER,
    completed_at INTEGER,
    created_at INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_tasks_session ON tasks(session_id, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_tasks_status ON tasks(status);

CREATE TABLE IF NOT EXISTS tool_calls (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    task_id INTEGER NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    tool TEXT NOT NULL,
    args TEXT NOT NULL,
    result TEXT,
    error TEXT,
    duration_ms INTEGER,
    created_at INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_tool_calls_task ON tool_calls(task_id, created_at);

-- ─── Architectural decisions ────────────────────────────────────────────────

CREATE TABLE IF NOT EXISTS decisions (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    project_id INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    title TEXT NOT NULL,
    summary TEXT NOT NULL,
    rationale TEXT,
    related_files TEXT,
    created_at INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_decisions_project ON decisions(project_id, created_at DESC);

CREATE VIRTUAL TABLE IF NOT EXISTS decision_embeddings USING vec0(
    decision_id INTEGER PRIMARY KEY,
    embedding FLOAT[768]
);

-- ─── Activity feed (UI stream) ──────────────────────────────────────────────

CREATE TABLE IF NOT EXISTS activity (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id INTEGER REFERENCES sessions(id) ON DELETE SET NULL,
    kind TEXT NOT NULL,
    payload TEXT NOT NULL,
    created_at INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_activity_session ON activity(session_id, created_at DESC);
