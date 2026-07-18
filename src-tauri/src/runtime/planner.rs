//! Planner — decomposes a goal into a task graph.
//!
//! Calls Ollama in JSON-mode so we get a structured plan back. The plan
//! schema is deliberately small: each task has a type, agent role,
//! title, and the input the task should operate on. The executor takes
//! it from there.

use crate::generation::{ChatClient, ChatOptions};
use anyhow::Result;
use serde::Deserialize;

#[derive(Deserialize, Clone, Debug)]
pub struct RawPlan {
    pub tasks: Vec<RawTask>,
    #[serde(default)]
    pub overview: Option<String>,
}

#[derive(Deserialize, Clone, Debug)]
pub struct RawTask {
    /// One of: plan, review, code, refactor, test, analysis, doc.
    pub r#type: String,
    /// One of: planner, architect, developer, qa, reviewer, orchestrator.
    pub agent: String,
    pub title: String,
    pub input: String,
}

const SYSTEM: &str = r#"You are Kilroy's planner agent. Given a user goal and the relevant project context, decompose the work into a small sequence of concrete tasks (1–6 tasks; pick the smallest number that covers the goal). Each task must be self-contained and executable by a single role.

Available task types: plan, review, code, refactor, test, analysis, doc.
Available agent roles: planner, architect, developer, qa, reviewer, orchestrator.

Respond ONLY with strict JSON in this shape:
{
  "overview": "one short sentence describing the plan",
  "tasks": [
    {"type": "review", "agent": "reviewer", "title": "Concrete task title", "input": "Concrete, verbatim instruction for the agent — never the literal string \"What to do\""},
    {"type": "code",   "agent": "developer", "title": "Another concrete title", "input": "Concrete instruction — never the literal string \"What to write\""}
  ]
}

Rules:
- SINGLE-TASK ESCAPE HATCH: if the goal can be fully completed by one agent in one step (e.g. "explain this function", "add a doc comment to X", "what does this regex do"), return exactly ONE task. Do NOT pad to two or more.
- AMBIGUITY ESCAPE HATCH: if the goal is under-specified, contradictory, or could be interpreted multiple substantively-different ways, return ONE task of type "plan" with agent "planner" whose input field asks the user the specific clarifying question(s) needed before execution can proceed. Do NOT hallucinate a multi-step plan against context you don't have.
- Prefer fewer, larger tasks over many small ones.
- The user goal goes verbatim into one of the tasks' input fields — never paraphrase it away.
- Task `input` fields must be CONCRETE instructions, never placeholder literals like "What to do" or "What to write" — those are from the example above and would confuse the executor.
- Do NOT include shell commands or file paths the agent cannot verify against retrieved code.
- Do NOT wrap the JSON in backticks or any prose.
"#;

/// Additional clause prepended to the planner's system prompt when the
/// caller is in TestFirst mode. The two-task contract is rigid on
/// purpose — TestFirst's whole value is that the user gets to review
/// the test contract before any implementation lands, which means we
/// CANNOT collapse it to a single task even if the goal is small.
const TEST_FIRST_CLAUSE: &str = r#"

# TEST-FIRST OVERRIDE

The user is in TEST-FIRST mode. Your plan MUST follow this shape:

1. First task: type="test", agent="qa", title="Write failing tests for <feature>", input="Concrete spec for the tests — describe what should pass, edge cases, and where the tests live in the project. Produce ONLY failing tests; do not implement the feature."
2. Second task: type="code", agent="developer", title="Implement <feature> to pass the tests", input="The implementation instruction. Reference the tests from task #1 as the contract — your work is done when those tests pass."

You may add a third task (type="review", agent="reviewer") if the change is large enough to benefit from a code review pass. Do not exceed three tasks.

The single-task and ambiguity escape hatches above DO NOT APPLY in TestFirst mode. Always produce at least two tasks (qa then developer) — even for small features. If the goal is genuinely too small for tests (e.g. "add a doc comment"), say so by returning ONE planner task whose input explains that the user should use a different mode."#;

pub async fn plan(client: &ChatClient, user_goal: &str, context: &str) -> Result<RawPlan> {
    plan_with_mode(client, user_goal, context, false).await
}

/// Mode-aware planner. When `test_first` is true, the TestFirst
/// override clause is appended to the system prompt so the planner
/// reliably produces the qa→developer task sequence.
pub async fn plan_with_mode(
    client: &ChatClient,
    user_goal: &str,
    context: &str,
    test_first: bool,
) -> Result<RawPlan> {
    let user = format!(
        "Project context:\n{}\n\nUser goal:\n{}",
        context.trim(),
        user_goal.trim()
    );
    let system: String = if test_first {
        format!("{}{}", SYSTEM, TEST_FIRST_CLAUSE)
    } else {
        SYSTEM.to_string()
    };
    client
        .generate_json::<RawPlan>(
            &system,
            &user,
            Some(ChatOptions {
                temperature: Some(0.2),
                num_predict: Some(1024),
                top_p: None,
                num_ctx: Some(8192),
            }),
        )
        .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_full_plan() {
        let json = r#"{
            "overview": "do the thing",
            "tasks": [
                {"type": "code",   "agent": "developer", "title": "impl",   "input": "write it"},
                {"type": "review", "agent": "reviewer",  "title": "review", "input": "check it"}
            ]
        }"#;
        let plan: RawPlan = serde_json::from_str(json).unwrap();
        assert_eq!(plan.overview.as_deref(), Some("do the thing"));
        assert_eq!(plan.tasks.len(), 2);
        assert_eq!(plan.tasks[0].r#type, "code");
        assert_eq!(plan.tasks[0].agent, "developer");
        assert_eq!(plan.tasks[1].title, "review");
    }

    #[test]
    fn overview_is_optional() {
        let json = r#"{"tasks":[{"type":"plan","agent":"planner","title":"t","input":"i"}]}"#;
        let plan: RawPlan = serde_json::from_str(json).unwrap();
        assert!(plan.overview.is_none());
        assert_eq!(plan.tasks.len(), 1);
    }

    #[test]
    fn missing_required_task_field_is_rejected() {
        // `agent` is absent — deserialization must fail rather than silently
        // produce a half-formed task the executor can't dispatch.
        let json = r#"{"tasks":[{"type":"code","title":"t","input":"i"}]}"#;
        assert!(serde_json::from_str::<RawPlan>(json).is_err());
    }

    #[test]
    fn empty_task_list_parses() {
        let plan: RawPlan = serde_json::from_str(r#"{"tasks":[]}"#).unwrap();
        assert!(plan.tasks.is_empty());
    }
}
