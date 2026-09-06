//! Search / Replace block parser and fuzzy matching engine.
//!
//! Provides robust, whitespace-tolerant SEARCH/REPLACE block parsing and
//! application. This is vastly more reliable for local LLMs (Qwen 2.5 Coder,
//! DeepSeek, Llama) than unified diffs, which frequently miscalculate line
//! numbers in `@@ -l,s +l,s @@` hunk headers.
//!
//! Format accepted:
//! ```text
//! [optional file path]
//! <<<<<<< SEARCH
//! [existing code to replace]
//! =======
//! [replacement code]
//! >>>>>>> REPLACE
//! ```

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};

/// A single SEARCH/REPLACE edit block.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchReplaceBlock {
    /// Optional target file path (inferred from preceding lines, code fence, or SEARCH header).
    pub path: Option<String>,
    /// Exact or near-exact code from target file to locate.
    pub search: String,
    /// New code to substitute in place of `search`.
    pub replace: String,
}

/// Result of applying one or more search/replace blocks to file content.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchReplaceResult {
    /// Fully patched text.
    pub patched_content: String,
    /// Canonical unified diff computed from `original` -> `patched_content`.
    pub unified_diff: String,
    /// Number of blocks successfully applied.
    pub blocks_applied: usize,
    /// Total number of blocks attempted.
    pub total_blocks: usize,
}

/// Parse all SEARCH/REPLACE blocks from LLM text.
///
/// Can extract blocks anywhere in the text: inside code fences, in raw markdown,
/// or tagged with file paths.
pub fn extract_search_replace_blocks(text: &str) -> Vec<SearchReplaceBlock> {
    let mut blocks = Vec::new();
    let lines: Vec<&str> = text.lines().collect();
    let mut i = 0;

    while i < lines.len() {
        let line = lines[i].trim();

        // Check if line looks like <<<<<<< SEARCH or <<<<<<< SEARCH: path/to/file
        if line.starts_with("<<<<<<<") && line.contains("SEARCH") {
            let mut path = None;

            // Check if path is embedded in header: <<<<<<< SEARCH: src/main.rs
            if let Some(pos) = line.find(':') {
                let candidate = line[pos + 1..].trim();
                if !candidate.is_empty() {
                    path = Some(candidate.to_string());
                }
            }

            // If no path in header, look back 1-3 lines for a file path or code fence info
            if path.is_none() {
                let mut lookback = 1;
                while lookback <= 3 && i >= lookback {
                    let prev = lines[i - lookback].trim();
                    if prev.starts_with("```") {
                        let info = prev.trim_start_matches('`').trim();
                        for token in info.split_whitespace() {
                            if let Some(p) = token
                                .strip_prefix("path=")
                                .or_else(|| token.strip_prefix("file="))
                            {
                                path = Some(p.to_string());
                                break;
                            } else if token.contains('/')
                                || token.contains('\\')
                                || token.contains('.')
                            {
                                path = Some(token.to_string());
                                break;
                            }
                        }
                        break;
                    } else if (prev.contains('/') || prev.contains('\\') || prev.contains('.'))
                        && !prev.starts_with('#')
                        && !prev.contains(' ')
                    {
                        path = Some(
                            prev.trim_matches(|c| c == '`' || c == '*' || c == ':')
                                .to_string(),
                        );
                        break;
                    }
                    lookback += 1;
                }
            }

            i += 1;
            let mut search_lines = Vec::new();
            while i < lines.len() {
                let l = lines[i];
                if l.trim().starts_with("=======") {
                    i += 1;
                    break;
                }
                search_lines.push(l);
                i += 1;
            }

            let mut replace_lines = Vec::new();
            while i < lines.len() {
                let l = lines[i];
                if l.trim().starts_with(">>>>>>>") && l.contains("REPLACE") {
                    i += 1;
                    break;
                }
                replace_lines.push(l);
                i += 1;
            }

            let search = search_lines.join("\n");
            let replace = replace_lines.join("\n");

            if !search.is_empty() || !replace.is_empty() {
                blocks.push(SearchReplaceBlock {
                    path,
                    search,
                    replace,
                });
            }
            continue;
        }

        i += 1;
    }

    blocks
}

/// Apply a list of SEARCH/REPLACE blocks sequentially to the original file content.
///
/// Employs a multi-tier matching strategy:
/// 1. Exact substring match.
/// 2. Normalized CRLF/LF line match with whitespace tolerance.
/// 3. Indentation-insensitive line match.
/// 4. Context anchor match (head + tail match with high middle similarity).
pub fn apply_search_replace(
    path: &str,
    original: &str,
    blocks: &[SearchReplaceBlock],
) -> Result<SearchReplaceResult> {
    if blocks.is_empty() {
        return Err(anyhow!("no SEARCH/REPLACE blocks provided to apply"));
    }

    let is_crlf = original.contains("\r\n");
    let mut current = original.to_string();
    let mut applied = 0;

    for (idx, block) in blocks.iter().enumerate() {
        match match_and_replace(&current, &block.search, &block.replace) {
            Some(updated) => {
                current = updated;
                applied += 1;
            }
            None => {
                let search_preview: String =
                    block.search.lines().take(5).collect::<Vec<_>>().join("\n");
                return Err(anyhow!(
                    "Block {}/{} failed to match in '{}'. Could not find:\n---\n{}\n---",
                    idx + 1,
                    blocks.len(),
                    path,
                    search_preview
                ));
            }
        }
    }

    // Ensure newline consistency matches the original file
    if is_crlf && !current.contains("\r\n") {
        current = current.replace('\n', "\r\n");
    }

    // Generate canonical unified diff using `similar`
    let diff = similar::TextDiff::from_lines(original, &current);
    let unified = diff
        .unified_diff()
        .header(&format!("a/{}", path), &format!("b/{}", path))
        .context_radius(3)
        .to_string();

    Ok(SearchReplaceResult {
        patched_content: current,
        unified_diff: unified,
        blocks_applied: applied,
        total_blocks: blocks.len(),
    })
}

/// Attempt to match `search` within `target` and replace it with `replace`.
fn match_and_replace(target: &str, search: &str, replace: &str) -> Option<String> {
    // 0. Empty search block: prepends replacement to the top of the file
    if search.trim().is_empty() {
        if target.is_empty() {
            return Some(replace.to_string());
        }
        return Some(format!("{}\n{}", replace, target));
    }

    // 1. Exact match (fastest)
    if let Some(pos) = target.find(search) {
        let mut res = String::with_capacity(target.len() + replace.len());
        res.push_str(&target[..pos]);
        res.push_str(replace);
        res.push_str(&target[pos + search.len()..]);
        return Some(res);
    }

    // Normalize newlines for multi-tier matching
    let target_normalized = target.replace("\r\n", "\n");
    let search_normalized = search.replace("\r\n", "\n");
    let replace_normalized = replace.replace("\r\n", "\n");

    // 1b. Exact match on normalized newlines
    if let Some(pos) = target_normalized.find(&search_normalized) {
        let mut res = String::with_capacity(target_normalized.len() + replace_normalized.len());
        res.push_str(&target_normalized[..pos]);
        res.push_str(&replace_normalized);
        res.push_str(&target_normalized[pos + search_normalized.len()..]);
        return Some(res);
    }

    let target_lines: Vec<&str> = target_normalized.lines().collect();
    let search_lines: Vec<&str> = search_normalized.lines().collect();

    if search_lines.is_empty() || target_lines.len() < search_lines.len() {
        return None;
    }

    // 2. Line-trimmed match (trailing spaces stripped)
    if let Some(start_line) = find_trimmed_line_match(&target_lines, &search_lines) {
        return Some(replace_line_range(
            &target_lines,
            start_line,
            start_line + search_lines.len(),
            &replace_normalized,
        ));
    }

    // 3. Indentation-insensitive match (strips leading and trailing whitespace)
    if let Some(start_line) = find_indent_insensitive_match(&target_lines, &search_lines) {
        // Compute base indentation difference if needed
        let target_indent = get_indentation(target_lines[start_line]);
        let search_indent = get_indentation(search_lines[0]);
        let adjusted_replace =
            adjust_indentation(&replace_normalized, search_indent, target_indent);

        return Some(replace_line_range(
            &target_lines,
            start_line,
            start_line + search_lines.len(),
            &adjusted_replace,
        ));
    }

    // 4. Anchor match: for blocks >= 3 lines, match head line and tail line,
    // require >= 75% similarity in middle lines
    if search_lines.len() >= 3 {
        if let Some(start_line) = find_anchor_match(&target_lines, &search_lines) {
            return Some(replace_line_range(
                &target_lines,
                start_line,
                start_line + search_lines.len(),
                &replace_normalized,
            ));
        }
    }

    None
}

/// Find start line where all lines match after stripping trailing whitespace.
fn find_trimmed_line_match(target: &[&str], search: &[&str]) -> Option<usize> {
    let window_size = search.len();
    for i in 0..=(target.len() - window_size) {
        let mut matches = true;
        for j in 0..window_size {
            if target[i + j].trim_end() != search[j].trim_end() {
                matches = false;
                break;
            }
        }
        if matches {
            return Some(i);
        }
    }
    None
}

/// Find start line where all lines match after stripping leading AND trailing whitespace.
fn find_indent_insensitive_match(target: &[&str], search: &[&str]) -> Option<usize> {
    let window_size = search.len();
    for i in 0..=(target.len() - window_size) {
        let mut matches = true;
        for j in 0..window_size {
            if target[i + j].trim() != search[j].trim() {
                matches = false;
                break;
            }
        }
        if matches {
            return Some(i);
        }
    }
    None
}

/// Anchor matching: matches the first line and last line strictly (trimmed),
/// and checks that >= 75% of intermediate lines match.
fn find_anchor_match(target: &[&str], search: &[&str]) -> Option<usize> {
    let window_size = search.len();
    let first = search[0].trim();
    let last = search[search.len() - 1].trim();

    for i in 0..=(target.len() - window_size) {
        if target[i].trim() != first || target[i + window_size - 1].trim() != last {
            continue;
        }

        let mut matched_inner = 0;
        let inner_count = window_size - 2;
        for j in 1..=inner_count {
            if target[i + j].trim() == search[j].trim() {
                matched_inner += 1;
            }
        }

        if inner_count == 0 || (matched_inner as f64 / inner_count as f64) >= 0.75 {
            return Some(i);
        }
    }
    None
}

fn get_indentation(line: &str) -> &str {
    let idx = line
        .find(|c: char| !c.is_whitespace())
        .unwrap_or(line.len());
    &line[..idx]
}

fn adjust_indentation(text: &str, orig_indent: &str, new_indent: &str) -> String {
    if orig_indent == new_indent {
        return text.to_string();
    }
    let mut out = Vec::new();
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix(orig_indent) {
            out.push(format!("{}{}", new_indent, rest));
        } else {
            out.push(line.to_string());
        }
    }
    out.join("\n")
}

fn replace_line_range(
    target_lines: &[&str],
    start: usize,
    end: usize,
    replacement: &str,
) -> String {
    let mut out = Vec::new();
    for line in &target_lines[..start] {
        out.push(*line);
    }
    if !replacement.is_empty() {
        out.push(replacement);
    }
    for line in &target_lines[end..] {
        out.push(*line);
    }
    out.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_search_replace_block_with_path() {
        let text = r#"
Here is the fix for the database query:
```rust path=src/db.rs
<<<<<<< SEARCH
    let count = 0;
    println!("count: {}", count);
=======
    let count = 42;
    println!("new count: {}", count);
>>>>>>> REPLACE
```
That should solve the problem.
"#;

        let blocks = extract_search_replace_blocks(text);
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].path.as_deref(), Some("src/db.rs"));
        assert!(blocks[0].search.contains("let count = 0;"));
        assert!(blocks[0].replace.contains("let count = 42;"));
    }

    #[test]
    fn parses_block_with_header_colon_path() {
        let text = r#"
<<<<<<< SEARCH: src/main.rs
fn old_main() {}
=======
fn new_main() {}
>>>>>>> REPLACE
"#;
        let blocks = extract_search_replace_blocks(text);
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].path.as_deref(), Some("src/main.rs"));
        assert_eq!(blocks[0].search.trim(), "fn old_main() {}");
        assert_eq!(blocks[0].replace.trim(), "fn new_main() {}");
    }

    #[test]
    fn applies_exact_match() {
        let original = "line 1\nline 2\nline 3\n";
        let blocks = vec![SearchReplaceBlock {
            path: Some("test.txt".into()),
            search: "line 2\n".into(),
            replace: "line 2 modified\n".into(),
        }];

        let res = apply_search_replace("test.txt", original, &blocks).unwrap();
        assert_eq!(res.blocks_applied, 1);
        assert!(res.patched_content.contains("line 2 modified"));
        assert!(res.unified_diff.contains("-line 2"));
        assert!(res.unified_diff.contains("+line 2 modified"));
    }

    #[test]
    fn applies_whitespace_and_crlf_tolerant_match() {
        let original = "fn calculate() {\r\n    let x = 10;   \r\n    return x;\r\n}";
        let blocks = vec![SearchReplaceBlock {
            path: Some("calc.rs".into()),
            search: "fn calculate() {\n    let x = 10;\n    return x;\n}".into(),
            replace: "fn calculate() {\n    let x = 20;\n    return x * 2;\n}".into(),
        }];

        let res = apply_search_replace("calc.rs", original, &blocks).unwrap();
        assert_eq!(res.blocks_applied, 1);
        assert!(res.patched_content.contains("let x = 20;"));
        assert!(res.patched_content.contains("return x * 2;"));
    }

    #[test]
    fn applies_anchor_matching_when_middle_differs_slightly() {
        let original = r#"pub fn process_data(items: &[Item]) -> Result<()> {
    validate_items(items)?;
    let filtered = items.iter().filter(|i| i.is_active()).collect::<Vec<_>>();
    save_items(&filtered)?;
    Ok(())
}"#;

        let blocks = vec![SearchReplaceBlock {
            path: Some("data.rs".into()),
            search: r#"pub fn process_data(items: &[Item]) -> Result<()> {
    validate_items(items)?;
    let filtered = items.iter().filter(|i| i.active).collect::<Vec<_>>();
    save_items(&filtered)?;
    Ok(())
}"#
            .into(),
            replace: r#"pub fn process_data(items: &[Item]) -> Result<()> {
    validate_items(items)?;
    let filtered = items.iter().filter(|i| i.is_active()).collect::<Vec<_>>();
    save_items(&filtered)?;
    log_audit("data processed")?;
    Ok(())
}"#
            .into(),
        }];

        let res = apply_search_replace("data.rs", original, &blocks).unwrap();
        assert_eq!(res.blocks_applied, 1);
        assert!(res
            .patched_content
            .contains("log_audit(\"data processed\")?;"));
    }

    #[test]
    fn reports_error_with_search_preview_when_unmatched() {
        let original = "fn a() {}\nfn b() {}\n";
        let blocks = vec![SearchReplaceBlock {
            path: Some("missing.rs".into()),
            search: "fn nonexistent() {}".into(),
            replace: "fn replacement() {}".into(),
        }];

        let err = apply_search_replace("missing.rs", original, &blocks).unwrap_err();
        assert!(err.to_string().contains("failed to match in 'missing.rs'"));
        assert!(err.to_string().contains("fn nonexistent() {}"));
    }
}
