//! Runtime event payloads.
//!
//! These are emitted on the `agent://run/...` channels and consumed by
//! the React runtime store, which renders the task stream inside the
//! chat panel.

use serde::Serialize;

#[derive(Serialize, Clone)]
pub struct RunStarted {
    pub run_id: String,
    pub session_id: i64,
    pub mode: String,
    pub user_message: String,
}

#[derive(Serialize, Clone)]
pub struct PlanReady {
    pub run_id: String,
    pub tasks: Vec<PlannedTask>,
}

#[derive(Serialize, Clone)]
pub struct PlannedTask {
    pub task_id: i64,
    pub r#type: String,
    pub agent: String,
    pub title: String,
    pub input: String,
}

#[derive(Serialize, Clone)]
pub struct TaskStarted {
    pub run_id: String,
    pub task_id: i64,
}

#[derive(Serialize, Clone)]
pub struct TaskChunk {
    pub run_id: String,
    pub task_id: i64,
    pub delta: String,
}

#[derive(Serialize, Clone)]
pub struct TaskCompleted {
    pub run_id: String,
    pub task_id: i64,
    pub success: bool,
    pub output_preview: String,
}

#[derive(Serialize, Clone)]
pub struct RunCompleted {
    pub run_id: String,
    pub success: bool,
    pub summary: String,
}
