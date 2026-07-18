//! Typed read-only tools for the Rust agent runtime.

use crate::actuator;
use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

const MAX_READ_BYTES: u64 = 2 * 1024 * 1024;
const MAX_READ_LINES: usize = 400;
const MAX_SEARCH_FILES: usize = 20_000;
const MAX_SEARCH_MATCHES: usize = 80;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "tool", rename_all = "snake_case")]
pub enum ToolCall {
    ReadFile {
        path: String,
        #[serde(default)]
        start_line: Option<usize>,
        #[serde(default)]
        end_line: Option<usize>,
    },
    SearchFiles {
        query: String,
        #[serde(default)]
        path: Option<String>,
    },
}

impl ToolCall {
    pub fn name(&self) -> &'static str {
        match self {
            Self::ReadFile { .. } => "read_file",
            Self::SearchFiles { .. } => "search_files",
        }
    }

    pub fn editor_path(&self) -> Option<&str> {
        match self {
            Self::ReadFile { path, .. } => Some(path),
            Self::SearchFiles { .. } => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResult {
    pub tool: String,
    pub success: bool,
    pub output: String,
    pub error: Option<String>,
    pub duration_ms: u64,
}

pub fn execute(root: &Path, call: &ToolCall) -> ToolResult {
    let started = std::time::Instant::now();
    let result = match call {
        ToolCall::ReadFile {
            path,
            start_line,
            end_line,
        } => read_file(root, path, *start_line, *end_line),
        ToolCall::SearchFiles { query, path } => search_files(root, query, path.as_deref()),
    };
    let duration_ms = started.elapsed().as_millis() as u64;
    match result {
        Ok(output) => ToolResult {
            tool: call.name().to_string(),
            success: true,
            output,
            error: None,
            duration_ms,
        },
        Err(error) => ToolResult {
            tool: call.name().to_string(),
            success: false,
            output: String::new(),
            error: Some(format!("{error:#}")),
            duration_ms,
        },
    }
}

fn read_file(
    root: &Path,
    path: &str,
    start_line: Option<usize>,
    end_line: Option<usize>,
) -> Result<String> {
    let abs = actuator::resolve_safe(root, path)?;
    let metadata = fs::metadata(&abs).with_context(|| format!("stat {}", abs.display()))?;
    if !metadata.is_file() {
        return Err(anyhow!("not a regular file: {path}"));
    }
    if metadata.len() > MAX_READ_BYTES {
        return Err(anyhow!(
            "file exceeds {} byte read limit: {path}",
            MAX_READ_BYTES
        ));
    }
    let contents = fs::read_to_string(&abs).with_context(|| format!("read {path}"))?;
    let lines: Vec<&str> = contents.lines().collect();
    let start = start_line.unwrap_or(1).max(1);
    let requested_end = end_line.unwrap_or_else(|| start.saturating_add(MAX_READ_LINES - 1));
    let end = requested_end
        .min(lines.len())
        .min(start.saturating_add(MAX_READ_LINES - 1));
    if start > lines.len().saturating_add(1) || end < start {
        return Err(anyhow!("invalid line range {start}-{end} for {path}"));
    }

    let mut output = format!("FILE {path} lines {start}-{end} of {}\n", lines.len());
    for (index, line) in lines
        .iter()
        .enumerate()
        .skip(start.saturating_sub(1))
        .take(end.saturating_sub(start).saturating_add(1))
    {
        output.push_str(&format!("{:>6} | {}\n", index + 1, line));
    }
    Ok(output)
}

fn search_files(root: &Path, query: &str, relative: Option<&str>) -> Result<String> {
    let query = query.trim();
    if query.is_empty() {
        return Err(anyhow!("search query is empty"));
    }
    let start = match relative {
        Some(path) => actuator::resolve_safe(root, path)?,
        None => root.canonicalize().context("canonicalize project root")?,
    };

    let mut stack = vec![start];
    let mut scanned = 0usize;
    let mut matches = Vec::new();
    while let Some(path) = stack.pop() {
        if scanned >= MAX_SEARCH_FILES || matches.len() >= MAX_SEARCH_MATCHES {
            break;
        }
        let metadata = match fs::metadata(&path) {
            Ok(metadata) => metadata,
            Err(_) => continue,
        };
        if metadata.is_dir() {
            if is_ignored_dir(&path) {
                continue;
            }
            let entries = match fs::read_dir(&path) {
                Ok(entries) => entries,
                Err(_) => continue,
            };
            for entry in entries.flatten() {
                stack.push(entry.path());
            }
            continue;
        }
        scanned += 1;
        if metadata.len() > MAX_READ_BYTES || !is_text_candidate(&path) {
            continue;
        }
        let contents = match fs::read_to_string(&path) {
            Ok(contents) => contents,
            Err(_) => continue,
        };
        for (line_index, line) in contents.lines().enumerate() {
            if line.contains(query) {
                let display = path.strip_prefix(root).unwrap_or(&path);
                matches.push(format!(
                    "{}:{}: {}",
                    display.display(),
                    line_index + 1,
                    line.trim()
                ));
                if matches.len() >= MAX_SEARCH_MATCHES {
                    break;
                }
            }
        }
    }

    Ok(format!(
        "SEARCH {:?}: {} match(es) across {} file(s){}\n{}",
        query,
        matches.len(),
        scanned,
        if matches.len() == MAX_SEARCH_MATCHES {
            " (truncated)"
        } else {
            ""
        },
        matches.join("\n")
    ))
}

fn is_ignored_dir(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .map(|name| {
            matches!(
                name,
                ".git" | ".venv" | "venv" | "node_modules" | "target" | "dist" | "__pycache__"
            )
        })
        .unwrap_or(false)
}

fn is_text_candidate(path: &PathBuf) -> bool {
    const EXTENSIONS: &[&str] = &[
        "rs", "ts", "tsx", "js", "jsx", "py", "toml", "json", "md", "css", "html", "sql", "yaml",
        "yml", "ps1", "cmd", "sh", "txt", "xml", "c", "h", "cpp", "hpp", "java", "go",
    ];
    path.extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| EXTENSIONS.contains(&ext.to_ascii_lowercase().as_str()))
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_root() -> PathBuf {
        let root = std::env::temp_dir().join(format!("kilroy-tools-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(root.join("src/lib.rs"), "fn alpha() {}\nfn beta() {}\n").unwrap();
        root
    }

    #[test]
    fn reads_bounded_lines_with_numbers() {
        let root = temp_root();
        let result = execute(
            &root,
            &ToolCall::ReadFile {
                path: "src/lib.rs".into(),
                start_line: Some(2),
                end_line: Some(2),
            },
        );
        assert!(result.success);
        assert!(result.output.contains("2 | fn beta"));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn search_returns_real_file_evidence() {
        let root = temp_root();
        let result = execute(
            &root,
            &ToolCall::SearchFiles {
                query: "alpha".into(),
                path: None,
            },
        );
        assert!(result.success);
        assert!(result.output.contains("src\\lib.rs:1") || result.output.contains("src/lib.rs:1"));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn traversal_is_rejected() {
        let root = temp_root();
        let result = execute(
            &root,
            &ToolCall::ReadFile {
                path: "../secret.txt".into(),
                start_line: None,
                end_line: None,
            },
        );
        assert!(!result.success);
        fs::remove_dir_all(root).unwrap();
    }
}
