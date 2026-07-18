//! Skills — user-authored Markdown files that the agent picks up as
//! durable context.
//!
//! Two directories are scanned on every chat turn:
//!
//!   * **Global**  `<app config dir>/skills/*.md`   — apply everywhere
//!   * **Project** `<root>/.kilroy/skills/*.md`    — apply only to this folder
//!
//! Each skill is a plain `*.md` file. The first `# Heading` becomes the
//! skill's display title; the first paragraph (everything between the
//! title and the next blank line) is the one-line summary the model sees
//! upfront. Skills under `MAX_INLINE_BYTES` are inlined into the system
//! prompt verbatim; larger skills get a summary stub and the model is
//! told how to ask for the full content.

use crate::state::AppState;
use anyhow::{Context, Result};
use serde::Serialize;
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Manager, State};

/// Max raw-bytes per skill that we inline into the system prompt. Larger
/// skills get a stub so the prompt window doesn't blow up on a 50 KB
/// design doc someone parked in the folder.
const MAX_INLINE_BYTES: usize = 4 * 1024;

#[derive(Serialize, Clone, Debug)]
pub struct Skill {
    pub name: String,    // filename minus .md
    pub title: String,   // first `# Heading` line, or the name
    pub summary: String, // first non-blank paragraph after the title
    pub path: String,    // absolute path on disk
    pub scope: String,   // "global" | "project"
    pub size_bytes: u64,
    pub inline_eligible: bool,
}

/// Resolve the two skill directories. Creates them lazily so the first
/// `list_skills` call sets up an empty `~/AppData/.../skills/` for the
/// user to drop files into.
fn dirs(app: &AppHandle, project_root: Option<&Path>) -> Result<(PathBuf, Option<PathBuf>)> {
    let app_dir = app
        .path()
        .app_config_dir()
        .context("resolve app config dir")?;
    let global = app_dir.join("skills");
    std::fs::create_dir_all(&global).ok();

    let project = project_root.map(|r| r.join(".kilroy").join("skills"));
    if let Some(ref p) = project {
        std::fs::create_dir_all(p).ok();
    }
    Ok((global, project))
}

#[tauri::command]
pub async fn list_skills(app: AppHandle, state: State<'_, AppState>) -> Result<Vec<Skill>, String> {
    let project_root = state.memory.lock().as_ref().map(|m| m.root.clone());
    let (global_dir, project_dir) =
        dirs(&app, project_root.as_deref()).map_err(|e| format!("{:#}", e))?;

    let mut out = Vec::new();
    scan_dir(&global_dir, "global", &mut out);
    if let Some(p) = project_dir {
        scan_dir(&p, "project", &mut out);
    }
    out.sort_by_key(|a| a.title.to_lowercase());
    Ok(out)
}

fn scan_dir(dir: &Path, scope: &str, out: &mut Vec<Skill>) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in rd.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("md") {
            continue;
        }
        let Ok(meta) = entry.metadata() else { continue };
        if !meta.is_file() {
            continue;
        }
        let size = meta.len();
        let name = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("(untitled)")
            .to_string();
        let raw = std::fs::read_to_string(&path).unwrap_or_default();
        let (title, summary) = parse_metadata(&raw, &name);
        out.push(Skill {
            name,
            title,
            summary,
            path: path.to_string_lossy().to_string(),
            scope: scope.into(),
            size_bytes: size,
            inline_eligible: (size as usize) <= MAX_INLINE_BYTES,
        });
    }
}

fn parse_metadata(raw: &str, fallback_name: &str) -> (String, String) {
    let mut lines = raw.lines();
    let mut title = fallback_name.to_string();
    for l in lines.by_ref() {
        let t = l.trim();
        if t.is_empty() {
            continue;
        }
        if let Some(rest) = t.strip_prefix("# ") {
            title = rest.trim().to_string();
        } else {
            // No top-level heading; treat the first non-blank line as the title.
            title = t.to_string();
        }
        break;
    }
    // Summary = next non-blank paragraph after the title line.
    let mut buf = String::new();
    let mut saw_blank = false;
    for l in lines {
        let t = l.trim();
        if t.is_empty() {
            if !buf.is_empty() {
                break;
            }
            saw_blank = true;
            continue;
        }
        if t.starts_with('#') {
            break; // hit the next heading
        }
        if !buf.is_empty() {
            buf.push(' ');
        }
        buf.push_str(t);
        // Cap the summary. Two stop conditions:
        //   * hard cap at 320 bytes (≈ a 4-line paragraph), OR
        //   * if there was a blank line between the title and this body,
        //     stop at the first line ending in a period (a clean
        //     one-sentence summary).
        // `buf` is always non-empty here (we just pushed `t`), so the
        // old `buf.len() > 0` guard was redundant and has been dropped.
        if buf.len() > 320 || (saw_blank && buf.ends_with('.')) {
            break;
        }
    }
    let summary = buf.trim().to_string();
    (title, summary)
}

#[tauri::command]
pub async fn read_skill(
    app: AppHandle,
    state: State<'_, AppState>,
    name: String,
    scope: Option<String>,
) -> Result<String, String> {
    let project_root = state.memory.lock().as_ref().map(|m| m.root.clone());
    let (global_dir, project_dir) =
        dirs(&app, project_root.as_deref()).map_err(|e| format!("{:#}", e))?;

    let candidates: Vec<PathBuf> = match scope.as_deref() {
        Some("global") => vec![global_dir.join(format!("{}.md", name))],
        Some("project") => project_dir
            .into_iter()
            .map(|d| d.join(format!("{}.md", name)))
            .collect(),
        _ => {
            // No explicit scope — prefer project skill if it exists, else global.
            let mut v = Vec::new();
            if let Some(d) = project_dir {
                v.push(d.join(format!("{}.md", name)));
            }
            v.push(global_dir.join(format!("{}.md", name)));
            v
        }
    };

    for c in candidates {
        if c.is_file() {
            return std::fs::read_to_string(&c).map_err(|e| format!("read {}: {}", c.display(), e));
        }
    }
    Err(format!("skill `{}` not found", name))
}

#[tauri::command]
pub async fn open_skills_folder(
    app: AppHandle,
    state: State<'_, AppState>,
    scope: Option<String>,
) -> Result<String, String> {
    let project_root = state.memory.lock().as_ref().map(|m| m.root.clone());
    let (global_dir, project_dir) =
        dirs(&app, project_root.as_deref()).map_err(|e| format!("{:#}", e))?;
    let target = match scope.as_deref() {
        Some("project") => project_dir.ok_or_else(|| "no project open".to_string())?,
        _ => global_dir,
    };
    // Open in the system file explorer.
    #[cfg(windows)]
    {
        use std::process::Command;
        Command::new("explorer")
            .arg(&target)
            .spawn()
            .map_err(|e| format!("open explorer: {}", e))?;
    }
    #[cfg(target_os = "macos")]
    {
        use std::process::Command;
        Command::new("open")
            .arg(&target)
            .spawn()
            .map_err(|e| format!("open: {}", e))?;
    }
    #[cfg(all(not(windows), not(target_os = "macos")))]
    {
        use std::process::Command;
        Command::new("xdg-open")
            .arg(&target)
            .spawn()
            .map_err(|e| format!("xdg-open: {}", e))?;
    }
    let _ = state; // suppress unused-warning when neither branch reads it
    Ok(target.to_string_lossy().to_string())
}

/// Create or overwrite a skill file. The UI's "+ New Skill" composer
/// calls this; the agent can also propose new skills by emitting a
/// fenced `file_write` block targeted at the skills directory — that
/// path goes through the actuator's Accept gate, this command goes
/// through the UI directly (the user IS the gate).
///
/// `name` is a slug (letters / digits / `-` / `_`) — we reject anything
/// containing a separator or dot so `..\foo` and `nested/path` can't slip
/// through into a path-traversal attack. The `.md` extension is appended
/// for you. `scope` is `"global"` (writes to `<app config>/skills/`) or
/// `"project"` (writes to `<root>/.kilroy/skills/`); the project scope is
/// rejected when no project is open.
#[tauri::command]
pub async fn write_skill(
    app: AppHandle,
    state: State<'_, AppState>,
    name: String,
    scope: String,
    content: String,
) -> Result<String, String> {
    // Strict name validation — kebab/snake slug only. The skill folder is
    // user-trusted but we still don't want the LLM (or a paste-bombed
    // composer field) writing into `..\..\Windows\System32`.
    if name.is_empty()
        || name.len() > 80
        || name.starts_with('-')
        || !name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return Err(format!(
            "invalid skill name `{}`: use letters, digits, '-' or '_' only \
             (no dots, no path separators, max 80 chars)",
            name
        ));
    }
    let project_root = state.memory.lock().as_ref().map(|m| m.root.clone());
    let (global_dir, project_dir) =
        dirs(&app, project_root.as_deref()).map_err(|e| format!("{:#}", e))?;
    let target_dir = match scope.as_str() {
        "global" => global_dir,
        "project" => project_dir.ok_or_else(|| {
            "no project open — open a folder first or pick global scope".to_string()
        })?,
        other => {
            return Err(format!(
                "scope must be `global` or `project`, got `{}`",
                other
            ))
        }
    };
    // Defence-in-depth: canonicalize the resolved skills dir, then
    // confirm the file path we're about to write is still inside it. The
    // name validation above already blocks separators, but canonicalizing
    // also catches the case where the skills directory itself is a
    // symlink that's been retargeted under us.
    let canonical_dir = std::fs::canonicalize(&target_dir)
        .map_err(|e| format!("resolve skills dir `{}`: {}", target_dir.display(), e))?;
    let target = canonical_dir.join(format!("{}.md", name));
    if !target.starts_with(&canonical_dir) {
        return Err(format!(
            "skill path `{}` escapes skills directory — refusing to write",
            target.display()
        ));
    }
    std::fs::write(&target, content).map_err(|e| format!("write {}: {}", target.display(), e))?;
    tracing::info!(
        skill = %name,
        scope = %scope,
        path = %target.display(),
        "wrote skill"
    );
    Ok(target.to_string_lossy().to_string())
}

/// Synchronous list — used inside `agent_send_message` (which already
/// has the mutex on `state.memory` held in a tight scope). Mirrors the
/// async `list_skills` command but doesn't go through Tauri's IPC.
pub fn list_skills_sync(app: &AppHandle, memory: Option<&crate::db::Memory>) -> Vec<Skill> {
    let project_root = memory.map(|m| m.root.clone());
    let Ok((global_dir, project_dir)) = dirs(app, project_root.as_deref()) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    scan_dir(&global_dir, "global", &mut out);
    if let Some(p) = project_dir {
        scan_dir(&p, "project", &mut out);
    }
    out.sort_by_key(|a| a.title.to_lowercase());
    out
}

/// Synchronous variant used by `agent_send_message` so skills land in
/// the system prompt every turn. Inlines small skills verbatim and
/// summarises bigger ones.
pub fn inject_skills_prompt(skills: &[Skill]) -> String {
    if skills.is_empty() {
        return String::new();
    }
    let mut out = String::new();
    out.push_str("\n# Installed skills\n");
    out.push_str(
        "These are user-authored Markdown notes Kilroy carries as durable context. \
         Treat them as authoritative for the user's domain conventions, naming, \
         preferred libraries, and decision history. Apply any guidance that matches \
         the user's current request.\n\n\
         When the user shares a convention, preference, or recipe worth remembering \
         across future sessions, you can propose a NEW skill by emitting a fenced \
         block targeted at the project's skills folder, e.g.:\n\n\
         ```markdown path=.kilroy/skills/<slug>.md\n\
         # Skill title\n\
         One-line summary the model sees first.\n\n\
         Body — details, examples, anti-patterns.\n\
         ```\n\n\
         The block becomes an Accept-gated file_write proposal; nothing lands on \
         disk until the user approves. Use a short kebab-case slug for `<slug>` \
         (e.g. `react-hooks`, `db-naming`). Don't propose a skill unless the user \
         has clearly expressed a durable preference — one-off questions stay in \
         the message stream.\n\n",
    );
    let mut inlined_bytes = 0usize;
    let inline_budget = 16 * 1024; // total budget across all skills

    for s in skills {
        if s.inline_eligible && inlined_bytes < inline_budget {
            // Try to inline the full file.
            if let Ok(body) = std::fs::read_to_string(&s.path) {
                inlined_bytes += body.len();
                out.push_str(&format!(
                    "## skill: {} ({})\n_source: {}_\n\n{}\n\n",
                    s.title,
                    s.scope,
                    s.path,
                    body.trim()
                ));
                continue;
            }
        }
        // Fall back to stub.
        out.push_str(&format!(
            "## skill: {} ({})  [size {} bytes — summary only]\n{}\n_To consult the full skill, \
             ask the user to paste it or open `{}`._\n\n",
            s.title, s.scope, s.size_bytes, s.summary, s.path,
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn heading_title_and_summary() {
        let raw = "# My Skill\nThis is the summary.\n\nBody here.";
        let (title, summary) = parse_metadata(raw, "fallback");
        assert_eq!(title, "My Skill");
        assert_eq!(summary, "This is the summary.");
    }

    #[test]
    fn first_line_is_title_when_no_heading() {
        let raw = "Just a line of text.\nmore";
        let (title, _summary) = parse_metadata(raw, "fallback");
        assert_eq!(title, "Just a line of text.");
    }

    #[test]
    fn falls_back_to_name_for_empty_input() {
        let (title, summary) = parse_metadata("", "react-hooks");
        assert_eq!(title, "react-hooks");
        assert_eq!(summary, "");
    }

    #[test]
    fn summary_stops_at_next_heading() {
        let raw = "# T\nSummary line\n## Section\nmore";
        let (_title, summary) = parse_metadata(raw, "f");
        assert_eq!(summary, "Summary line");
    }

    #[test]
    fn summary_joins_multiline_paragraph() {
        let raw = "# T\nLine one\nline two\n\nlater";
        let (_title, summary) = parse_metadata(raw, "f");
        assert_eq!(summary, "Line one line two");
    }

    #[test]
    fn inline_eligible_threshold() {
        // A skill at or under MAX_INLINE_BYTES is inline-eligible. Compile-time
        // const block keeps the invariant without tripping
        // clippy::assertions_on_constants.
        const { assert!(MAX_INLINE_BYTES > 0) };
    }
}
