-- Refactor proposals — the inbox for the background-refactoring swarm.
--
-- Each row is one refactor suggestion produced by the 4-voice refactor
-- swarm (duplicate-hunter / complexity-hunter / error-handling auditor
-- / pattern modernizer), already filtered through the synthesizer pass
-- that picks the highest-impact / lowest-risk proposal per scan.
--
-- The diff lives here so the user can review it before it touches disk.
-- When the user clicks Apply, the diff is routed through the existing
-- actuator system as a pending file_patch action — that means accept /
-- reject / per-hunk-cherry-pick all work the same way they do for
-- agent-proposed edits in chat.
--
-- `verification_status` distinguishes proposals we've test-verified
-- ('passed') from those we haven't ('untested') from those we tried
-- ('failed'). The MVP only stores 'untested'; auto-running tests on
-- each proposal lands in a follow-up pass.

CREATE TABLE IF NOT EXISTS refactor_proposals (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    -- File the proposal targets. Absolute path so we can write back
    -- without ambiguity even if the user changes the project root mid-
    -- session.
    file_path TEXT NOT NULL,
    -- Short human-readable title. Becomes the inbox row label.
    title TEXT NOT NULL,
    -- Free-form rationale from the synthesizer (Markdown). Includes
    -- the risk assessment and the verification command suggestion.
    rationale TEXT NOT NULL,
    -- Voice that proposed the change (one of:
    -- duplicate / complexity / error_handling / modernizer / synthesis).
    -- 'synthesis' means the final synthesized proposal — what the user
    -- normally reviews. The voice ids are also kept for the future
    -- "show me ONLY duplicate-hunter proposals" filter view.
    voice TEXT NOT NULL,
    -- Severity / impact score from 0-100. Used to sort the inbox.
    impact_score INTEGER NOT NULL DEFAULT 50,
    -- Risk: 'low' | 'medium' | 'high'.
    risk TEXT NOT NULL DEFAULT 'medium',
    -- Unified diff to apply. May span multiple hunks. The actuator
    -- accepts unified-diff format directly.
    diff TEXT NOT NULL,
    -- 'untested' (default) | 'verified_pass' | 'verified_fail'.
    -- When auto-test verification lands, this gets updated when the
    -- background worker runs the project's tests against the diff.
    verification_status TEXT NOT NULL DEFAULT 'untested',
    -- Output from the last test run, if any.
    verification_output TEXT,
    -- Lifecycle: 'pending' | 'applied' | 'dismissed'.
    -- 'applied' means the user clicked Apply and the diff was handed
    -- off to the actuator (the actuator's own status tracks what
    -- happens after that). 'dismissed' means the user explicitly
    -- rejected; we keep the row so the same proposal doesn't pop back
    -- up on the next scan.
    status TEXT NOT NULL DEFAULT 'pending',
    -- Source scan run id. Lets the UI group proposals from the same
    -- batch and the next scan dedupe against prior batches.
    scan_run_id TEXT NOT NULL,
    created_at INTEGER NOT NULL DEFAULT (strftime('%s', 'now')),
    resolved_at INTEGER
);

CREATE INDEX IF NOT EXISTS idx_refactor_proposals_status
    ON refactor_proposals(status, created_at DESC);

CREATE INDEX IF NOT EXISTS idx_refactor_proposals_file
    ON refactor_proposals(file_path, status);

-- Scan runs — one row per `refactor_scan` invocation. Lets us track
-- which files were scanned in which run, dedupe candidates, and show
-- "last scan: 14 minutes ago, 23 candidates, 7 proposals."
CREATE TABLE IF NOT EXISTS refactor_scan_runs (
    id TEXT PRIMARY KEY,
    started_at INTEGER NOT NULL DEFAULT (strftime('%s', 'now')),
    completed_at INTEGER,
    candidates_count INTEGER NOT NULL DEFAULT 0,
    proposals_count INTEGER NOT NULL DEFAULT 0,
    -- 'manual' (user clicked the scan button) | 'scheduled'
    -- (background timer). The scheduled scanner is scaffolded but not
    -- yet running — when it lands, this is the discriminator the UI
    -- uses for "auto vs manual" badges in the inbox.
    trigger TEXT NOT NULL DEFAULT 'manual',
    -- Optional file filter — null means full project scan, otherwise
    -- a specific path the user targeted.
    target_path TEXT
);
