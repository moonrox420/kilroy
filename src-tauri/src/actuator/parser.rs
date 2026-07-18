//! Fenced-block parser for task outputs.
//!
//! Accepted info-string shapes (inside ```...```):
//!   * `rust`                    → bare code block, no path hint (skipped)
//!   * `rust src/lib.rs`         → file write to src/lib.rs
//!   * `path=src/lib.rs`         → file write, language inferred from path
//!   * `bash` / `sh` / `powershell` / `cmd` → shell command
//!
//! The parser is intentionally permissive: it tolerates indented fences,
//! Windows newlines, and missing closing fences (treats EOF as a close).

#[derive(Debug, Clone)]
pub struct Block {
    pub lang: Option<String>,
    pub path: Option<String>,
    pub body: String,
}

pub fn extract_blocks(text: &str) -> Vec<Block> {
    let mut out = Vec::new();
    let lines: Vec<&str> = text.split_inclusive('\n').collect();
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        let stripped = line.trim_start();
        if !stripped.starts_with("```") {
            i += 1;
            continue;
        }
        let info = stripped
            .trim_start_matches('`')
            .trim()
            .trim_end_matches('\n');
        let (lang, path) = parse_info(info);
        i += 1;
        let mut body = String::new();
        while i < lines.len() {
            let l = lines[i];
            if l.trim_start().starts_with("```") {
                i += 1;
                break;
            }
            body.push_str(l);
            i += 1;
        }
        if !body.is_empty() {
            out.push(Block { lang, path, body });
        }
    }
    out
}

fn parse_info(info: &str) -> (Option<String>, Option<String>) {
    if info.is_empty() {
        return (None, None);
    }
    let mut lang: Option<String> = None;
    let mut path: Option<String> = None;

    for token in info.split_whitespace() {
        if let Some(rest) = token.strip_prefix("path=") {
            path = Some(rest.to_string());
        } else if let Some(rest) = token.strip_prefix("file=") {
            path = Some(rest.to_string());
        } else if token.contains('/') || token.contains('\\') || token.contains('.') {
            // Heuristic: looks like a file path.
            if path.is_none() && lang.is_some() {
                path = Some(token.to_string());
            } else if path.is_none() {
                // First token but it's a path? Treat as path; infer language from extension.
                path = Some(token.to_string());
            }
        } else if lang.is_none() {
            lang = Some(token.to_string());
        }
    }

    if let Some(p) = path.as_deref() {
        if lang.is_none() {
            lang = lang_from_path(p);
        }
    }
    (lang, path)
}

fn lang_from_path(p: &str) -> Option<String> {
    let ext = std::path::Path::new(p)
        .extension()?
        .to_str()?
        .to_lowercase();
    Some(
        match ext.as_str() {
            "rs" => "rust",
            "ts" | "tsx" => "typescript",
            "js" | "jsx" | "mjs" | "cjs" => "javascript",
            "py" => "python",
            "go" => "go",
            "md" => "markdown",
            "json" => "json",
            "toml" => "toml",
            "yml" | "yaml" => "yaml",
            "sh" | "bash" | "zsh" => "shell",
            "ps1" | "psm1" => "powershell",
            "html" | "htm" => "html",
            "css" => "css",
            "sql" => "sql",
            "c" | "h" => "c",
            "cpp" | "cc" | "hpp" => "cpp",
            other => return Some(other.to_string()),
        }
        .to_string(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_lang_and_path_block() {
        let text = "intro\n```rust src/lib.rs\nfn main() {}\n```\noutro";
        let blocks = extract_blocks(text);
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].lang.as_deref(), Some("rust"));
        assert_eq!(blocks[0].path.as_deref(), Some("src/lib.rs"));
        assert!(blocks[0].body.contains("fn main()"));
    }

    #[test]
    fn path_eq_form_infers_language() {
        let text = "```path=src/x.ts\nconst a = 1;\n```";
        let b = extract_blocks(text);
        assert_eq!(b[0].path.as_deref(), Some("src/x.ts"));
        assert_eq!(b[0].lang.as_deref(), Some("typescript"));
    }

    #[test]
    fn bare_lang_has_no_path() {
        let text = "```bash\nls -la\n```";
        let b = extract_blocks(text);
        assert_eq!(b.len(), 1);
        assert_eq!(b[0].lang.as_deref(), Some("bash"));
        assert!(b[0].path.is_none());
    }

    #[test]
    fn unclosed_fence_closes_at_eof() {
        // Tolerate a missing closing fence (common in truncated LLM output).
        let text = "```rust src/a.rs\nfn a() {}";
        let b = extract_blocks(text);
        assert_eq!(b.len(), 1);
        assert!(b[0].body.contains("fn a()"));
    }

    #[test]
    fn ignores_non_fenced_text() {
        let text = "just some prose with no code blocks at all";
        assert!(extract_blocks(text).is_empty());
    }

    #[test]
    fn handles_crlf_line_endings() {
        let text = "```rust src/a.rs\r\nfn a() {}\r\n```\r\n";
        let b = extract_blocks(text);
        assert_eq!(b.len(), 1);
        assert_eq!(b[0].path.as_deref(), Some("src/a.rs"));
    }
}
