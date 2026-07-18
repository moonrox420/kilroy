//! Native policy and classification primitives shared by Kilroy runtimes.
//!
//! This crate contains no Python ABI and performs no I/O. Decisions based on
//! compiler or test success belong to the runtime evidence layer, not here.

use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaskClassification {
    pub complexity: TaskComplexity,
    pub blocked_keywords: Vec<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TaskComplexity {
    Conversational,
    Simple,
    Complex,
}

pub fn classify_task(task: &str) -> TaskClassification {
    let normalized = task
        .trim()
        .trim_matches(|character: char| character.is_ascii_punctuation())
        .to_ascii_lowercase();
    let greetings = [
        "hi",
        "hello",
        "hey",
        "howdy",
        "good morning",
        "good afternoon",
        "good evening",
    ];
    if greetings.contains(&normalized.as_str()) {
        return TaskClassification {
            complexity: TaskComplexity::Conversational,
            blocked_keywords: Vec::new(),
        };
    }

    const SYSTEM_KEYWORDS: &[&str] = &[
        "database",
        "api",
        "auth",
        "migration",
        "deploy",
        "config",
        "endpoint",
        "route",
        "schema",
        "async",
        "websocket",
        "middleware",
        "plugin",
        "container",
        "docker",
        "kubernetes",
        "pipeline",
        "observability",
        "security",
        "encrypt",
        "oauth",
        "jwt",
        "session",
    ];
    let blocked_keywords = SYSTEM_KEYWORDS
        .iter()
        .filter(|keyword| normalized.contains(**keyword))
        .map(|keyword| (*keyword).to_string())
        .collect::<Vec<_>>();
    let simple_phrases = [
        "simple function",
        "add two numbers",
        "hello world",
        "small script",
        "one-liner",
    ];
    let complexity = if task.chars().count() < 120
        && blocked_keywords.is_empty()
        && simple_phrases
            .iter()
            .any(|phrase| normalized.contains(phrase))
    {
        TaskComplexity::Simple
    } else {
        TaskComplexity::Complex
    };
    TaskClassification {
        complexity,
        blocked_keywords,
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VetoRecord {
    pub veto_id: String,
    pub target: String,
    pub reason: String,
    pub dimensions: Vec<String>,
    pub overridden: bool,
    pub override_reason: Option<String>,
    pub timestamp: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuardianReport {
    pub passed: bool,
    pub vetoes: Vec<VetoRecord>,
    pub warnings: Vec<String>,
    pub recommendations: Vec<String>,
}

#[derive(Debug, Default)]
pub struct ArchitectureGuardian {
    veto_log: Vec<VetoRecord>,
}

impl ArchitectureGuardian {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn evaluate(
        &mut self,
        content: &str,
        context: Option<&HashMap<String, String>>,
    ) -> GuardianReport {
        let lowered = content.to_ascii_lowercase();
        let mut vetoes = Vec::new();
        let mut warnings = Vec::new();
        let mut recommendations = Vec::new();

        for (pattern, id, reason) in [
            ("exec(", "exec", "Dynamic exec is prohibited."),
            ("eval(", "eval", "Dynamic eval is prohibited."),
            (
                "os.system",
                "os-system",
                "Unstructured os.system execution is prohibited.",
            ),
            (
                "subprocess.call",
                "subprocess",
                "Unstructured subprocess.call execution is prohibited.",
            ),
        ] {
            if lowered.contains(pattern) {
                vetoes.push(veto(id, content, reason, "security"));
            }
        }
        if lowered.contains("except:") {
            vetoes.push(veto(
                "bare-except",
                content,
                "Bare exception handlers hide failures.",
                "maintainability",
            ));
        }
        if lowered.contains("import *") {
            warnings.push("Wildcard import weakens namespace ownership.".to_string());
        }
        if lowered.contains("select *") {
            warnings.push("Unbounded SELECT * may create unnecessary data transfer.".to_string());
        }
        if lowered.contains("for ") {
            recommendations.push("Verify loop complexity against expected input size.".to_string());
        }

        if let Some(existing) = context.and_then(|values| values.get("existing_architecture")) {
            let avoid = Regex::new(r"(?im)^\s*(?:avoid|banned)\s*:\s*(.+)$")
                .expect("static architecture constraint regex");
            for capture in avoid.captures_iter(existing) {
                let term = capture
                    .get(1)
                    .map(|value| value.as_str().trim())
                    .unwrap_or("");
                if !term.is_empty() && lowered.contains(&term.to_ascii_lowercase()) {
                    vetoes.push(veto(
                        "architecture-constraint",
                        content,
                        &format!("Proposed content violates the architecture constraint: {term}"),
                        "architecture",
                    ));
                }
            }
        }

        self.veto_log.extend(vetoes.iter().cloned());
        GuardianReport {
            passed: vetoes.is_empty(),
            vetoes,
            warnings,
            recommendations,
        }
    }

    pub fn override_veto(&mut self, veto_id: &str, reason: &str) -> bool {
        let Some(veto) = self
            .veto_log
            .iter_mut()
            .find(|veto| veto.veto_id == veto_id && !veto.overridden)
        else {
            return false;
        };
        veto.overridden = true;
        veto.override_reason = Some(reason.to_string());
        true
    }

    pub fn unresolved_vetoes(&self) -> impl Iterator<Item = &VetoRecord> {
        self.veto_log.iter().filter(|veto| !veto.overridden)
    }
}

fn veto(id: &str, content: &str, reason: &str, dimension: &str) -> VetoRecord {
    VetoRecord {
        veto_id: id.to_string(),
        target: content.chars().take(120).collect(),
        reason: reason.to_string(),
        dimensions: vec![dimension.to_string()],
        overridden: false,
        override_reason: None,
        timestamp: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_secs_f64())
            .unwrap_or_default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn greeting_is_conversational() {
        assert_eq!(
            classify_task("Hello!").complexity,
            TaskComplexity::Conversational
        );
    }

    #[test]
    fn dangerous_dynamic_execution_is_vetoed() {
        let report = ArchitectureGuardian::new().evaluate("eval(user_input)", None);
        assert!(!report.passed);
        assert_eq!(report.vetoes[0].veto_id, "eval");
    }
}
