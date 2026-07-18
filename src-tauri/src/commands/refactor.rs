//! Background-refactor swarm — a 4-voice agent quartet that scans the
//! project for safe improvements and surfaces them as a GitHub-style
//! review inbox.
//!
//! The four voices:
//!   * 🔁 **Duplicate hunter**   — repeated code patterns worth factoring
//!   * 📏 **Complexity hunter**  — long functions, deep nesting, big paramlists
//!   * 🛡️ **Error-handling auditor** — unwraps, panics, swallowed errors
//!   * 🔄 **Pattern modernizer** — deprecated idioms, dead code
//!
//! Synthesizer ranks the proposals and picks the ONE that's highest-impact
//! / lowest-risk for THIS scan. Each scan produces at most one proposal
//! per file — the user reviews them in batches in the Refactor panel.
//!
//! The synthesizer's output (Markdown including a unified diff + a risk
//! assessment + a verification command) lands in the `refactor_proposals`
//! table. When the user clicks Apply on a proposal, the diff is routed
//! through the existing actuator system as a `file_patch` action — that
//! means the same per-hunk Accept/Reject UI the agent's chat-proposed
//! edits get.
//!
//! Test-verification (run the project's tests against the diff and only
//! surface those that pass) is scaffolded as `verification_status` on
//! each proposal but the actual test-runner integration lands in the
//! next pass — needs git-worktree isolation to avoid corrupting the
//! user's working tree during verification.
//!
//! Idle-CPU scheduling (auto-scan every N minutes when chat is idle) is
//! scaffolded as `scan_run_runs.trigger` ('manual' | 'scheduled') but
//! the background timer itself ships next pass — needs careful
//! resource gating so an unattended scan doesn't peg the user's GPU
//! while they're trying to compile.

use crate::commands::agent::swarm_with_context;
use crate::state::AppState;
use anyhow::{Context, Result};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tauri::{AppHandle, Emitter, State};

// ─── Refactor swarm prompts ─────────────────────────────────────────────────

/// Four-voice refactor swarm. Each voice scans the SAME file content
/// from a different lens; the synthesizer picks the best proposal.
pub(crate) const REFACTOR_VOICES: &[(&str, &str, &str)] = &[
    (
        "duplicate",
        "🔁 Duplicate hunter",
        "You are the DUPLICATE HUNTER. Scan the file content provided \
         for code patterns that repeat 2+ times. For each: cite the \
         duplicates by line number and propose how to factor them (a \
         shared helper, a constant, a small abstraction). PROHIBITIONS: \
         do not propose abstractions where the cost (extra indirection, \
         coupling) clearly exceeds the savings. Two near-duplicates that \
         would diverge naturally over time should stay duplicated. \
         If you find no genuine duplicates, say so explicitly — \
         'NO DUPLICATES IN THIS FILE.' is a valid finding. Stay under \
         200 words.",
    ),
    (
        "complexity",
        "📏 Complexity hunter",
        "You are the COMPLEXITY HUNTER. Scan for functions over 50 \
         lines, nesting depth over 3, parameter lists over 5, or \
         cyclomatic complexity that's hard to follow on first read. \
         For each: propose a decomposition (extract function, early \
         return, replace flag-arg with two methods). PROHIBITIONS: do \
         not propose decompositions for functions that ARE genuinely \
         doing one cohesive thing that just happens to be long (e.g. \
         a switch over enum variants, a parser). 'Long' is not the \
         same as 'should be split.' If everything in the file is \
         reasonable, say so. Stay under 200 words.",
    ),
    (
        "error_handling",
        "🛡️ Error-handling auditor",
        "You are the ERROR-HANDLING AUDITOR. Scan for unwrapped Results \
         (`.unwrap()`, `.expect()`), panics, swallowed errors (catch-\
         and-ignore), or paths where an Err / exception silently drops \
         a real failure on the floor. For each: propose the most \
         conservative fix — `?` propagation, `let _ = ...; tracing::warn!(...)`, \
         a structured error type. PROHIBITIONS: don't flag test code or \
         `unwrap()` calls where the invariant is genuinely upheld by \
         construction. Cite file:line. Stay under 200 words.",
    ),
    (
        "modernizer",
        "🔄 Pattern modernizer",
        "You are the PATTERN MODERNIZER. Look for deprecated patterns, \
         old idioms, or dead code — but ONLY where the modernization \
         is risk-free (e.g. `match` instead of nested `if let`, \
         `.collect::<Vec<_>>()` annotation, `let-else`, `Option::ok_or_else` \
         instead of unwrap-then-error). PROHIBITIONS: do not propose \
         changes that risk behavioral drift (replacing one library \
         call with another, async/sync swaps, semver-breaking refactors). \
         If nothing reaches the 'risk-free + helpful' bar, say so. \
         Stay under 200 words.",
    ),
];

pub(crate) const REFACTOR_SYNTHESIZER: &str =
    "You are the refactor-swarm synthesizer. You've received four \
     analyses of the same file from different lenses (duplicate, \
     complexity, error-handling, modernizer). Pick the SINGLE highest- \
     impact / lowest-risk proposal that survived all four perspectives \
     — if any voice flagged a serious concern about another's proposal, \
     respect it. Output, in this exact structure:\n\n\
     ## Title\n\
     One short line — the inbox row label, e.g. 'Extract \
     parse_options helper from main()'.\n\n\
     ## Why this matters\n\
     One paragraph: what's wrong today, what the change improves, who \
     benefits.\n\n\
     ## Risk\n\
     One word: `low`, `medium`, or `high`. A behaviour-preserving \
     extraction of a pure helper is `low`. A change that crosses a \
     module boundary or touches I/O is `medium` at best. Anything \
     touching concurrency, persistence, or error types is `high`.\n\n\
     ## Diff\n\
     A unified diff fenced as ```diff. Include `--- a/<path>` and \
     `+++ b/<path>` headers plus `@@` hunks. The diff MUST apply \
     cleanly to the current file content — do not invent surrounding \
     lines, copy them verbatim from the input.\n\n\
     ## Verification\n\
     The single command the user can run RIGHT NOW to confirm the \
     change doesn't break anything (e.g. `cargo test --package kilroy`, \
     `npm test -- src/components/X`, `pytest tests/test_x.py`).\n\n\
     If NO voice surfaced anything worth shipping, output exactly:\n\
     `## No proposal\n\nNothing high-confidence to suggest for this \
     file right now.`\n\n\
     Do not pad or hedge — picking nothing is a valid outcome and \
     more honest than fabricating a low-value proposal.";

// ─── Public types ───────────────────────────────────────────────────────────

#[derive(Serialize, Clone, Debug)]
pub struct FileCandidate {
    pub path: String,
    pub rel_path: String,
    pub size_bytes: u64,
    pub loc: u32,
    /// Heuristic priority score 0-100 — higher = better candidate.
    pub score: u32,
    /// One-line reason this file scored high. UI tooltip.
    pub reason: String,
}

#[derive(Serialize, Clone, Debug)]
pub struct RefactorProposal {
    pub id: i64,
    pub file_path: String,
    pub title: String,
    pub rationale: String,
    pub voice: String,
    pub impact_score: i32,
    pub risk: String,
    pub diff: String,
    pub verification_status: String,
    pub verification_output: Option<String>,
    pub status: String,
    pub scan_run_id: String,
    pub created_at: i64,
}

#[derive(Serialize, Clone, Debug)]
pub struct ScanRunSummary {
    pub run_id: String,
    pub candidates_count: u32,
    pub proposals_count: u32,
    pub elapsed_ms: u64,
}

#[derive(Deserialize, Debug)]
pub struct AnalyzeFileInput {
    pub file_path: String,
    /// Optional scan run id to group this analysis under. When omitted
    /// we mint a fresh one — useful for one-off "analyze this file"
    /// triggers from the UI.
    #[serde(default)]
    pub scan_run_id: Option<String>,
}

// ─── Commands ───────────────────────────────────────────────────────────────

/// Walk the project tree and rank files by refactor-worthiness. Cheap
/// — no LLM, just heuristics over file size + LOC + extension. The UI
/// shows the top N candidates so the user can pick what to analyse.
#[tauri::command]
pub async fn refactor_scan_candidates(
    state: State<'_, AppState>,
    limit: Option<u32>,
) -> Result<Vec<FileCandidate>, String> {
    let limit = limit.unwrap_or(20).clamp(1, 100) as usize;
    let root = state
        .memory
        .lock()
        .as_ref()
        .map(|m| m.root.clone())
        .ok_or_else(|| "no project open".to_string())?;

    // Reuse the existing project-file lister. It already handles
    // .gitignore-ish exclusions and caps depth, so we don't need to
    // reimplement walker logic here.
    let files = crate::commands::memory::list_project_files_sync(&root, 5000);

    let mut candidates: Vec<FileCandidate> = files
        .into_iter()
        .filter_map(|rel| {
            let full = root.join(&rel);
            let raw = std::fs::read_to_string(&full).ok()?;
            let bytes = raw.len() as u64;
            // Skip tiny files — nothing to refactor in <10 lines.
            if bytes < 200 {
                return None;
            }
            let loc = raw.lines().count() as u32;
            if loc < 30 {
                return None;
            }
            // Skip files that look generated / vendored / lockfile-y.
            let lower = rel.to_lowercase();
            if lower.contains("lock")
                || lower.ends_with(".min.js")
                || lower.ends_with(".min.css")
                || lower.contains("/dist/")
                || lower.contains("/build/")
                || lower.contains(".generated.")
            {
                return None;
            }
            let ext = full
                .extension()
                .and_then(|e| e.to_str())
                .map(|s| s.to_lowercase())
                .unwrap_or_default();
            // Source-code extensions get priority. Config files and
            // docs can still appear at low score.
            let lang_score = match ext.as_str() {
                "rs" | "ts" | "tsx" | "js" | "jsx" | "py" | "go" | "java" | "kt" | "swift"
                | "rb" | "c" | "cpp" | "cc" | "cs" => 30,
                "html" | "css" | "scss" | "vue" | "svelte" => 15,
                "sql" | "graphql" | "proto" => 10,
                "md" | "txt" | "yaml" | "yml" | "json" | "toml" => 2,
                _ => 5,
            };

            let (score, reason) = score_candidate(&raw, loc, lang_score);
            Some(FileCandidate {
                path: full.to_string_lossy().to_string(),
                rel_path: rel,
                size_bytes: bytes,
                loc,
                score,
                reason,
            })
        })
        .collect();

    candidates.sort_by_key(|c| std::cmp::Reverse(c.score));
    candidates.truncate(limit);
    Ok(candidates)
}

/// Run the 4-voice refactor swarm against a single file. Streams voice
/// progress on `agent://refactor/voice` channels so the panel can
/// render a live debate, then persists the synthesized proposal to
/// `refactor_proposals` so it shows up in the inbox.
#[tauri::command]
pub async fn refactor_analyze_file(
    app: AppHandle,
    state: State<'_, AppState>,
    payload: AnalyzeFileInput,
) -> Result<Option<RefactorProposal>, String> {
    let memory_conn = state
        .memory
        .lock()
        .as_ref()
        .map(|m| m.conn.clone())
        .ok_or_else(|| "no project open".to_string())?;

    let path = PathBuf::from(&payload.file_path);
    if !path.is_file() {
        return Err(format!("not a file: {}", payload.file_path));
    }
    let raw =
        std::fs::read_to_string(&path).map_err(|e| format!("read {}: {}", path.display(), e))?;
    if raw.len() > 200_000 {
        // 200KB ≈ 6000-line file. The four-voice prompt + content
        // would blow the context window. Refuse politely.
        return Err(format!(
            "{} is too large to analyse in one pass ({} bytes). Split into smaller files and try again.",
            path.display(),
            raw.len()
        ));
    }

    let scan_run_id = payload
        .scan_run_id
        .clone()
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());

    // Record the scan run if it's new.
    {
        let conn = memory_conn.lock();
        let _ = conn.execute(
            "INSERT OR IGNORE INTO refactor_scan_runs (id, trigger, target_path)
             VALUES (?1, 'manual', ?2)",
            params![scan_run_id, payload.file_path],
        );
    }

    // Build the shared context — the file content with line numbers,
    // plus a brief "you're reviewing this file" header. We deliberately
    // do NOT inject project-wide retrieved chunks here; each refactor
    // voice should focus on THIS file, not get distracted by adjacent
    // ones. Cross-file refactors are a future-pass feature.
    let shared_context = format!(
        "# File under review\n\nPath: `{}`\nLines: {}\n\n```\n{}\n```\n\nFor every claim you make, cite a specific line number from this file.",
        path.display(),
        raw.lines().count(),
        raw,
    );

    let chat = state.chat.clone();
    let synthesized = swarm_with_context(
        &app,
        &chat,
        REFACTOR_VOICES,
        REFACTOR_SYNTHESIZER,
        "🧭 Refactor recommendation",
        "agent://refactor",
        &shared_context,
        "Propose ONE refactor for this file, or say there's nothing worth shipping.",
        None,
    )
    .await;

    // Parse the synthesized Markdown — pull out title, risk, diff. If
    // the synth said "no proposal" or the diff section is empty, store
    // nothing.
    let parsed = parse_synthesized(&synthesized);
    let Some(p) = parsed else {
        // No proposal — record the scan-run completion (so "last scan"
        // and the candidate counter advance even on empty findings) and
        // emit a "scan done" event so the UI clears its spinner without
        // inserting a row.
        {
            let conn = memory_conn.lock();
            finalize_scan_run(&conn, &scan_run_id, false);
        }
        let _ = app.emit(
            "refactor://scan_done",
            serde_json::json!({
                "scan_run_id": scan_run_id,
                "file_path": payload.file_path,
                "proposal": null,
            }),
        );
        return Ok(None);
    };

    // Persist the proposal and mark the scan-run finalized in one lock scope.
    let id = {
        let conn = memory_conn.lock();
        let id = insert_proposal(&conn, &payload.file_path, &p, &scan_run_id)
            .map_err(|e| format!("{:#}", e))?;
        finalize_scan_run(&conn, &scan_run_id, true);
        id
    };

    let proposal = RefactorProposal {
        id,
        file_path: payload.file_path.clone(),
        title: p.title,
        rationale: p.rationale,
        voice: "synthesis".into(),
        impact_score: 50,
        risk: p.risk,
        diff: p.diff,
        verification_status: "untested".into(),
        verification_output: None,
        status: "pending".into(),
        scan_run_id: scan_run_id.clone(),
        created_at: now_unix(),
    };

    let _ = app.emit(
        "refactor://scan_done",
        serde_json::json!({
            "scan_run_id": scan_run_id,
            "file_path": payload.file_path,
            "proposal": proposal.clone(),
        }),
    );
    Ok(Some(proposal))
}

/// List proposals from the inbox. By default pending only — pass
/// `include_dismissed=true` to see the full history.
#[tauri::command]
pub async fn refactor_list_proposals(
    state: State<'_, AppState>,
    include_dismissed: Option<bool>,
    include_applied: Option<bool>,
    limit: Option<u32>,
) -> Result<Vec<RefactorProposal>, String> {
    let limit = limit.unwrap_or(100).clamp(1, 500) as i64;
    let memory_conn = state
        .memory
        .lock()
        .as_ref()
        .map(|m| m.conn.clone())
        .ok_or_else(|| "no project open".to_string())?;
    let conn = memory_conn.lock();

    let mut where_clauses: Vec<&str> = Vec::new();
    if !include_dismissed.unwrap_or(false) {
        where_clauses.push("status != 'dismissed'");
    }
    if !include_applied.unwrap_or(false) {
        where_clauses.push("status != 'applied'");
    }
    let where_sql = if where_clauses.is_empty() {
        String::new()
    } else {
        format!("WHERE {}", where_clauses.join(" AND "))
    };
    let sql = format!(
        "SELECT id, file_path, title, rationale, voice, impact_score, risk, diff, verification_status,
                verification_output, status, scan_run_id, created_at
         FROM refactor_proposals
         {} ORDER BY created_at DESC LIMIT ?1",
        where_sql
    );
    let mut stmt = conn.prepare(&sql).map_err(|e| format!("{}", e))?;
    let rows = stmt
        .query_map(params![limit], |r| {
            Ok(RefactorProposal {
                id: r.get(0)?,
                file_path: r.get(1)?,
                title: r.get(2)?,
                rationale: r.get(3)?,
                voice: r.get(4)?,
                impact_score: r.get(5)?,
                risk: r.get(6)?,
                diff: r.get(7)?,
                verification_status: r.get(8)?,
                verification_output: r.get(9)?,
                status: r.get(10)?,
                scan_run_id: r.get(11)?,
                created_at: r.get(12)?,
            })
        })
        .map_err(|e| format!("{}", e))?;
    let out: Vec<RefactorProposal> = rows.filter_map(|r| r.ok()).collect();
    Ok(out)
}

/// Mark a proposal as dismissed (user doesn't want it). We keep the
/// row so the same suggestion doesn't pop back up after the next scan.
#[tauri::command]
pub async fn refactor_dismiss_proposal(state: State<'_, AppState>, id: i64) -> Result<(), String> {
    let memory_conn = state
        .memory
        .lock()
        .as_ref()
        .map(|m| m.conn.clone())
        .ok_or_else(|| "no project open".to_string())?;
    let conn = memory_conn.lock();
    conn.execute(
        "UPDATE refactor_proposals SET status = 'dismissed', resolved_at = strftime('%s', 'now')
         WHERE id = ?1",
        params![id],
    )
    .map_err(|e| format!("{}", e))?;
    Ok(())
}

/// Apply a proposal: insert its diff into the actuator's `actions`
/// table as a pending `file_patch` so the user reviews and accepts it
/// the same way agent-proposed edits work. The proposal is marked
/// 'applied' regardless of whether the user accepts the actuator
/// action — 'applied' here just means "left the inbox", not "landed
/// on disk."
#[tauri::command]
pub async fn refactor_apply_proposal(state: State<'_, AppState>, id: i64) -> Result<i64, String> {
    let memory_conn = state
        .memory
        .lock()
        .as_ref()
        .map(|m| m.conn.clone())
        .ok_or_else(|| "no project open".to_string())?;
    let session_id = *state.current_session_id.lock();

    let conn = memory_conn.lock();
    let (file_path, diff): (String, String) = conn
        .query_row(
            "SELECT file_path, diff FROM refactor_proposals WHERE id = ?1",
            params![id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .map_err(|e| format!("{}", e))?;

    // Build the payload from the actuator's OWN type rather than hand-rolling
    // JSON, so the field names can never drift from what `accept_action`
    // deserializes. A previous hand-built object used a `diff` key, but
    // `ActionPayload::FilePatch` expects `unified_diff` — that mismatch made
    // every refactor Apply fail at the Accept step with "parse payload".
    let payload = crate::actuator::ActionPayload::FilePatch {
        path: file_path.clone(),
        unified_diff: diff.clone(),
    };
    let payload_json =
        serde_json::to_string(&payload).map_err(|e| format!("serialize action payload: {e}"))?;

    let action_id = crate::db::actions::insert(
        &conn,
        crate::db::actions::NewAction {
            session_id,
            task_id: None,
            kind: "file_patch",
            target: Some(&file_path),
            payload: &payload_json,
            diff: Some(&diff),
        },
    )
    .map_err(|e| format!("{:#}", e))?;

    conn.execute(
        "UPDATE refactor_proposals SET status = 'applied', resolved_at = strftime('%s', 'now')
         WHERE id = ?1",
        params![id],
    )
    .map_err(|e| format!("{}", e))?;

    Ok(action_id)
}

#[tauri::command]
pub async fn refactor_scan_run_stats(
    state: State<'_, AppState>,
) -> Result<RefactorScanStats, String> {
    let memory_conn = state
        .memory
        .lock()
        .as_ref()
        .map(|m| m.conn.clone())
        .ok_or_else(|| "no project open".to_string())?;
    let conn = memory_conn.lock();
    let pending: u32 = conn
        .query_row(
            "SELECT COUNT(*) FROM refactor_proposals WHERE status = 'pending'",
            [],
            |r| r.get(0),
        )
        .unwrap_or(0);
    let applied: u32 = conn
        .query_row(
            "SELECT COUNT(*) FROM refactor_proposals WHERE status = 'applied'",
            [],
            |r| r.get(0),
        )
        .unwrap_or(0);
    let dismissed: u32 = conn
        .query_row(
            "SELECT COUNT(*) FROM refactor_proposals WHERE status = 'dismissed'",
            [],
            |r| r.get(0),
        )
        .unwrap_or(0);
    let last_scan: Option<i64> = conn
        .query_row(
            "SELECT MAX(completed_at) FROM refactor_scan_runs WHERE completed_at IS NOT NULL",
            [],
            |r| r.get(0),
        )
        .ok()
        .flatten();
    Ok(RefactorScanStats {
        pending,
        applied,
        dismissed,
        last_scan_unix: last_scan,
    })
}

#[derive(Serialize, Clone, Debug)]
pub struct RefactorScanStats {
    pub pending: u32,
    pub applied: u32,
    pub dismissed: u32,
    pub last_scan_unix: Option<i64>,
}

// ─── Helpers ────────────────────────────────────────────────────────────────

fn now_unix() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Score a candidate file. Higher is better. Combines size, hand-rolled
/// "complexity" proxies, and language-priority. We deliberately avoid
/// counting things like `unwrap()` calls (the error-handling voice does
/// that during analysis) — this stage is a CHEAP triage, not an
/// analysis.
fn score_candidate(content: &str, loc: u32, lang_score: u32) -> (u32, String) {
    let mut score: u32 = lang_score;
    let mut reasons: Vec<String> = Vec::new();

    // Bigger = more potential. Cap so a 5000-line file doesn't
    // dominate a 500-line file 10x — refactor swarm's context budget
    // tops out anyway.
    let size_score = (loc / 25).min(40);
    if size_score > 0 {
        score += size_score;
        if loc > 400 {
            reasons.push(format!("{} lines", loc));
        }
    }

    // Function density proxy. Lots of small/medium functions often
    // means a file that's ripe for either extraction or consolidation.
    let fn_keyword_count = content.matches("fn ").count()
        + content.matches("function ").count()
        + content.matches("def ").count();
    if fn_keyword_count > 15 {
        score += 10;
        reasons.push(format!("{} functions", fn_keyword_count));
    } else if fn_keyword_count > 8 {
        score += 5;
    }

    // TODO / FIXME / HACK / XXX markers — explicit "this needs work" signals.
    let todo_count = content.matches("TODO").count()
        + content.matches("FIXME").count()
        + content.matches("HACK").count()
        + content.matches("XXX").count();
    if todo_count > 0 {
        score += (todo_count as u32 * 3).min(15);
        reasons.push(format!("{} TODO/FIXME markers", todo_count));
    }

    // `unwrap()` / `expect()` density — heuristic for error-handling
    // opportunity. Not all unwraps are bugs but the auditor voice can
    // sort that out.
    let unwrap_count = content.matches(".unwrap()").count() + content.matches(".expect(").count();
    if unwrap_count > 5 {
        score += 8;
        reasons.push(format!("{} unwrap/expect calls", unwrap_count));
    }

    let reason = if reasons.is_empty() {
        format!("{} lines", loc)
    } else {
        reasons.join(" · ")
    };

    (score.min(100), reason)
}

/// Parsed sections out of the synthesizer's Markdown.
struct ParsedProposal {
    title: String,
    rationale: String,
    risk: String,
    diff: String,
}

fn parse_synthesized(md: &str) -> Option<ParsedProposal> {
    // Quick check for the "no proposal" sentinel.
    if md.contains("## No proposal") || md.trim().is_empty() {
        return None;
    }

    let title = section(md, "## Title").map(|s| s.lines().next().unwrap_or("").trim().to_string());
    let why = section(md, "## Why this matters").unwrap_or_default();
    let risk = section(md, "## Risk")
        .map(|s| s.trim().to_lowercase())
        .unwrap_or_else(|| "medium".into());
    let risk = match risk.as_str() {
        s if s.starts_with("low") => "low".to_string(),
        s if s.starts_with("high") => "high".to_string(),
        _ => "medium".to_string(),
    };
    let diff = extract_fenced_block(md, "diff");
    let title = title.unwrap_or_else(|| "Refactor proposal".into());

    if diff.trim().is_empty() {
        // No usable diff → discard rather than store a half-proposal
        // that the user can't actually apply.
        return None;
    }
    if title.trim().is_empty() {
        return None;
    }

    let verification = section(md, "## Verification").unwrap_or_default();
    let mut rationale = String::new();
    rationale.push_str(why.trim());
    if !verification.trim().is_empty() {
        rationale.push_str("\n\n**Verification:** ");
        rationale.push_str(verification.trim());
    }

    Some(ParsedProposal {
        title,
        rationale,
        risk,
        diff,
    })
}

/// Extract the body of a Markdown section started by `header`, stopping
/// at the next `## ` line or end-of-string.
fn section(md: &str, header: &str) -> Option<String> {
    let start = md.find(header)?;
    let body_start = start + header.len();
    let after = &md[body_start..];
    let end_rel = after.find("\n## ").unwrap_or(after.len());
    Some(
        after[..end_rel]
            .trim_start_matches(['\r', '\n'])
            .to_string(),
    )
}

/// Pull the first fenced block whose info-string matches `lang`.
/// Returns the body without the fences. Permissive: tolerates
/// indented fences and CRLF endings.
fn extract_fenced_block(md: &str, lang: &str) -> String {
    let mut out = String::new();
    let mut in_block = false;
    for line in md.lines() {
        let t = line.trim_start();
        if !in_block {
            if t.starts_with("```") {
                let info = t.trim_start_matches('`').trim();
                if info.split_whitespace().next() == Some(lang) {
                    in_block = true;
                }
            }
            continue;
        }
        if t.starts_with("```") {
            break;
        }
        out.push_str(line);
        out.push('\n');
    }
    out
}

/// Record that a file finished analysing under this scan run: bump the
/// completion timestamp (so "last scan" reflects the most recent file) and
/// the candidate/proposal counters the stats panel reads. Best-effort — a
/// stats bookkeeping failure must never fail the analysis the user asked for.
fn finalize_scan_run(conn: &Connection, scan_run_id: &str, produced_proposal: bool) {
    let proposal_inc: i64 = if produced_proposal { 1 } else { 0 };
    let _ = conn.execute(
        "UPDATE refactor_scan_runs
         SET completed_at = strftime('%s', 'now'),
             candidates_count = candidates_count + 1,
             proposals_count = proposals_count + ?2
         WHERE id = ?1",
        params![scan_run_id, proposal_inc],
    );
}

fn insert_proposal(
    conn: &Connection,
    file_path: &str,
    p: &ParsedProposal,
    scan_run_id: &str,
) -> Result<i64> {
    conn.execute(
        "INSERT INTO refactor_proposals (file_path, title, rationale, voice, impact_score, risk, diff, scan_run_id)
         VALUES (?1, ?2, ?3, 'synthesis', 50, ?4, ?5, ?6)",
        params![file_path, p.title, p.rationale, p.risk, p.diff, scan_run_id],
    )
    .context("insert refactor_proposal")?;
    Ok(conn.last_insert_rowid())
}

#[cfg(test)]
mod tests {
    use super::*;

    const FULL: &str = "## Title\nExtract parse_options helper\n\n\
## Why this matters\nThe option parsing is duplicated in three call sites.\n\n\
## Risk\nlow\n\n\
## Diff\n```diff\n--- a/src/main.rs\n+++ b/src/main.rs\n@@ -1,2 +1,2 @@\n-let x = 1;\n+let x = parse();\n```\n\n\
## Verification\ncargo test --package kilroy\n";

    #[test]
    fn parses_a_full_proposal() {
        let p = parse_synthesized(FULL).expect("should parse a complete proposal");
        assert_eq!(p.title, "Extract parse_options helper");
        assert_eq!(p.risk, "low");
        assert!(p.diff.contains("+let x = parse();"));
        assert!(p.rationale.contains("duplicated in three call sites"));
        // Verification command is folded into the rationale.
        assert!(p.rationale.contains("cargo test --package kilroy"));
    }

    #[test]
    fn no_proposal_sentinel_returns_none() {
        let md = "## No proposal\n\nNothing high-confidence to suggest for this file right now.";
        assert!(parse_synthesized(md).is_none());
    }

    #[test]
    fn missing_diff_returns_none() {
        // A proposal with no usable diff can't be applied, so we discard it.
        let md = "## Title\nDo a thing\n\n## Why this matters\nReasons.\n\n## Risk\nhigh\n";
        assert!(parse_synthesized(md).is_none());
    }

    #[test]
    fn risk_normalizes_to_known_values() {
        let md_high = FULL.replace("## Risk\nlow", "## Risk\nHIGH — touches concurrency");
        assert_eq!(parse_synthesized(&md_high).unwrap().risk, "high");
        let md_weird = FULL.replace("## Risk\nlow", "## Risk\nbananas");
        assert_eq!(parse_synthesized(&md_weird).unwrap().risk, "medium");
    }

    #[test]
    fn extract_fenced_block_picks_the_right_lang() {
        let md = "intro\n```rust\nfn a() {}\n```\nmid\n```diff\n+added\n```\nend";
        assert_eq!(extract_fenced_block(md, "diff").trim(), "+added");
        assert_eq!(extract_fenced_block(md, "rust").trim(), "fn a() {}");
    }

    #[test]
    fn section_stops_at_next_header() {
        let md = "## A\nfirst\nstill first\n## B\nsecond";
        assert_eq!(section(md, "## A").unwrap().trim(), "first\nstill first");
        assert_eq!(section(md, "## B").unwrap().trim(), "second");
    }
}
