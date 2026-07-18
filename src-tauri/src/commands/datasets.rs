//! Dataset commands — load training data files, inspect them, and turn
//! them into custom Ollama models.
//!
//! Three tiers of "use a dataset to improve the agent":
//!
//!   1. **Inspect** — open a `.json`, `.jsonl`, or `.arrow` file, detect
//!      its format (alpaca / sharegpt / openai-chat), and report stats:
//!      record count, average length, sample rows. No model touched.
//!
//!   2. **Modelfile composition** — turn the dataset into a custom
//!      Ollama model. We extract a "persona / convention" digest from the
//!      records and use it as the `SYSTEM` directive in a generated
//!      Modelfile, then run `ollama create <name>`. This is NOT actual
//!      fine-tuning — it's behaviour shaping that works on every Ollama
//!      install without any extra tooling.
//!
//!   3. **LoRA fine-tuning** — the real thing. Spawns a Python subprocess
//!      (Unsloth / Axolotl / llama.cpp finetune) against the dataset.
//!      Requires Python + the training tool installed; the
//!      `training_env_status` command reports whether the environment is
//!      ready. The training itself is scaffolded — wired up to events
//!      and progress but the actual subprocess invocation is gated
//!      behind a "training stack ready" check so users don't fire it on
//!      a bare install and get a cryptic error.
//!
//! `.arrow` / `.parquet` / `.feather` (Hugging Face / Apache Arrow IPC)
//! are NOT yet supported by `dataset_inspect` — it returns an actionable
//! "install pyarrow, support coming next pass" error rather than
//! pretending. JSON and JSONL (the formats the rest of the pipeline
//! actually consumes) are fully handled. When arrow support lands it will
//! go through a small Python helper rather than the ~5 MB `arrow-rs`
//! crate, to keep the binary lean.

use crate::state::AppState;
use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;
use tauri::{AppHandle, Emitter, Manager, State};

/// What format does this dataset look like? We auto-detect from the
/// first record's keys — the user shouldn't have to remember whether
/// their HF download is alpaca-style or sharegpt-style.
#[derive(Serialize, Clone, Debug, PartialEq)]
pub enum DatasetFormat {
    /// `[{ "instruction": "...", "input": "...", "output": "..." }, ...]`
    Alpaca,
    /// `[{ "conversations": [{ "from": "human", "value": "..." }, ...] }, ...]`
    ShareGpt,
    /// `[{ "messages": [{ "role": "user", "content": "..." }, ...] }, ...]`
    OpenAiChat,
    /// `[{ "prompt": "...", "completion": "..." }, ...]` (legacy OpenAI fine-tune)
    PromptCompletion,
    /// Couldn't tell. Caller falls back to "raw JSON inspection".
    Unknown,
}

#[derive(Serialize, Clone, Debug)]
pub struct DatasetInspect {
    pub path: String,
    /// `json`, `jsonl`, `arrow`, or `unknown`.
    pub container: String,
    pub format: DatasetFormat,
    pub record_count: u64,
    /// If we sampled (large file), the count we actually read. Same as
    /// `record_count` for files we read fully.
    pub sampled_count: u64,
    /// Total bytes on disk.
    pub size_bytes: u64,
    /// Up to 3 sample records, JSON-stringified, for the UI to render.
    pub samples: Vec<String>,
    /// Average input/output length (chars). 0 if not applicable.
    pub avg_input_chars: u64,
    pub avg_output_chars: u64,
    /// Diagnostic notes — e.g. "skipped 12 rows missing 'output' field".
    pub notes: Vec<String>,
}

const SAMPLE_LIMIT: usize = 3;
/// Max rows we read for stats. Real datasets can be millions of rows;
/// we sample to keep inspection responsive. The user gets total
/// `record_count` separately (cheap to count newlines / array length).
const STAT_SAMPLE_ROWS: usize = 2000;

/// Inspect a dataset file: detect format, count records, surface
/// samples. Pure read; nothing written to disk.
#[tauri::command]
pub async fn dataset_inspect(path: String) -> Result<DatasetInspect, String> {
    let p = Path::new(&path);
    if !p.is_file() {
        return Err(format!("not a file: {}", path));
    }
    let size_bytes = p.metadata().map(|m| m.len()).unwrap_or(0);
    let ext = p
        .extension()
        .and_then(|e| e.to_str())
        .map(|s| s.to_lowercase())
        .unwrap_or_default();

    match ext.as_str() {
        "json" => inspect_json(&path, size_bytes).map_err(|e| format!("{:#}", e)),
        "jsonl" | "ndjson" => inspect_jsonl(&path, size_bytes).map_err(|e| format!("{:#}", e)),
        "arrow" | "feather" | "parquet" => Err(format!(
            "{} files need Python + pyarrow to inspect. Install with: \
             `pip install pyarrow` then re-open. Falling back to format \
             detection only is not implemented yet — coming in the next \
             pass.",
            ext
        )),
        other => Err(format!(
            "unsupported dataset extension `.{}` — accepted: .json, .jsonl, .ndjson, .arrow",
            other
        )),
    }
}

fn inspect_json(path: &str, size_bytes: u64) -> Result<DatasetInspect> {
    let raw = std::fs::read_to_string(path).with_context(|| format!("read {}", path))?;
    let value: serde_json::Value =
        serde_json::from_str(&raw).with_context(|| format!("parse {}", path))?;
    let arr = match value {
        serde_json::Value::Array(a) => a,
        // Some HF dumps wrap the records under a top-level key.
        serde_json::Value::Object(mut m) => {
            // Common wrapper keys: "data", "rows", "records", "train".
            for k in &["data", "rows", "records", "train", "examples"] {
                if let Some(serde_json::Value::Array(a)) = m.remove(*k) {
                    return aggregate(path, "json", size_bytes, a);
                }
            }
            return Err(anyhow!(
                "top-level JSON is an object — no `data` / `rows` / `records` / `train` / `examples` key found"
            ));
        }
        _ => return Err(anyhow!("expected top-level JSON array, got scalar")),
    };
    aggregate(path, "json", size_bytes, arr)
}

fn inspect_jsonl(path: &str, size_bytes: u64) -> Result<DatasetInspect> {
    let raw = std::fs::read_to_string(path).with_context(|| format!("read {}", path))?;
    let mut records = Vec::new();
    let mut notes = Vec::new();
    let mut parse_errors = 0u64;
    let mut total_lines = 0u64;
    for line in raw.lines() {
        let t = line.trim();
        if t.is_empty() {
            continue;
        }
        total_lines += 1;
        if records.len() < STAT_SAMPLE_ROWS {
            match serde_json::from_str::<serde_json::Value>(t) {
                Ok(v) => records.push(v),
                Err(_) => parse_errors += 1,
            }
        }
    }
    if parse_errors > 0 {
        notes.push(format!(
            "{} line(s) failed to parse as JSON — skipped in stats",
            parse_errors
        ));
    }
    // For JSONL the real `record_count` is the non-blank line count.
    // We sampled the first N for stats but report the true total.
    let mut out = aggregate(path, "jsonl", size_bytes, records)?;
    out.record_count = total_lines;
    out.notes.extend(notes);
    Ok(out)
}

fn aggregate(
    path: &str,
    container: &str,
    size_bytes: u64,
    arr: Vec<serde_json::Value>,
) -> Result<DatasetInspect> {
    let mut notes = Vec::new();
    let record_count = arr.len() as u64;
    let format = detect_format(&arr);
    let sample_iter: Vec<&serde_json::Value> = arr.iter().take(STAT_SAMPLE_ROWS).collect();
    let sampled_count = sample_iter.len() as u64;

    let mut samples: Vec<String> = sample_iter
        .iter()
        .take(SAMPLE_LIMIT)
        .map(|v| {
            // Pretty-print and truncate so the UI doesn't render a 50 KB
            // single record.
            let pretty = serde_json::to_string_pretty(v).unwrap_or_default();
            // Char-safe truncation — see `truncate` below. Byte-slicing
            // `&pretty[..2000]` panics when byte 2000 lands mid-codepoint
            // (common in datasets with non-ASCII content).
            let mut chars = pretty.chars();
            let head: String = chars.by_ref().take(2000).collect();
            if chars.next().is_some() {
                format!("{}\n...[truncated]", head)
            } else {
                head
            }
        })
        .collect();
    if samples.is_empty() {
        samples.push("(no records)".into());
    }

    let mut sum_in: u64 = 0;
    let mut sum_out: u64 = 0;
    let mut counted: u64 = 0;
    for rec in &sample_iter {
        let (i, o) = extract_io(rec, &format);
        if i.is_some() || o.is_some() {
            counted += 1;
            sum_in += i.unwrap_or_default() as u64;
            sum_out += o.unwrap_or_default() as u64;
        }
    }
    if counted == 0 && format != DatasetFormat::Unknown {
        notes.push(format!(
            "format detected as {:?} but no records had matching fields",
            format
        ));
    }
    let avg_input_chars = sum_in.checked_div(counted).unwrap_or(0);
    let avg_output_chars = sum_out.checked_div(counted).unwrap_or(0);

    Ok(DatasetInspect {
        path: path.into(),
        container: container.into(),
        format,
        record_count,
        sampled_count,
        size_bytes,
        samples,
        avg_input_chars,
        avg_output_chars,
        notes,
    })
}

fn detect_format(records: &[serde_json::Value]) -> DatasetFormat {
    let probe = records.iter().take(20).filter_map(|v| v.as_object());
    for obj in probe {
        if obj.contains_key("messages") {
            return DatasetFormat::OpenAiChat;
        }
        if obj.contains_key("conversations") {
            return DatasetFormat::ShareGpt;
        }
        if obj.contains_key("instruction") && obj.contains_key("output") {
            return DatasetFormat::Alpaca;
        }
        if obj.contains_key("prompt") && obj.contains_key("completion") {
            return DatasetFormat::PromptCompletion;
        }
    }
    DatasetFormat::Unknown
}

/// Extract (input_len, output_len) in chars for a single record so we
/// can compute averages. Returns (None, None) when the record doesn't
/// match the expected shape.
fn extract_io(rec: &serde_json::Value, fmt: &DatasetFormat) -> (Option<usize>, Option<usize>) {
    let obj = match rec.as_object() {
        Some(o) => o,
        None => return (None, None),
    };
    match fmt {
        DatasetFormat::Alpaca => {
            let inp = obj
                .get("instruction")
                .and_then(|v| v.as_str())
                .map(|s| s.len())
                .unwrap_or(0)
                + obj
                    .get("input")
                    .and_then(|v| v.as_str())
                    .map(|s| s.len())
                    .unwrap_or(0);
            let out = obj
                .get("output")
                .and_then(|v| v.as_str())
                .map(|s| s.len())
                .unwrap_or(0);
            (Some(inp), Some(out))
        }
        DatasetFormat::PromptCompletion => (
            obj.get("prompt").and_then(|v| v.as_str()).map(|s| s.len()),
            obj.get("completion")
                .and_then(|v| v.as_str())
                .map(|s| s.len()),
        ),
        DatasetFormat::OpenAiChat => {
            let msgs = obj.get("messages").and_then(|v| v.as_array());
            sum_conversation_lengths(msgs, "role", "content", &["user", "system"], &["assistant"])
        }
        DatasetFormat::ShareGpt => {
            let msgs = obj.get("conversations").and_then(|v| v.as_array());
            sum_conversation_lengths(
                msgs,
                "from",
                "value",
                &["human", "user", "system"],
                &["gpt", "assistant"],
            )
        }
        DatasetFormat::Unknown => (None, None),
    }
}

fn sum_conversation_lengths(
    msgs: Option<&Vec<serde_json::Value>>,
    role_key: &str,
    content_key: &str,
    in_roles: &[&str],
    out_roles: &[&str],
) -> (Option<usize>, Option<usize>) {
    let Some(arr) = msgs else { return (None, None) };
    let mut in_chars = 0;
    let mut out_chars = 0;
    for m in arr {
        let Some(obj) = m.as_object() else { continue };
        let role = obj.get(role_key).and_then(|v| v.as_str()).unwrap_or("");
        let content = obj.get(content_key).and_then(|v| v.as_str()).unwrap_or("");
        if in_roles.contains(&role) {
            in_chars += content.len();
        } else if out_roles.contains(&role) {
            out_chars += content.len();
        }
    }
    (Some(in_chars), Some(out_chars))
}

// ─── Modelfile composition (tier-1 "training") ──────────────────────────────

#[derive(Deserialize, Debug)]
pub struct CreateModelInput {
    /// New model tag, e.g. `kilroy-react-conventions`. Validated for
    /// Ollama compatibility (lowercase + digits + `-_:.`).
    pub name: String,
    /// Base model to extend, e.g. `qwen2.5-coder:14b-instruct-q8_0`.
    pub base: String,
    /// Path to the dataset file. We re-inspect it to derive the system
    /// prompt — caller already has the inspection but we don't trust
    /// frontend-supplied content.
    pub dataset_path: String,
    /// Optional extra instructions to prepend to the auto-derived
    /// system prompt — lets the user steer the "personality" beyond
    /// what the dataset alone implies.
    #[serde(default)]
    pub extra_system: Option<String>,
    /// Temperature override for the new model. Ollama default is 0.8;
    /// for code-style emulation 0.3-0.5 produces more faithful output.
    #[serde(default)]
    pub temperature: Option<f32>,
}

#[derive(Serialize, Clone, Debug)]
pub struct ModelfileBuilt {
    pub name: String,
    pub modelfile_path: String,
    pub system_prompt: String,
    pub note: String,
}

/// Derive a SYSTEM prompt from the dataset and create a new model via
/// Ollama's `/api/create` (structured `model`/`from`/`system`/`parameters`
/// form). A copy of the equivalent Modelfile is also written to disk for the
/// user to inspect/edit. The new model shows up in Settings → Chat model.
/// Streams creation progress on `ollama://create/progress`.
#[tauri::command]
pub async fn dataset_create_modelfile(
    app: AppHandle,
    state: State<'_, AppState>,
    payload: CreateModelInput,
) -> Result<ModelfileBuilt, String> {
    // Validate the new model name — Ollama is picky about tags.
    if !is_valid_ollama_tag(&payload.name) {
        return Err(format!(
            "invalid model name `{}` — use lowercase letters, digits, `-`, `_`, `.`, or `:` only",
            payload.name
        ));
    }

    // Re-inspect the dataset so we know the format AND build the SYSTEM
    // prompt deterministically from disk content (frontend can't lie).
    let inspect = dataset_inspect(payload.dataset_path.clone()).await?;

    let system_prompt = build_system_prompt_from_dataset(&inspect, payload.extra_system.as_deref())
        .map_err(|e| format!("{:#}", e))?;

    // One temperature value drives BOTH the on-disk Modelfile (for the user
    // to inspect) and the structured /api/create request below, so they
    // can never drift apart.
    let temperature = payload.temperature.unwrap_or(0.4);
    let modelfile = render_modelfile(&payload.base, &system_prompt, temperature);

    // Persist the Modelfile to disk so the user can inspect / edit it
    // later. Lives next to skills/ under the app config dir.
    let app_dir = app
        .path()
        .app_config_dir()
        .map_err(|e| format!("app config dir: {}", e))?;
    let mf_dir = app_dir.join("modelfiles");
    std::fs::create_dir_all(&mf_dir).map_err(|e| format!("mkdir modelfiles: {}", e))?;
    let mf_path = mf_dir.join(format!("{}.Modelfile", payload.name));
    std::fs::write(&mf_path, &modelfile)
        .map_err(|e| format!("write {}: {}", mf_path.display(), e))?;

    // Hit Ollama's /api/create endpoint. It streams NDJSON status events;
    // we re-emit them as `ollama://create/progress`.
    let url = state.settings.read().ollama_url.clone();
    let endpoint = format!("{}/api/create", url.trim_end_matches('/'));
    // Ollama's /api/create takes STRUCTURED fields (model / from / system /
    // parameters) as of the v0.4+ API. The legacy `{ name, modelfile }`
    // shape this used to send is no longer parsed by current daemons, so it
    // silently created an empty/wrong model. We still wrote the rendered
    // Modelfile to disk above for the user to read; the daemon gets the
    // structured request here. `template` is intentionally omitted — `from`
    // makes the new model inherit the base model's template, which is what
    // we want.
    let body = build_create_request(&payload.name, &payload.base, &system_prompt, temperature);

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(60 * 10))
        .connect_timeout(std::time::Duration::from_secs(5))
        .build()
        .map_err(|e| format!("reqwest build: {}", e))?;

    let emit_progress = |status: &str, error: Option<String>, done: bool| {
        let _ = app.emit(
            "ollama://create/progress",
            serde_json::json!({
                "name": payload.name,
                "status": status,
                "error": error,
                "done": done,
            }),
        );
    };

    emit_progress("starting", None, false);

    let mut resp = match client.post(&endpoint).json(&body).send().await {
        Ok(r) => r,
        Err(e) => {
            let err = format!("POST {}: {}", endpoint, e);
            emit_progress("error", Some(err.clone()), true);
            return Err(err);
        }
    };
    if !resp.status().is_success() {
        let st = resp.status();
        let txt = resp.text().await.unwrap_or_default();
        let err = format!("Ollama create returned {}: {}", st, txt);
        emit_progress("error", Some(err.clone()), true);
        return Err(err);
    }

    // Drain NDJSON progress lines.
    let mut buf = Vec::<u8>::new();
    loop {
        match resp.chunk().await {
            Ok(Some(chunk)) => {
                buf.extend_from_slice(&chunk);
                while let Some(pos) = buf.iter().position(|&b| b == b'\n') {
                    let line: Vec<u8> = buf.drain(..=pos).collect();
                    let line_str = String::from_utf8_lossy(&line);
                    let line_str = line_str.trim();
                    if line_str.is_empty() {
                        continue;
                    }
                    if let Ok(v) = serde_json::from_str::<serde_json::Value>(line_str) {
                        if let Some(s) = v.get("status").and_then(|x| x.as_str()) {
                            emit_progress(s, None, false);
                        }
                    }
                }
            }
            Ok(None) => break,
            Err(e) => {
                let err = format!("create stream: {}", e);
                emit_progress("error", Some(err.clone()), true);
                return Err(err);
            }
        }
    }

    emit_progress("success", None, true);
    tracing::info!(model = %payload.name, base = %payload.base, "model created");

    Ok(ModelfileBuilt {
        name: payload.name,
        modelfile_path: mf_path.to_string_lossy().to_string(),
        system_prompt,
        note: "Model created via Ollama /api/create. Select it from Settings → Chat model.".into(),
    })
}

fn is_valid_ollama_tag(s: &str) -> bool {
    if s.is_empty() || s.len() > 80 {
        return false;
    }
    s.chars().all(|c| {
        c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '-' | '_' | '.' | ':' | '/')
    })
}

/// Build the SYSTEM directive content from the dataset. The approach
/// depends on format:
///
/// * **Alpaca / PromptCompletion** — extract the most-common instruction
///   patterns + their outputs into a "When asked X, respond like Y"
///   style prompt.
///
/// * **OpenAI / ShareGPT** — extract any messages whose role is
///   `"system"` and concatenate them (those are the user's intent
///   captured upstream). Fall back to "answer like the assistant
///   examples below" with 3-5 representative pairs.
///
/// * **Unknown** — return an error explaining what failed; the user can
///   provide an `extra_system` string instead.
fn build_system_prompt_from_dataset(
    inspect: &DatasetInspect,
    extra: Option<&str>,
) -> Result<String> {
    let mut out = String::new();
    if let Some(e) = extra {
        if !e.trim().is_empty() {
            out.push_str(e.trim());
            out.push_str("\n\n");
        }
    }
    match inspect.format {
        DatasetFormat::Unknown => {
            if out.is_empty() {
                return Err(anyhow!(
                    "couldn't detect dataset format (not Alpaca / ShareGPT / OpenAI). \
                     Provide an `extra_system` instruction so the new model has a system prompt to ground on."
                ));
            }
            // The user gave us extra_system but the dataset is unknown —
            // we still proceed; the new model just gets the user's
            // instruction without dataset-derived examples.
            out.push_str("\n(No example pairs extracted — dataset format unrecognised.)\n");
        }
        _ => {
            out.push_str(&format!(
                "You learned from a {} dataset of {} records. \
                 Mirror the style, vocabulary, and structure of the example responses below when answering similar questions.\n\n",
                format_label(&inspect.format),
                inspect.record_count
            ));
            // Re-open the file and grab a handful of representative
            // records to embed verbatim — gives the model concrete
            // anchoring beyond the description.
            let examples = sample_examples(&inspect.path, &inspect.format, 5)?;
            for (i, (q, a)) in examples.iter().enumerate() {
                out.push_str(&format!("## Example {}\n", i + 1));
                out.push_str(&format!("**Q:** {}\n", truncate(q, 600)));
                out.push_str(&format!("**A:** {}\n\n", truncate(a, 600)));
            }
        }
    }
    out.push_str(
        "When asked questions outside the topics covered by the examples, \
         answer from your base knowledge — do NOT fabricate examples that \
         look like the dataset.\n",
    );
    Ok(out)
}

fn format_label(f: &DatasetFormat) -> &'static str {
    match f {
        DatasetFormat::Alpaca => "Alpaca instruction-tuning",
        DatasetFormat::ShareGpt => "ShareGPT conversation",
        DatasetFormat::OpenAiChat => "OpenAI chat-completion",
        DatasetFormat::PromptCompletion => "prompt/completion",
        DatasetFormat::Unknown => "unrecognised",
    }
}

/// Truncate to at most `n` CHARS (not bytes) so we never slice a
/// multi-byte UTF-8 codepoint in half. Training-data text routinely
/// contains CJK, emoji, and accented characters; byte-slicing at a
/// fixed offset panics with "byte index N is not a char boundary".
fn truncate(s: &str, n: usize) -> String {
    let mut chars = s.chars();
    let head: String = chars.by_ref().take(n).collect();
    // If the iterator still has anything left, we truncated.
    if chars.next().is_some() {
        format!("{}…", head)
    } else {
        head
    }
}

fn sample_examples(path: &str, fmt: &DatasetFormat, want: usize) -> Result<Vec<(String, String)>> {
    let raw = std::fs::read_to_string(path)?;
    let records: Vec<serde_json::Value> = if path.ends_with(".jsonl") || path.ends_with(".ndjson") {
        raw.lines()
            .filter(|l| !l.trim().is_empty())
            .filter_map(|l| serde_json::from_str(l).ok())
            .take(want * 3)
            .collect()
    } else {
        let v: serde_json::Value = serde_json::from_str(&raw)?;
        match v {
            serde_json::Value::Array(a) => a.into_iter().take(want * 3).collect(),
            _ => Vec::new(),
        }
    };

    let mut out = Vec::new();
    for rec in records {
        let (q, a) = match fmt {
            DatasetFormat::Alpaca => {
                let q = rec
                    .get("instruction")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let extra_in = rec.get("input").and_then(|v| v.as_str()).unwrap_or("");
                let a = rec.get("output").and_then(|v| v.as_str()).unwrap_or("");
                if q.is_empty() || a.is_empty() {
                    continue;
                }
                let q_full = if extra_in.is_empty() {
                    q.to_string()
                } else {
                    format!("{}\n\n{}", q, extra_in)
                };
                (q_full, a.to_string())
            }
            DatasetFormat::PromptCompletion => {
                let q = rec.get("prompt").and_then(|v| v.as_str()).unwrap_or("");
                let a = rec.get("completion").and_then(|v| v.as_str()).unwrap_or("");
                if q.is_empty() || a.is_empty() {
                    continue;
                }
                (q.to_string(), a.to_string())
            }
            DatasetFormat::OpenAiChat | DatasetFormat::ShareGpt => {
                let (role_key, content_key, ask_roles, ans_roles): (&str, &str, &[&str], &[&str]) =
                    match fmt {
                        DatasetFormat::OpenAiChat => ("role", "content", &["user"], &["assistant"]),
                        DatasetFormat::ShareGpt => {
                            ("from", "value", &["human", "user"], &["gpt", "assistant"])
                        }
                        _ => unreachable!(),
                    };
                let arr_key = if matches!(fmt, DatasetFormat::OpenAiChat) {
                    "messages"
                } else {
                    "conversations"
                };
                let msgs = match rec.get(arr_key).and_then(|v| v.as_array()) {
                    Some(a) => a,
                    None => continue,
                };
                let mut q = String::new();
                let mut a = String::new();
                for m in msgs {
                    let role = m.get(role_key).and_then(|v| v.as_str()).unwrap_or("");
                    let content = m.get(content_key).and_then(|v| v.as_str()).unwrap_or("");
                    if ask_roles.contains(&role) && q.is_empty() {
                        q = content.to_string();
                    } else if ans_roles.contains(&role) && a.is_empty() {
                        a = content.to_string();
                    }
                    if !q.is_empty() && !a.is_empty() {
                        break;
                    }
                }
                if q.is_empty() || a.is_empty() {
                    continue;
                }
                (q, a)
            }
            DatasetFormat::Unknown => continue,
        };
        out.push((q, a));
        if out.len() >= want {
            break;
        }
    }
    Ok(out)
}

/// Build the JSON body for Ollama's `/api/create`. Centralised + unit-tested
/// so the request shape can't silently drift back to the legacy
/// `{ name, modelfile }` form that current Ollama (v0.4+) ignores. Mirrors
/// the PARAMETER lines `render_modelfile` writes so the created model behaves
/// the same whether built via this API or the on-disk Modelfile.
fn build_create_request(
    model: &str,
    base: &str,
    system: &str,
    temperature: f32,
) -> serde_json::Value {
    serde_json::json!({
        "model": model,
        "from": base,
        "system": system,
        "parameters": {
            "temperature": temperature,
            "top_p": 0.9,
            "num_ctx": 8192
        },
        "stream": true
    })
}

fn render_modelfile(base: &str, system: &str, temperature: f32) -> String {
    // Ollama's Modelfile parser treats `"""` as the SYSTEM block
    // delimiter. If the user's dataset contains a literal `"""` (e.g.
    // Python docstrings, JSON-encoded strings with three quotes) we'd
    // close the directive early and silently lose the rest of the
    // prompt. Substitute with a visually-similar marker so the model
    // still reads coherent prose.
    let escaped = system.replace("\"\"\"", "\"\u{200B}\"\u{200B}\"");
    format!(
        "# Generated by Kilroy — dataset-derived custom model.\n\
         FROM {base}\n\n\
         PARAMETER temperature {temperature}\n\
         PARAMETER top_p 0.9\n\
         PARAMETER num_ctx 8192\n\n\
         SYSTEM \"\"\"\n{system}\n\"\"\"\n",
        base = base,
        temperature = temperature,
        system = escaped,
    )
}

// ─── Training environment probe (tier-3 LoRA scaffold) ──────────────────────

#[derive(Serialize, Clone, Debug)]
pub struct TrainingEnv {
    pub python_available: bool,
    pub python_version: Option<String>,
    pub unsloth_installed: bool,
    pub transformers_installed: bool,
    pub gpu_visible: bool,
    pub hint: String,
}

/// Probe for the training stack so the UI can show "ready / not ready"
/// before exposing a Run-LoRA button. We don't pull dependencies
/// ourselves — the user opts in, and we surface the install commands.
#[tauri::command]
pub async fn training_env_status() -> Result<TrainingEnv, String> {
    use std::process::Command;

    let py = which_python();
    let python_available = py.is_some();
    let python_version = py.as_ref().and_then(|exe| {
        Command::new(exe).arg("--version").output().ok().map(|o| {
            String::from_utf8_lossy(&o.stdout)
                .trim()
                .to_string()
                .replace("Python ", "")
        })
    });

    let (unsloth_installed, transformers_installed) = match &py {
        Some(exe) => {
            let probe = |module: &str| -> bool {
                Command::new(exe)
                    .args(["-c", &format!("import {}", module)])
                    .output()
                    .map(|o| o.status.success())
                    .unwrap_or(false)
            };
            (probe("unsloth"), probe("transformers"))
        }
        None => (false, false),
    };

    // GPU visibility — nvidia-smi exit 0 means CUDA, on Windows we
    // could also probe DirectML / ROCm, but nvidia-smi covers the
    // common case for this audience.
    let gpu_visible = Command::new("nvidia-smi")
        .arg("-L")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);

    let hint = if !python_available {
        "Install Python 3.10+ from python.org or `winget install Python.Python.3.11`.".to_string()
    } else if !transformers_installed && !unsloth_installed {
        "Install training deps: `pip install unsloth transformers accelerate datasets bitsandbytes`."
            .to_string()
    } else if !gpu_visible {
        "No NVIDIA GPU detected — CPU LoRA is extremely slow. Consider running training on a cloud box and importing the resulting GGUF."
            .to_string()
    } else {
        "Training stack ready.".to_string()
    };

    Ok(TrainingEnv {
        python_available,
        python_version,
        unsloth_installed,
        transformers_installed,
        gpu_visible,
        hint,
    })
}

fn which_python() -> Option<String> {
    use std::process::Command;
    for candidate in ["python3", "python", "py"] {
        if Command::new(candidate)
            .arg("--version")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
        {
            return Some(candidate.to_string());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncate_is_char_safe_on_multibyte() {
        // Ten rockets (4 bytes each). Truncating to 5 must NOT panic
        // (the old `&s[..n]` byte-slice would split a codepoint here)
        // and must yield exactly 5 rockets plus an ellipsis.
        let s = "🚀".repeat(10);
        let out = truncate(&s, 5);
        assert_eq!(out, format!("{}…", "🚀".repeat(5)));
    }

    #[test]
    fn truncate_noop_when_short() {
        assert_eq!(truncate("hello", 10), "hello");
        assert_eq!(truncate("héllo", 10), "héllo");
    }

    #[test]
    fn truncate_exact_boundary_no_ellipsis() {
        // Exactly n chars → no truncation marker.
        assert_eq!(truncate("abcde", 5), "abcde");
    }

    #[test]
    fn detect_alpaca() {
        let recs = vec![serde_json::json!({"instruction":"x","output":"y"})];
        assert_eq!(detect_format(&recs), DatasetFormat::Alpaca);
    }

    #[test]
    fn detect_openai_chat() {
        let recs = vec![serde_json::json!({"messages":[{"role":"user","content":"hi"}]})];
        assert_eq!(detect_format(&recs), DatasetFormat::OpenAiChat);
    }

    #[test]
    fn detect_sharegpt() {
        let recs = vec![serde_json::json!({"conversations":[{"from":"human","value":"hi"}]})];
        assert_eq!(detect_format(&recs), DatasetFormat::ShareGpt);
    }

    #[test]
    fn detect_prompt_completion() {
        let recs = vec![serde_json::json!({"prompt":"p","completion":"c"})];
        assert_eq!(detect_format(&recs), DatasetFormat::PromptCompletion);
    }

    #[test]
    fn detect_unknown_when_no_known_keys() {
        let recs = vec![serde_json::json!({"foo":"bar"})];
        assert_eq!(detect_format(&recs), DatasetFormat::Unknown);
    }

    #[test]
    fn ollama_tag_validation() {
        assert!(is_valid_ollama_tag("kilroy-react-conventions"));
        assert!(is_valid_ollama_tag("qwen2.5-coder:14b-instruct-q8_0"));
        assert!(!is_valid_ollama_tag("Has Spaces"));
        assert!(!is_valid_ollama_tag("UPPER")); // ollama tags are lowercase
        assert!(!is_valid_ollama_tag(""));
    }

    #[test]
    fn modelfile_escapes_triple_quotes() {
        // A dataset-derived system prompt containing `"""` must not be
        // able to prematurely close the SYSTEM block.
        let mf = render_modelfile("base:latest", "doc with \"\"\" inside", 0.4);
        // The raw triple-quote must not survive verbatim inside the body.
        let body_start = mf.find("SYSTEM \"\"\"").unwrap() + "SYSTEM \"\"\"".len();
        let body = &mf[body_start..];
        // Everything up to the CLOSING delimiter should not contain a
        // bare triple-quote from the user content.
        assert!(body.contains('\u{200B}'));
    }
}

#[cfg(test)]
mod create_request_tests {
    use super::*;

    #[test]
    fn uses_structured_fields_not_legacy_modelfile() {
        let b = build_create_request(
            "kilroy-react",
            "qwen2.5-coder:14b-instruct-q8_0",
            "Be terse.",
            0.4,
        );
        // Current Ollama API shape.
        assert_eq!(b["model"], "kilroy-react");
        assert_eq!(b["from"], "qwen2.5-coder:14b-instruct-q8_0");
        assert_eq!(b["system"], "Be terse.");
        assert_eq!(b["stream"], true);
        assert_eq!(b["parameters"]["top_p"], 0.9);
        assert_eq!(b["parameters"]["num_ctx"], 8192);
        let temp = b["parameters"]["temperature"].as_f64().unwrap();
        assert!(
            (temp - 0.4).abs() < 1e-5,
            "temperature should be ~0.4, got {temp}"
        );
        // The legacy fields current Ollama silently ignores must NOT be sent.
        assert!(b.get("name").is_none(), "legacy `name` field must be gone");
        assert!(
            b.get("modelfile").is_none(),
            "legacy `modelfile` field must be gone"
        );
    }
}
