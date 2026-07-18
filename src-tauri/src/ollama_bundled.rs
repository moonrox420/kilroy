//! Bundled-Ollama detection and lifecycle.
//!
//! Kilroy ships an Ollama binary inside `resources/ollama/` (populated
//! at build time by `scripts/fetch-ollama.ps1`). On boot we check
//! whether the user already has a system-level Ollama listening on the
//! configured URL — if yes, defer to that and do nothing. If no, start
//! the bundled binary as a managed child process pointed at the same
//! port.
//!
//! Why both? Some users have Ollama installed system-wide (via winget
//! or the Ollama installer), which auto-starts on boot. We don't want
//! to fight that — running two daemons on the same port would collide.
//! When no system Ollama is present, the bundled copy makes Kilroy
//! self-sufficient on a fresh consumer machine.
//!
//! Lifecycle: spawned at app setup, killed on app exit. The child is
//! held in `AppState.bundled_ollama` so its handle survives.

use anyhow::{Context, Result};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use tauri::{AppHandle, Manager};

/// Resolved path to the bundled `ollama.exe`, if present.
pub fn bundled_path(app: &AppHandle) -> Option<PathBuf> {
    let res = app.path().resource_dir().ok()?;
    let candidate = res.join("resources").join("ollama").join("ollama.exe");
    if candidate.exists() {
        Some(candidate)
    } else {
        None
    }
}

/// Probe the configured Ollama URL with a short timeout. Returns true if
/// a daemon is already answering (and we should defer).
pub async fn already_running(url: &str) -> bool {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_millis(1500))
        .build();
    let Ok(client) = client else {
        return false;
    };
    let probe = format!("{}/api/tags", url.trim_end_matches('/'));
    client
        .get(&probe)
        .send()
        .await
        .map(|r| r.status().is_success())
        .unwrap_or(false)
}

/// Spawn the bundled Ollama daemon. Returns the Child handle so the
/// caller can keep it alive for the app's lifetime.
pub fn spawn_bundled(binary: &PathBuf) -> Result<Child> {
    let mut cmd = Command::new(binary);
    cmd.arg("serve");
    cmd.stdout(Stdio::null());
    cmd.stderr(Stdio::null());
    // Hide the console window on Windows.
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd.spawn()
        .with_context(|| format!("spawn bundled Ollama at {}", binary.display()))
}

/// Convenience helper called from app setup. Returns Some(Child) if we
/// started a bundled instance, None if we deferred to a running system
/// Ollama or no bundled binary is available.
pub async fn ensure_running(app: &AppHandle, url: &str) -> Option<Child> {
    if already_running(url).await {
        tracing::info!(
            ollama = "system",
            url = url,
            "deferring to existing Ollama daemon"
        );
        return None;
    }

    let bin = bundled_path(app)?;
    tracing::info!(
        ollama = "bundled",
        path = %bin.display(),
        "starting bundled Ollama daemon"
    );

    let child = match spawn_bundled(&bin) {
        Ok(child) => child,
        Err(e) => {
            tracing::warn!(
                "bundled Ollama failed to start: {:#} — falling back to system",
                e
            );
            return None;
        }
    };

    // Avoid UI false-negatives: smartcoder_status() may run immediately after
    // app startup. Wait briefly for /api/tags to become reachable.
    //
    // This prevents "Start Ollama (ollama serve)" hints while the daemon is
    // still booting.
    let mut last_ok = false;
    for _ in 0..6 {
        if already_running(url).await {
            last_ok = true;
            break;
        }
        // Avoid adding a tokio dependency in this crate.
        // This is called during app startup in an async context.
        std::thread::sleep(std::time::Duration::from_millis(500));
    }

    if !last_ok {
        tracing::warn!(
            ollama = "bundled",
            url = url,
            "bundled Ollama spawned but /api/tags did not become reachable within timeout"
        );
    }

    Some(child)
}
