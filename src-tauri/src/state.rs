//! Shared application state.
//!
//! Holds the registry of live PTY sessions, the current agent mode, the
//! per-project memory DB handle, the embedding + chat clients, and the
//! user-editable `Settings` (shared with Embedder + ChatClient via
//! `Arc<RwLock<Settings>>`).
//!
//! ## PTY session resource caps
//!
//! Every terminal session is bounded so a runaway shell cannot exhaust
//! host memory or hold a PTY open indefinitely:
//!   `PTY_BYTE_CAP`      — 16 MiB total bytes a session may emit before it is
//!                         auto-killed and the UI is notified.
//!   `PTY_IDLE_TIMEOUT`  — 30 minutes without ANY new output before the
//!                         session is killed. Active throughput resets the
//!                         clock; the frontend is expected to send a keepalive
//!                         (or the user keeps typing) to keep the session alive.

use crate::db::Memory;
use crate::embeddings::Embedder;
use crate::generation::ChatClient;
use crate::settings::Settings;
use parking_lot::{Mutex, RwLock};
use portable_pty::{Child, MasterPty};
use std::collections::HashMap;
use std::sync::Arc;

/// Hard ceiling on bytes a single PTY session may emit to the frontend. A
/// session that hits this cap is killed and the UI is notified via
/// `terminal://{id}/bytes_exceeded`. 16 MiB is large enough for substantial
/// output but prevents a `yes | cat` or runaway dump from pinning memory.
pub const PTY_BYTE_CAP: usize = 16 * 1024 * 1024;

/// Seconds of complete silence before a PTY session is auto-killed. The
/// clock resets on every incoming byte (both backlog and live emission).
/// 30 min matches typical interactive-shell expectations without leaking
/// abandoned sessions forever.
pub const PTY_IDLE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30 * 60);

/// Frontend-attachment state for one terminal session.
///
/// The PTY reader thread starts pumping the instant the shell spawns, but the
/// frontend can only register its `terminal://{id}/data` listener AFTER
/// `terminal_spawn` returns the session id (invoke round-trip → React effect
/// → `listen()` registration round-trip). Tauri events are fire-and-forget —
/// there is no replay — so anything emitted in that window (the PowerShell
/// banner, profile output like the user's `Invoke-RagIndex` line, the first
/// prompt, even an instant crash-exit) used to vanish, presenting as a
/// permanently blank terminal with a blinking cursor.
///
/// The pipe buffers every chunk (and the exit, if it already happened) until
/// the frontend calls `terminal_attach`, which flushes the backlog in order
/// and flips the session to live emission. The reader thread emits while
/// holding the lock so flushed backlog and live chunks can never interleave
/// out of order.
pub struct TerminalPipe {
    /// True once the frontend has registered listeners and called attach.
    pub attached: bool,
    /// Output chunks produced before attach, in arrival order.
    pub backlog: Vec<String>,
    /// True if the shell already exited (EOF on the PTY) before attach.
    pub exited: bool,
    /// Running total of bytes emitted for this session (backlog + live).
    /// Used to enforce `PTY_BYTE_CAP`; once reached the session is killed.
    pub output_bytes: usize,
    /// Wall-clock instant of the most recent write into this session.
    /// The reader thread updates this on every chunk; `terminal_write` updates
    /// it too. If the gap between now and `last_write_ts` exceeds
    /// `PTY_IDLE_TIMEOUT`, the session is auto-killed.
    pub last_write_ts: Option<std::time::Instant>,
}

impl TerminalPipe {
    pub fn new() -> Self {
        Self {
            attached: false,
            backlog: Vec::new(),
            exited: false,
            output_bytes: 0,
            last_write_ts: Some(std::time::Instant::now()),
        }
    }
}

impl Default for TerminalPipe {
    fn default() -> Self {
        Self::new()
    }
}

/// One live terminal session.
pub struct PtySession {
    pub master: Box<dyn MasterPty + Send>,
    pub writer: Box<dyn std::io::Write + Send>,
    pub child: Box<dyn Child + Send + Sync>,
    /// Shared with the reader thread; drives the attach/flush handshake.
    pub pipe: Arc<Mutex<TerminalPipe>>,
}

/// Global agent mode. Matches the dropdown in the left pane.
///
/// * `CodeAgent` — Kilroy's default agent (SmartCoder): Python smolagents CodeAgent
///   that writes, executes, and self-corrects against the open project.
/// * `Copilot` — quick-reply Ollama chat only; no code execution loop.
/// * `Autonomous` — planner produces a task DAG; user approves; executor runs it.
/// * `MultiAgent` — same as Autonomous with role-differentiated executor prompts.
/// * `Governance` — analysis only; required output schema, no edits proposed.
/// * `Council` — four-voice adversarial debate on a single question plus a synthesizer recommending a path, for stakes-shaped decisions where disagreement should be rendered visibly.
/// * `Debug` — four-voice debugging swarm (error-reader, recent-changes reviewer, hypothesis generator, fix author); the synthesizer ranks hypotheses and surfaces the most-likely root cause plus a minimal repro/fix. Trigger when a test fails or a stack trace lands, since single agents tunnel-vision on one hypothesis.
/// * `TestFirst` — autonomous plan with a forced QA-then-developer sequence: qa writes failing tests for the feature, then the developer makes them pass, anchoring on the contract before implementation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub enum AgentMode {
    #[default]
    CodeAgent,
    Copilot,
    Autonomous,
    MultiAgent,
    Governance,
    Council,
    Debug,
    TestFirst,
}

pub struct AppState {
    pub ptys: Arc<Mutex<HashMap<String, PtySession>>>,
    pub agent_mode: Arc<Mutex<AgentMode>>,

    /// Memory DB for the currently open project. `None` until the user opens a folder.
    pub memory: Arc<Mutex<Option<Memory>>>,
    pub current_project_id: Arc<Mutex<Option<i64>>>,
    pub current_session_id: Arc<Mutex<Option<i64>>>,

    /// Live, user-editable settings. The embedder, chat client, and sandbox
    /// dispatcher all read from this on every call so edits are picked up
    /// instantly.
    pub settings: Arc<RwLock<Settings>>,

    /// Embedding + chat clients, both sharing the settings handle.
    pub embedder: Embedder,
    pub chat: ChatClient,

    /// Bundled-Ollama child process handle. `None` if a system Ollama
    /// daemon was already running on the configured URL and we deferred
    /// to it, OR if no bundled binary is present. When `Some`, the
    /// RunEvent::Exit hook kills the process so we don't leak `ollama
    /// serve` past the app's lifetime.
    pub bundled_ollama: Arc<Mutex<Option<std::process::Child>>>,
}

impl AppState {
    pub fn new(settings: Settings) -> Self {
        let settings = Arc::new(RwLock::new(settings));
        let embedder = Embedder::new(settings.clone());
        let chat = ChatClient::new(settings.clone());
        Self {
            ptys: Arc::new(Mutex::new(HashMap::new())),
            agent_mode: Arc::new(Mutex::new(AgentMode::default())),
            memory: Arc::new(Mutex::new(None)),
            current_project_id: Arc::new(Mutex::new(None)),
            current_session_id: Arc::new(Mutex::new(None)),
            settings,
            embedder,
            chat,
            bundled_ollama: Arc::new(Mutex::new(None)),
        }
    }

    /// Kill the bundled Ollama daemon (if we started one) at app exit.
    /// Wired from lib.rs as a RunEvent::Exit handler.
    pub fn shutdown(&self) {
        if let Some(mut child) = self.bundled_ollama.lock().take() {
            tracing::info!("killing bundled Ollama daemon on app exit");
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}
