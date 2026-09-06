//! Editor-Grounded Coder runtime.
//!
//! Replaces open-ended multi-step ReAct loops with a fast, deterministic,
//! editor-grounded coder loop based on SEARCH/REPLACE blocks.
//!
//! Grounds directly on:
//! 1. Active editor file & in-memory buffer
//! 2. Active editor selection / cursor line
//! 3. Explicitly referenced `@file` mentions
//!
//! Converts model SEARCH/REPLACE blocks via fuzzy matching into canonical
//! unified diffs, stages pending actions in SQLite, and emits `agent://editor/preview`
//! so Monaco `<DiffEditor />` immediately renders a side-by-side visual diff.

use super::agent::{self, AgentRequest, AgentRun, VerificationStatus};
use crate::actuator::{self, ActionPayload};
use crate::commands::agent_context::BuiltAgentContext;
use crate::db::agent_runtime;
use crate::generation::ChatOptions;
use crate::state::AppState;
use anyhow::{anyhow, Result};
use std::collections::HashMap;
use std::path::Path;
use std::time::Instant;
use tauri::AppHandle;

/// Build the master editor-coder system prompt mandating SEARCH/REPLACE blocks.
pub fn build_editor_coder_system_prompt(language_name: &str, file_path: &str) -> String {
    format!(
        r#"You are an elite Lead Software Architect editing '{file_path}' in {lang_upper}.
You modify existing code using precise SEARCH/REPLACE blocks. This ensures focused, safe, and easily reviewable diffs.

===============================================================================
SEARCH / REPLACE BLOCK SPECIFICATION
===============================================================================
Every edit MUST use this exact format:

```{lang_lower} path={file_path}
<<<<<<< SEARCH
[exact lines from the file to locate]
=======
[replacement lines to insert]
>>>>>>> REPLACE
```

CRITICAL RULES:
1. SEARCH block must match existing code EXACTLY (including indentation and line breaks).
2. Include enough context (2-4 surrounding lines) to make the search match unique.
3. Do NOT rewrite unchanged parts of the file. Only target the lines that must change.
4. Multiple blocks are allowed in a single response for edits across different functions.
5. ABSOLUTELY ZERO PLACEHOLDERS: NEVER use '// ...' or '// rest remains unchanged' inside REPLACE blocks.
6. Provide a concise, clear explanation of your changes."#,
        lang_upper = language_name.to_uppercase(),
        lang_lower = language_name.to_lowercase(),
        file_path = file_path,
    )
}

/// Infer language display name and markdown tag from file extension.
pub fn infer_language(path: &str) -> (&'static str, &'static str) {
    let ext = Path::new(path)
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();

    match ext.as_str() {
        "rs" => ("Rust", "rust"),
        "ts" => ("TypeScript", "typescript"),
        "tsx" => ("TypeScript (TSX)", "tsx"),
        "js" => ("JavaScript", "javascript"),
        "jsx" => ("JavaScript (JSX)", "jsx"),
        "py" => ("Python", "python"),
        "go" => ("Go", "go"),
        "cpp" | "cc" | "cxx" | "h" | "hpp" => ("C++", "cpp"),
        "c" => ("C", "c"),
        "cs" => ("C#", "csharp"),
        "java" => ("Java", "java"),
        "html" | "htm" => ("HTML", "html"),
        "css" => ("CSS", "css"),
        "sql" => ("SQL", "sql"),
        "json" => ("JSON", "json"),
        "toml" => ("TOML", "toml"),
        "yaml" | "yml" => ("YAML", "yaml"),
        "md" => ("Markdown", "markdown"),
        "sh" | "bash" => ("Bash", "bash"),
        _ => ("Source Code", "text"),
    }
}

/// Run the editor-grounded coder pass.
pub async fn run_editor_coder(
    app: &AppHandle,
    state: &AppState,
    request: AgentRequest,
    context: &BuiltAgentContext,
) -> Result<AgentRun> {
    let root = context
        .project_root
        .as_deref()
        .ok_or_else(|| anyhow!("open a project before running Code mode"))?;

    // Determine target file: active file has highest priority, then referenced files
    let (target_path, file_content, lang_name, lang_tag) = if let Some(ref af) = context.active_file
    {
        let (name, tag) = infer_language(&af.path);
        let content = if let Some(ref c) = af.contents {
            c.clone()
        } else if let Ok(abs) = actuator::resolve_safe(root, &af.path) {
            std::fs::read_to_string(&abs).unwrap_or_default()
        } else {
            String::new()
        };
        (af.path.clone(), content, name, tag)
    } else if let Some(rf) = context.referenced_files.first() {
        let (name, tag) = infer_language(&rf.path);
        (rf.path.clone(), rf.contents.clone(), name, tag)
    } else {
        // No grounded file detected: fall back to the project tool agent loop
        return agent::run_code(app, state, request, context).await;
    };

    let memory = state
        .memory
        .lock()
        .as_ref()
        .map(|m| m.conn.clone())
        .ok_or_else(|| anyhow!("project memory is not initialized"))?;
    let default_sandbox = state.settings.read().default_sandbox.clone();

    {
        let conn = memory.lock();
        agent_runtime::create_run(
            &conn,
            &request.run_id,
            request.session_id,
            request.mode.as_str(),
            &request.message,
        )?;
    }

    agent::emit_progress(
        app,
        &request.run_id,
        1,
        "thinking",
        &format!(
            "Grounded on '{}' — generating SEARCH/REPLACE edits",
            target_path
        ),
    );

    let step_started = Instant::now();
    let step_id = {
        let conn = memory.lock();
        agent_runtime::create_step(
            &conn,
            &request.run_id,
            1,
            "editor_coder",
            Some(&format!("Target: {}", target_path)),
        )?
    };

    let system_prompt = build_editor_coder_system_prompt(lang_name, &target_path);

    let mut user_prompt =
        format!("CURRENT FILE: {target_path}\n```{lang_tag}\n{file_content}\n```\n\n");

    if let Some(ref af) = context.active_file {
        if let Some(ref sel) = af.selection {
            if !sel.trim().is_empty() {
                user_prompt.push_str(&format!(
                    "USER SELECTION (Line {}):\n{}\n\n",
                    af.cursor_line.unwrap_or(1),
                    sel
                ));
            }
        }
    }

    user_prompt.push_str(&format!("USER GOAL:\n{}\n\n", request.message));
    user_prompt
        .push_str("Provide focused SEARCH/REPLACE blocks to implement the requested change.");

    let model_result = state
        .chat
        .generate_text_with_images(
            &system_prompt,
            &user_prompt,
            request.images.clone(),
            Some(ChatOptions {
                temperature: Some(0.1),
                num_predict: Some(4096),
                top_p: None,
                num_ctx: Some(16_384),
            }),
        )
        .await;

    let model_reply = match model_result {
        Ok(reply) => reply,
        Err(err) => {
            let msg = format!("model generation failed: {err:#}");
            let conn = memory.lock();
            let _ = agent_runtime::finish_step(
                &conn,
                step_id,
                "failed",
                None,
                Some(&msg),
                1,
                step_started.elapsed().as_millis() as i64,
            );
            let _ = agent_runtime::finish_run(
                &conn,
                &request.run_id,
                "failed",
                "blocked",
                None,
                Some(&msg),
                1,
                0,
                0,
            );
            return Err(anyhow!(msg));
        }
    };

    // 1. Extract SEARCH/REPLACE blocks
    let raw_blocks = actuator::search_replace::extract_search_replace_blocks(&model_reply);
    let mut pending_action_ids = Vec::new();

    if !raw_blocks.is_empty() {
        // Group blocks by file path (defaulting to target_path if block has no path)
        let mut by_file: HashMap<String, Vec<actuator::search_replace::SearchReplaceBlock>> =
            HashMap::new();
        for b in raw_blocks {
            let path = b.path.clone().unwrap_or_else(|| target_path.clone());
            by_file.entry(path).or_default().push(b);
        }

        for (path, file_blocks) in by_file {
            let content_to_patch = if path == target_path {
                file_content.clone()
            } else if let Ok(abs) = actuator::resolve_safe(root, &path) {
                std::fs::read_to_string(&abs).unwrap_or_default()
            } else {
                String::new()
            };

            match actuator::search_replace::apply_search_replace(
                &path,
                &content_to_patch,
                &file_blocks,
            ) {
                Ok(patch_res) => {
                    let payload = ActionPayload::FilePatch {
                        path: path.clone(),
                        unified_diff: patch_res.unified_diff,
                    };
                    match agent::persist_proposal(
                        app,
                        root,
                        &memory,
                        &request,
                        step_id,
                        payload,
                        &default_sandbox,
                    ) {
                        Ok(id) => pending_action_ids.push(id),
                        Err(e) => tracing::error!("failed to persist search_replace action: {e:#}"),
                    }
                }
                Err(err) => {
                    tracing::warn!("SEARCH/REPLACE failed for {path}: {err:#}");
                }
            }
        }
    }

    // 2. Fallback: if no SEARCH/REPLACE blocks, check for code block replacement
    if pending_action_ids.is_empty() {
        let code_blocks = actuator::parser::extract_blocks(&model_reply);
        for b in code_blocks {
            let path = b.path.unwrap_or_else(|| target_path.clone());
            if file_content.is_empty() {
                let payload = ActionPayload::FileWrite {
                    path: path.clone(),
                    content: b.body.clone(),
                    language: b.lang,
                };
                if let Ok(id) = agent::persist_proposal(
                    app,
                    root,
                    &memory,
                    &request,
                    step_id,
                    payload,
                    &default_sandbox,
                ) {
                    pending_action_ids.push(id);
                }
            } else if let Some(diff) = actuator::diff_against_disk(root, &path, &b.body) {
                let payload = ActionPayload::FilePatch {
                    path: path.clone(),
                    unified_diff: diff,
                };
                if let Ok(id) = agent::persist_proposal(
                    app,
                    root,
                    &memory,
                    &request,
                    step_id,
                    payload,
                    &default_sandbox,
                ) {
                    pending_action_ids.push(id);
                }
            }
        }
    }

    let conn = memory.lock();
    if !pending_action_ids.is_empty() {
        let _ = agent_runtime::finish_step(
            &conn,
            step_id,
            "completed",
            Some("Proposed editor code patch"),
            None,
            1,
            step_started.elapsed().as_millis() as i64,
        );
        let _ = agent_runtime::finish_run(
            &conn,
            &request.run_id,
            "awaiting_approval",
            "unverified",
            Some("Proposed editor code patch"),
            None,
            1,
            0,
            0,
        );
        agent::emit_progress(
            app,
            &request.run_id,
            1,
            "awaiting_approval",
            &format!("Proposed code changes for {}", target_path),
        );
        Ok(AgentRun {
            run_id: request.run_id,
            status: "awaiting_approval".into(),
            verification_status: VerificationStatus::Unverified,
            summary: model_reply,
            pending_action_ids,
        })
    } else {
        let _ = agent_runtime::finish_step(
            &conn,
            step_id,
            "completed",
            Some("Replied without code changes"),
            None,
            1,
            step_started.elapsed().as_millis() as i64,
        );
        let _ = agent_runtime::finish_run(
            &conn,
            &request.run_id,
            "completed",
            "verified",
            Some("Replied without code changes"),
            None,
            1,
            0,
            0,
        );
        Ok(AgentRun {
            run_id: request.run_id,
            status: "completed".into(),
            verification_status: VerificationStatus::Verified,
            summary: model_reply,
            pending_action_ids: Vec::new(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn infers_languages_correctly() {
        assert_eq!(infer_language("src/main.rs"), ("Rust", "rust"));
        assert_eq!(infer_language("src/App.tsx"), ("TypeScript (TSX)", "tsx"));
        assert_eq!(infer_language("script.py"), ("Python", "python"));
        assert_eq!(infer_language("unknown.xyz"), ("Source Code", "text"));
    }

    #[test]
    fn builds_editor_coder_system_prompt() {
        let p = build_editor_coder_system_prompt("Rust", "src/lib.rs");
        assert!(p.contains("RUST"));
        assert!(p.contains("src/lib.rs"));
        assert!(p.contains("<<<<<<< SEARCH"));
        assert!(p.contains(">>>>>>> REPLACE"));
    }
}
