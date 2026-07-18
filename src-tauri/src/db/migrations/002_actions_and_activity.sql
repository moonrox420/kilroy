-- Kilroy memory schema v2 — actuator actions + richer activity feed.

CREATE TABLE IF NOT EXISTS actions (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id INTEGER REFERENCES sessions(id) ON DELETE SET NULL,
    task_id INTEGER REFERENCES tasks(id) ON DELETE SET NULL,
    kind TEXT NOT NULL CHECK(kind IN ('file_write', 'file_patch', 'shell')),
    target TEXT,                  -- file path or shell label
    payload TEXT NOT NULL,        -- JSON: { content, language?, command?, sandbox? }
    diff TEXT,                    -- precomputed unified diff (for file_write)
    status TEXT NOT NULL CHECK(status IN ('pending', 'accepted', 'rejected', 'applied', 'failed')) DEFAULT 'pending',
    error TEXT,
    created_at INTEGER NOT NULL,
    resolved_at INTEGER
);

CREATE INDEX IF NOT EXISTS idx_actions_session ON actions(session_id, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_actions_task ON actions(task_id);
CREATE INDEX IF NOT EXISTS idx_actions_status ON actions(status);

-- We had an `activity` table from v1 but never wrote to it. Now we do, and we
-- want a generated index over JSON payload for quick filtering. SQLite's JSON1
-- extension is bundled with rusqlite, so this works.
-- (No new columns needed; v1's schema already has kind + payload + created_at.)
