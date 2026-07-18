//! Actuator — turns model text into reviewable side-effects.
//!
//! The executor produces task outputs (markdown + code). The actuator
//! parses those outputs into typed `Action`s (file writes, unified-diff
//! patches, shell commands), queues each as `pending` in the `actions`
//! table, and emits an event so the UI can render an Accept/Reject card
//! inline.
//!
//! Nothing in this module touches disk or spawns processes on its own.
//! `apply_*` helpers are called only from `commands::actions::accept_action`,
//! after the user has confirmed.

pub mod parser;
pub mod sandbox;

use anyhow::{anyhow, Context, Result};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::sync::LazyLock as Lazy;

use parser::Block;

/// Pre-compiled destructive-shell pattern. Combines 8 word-boundary patterns
/// from the original code into a single alternation so the regex engine
/// makes one pass instead of eight. Compiled once at first use via Lazy.
static VENV_DESTRUCTIVE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(
        r"(?x)
        \b(remove-item\s+-recurse | remove-item | rm\s+-rf | rm\s+-r |
           rmtree | shutil\.rmtree | rd\s+/s | rmdir\s+/s)\b
    ",
    )
    .expect("VENV_DESTRUCTIVE pattern compiles")
});

static GIT_CLEAN: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"\bgit\s+clean\b").expect("GIT_CLEAN pattern compiles"));

static VENV_MENTIONS: Lazy<Vec<Regex>> = Lazy::new(|| {
    [".venv", "venv", r"\venv", "/venv"]
        .iter()
        .map(|v| {
            let escaped = regex::escape(v.trim_start_matches('\\'));
            Regex::new(&format!(r"\b{}\b", escaped)).expect("venv mention pattern compiles")
        })
        .collect()
});

/// Destructive-command patterns applied only after a venv mention is detected.
/// One alternation regex keeps the engine in a single pass.
static DESTRUCTIVE_COMMANDS: Lazy<Regex> = Lazy::new(|| {
    Regex::new(
        r"(?x)
        \b(remove-item\s+-recurse | remove-item | rm\s+-rf | rm\s+-r |
           rmtree | shutil\.rmtree | rd\s+/s | rmdir\s+/s)\b
    ",
    )
    .expect("DESTRUCTIVE_COMMANDS pattern compiles")
});

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ActionPayload {
    /// Replace the entire file (or create it). Used for brand-new files.
    FileWrite {
        path: String,
        content: String,
        language: Option<String>,
    },
    /// Apply a unified diff to an existing file. Used for edits.
    FilePatch { path: String, unified_diff: String },
    /// Run a shell command in the chosen sandbox.
    Shell {
        command: String,
        sandbox: SandboxKind,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum SandboxKind {
    /// Run in the current user's PowerShell — fastest, no isolation.
    Host,
    /// Run inside Windows Sandbox (`WindowsSandbox.exe` with a generated
    /// `.wsb` config). The default for any non-trivial command.
    #[default]
    WindowsSandbox,
    /// Run inside a disposable Docker container with the project mounted at
    /// `/work`. Cross-platform escape hatch for users who prefer container
    /// isolation (or aren't on Windows). Image defaults to
    /// `debian:stable-slim`, overridable via `KILROY_DOCKER_IMAGE`.
    Docker,
}

/// Convert a parsed Block into a normalised action payload.
pub fn payload_from_block(block: &Block) -> Option<ActionPayload> {
    if let Some(path) = block.path.clone() {
        // diff / patch language hint → FilePatch. Anything else with a path → FileWrite.
        if matches!(block.lang.as_deref(), Some("diff") | Some("patch")) {
            return Some(ActionPayload::FilePatch {
                path,
                unified_diff: block.body.clone(),
            });
        }
        return Some(ActionPayload::FileWrite {
            path,
            content: block.body.clone(),
            language: block.lang.clone(),
        });
    }
    let lang = block.lang.as_deref().unwrap_or("");
    if matches!(
        lang,
        "sh" | "bash" | "shell" | "zsh" | "powershell" | "pwsh" | "cmd" | "batch"
    ) {
        return Some(ActionPayload::Shell {
            command: block.body.clone(),
            sandbox: SandboxKind::default(),
        });
    }
    None
}

/// Best-effort unified diff between the on-disk contents of `path` and
/// the proposed new contents. Returns None if the path is invalid (e.g.
/// would escape the project root), the file doesn't exist, or can't be
/// read — the caller renders the proposal as a brand-new file.
pub fn diff_against_disk(root: &std::path::Path, path: &str, new_content: &str) -> Option<String> {
    let abs = resolve_safe(root, path).ok()?;
    let current = std::fs::read_to_string(&abs).ok()?;
    let diff = similar::TextDiff::from_lines(&current, new_content);
    let unified = diff
        .unified_diff()
        .header(&format!("a/{}", path), &format!("b/{}", path))
        .context_radius(3)
        .to_string();
    Some(unified)
}

/// True when a relative path targets the project virtualenv (`.venv` / `venv`).
pub fn is_protected_path(path: &str) -> bool {
    let normalised = path.replace('\\', "/").to_lowercase();
    normalised
        .split('/')
        .filter(|s| !s.is_empty())
        .any(|seg| seg == ".venv" || seg == "venv")
}

/// True when a shell command would delete or wipe the project virtualenv.
///
/// Uses word-boundary regex matching so that a destructive pattern like
/// `rm -rf` is only flagged when it appears as a real command argument,
/// not when it is embedded inside a quoted string or comment (e.g.
/// `echo "just run rm -rf to start over"`) — false positives of that
/// sort would block every safe `echo` or `Write-Host` call.
pub fn shell_targets_protected(command: &str) -> bool {
    let lower = command.to_lowercase();
    if GIT_CLEAN.is_match(&lower) {
        return true;
    }
    let mentions_venv = VENV_MENTIONS.iter().any(|re| re.is_match(&lower));
    if !mentions_venv {
        return false;
    }
    DESTRUCTIVE_COMMANDS.is_match(&lower)
}

/// Apply a FileWrite to disk. Returns the absolute path written.
///
/// Path is resolved + canonicalised through `resolve_safe` so the LLM
/// cannot escape the project root via absolute paths or `..` segments.
pub fn apply_file_write(
    root: &std::path::Path,
    path: &str,
    content: &str,
) -> Result<std::path::PathBuf> {
    if is_protected_path(path) {
        return Err(anyhow!("protected path: project virtualenv ({path})"));
    }
    let abs = resolve_safe(root, path)?;
    if let Some(parent) = abs.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&abs, content)?;
    Ok(abs)
}

/// Apply a unified diff to a file on disk. Returns the absolute path patched.
///
/// We accept an `override_diff` so the caller (UI) can pass a subset of
/// the originally-proposed hunks — that's the per-hunk Accept/Reject path.
/// If `override_diff` is `None`, we apply the diff stored on the action.
pub fn apply_file_patch(
    root: &std::path::Path,
    path: &str,
    unified_diff: &str,
) -> Result<std::path::PathBuf> {
    if is_protected_path(path) {
        return Err(anyhow!("protected path: project virtualenv ({path})"));
    }
    let abs = resolve_safe(root, path)?;
    if !unified_diff.contains("@@") {
        return Err(anyhow!(
            "input does not look like a unified diff (no hunk headers): {}",
            unified_diff.chars().take(80).collect::<String>()
        ));
    }
    let original =
        std::fs::read_to_string(&abs).with_context(|| format!("read {}", abs.display()))?;
    let patch =
        diffy::Patch::from_str(unified_diff).map_err(|e| anyhow!("parse unified diff: {}", e))?;
    let patched =
        diffy::apply(&original, &patch).map_err(|e| anyhow!("apply unified diff: {}", e))?;
    std::fs::write(&abs, patched).with_context(|| format!("write {}", abs.display()))?;
    Ok(abs)
}

/// Resolve a (possibly relative, possibly LLM-supplied) path against the
/// project root, with strict containment checks.
///
/// Rejects:
///   * Absolute paths (`C:\Windows\System32\...`, `/etc/passwd`) — the
///     LLM has no business writing outside the project root.
///   * Relative paths that resolve outside the project root after `..`
///     resolution (`../../AppData/Roaming/...`).
///   * Paths containing NUL bytes.
///
/// This is the path-traversal guard for every file write the actuator
/// performs. It runs AFTER the user has clicked Accept, so the user is
/// already on the hook for the action — but the LLM doesn't get to
/// silently redirect that consent to a totally different file.
pub(crate) fn resolve_safe(root: &std::path::Path, path: &str) -> Result<std::path::PathBuf> {
    if path.is_empty() {
        return Err(anyhow!("empty target path"));
    }
    if path.contains('\0') {
        return Err(anyhow!("target path contains NUL byte"));
    }
    let p = std::path::Path::new(path);
    if p.is_absolute() {
        return Err(anyhow!(
            "absolute target path rejected (would write outside project root): {}",
            path
        ));
    }

    // Lexically resolve any `..` / `.` components without touching disk,
    // then check containment. We don't use std::fs::canonicalize on the
    // target because the file may not exist yet (FileWrite creates it).
    let mut normalised = std::path::PathBuf::new();
    for component in p.components() {
        use std::path::Component::*;
        match component {
            Prefix(_) | RootDir => {
                return Err(anyhow!("rooted component rejected: {}", path));
            }
            CurDir => continue,
            ParentDir => {
                if !normalised.pop() {
                    return Err(anyhow!("path escapes project root via `..`: {}", path));
                }
            }
            Normal(part) => normalised.push(part),
        }
    }

    // Canonicalise the ROOT (it exists), then join. We deliberately do
    // not canonicalise the joined path because the target may not exist.
    let root_canonical = root
        .canonicalize()
        .with_context(|| format!("canonicalize project root {}", root.display()))?;
    let abs = root_canonical.join(&normalised);

    // Belt-and-suspenders: after the join, verify the result still lives
    // under the canonical root. This catches edge cases like symlink
    // components inside the project root pointing elsewhere.
    if !abs.starts_with(&root_canonical) {
        return Err(anyhow!(
            "resolved path escapes project root: {}",
            abs.display()
        ));
    }
    Ok(abs)
}

/// Render the result of a unified diff without writing it to disk.
pub(crate) fn preview_file_patch(
    root: &std::path::Path,
    path: &str,
    unified_diff: &str,
) -> Result<String> {
    let abs = resolve_safe(root, path)?;
    let original =
        std::fs::read_to_string(&abs).with_context(|| format!("read {}", abs.display()))?;
    let patch =
        diffy::Patch::from_str(unified_diff).map_err(|e| anyhow!("parse unified diff: {}", e))?;
    diffy::apply(&original, &patch).map_err(|e| anyhow!("apply unified diff preview: {}", e))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    /// A fresh, real directory under the OS temp dir. `resolve_safe`
    /// canonicalizes the root, so it must actually exist on disk.
    fn temp_root() -> PathBuf {
        let p = std::env::temp_dir().join(format!("kilroy-actuator-test-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&p).unwrap();
        p
    }

    #[test]
    fn resolve_safe_accepts_nested_relative_path() {
        let root = temp_root();
        let resolved = resolve_safe(&root, "src/commands/agent.rs").unwrap();
        assert!(resolved.ends_with("src/commands/agent.rs"));
        let canonical_root = root.canonicalize().unwrap();
        assert!(resolved.starts_with(&canonical_root));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn resolve_safe_normalises_interior_parent_segments() {
        let root = temp_root();
        // `src/tmp/../lib.rs` resolves to `src/lib.rs` and stays inside root.
        let resolved = resolve_safe(&root, "src/tmp/../lib.rs").unwrap();
        assert!(resolved.ends_with("src/lib.rs"));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn resolve_safe_rejects_parent_escape() {
        let root = temp_root();
        assert!(resolve_safe(&root, "../escape.txt").is_err());
        assert!(resolve_safe(&root, "src/../../escape.txt").is_err());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn resolve_safe_rejects_absolute_path() {
        let root = temp_root();
        #[cfg(windows)]
        let abs = "C:\\Windows\\System32\\drivers\\etc\\hosts";
        #[cfg(not(windows))]
        let abs = "/etc/passwd";
        assert!(resolve_safe(&root, abs).is_err());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn resolve_safe_rejects_empty_and_nul() {
        let root = temp_root();
        assert!(resolve_safe(&root, "").is_err());
        assert!(resolve_safe(&root, "foo\0bar.txt").is_err());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn file_write_creates_file_with_exact_content() {
        let root = temp_root();
        let body = "fn main() {\n    println!(\"hi\");\n}\n";
        let written = apply_file_write(&root, "src/main.rs", body).unwrap();
        // Parent dirs are created on demand.
        assert_eq!(fs::read_to_string(&written).unwrap(), body);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn diff_against_disk_marks_changed_lines() {
        let root = temp_root();
        apply_file_write(&root, "notes.txt", "line one\nline two\nline three\n").unwrap();
        let diff = diff_against_disk(&root, "notes.txt", "line one\nline TWO\nline three\n")
            .expect("a diff should be produced for changed content");
        assert!(
            diff.contains("-line two"),
            "diff should remove the old line:\n{diff}"
        );
        assert!(
            diff.contains("+line TWO"),
            "diff should add the new line:\n{diff}"
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn diff_against_disk_returns_none_for_missing_file() {
        let root = temp_root();
        assert!(diff_against_disk(&root, "nope.txt", "whatever").is_none());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn apply_file_patch_applies_a_canonical_unified_diff() {
        // This mirrors the production path: the model emits a ```diff block,
        // which arrives as a canonical unified diff and is applied verbatim.
        let root = temp_root();
        apply_file_write(&root, "notes.txt", "line one\nline two\nline three\n").unwrap();
        let diff = "\
--- a/notes.txt
+++ b/notes.txt
@@ -1,3 +1,3 @@
 line one
-line two
+line TWO
 line three
";
        let patched = apply_file_patch(&root, "notes.txt", diff).unwrap();
        assert_eq!(
            fs::read_to_string(&patched).unwrap(),
            "line one\nline TWO\nline three\n"
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn apply_file_patch_rejects_unparseable_diff() {
        let root = temp_root();
        apply_file_write(&root, "notes.txt", "x\n").unwrap();
        assert!(apply_file_patch(&root, "notes.txt", "not a diff at all").is_err());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn payload_from_block_classifies_diff_write_and_shell() {
        let patch = Block {
            lang: Some("diff".into()),
            path: Some("src/lib.rs".into()),
            body: "@@ -1 +1 @@\n-a\n+b\n".into(),
        };
        assert!(matches!(
            payload_from_block(&patch),
            Some(ActionPayload::FilePatch { .. })
        ));

        let write = Block {
            lang: Some("rust".into()),
            path: Some("src/new.rs".into()),
            body: "fn x() {}".into(),
        };
        assert!(matches!(
            payload_from_block(&write),
            Some(ActionPayload::FileWrite { .. })
        ));

        let shell = Block {
            lang: Some("bash".into()),
            path: None,
            body: "ls -la".into(),
        };
        assert!(matches!(
            payload_from_block(&shell),
            Some(ActionPayload::Shell { .. })
        ));

        let prose = Block {
            lang: Some("text".into()),
            path: None,
            body: "just words".into(),
        };
        assert!(payload_from_block(&prose).is_none());
    }

    #[test]
    fn file_patch_payload_round_trips_with_unified_diff_key() {
        // Contract guard: both the executor and the refactor "Apply" path
        // persist a FilePatch payload that `accept_action` later parses back
        // into `ActionPayload`. The serialized field MUST be `unified_diff`
        // (not `diff`) or that parse fails and the patch never applies.
        let p = ActionPayload::FilePatch {
            path: "src/x.rs".into(),
            unified_diff: "@@ -1 +1 @@\n-a\n+b\n".into(),
        };
        let json = serde_json::to_string(&p).unwrap();
        assert!(
            json.contains("\"kind\":\"file_patch\""),
            "tag missing: {json}"
        );
        assert!(
            json.contains("\"unified_diff\""),
            "wrong field name: {json}"
        );
        let back: ActionPayload = serde_json::from_str(&json).unwrap();
        match back {
            ActionPayload::FilePatch { path, unified_diff } => {
                assert_eq!(path, "src/x.rs");
                assert!(unified_diff.contains("+b"));
            }
            other => panic!("expected FilePatch, got {other:?}"),
        }
    }
}
