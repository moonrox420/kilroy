//! Distillation corpus — captures user-blessed chat exchanges into a
//! JSONL file that the existing dataset / Modelfile pipeline can train
//! on.
//!
//! Flow: user thumbs-up an agent message → `corpus_append` writes
//! `{messages: [{role:"user",content:"..."}, {role:"assistant",content:"..."}]}`
//! to `<project>/.kilroy/corpus/training.jsonl` → `corpus_stats` reads
//! it back so the UI can show "you have N exchanges — train a custom
//! model on them?" → user clicks → DatasetsDialog opens with that
//! file path pre-loaded → the existing Modelfile composition path
//! takes over.
//!
//! Why OpenAI chat format: it's what the existing `dataset_inspect`
//! detector handles, AND it round-trips cleanly through Ollama and
//! Unsloth without conversion. One canonical shape keeps the pipeline
//! short.
//!
//! Corpus is per-project on purpose. A global corpus mixes signal
//! across unrelated codebases (your React conventions in Project A
//! would pollute the model you train for Project B's Rust style).
//! If a user wants a cross-project corpus they can copy the JSONL out
//! by hand or use the future "Promote to global" path.

use crate::state::AppState;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tauri::State;

#[derive(Deserialize, Debug)]
pub struct CorpusAppendInput {
    /// Raw user message text.
    pub user_message: String,
    /// Raw agent reply text.
    pub agent_message: String,
    /// Optional system-prompt snippet to bake into the record. When set,
    /// the JSONL row gets a leading `{role:"system",content:"..."}`
    /// entry which makes the resulting training example more faithful
    /// to the original turn (the model that wrote the reply WAS seeing
    /// that system prompt, after all). Caller is free to skip it for
    /// generic turns.
    #[serde(default)]
    pub system_prompt: Option<String>,
    /// Optional free-form tag to namespace the entry. Lets advanced
    /// users segregate "good examples" vs "great examples" later — we
    /// don't currently use the tag, but the JSONL preserves it.
    #[serde(default)]
    pub tag: Option<String>,
}

#[derive(Serialize, Clone, Debug)]
pub struct CorpusStats {
    /// Absolute path to the JSONL file. Empty when no project is open
    /// (a corpus needs a project to live under).
    pub path: String,
    /// True if the path exists on disk yet.
    pub exists: bool,
    /// Number of non-blank lines (≈ exchanges) in the file. 0 when the
    /// file doesn't exist.
    pub count: u64,
    /// Total bytes on disk.
    pub size_bytes: u64,
    /// Suggested "you have enough to train" threshold. Surfaced from
    /// the backend so the UI doesn't have to invent a number. 20 is a
    /// reasonable starting point — enough to imprint style without
    /// over-fitting a small LoRA.
    pub train_threshold: u64,
}

/// Append a turn to the project's distillation corpus.
#[tauri::command]
pub async fn corpus_append(
    state: State<'_, AppState>,
    payload: CorpusAppendInput,
) -> Result<CorpusStats, String> {
    let project_root = state
        .memory
        .lock()
        .as_ref()
        .map(|m| m.root.clone())
        .ok_or_else(|| "no project open — corpus is per-project".to_string())?;

    let dir = project_root.join(".kilroy").join("corpus");
    std::fs::create_dir_all(&dir).map_err(|e| format!("mkdir {}: {}", dir.display(), e))?;
    let path = dir.join("training.jsonl");

    let mut msgs: Vec<serde_json::Value> = Vec::new();
    if let Some(sys) = payload.system_prompt.as_deref() {
        let trimmed = sys.trim();
        if !trimmed.is_empty() {
            msgs.push(serde_json::json!({ "role": "system", "content": trimmed }));
        }
    }
    msgs.push(serde_json::json!({
        "role": "user",
        "content": payload.user_message.trim(),
    }));
    msgs.push(serde_json::json!({
        "role": "assistant",
        "content": payload.agent_message.trim(),
    }));

    let mut record = serde_json::json!({ "messages": msgs });
    if let Some(tag) = &payload.tag {
        if !tag.trim().is_empty() {
            record["tag"] = serde_json::Value::String(tag.trim().to_string());
        }
    }
    // Timestamp lets a future "filter / dedupe / preview" UI sort
    // chronologically without parsing the message content.
    record["ts"] =
        serde_json::Value::String(chrono_now_rfc3339().unwrap_or_else(|| String::from("")));

    let mut line =
        serde_json::to_string(&record).map_err(|e| format!("serialise record: {}", e))?;
    line.push('\n');

    // Append-only. We never rewrite the file, so corpus growth is
    // monotonic and the file's mtime is also the time of the latest
    // entry (useful for the UI "last saved" hint).
    use std::io::Write;
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|e| format!("open {}: {}", path.display(), e))?;
    f.write_all(line.as_bytes())
        .map_err(|e| format!("write {}: {}", path.display(), e))?;
    drop(f);

    stats_for(&path)
}

#[tauri::command]
pub async fn corpus_stats(state: State<'_, AppState>) -> Result<CorpusStats, String> {
    let project_root = state.memory.lock().as_ref().map(|m| m.root.clone());
    let Some(root) = project_root else {
        // No project open — return a "zero state" rather than an error
        // so the UI can poll this cheaply on every chat panel mount.
        return Ok(CorpusStats {
            path: String::new(),
            exists: false,
            count: 0,
            size_bytes: 0,
            train_threshold: TRAIN_THRESHOLD,
        });
    };
    let path = root.join(".kilroy").join("corpus").join("training.jsonl");
    stats_for(&path)
}

/// Open the corpus folder in the OS file explorer, so users can edit
/// or remove entries by hand. The corpus is plain JSONL; treating it
/// as an editable text file is a feature, not a bug.
#[tauri::command]
pub async fn corpus_open_folder(state: State<'_, AppState>) -> Result<String, String> {
    let project_root = state
        .memory
        .lock()
        .as_ref()
        .map(|m| m.root.clone())
        .ok_or_else(|| "no project open".to_string())?;
    let dir = project_root.join(".kilroy").join("corpus");
    std::fs::create_dir_all(&dir).ok();

    #[cfg(windows)]
    {
        use std::process::Command;
        Command::new("explorer")
            .arg(&dir)
            .spawn()
            .map_err(|e| format!("open explorer: {}", e))?;
    }
    #[cfg(target_os = "macos")]
    {
        use std::process::Command;
        Command::new("open")
            .arg(&dir)
            .spawn()
            .map_err(|e| format!("open: {}", e))?;
    }
    #[cfg(all(not(windows), not(target_os = "macos")))]
    {
        use std::process::Command;
        Command::new("xdg-open")
            .arg(&dir)
            .spawn()
            .map_err(|e| format!("xdg-open: {}", e))?;
    }
    Ok(dir.to_string_lossy().to_string())
}

const TRAIN_THRESHOLD: u64 = 20;

fn stats_for(path: &PathBuf) -> Result<CorpusStats, String> {
    let exists = path.is_file();
    let (count, size_bytes) = if exists {
        let meta =
            std::fs::metadata(path).map_err(|e| format!("stat {}: {}", path.display(), e))?;
        let size = meta.len();
        // Counting non-blank lines is cheap even for a 10 MB JSONL —
        // it's an O(file) read but the file is tiny by design. We
        // accept the small cost in exchange for not maintaining a
        // separate count cache that could go stale if the user edits
        // the JSONL by hand.
        let raw =
            std::fs::read_to_string(path).map_err(|e| format!("read {}: {}", path.display(), e))?;
        let count = raw.lines().filter(|l| !l.trim().is_empty()).count() as u64;
        (count, size)
    } else {
        (0, 0)
    };
    Ok(CorpusStats {
        path: path.to_string_lossy().to_string(),
        exists,
        count,
        size_bytes,
        train_threshold: TRAIN_THRESHOLD,
    })
}

/// Best-effort RFC3339 timestamp without pulling the `chrono` crate.
/// Uses `SystemTime` and formats it manually as `YYYY-MM-DDTHH:MM:SSZ`.
fn chrono_now_rfc3339() -> Option<String> {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now().duration_since(UNIX_EPOCH).ok()?.as_secs();
    // Days since 1970-01-01.
    let days = (secs / 86_400) as i64;
    let time_of_day = secs % 86_400;
    let hh = time_of_day / 3600;
    let mm = (time_of_day % 3600) / 60;
    let ss = time_of_day % 60;
    let (y, m, d) = days_to_ymd(days);
    Some(format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        y, m, d, hh, mm, ss
    ))
}

/// Convert "days since 1970-01-01" into (year, month, day). Inline
/// Civil-from-days algorithm so we don't drag in chrono just to stamp
/// JSONL rows.
fn days_to_ymd(days: i64) -> (i64, u32, u32) {
    // Howard Hinnant's algorithm — handles leap years and the
    // Gregorian cycle correctly. Adapted to i64 for "days since
    // 1970-01-01".
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = (if mp < 10 { mp + 3 } else { mp - 9 }) as u32;
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d)
}
