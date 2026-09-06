-- Preserve existing actions and their foreign-key identities while extending
-- the kind constraint for application-captured sandbox output.
CREATE TABLE actions_v5 (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id INTEGER REFERENCES sessions(id) ON DELETE SET NULL,
    task_id INTEGER REFERENCES tasks(id) ON DELETE SET NULL,
    kind TEXT NOT NULL CHECK(kind IN ('file_write', 'file_patch', 'shell', 'file_change')),
    target TEXT,
    payload TEXT NOT NULL,
    diff TEXT,
    status TEXT NOT NULL CHECK(status IN ('pending', 'accepted', 'rejected', 'applied', 'failed')) DEFAULT 'pending',
    error TEXT,
    created_at INTEGER NOT NULL,
    resolved_at INTEGER
);
INSERT INTO actions_v5 SELECT * FROM actions;
DROP TABLE actions;
ALTER TABLE actions_v5 RENAME TO actions;
CREATE INDEX idx_actions_session ON actions(session_id, created_at DESC);
CREATE INDEX idx_actions_task ON actions(task_id);
CREATE INDEX idx_actions_status ON actions(status);
