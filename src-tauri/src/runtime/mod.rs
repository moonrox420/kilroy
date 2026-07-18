//! Task graph runtime.
//!
//! Two flavours:
//!   * `single_shot` — direct streamed chat reply (Copilot mode).
//!   * `plan_and_execute` — planner agent decomposes the goal into a
//!     DAG of tasks, each executed sequentially with role-specific
//!     system prompts (Autonomous / Multi-Agent / Governance modes).
//!
//! Every task lifecycle event streams to the frontend over Tauri events
//! so the chat panel can render live progress, and every task and its
//! output is persisted to the `tasks` table for the audit trail.

pub mod agent;
pub mod events;
pub mod executor;
pub mod planner;
pub mod tools;
