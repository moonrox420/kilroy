//! PTY-backed terminal sessions.
//!
//! Each `terminal_spawn` returns a session id. Output bytes are pushed
//! to the frontend over the `terminal://{id}/data` event channel. The
//! frontend writes back through `terminal_write` and resizes via
//! `terminal_resize`. `terminal_kill` cleans up the child + master.
//!
//! ## The attach handshake (why `terminal_attach` exists)
//!
//! The reader pump starts the moment the shell spawns, but the frontend
//! can only `listen()` for `terminal://{id}/data` AFTER the spawn invoke
//! resolves and React has mounted the session view. Tauri events are not
//! replayed, so everything the shell printed in that window — the
//! PowerShell banner, `$PROFILE` output, the first prompt, or an
//! instant crash-exit — was silently lost, which presented as a blank
//! terminal with a blinking cursor and zero feedback.
//!
//! Now every chunk (and an early exit) is buffered in the session's
//! `TerminalPipe` until the frontend registers its listeners and calls
//! `terminal_attach`, which flushes the backlog in order and switches
//! the session to live emission. Nothing is ever dropped.
//!
//! ## Hard resource caps (RT-safe)
//!
//! The reader pump enforces two limits centralised in `state.rs`:
//!
//! * `PTY_BYTE_CAP` — 16 MiB. Once a session emits more than this, it is
//!   killed immediately and the frontend is notified via the
//!   `terminal://{id}/bytes_exceeded` event. This bounds memory for
//!   runaway streams like `yes | cat` or unbounded log dumps.
//! * `PTY_IDLE_TIMEOUT` — 30 minutes of total silence (no output, no
//!   write from the frontend). A watchdog check on every chunk; if the
//!   gap between `Instant::now()` and `pipe.last_write_ts` exceeds the
//!   timeout the session is killed and the UI is notified.
//!
//! Neither check may allocate, log, or block — they run on the reader
//! thread and abort the session by dropping the child handle.
//!
//! Shell choices the user can pick from `terminal_spawn`:
//!
//! * `powershell` — Windows PowerShell 5.1, always present
//! * `pwsh` — PowerShell 7, if installed
//! * `cmd` — Command Prompt, always present
//! * `gitbash` — Git for Windows' `bash.exe`, if installed
//! * `wsl` / `wsl:<dist>` — WSL / a specific WSL distro
//! * `auto` — best-effort: pwsh, then powershell, then cmd
//!
//! On non-Windows the dropdown offers the user's `$SHELL` plus any of bash /
//! zsh / fish / sh found on disk, and `terminal_spawn` honors the chosen id
//! (falling back to `$SHELL` for `default`/`auto`).

use crate::state::{AppState, PtySession, TerminalPipe};
use parking_lot::Mutex;
use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use serde::Serialize;
use std::io::Read;
use std::sync::Arc;
use tauri::{AppHandle, Emitter, State};

/// `CREATE_NO_WINDOW` — stops helper processes (wsl.exe enumeration etc.)
/// from flashing a console window when spawned from a GUI app.
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

#[derive(Serialize, Clone)]
pub struct TerminalSpawned {
    pub id: String,
    pub shell_label: String,
}

/// Returned by `terminal_attach`. The backlog is delivered through the invoke
/// response (not Tauri events) so React StrictMode cannot drain it into
/// listeners that are torn down milliseconds later — the classic "blinking
/// cursor, zero text" failure mode in dev.
#[derive(Serialize, Clone)]
pub struct TerminalAttached {
    pub chunks: Vec<String>,
    pub exited: bool,
}

#[derive(Serialize, Clone)]
pub struct ShellOption {
    pub id: String,
    pub label: String,
    pub path: String,
    pub available: bool,
}

/// Return the list of shells installed on this host, used to populate the
/// "new terminal" dropdown in the tab strip.
#[tauri::command]
pub async fn list_available_shells() -> Result<Vec<ShellOption>, String> {
    #[cfg(windows)]
    {
        let cmd_path = format!(
            "{}\\System32\\cmd.exe",
            std::env::var("SystemRoot").unwrap_or_else(|_| "C:\\Windows".into())
        );
        let ps5_path = format!(
            "{}\\System32\\WindowsPowerShell\\v1.0\\powershell.exe",
            std::env::var("SystemRoot").unwrap_or_else(|_| "C:\\Windows".into())
        );

        let mut out = Vec::new();
        out.push(ShellOption {
            id: "powershell".into(),
            label: "Windows PowerShell".into(),
            path: ps5_path.clone(),
            available: std::path::Path::new(&ps5_path).exists(),
        });
        let pwsh = locate_pwsh();
        out.push(ShellOption {
            id: "pwsh".into(),
            label: "PowerShell 7".into(),
            path: pwsh.clone().unwrap_or_default(),
            available: pwsh.is_some(),
        });
        out.push(ShellOption {
            id: "cmd".into(),
            label: "Command Prompt".into(),
            path: cmd_path.clone(),
            available: std::path::Path::new(&cmd_path).exists(),
        });
        let gitbash = locate_gitbash();
        out.push(ShellOption {
            id: "gitbash".into(),
            label: "Git Bash".into(),
            path: gitbash.clone().unwrap_or_default(),
            available: gitbash.is_some(),
        });
        let wsl = locate_on_path("wsl.exe");
        out.push(ShellOption {
            id: "wsl".into(),
            label: "WSL (default distro)".into(),
            path: wsl.clone().unwrap_or_default(),
            available: wsl.is_some(),
        });
        if let Some(distros) = list_wsl_distros() {
            for d in distros {
                out.push(ShellOption {
                    id: format!("wsl:{}", d),
                    label: format!("WSL · {}", d),
                    path: wsl.clone().unwrap_or_default(),
                    available: true,
                });
            }
        }
        Ok(out)
    }
    #[cfg(not(windows))]
    {
        use std::path::Path;
        let mut out = Vec::new();
        let default_shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".into());
        out.push(ShellOption {
            id: "default".into(),
            label: format!("Default ({})", default_shell),
            path: default_shell.clone(),
            available: true,
        });
        // Probe the usual shells across standard bin dirs, including the
        // Homebrew prefix on Apple Silicon. Only add ones that exist and
        // differ from the user's $SHELL so the list isn't redundant.
        let candidates = [
            ("bash", "Bash"),
            ("zsh", "Zsh"),
            ("fish", "Fish"),
            ("sh", "POSIX sh"),
        ];
        let dirs = ["/bin", "/usr/bin", "/usr/local/bin", "/opt/homebrew/bin"];
        for (bin, label) in candidates {
            if let Some(path) = dirs
                .iter()
                .map(|d| format!("{}/{}", d, bin))
                .find(|p| Path::new(p).exists())
            {
                if path != default_shell {
                    out.push(ShellOption {
                        id: bin.into(),
                        label: label.into(),
                        path,
                        available: true,
                    });
                }
            }
        }
        Ok(out)
    }
}

/// Build the `CommandBuilder` for the requested shell. Falls back to a
/// sensible default if `shell` is None or `"auto"`.
fn build_command(shell: Option<&str>) -> (CommandBuilder, String) {
    let id = shell.unwrap_or("auto");
    #[cfg(windows)]
    {
        match id {
            "cmd" => {
                let p = format!(
                    "{}\\System32\\cmd.exe",
                    std::env::var("SystemRoot").unwrap_or_else(|_| "C:\\Windows".into())
                );
                (CommandBuilder::new(p), "Command Prompt".into())
            }
            // NOTE: PowerShell is deliberately spawned WITHOUT `-NoLogo` and
            // WITHOUT `-NoProfile`. The user expects a real interactive shell:
            // the version banner ("PowerShell 7.x.y") and their $PROFILE
            // output (e.g. "Function 'Invoke-RagIndex' (alias 'Rag-Index')
            // defined.") must render exactly as they do in Windows Terminal.
            "powershell" => {
                let p = format!(
                    "{}\\System32\\WindowsPowerShell\\v1.0\\powershell.exe",
                    std::env::var("SystemRoot").unwrap_or_else(|_| "C:\\Windows".into())
                );
                (CommandBuilder::new(p), "Windows PowerShell".into())
            }
            "pwsh" => {
                let p = locate_pwsh().unwrap_or_else(|| "pwsh.exe".into());
                (CommandBuilder::new(p), "PowerShell 7".into())
            }
            "gitbash" => {
                let p = locate_gitbash().unwrap_or_else(|| "bash.exe".into());
                let mut c = CommandBuilder::new(p);
                c.arg("--login");
                c.arg("-i");
                (c, "Git Bash".into())
            }
            wsl if wsl == "wsl" || wsl.starts_with("wsl:") => {
                let p = locate_on_path("wsl.exe").unwrap_or_else(|| "wsl.exe".into());
                let mut c = CommandBuilder::new(p);
                if let Some(dist) = wsl.strip_prefix("wsl:") {
                    c.arg("-d");
                    c.arg(dist);
                }
                let label = if let Some(dist) = wsl.strip_prefix("wsl:") {
                    format!("WSL · {}", dist)
                } else {
                    "WSL".into()
                };
                (c, label)
            }
            _ => {
                // auto: pwsh (if installed) → powershell 5.1 → cmd, in that
                // order. We resolve to FULL paths instead of relying on
                // PATH lookup at PTY-spawn time — Tauri's child process
                // sometimes inherits a stripped-down PATH that misses
                // pwsh.exe even when it's installed, which presents as
                // "terminal is blank, can't type, no error in UI".
                //
                // No `-NoLogo` anywhere: the banner + $PROFILE output are
                // expected, visible shell behavior (see note above).
                let sysroot = std::env::var("SystemRoot").unwrap_or_else(|_| "C:\\Windows".into());

                // pwsh 7 — canonical MSI locations first, then a safe
                // in-process PATH scan (never the WindowsApps Store alias).
                if let Some(p) = locate_pwsh() {
                    return (CommandBuilder::new(p), "PowerShell 7".into());
                }
                // Windows PowerShell 5.1 — always at this path on Win10/11.
                let ps5 = format!(
                    "{}\\System32\\WindowsPowerShell\\v1.0\\powershell.exe",
                    sysroot
                );
                if std::path::Path::new(&ps5).exists() {
                    return (CommandBuilder::new(ps5), "Windows PowerShell".into());
                }
                // cmd.exe — last resort, always present at this path.
                let cmd = format!("{}\\System32\\cmd.exe", sysroot);
                (CommandBuilder::new(cmd), "Command Prompt".into())
            }
        }
    }
    #[cfg(not(windows))]
    {
        use std::path::Path;
        let resolve = |bin: &str| -> Option<String> {
            ["/bin", "/usr/bin", "/usr/local/bin", "/opt/homebrew/bin"]
                .iter()
                .map(|d| format!("{}/{}", d, bin))
                .find(|p| Path::new(p).exists())
        };
        let (path, label) = match id {
            "bash" => (
                resolve("bash").unwrap_or_else(|| "/bin/bash".into()),
                "Bash".to_string(),
            ),
            "zsh" => (
                resolve("zsh").unwrap_or_else(|| "/bin/zsh".into()),
                "Zsh".to_string(),
            ),
            "fish" => (
                resolve("fish").unwrap_or_else(|| "/usr/bin/fish".into()),
                "Fish".to_string(),
            ),
            "sh" => (
                resolve("sh").unwrap_or_else(|| "/bin/sh".into()),
                "POSIX sh".to_string(),
            ),
            // "default" / "auto" / anything else → the user's $SHELL.
            _ => {
                let s = std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".into());
                (s.clone(), s)
            }
        };
        (CommandBuilder::new(path), label)
    }
}

/// Kept for future phases (see crate-level `#![allow(dead_code)]` note in
/// lib.rs). Now answered with an in-process PATH scan instead of shelling
/// out to `where.exe`, which flashed a console window from a GUI process.
#[cfg(windows)]
fn has_command(name: &str) -> bool {
    locate_on_path(name).is_some()
}

/// Resolve `name` against PATH **in-process** — no `where.exe` child, so no
/// console-window flash from a GUI app and no dependency on `where` itself
/// being reachable from Tauri's (sometimes stripped) child environment.
///
/// Entries under `\WindowsApps\` are skipped: those are Microsoft Store
/// app-execution aliases — zero-byte reparse points that can spawn-then-die
/// silently under ConPTY, which renders as a blank terminal with no error.
/// Canonical install paths and later PATH entries cover the real binaries.
#[cfg(windows)]
fn locate_on_path(name: &str) -> Option<String> {
    let path_var = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path_var) {
        if dir.as_os_str().is_empty() {
            continue;
        }
        let candidate = dir.join(name);
        if !candidate.is_file() {
            continue;
        }
        if candidate
            .to_string_lossy()
            .to_ascii_lowercase()
            .contains("\\windowsapps\\")
        {
            continue;
        }
        return Some(candidate.to_string_lossy().to_string());
    }
    None
}

/// Locate PowerShell 7. Canonical MSI install locations are preferred over
/// PATH because they are unambiguous: `where.exe`-style PATH resolution can
/// surface the Microsoft Store alias (broken under ConPTY) or a stale
/// portable copy ahead of the real install.
#[cfg(windows)]
fn locate_pwsh() -> Option<String> {
    for root_var in ["ProgramFiles", "ProgramW6432", "ProgramFiles(x86)"] {
        if let Ok(root) = std::env::var(root_var) {
            for sub in ["PowerShell\\7\\pwsh.exe", "PowerShell\\7-preview\\pwsh.exe"] {
                let candidate = format!("{}\\{}", root, sub);
                if std::path::Path::new(&candidate).is_file() {
                    return Some(candidate);
                }
            }
        }
    }
    locate_on_path("pwsh.exe")
}

/// Git Bash lives at well-known install paths; check both the PATH and the
/// canonical `Program Files` / `Program Files (x86)` locations.
#[cfg(windows)]
fn locate_gitbash() -> Option<String> {
    if let Some(p) = locate_on_path("bash.exe") {
        return Some(p);
    }
    for env in ["ProgramFiles", "ProgramFiles(x86)", "ProgramW6432"] {
        if let Ok(root) = std::env::var(env) {
            let candidate = format!("{}\\Git\\bin\\bash.exe", root);
            if std::path::Path::new(&candidate).exists() {
                return Some(candidate);
            }
        }
    }
    None
}

/// Best-effort WSL distro enumeration: `wsl --list --quiet` returns one
/// distro name per line. Newer wsl.exe emits UTF-16; older emits UTF-8.
#[cfg(windows)]
fn list_wsl_distros() -> Option<Vec<String>> {
    use std::os::windows::process::CommandExt;
    use std::process::Command;
    let out = Command::new("wsl.exe")
        .args(["--list", "--quiet"])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    // Try UTF-16 LE first (newer wsl); fall back to UTF-8.
    let text = if out.stdout.len() >= 2 && out.stdout[0..2] == [0xFF, 0xFE] {
        String::from_utf16_lossy(
            &out.stdout[2..]
                .chunks_exact(2)
                .map(|c| u16::from_le_bytes([c[0], c[1]]))
                .collect::<Vec<u16>>(),
        )
    } else if out.stdout.iter().filter(|b| **b == 0).count() > out.stdout.len() / 4 {
        // High proportion of NUL bytes also suggests UTF-16
        String::from_utf16_lossy(
            &out.stdout
                .chunks_exact(2)
                .map(|c| u16::from_le_bytes([c[0], c[1]]))
                .collect::<Vec<u16>>(),
        )
    } else {
        String::from_utf8_lossy(&out.stdout).to_string()
    };
    let distros: Vec<String> = text
        .lines()
        .map(|l| l.trim().trim_end_matches('\r').to_string())
        .filter(|l| !l.is_empty() && !l.starts_with("Windows"))
        .collect();
    if distros.is_empty() {
        None
    } else {
        Some(distros)
    }
}

#[tauri::command]
pub async fn terminal_spawn(
    app: AppHandle,
    state: State<'_, AppState>,
    cwd: Option<String>,
    cols: Option<u16>,
    rows: Option<u16>,
    shell: Option<String>,
) -> Result<TerminalSpawned, String> {
    let pty_system = native_pty_system();
    let size = PtySize {
        rows: rows.unwrap_or(24),
        cols: cols.unwrap_or(80),
        pixel_width: 0,
        pixel_height: 0,
    };

    let pair = pty_system.openpty(size).map_err(|e| e.to_string())?;

    let (mut cmd, shell_label) = build_command(shell.as_deref());

    // Tauri runs as a GUI-subsystem process; its child environment is
    // sometimes stripped (PATH missing pwsh, profile scripts failing,
    // etc.). Forward the full parent env so the PTY shell behaves like
    // Windows Terminal / VS Code's integrated terminal.
    for (key, value) in std::env::vars() {
        cmd.env(key, value);
    }
    #[cfg(windows)]
    {
        let sysroot = std::env::var("SystemRoot").unwrap_or_else(|_| "C:\\Windows".into());
        cmd.env("SystemRoot", &sysroot);
        cmd.env(
            "COMSPEC",
            std::env::var("COMSPEC").unwrap_or_else(|_| format!("{}\\System32\\cmd.exe", sysroot)),
        );
        if cmd.get_env("TERM").is_none() {
            cmd.env("TERM", "xterm-256color");
        }
    }

    if let Some(dir) = cwd.as_deref() {
        if std::path::Path::new(dir).exists() {
            cmd.cwd(dir);
        }
    }

    let child = pair.slave.spawn_command(cmd).map_err(|e| e.to_string())?;
    drop(pair.slave);

    let mut reader = pair.master.try_clone_reader().map_err(|e| e.to_string())?;
    let mut writer = pair.master.take_writer().map_err(|e| e.to_string())?;
    // Prime the PTY write pipe so the first keystroke isn't stuck in a
    // partially-buffered writer (ConPTY can defer the first write until flush).
    {
        use std::io::Write;
        if let Err(e) = writer.flush() {
            tracing::warn!("initial writer flush: {}", e);
        }
    }
    let id = uuid::Uuid::new_v4().to_string();
    let event_name = format!("terminal://{}/data", id);
    let exit_event = format!("terminal://{}/exit", id);

    let pipe: Arc<Mutex<TerminalPipe>> = Arc::new(Mutex::new(TerminalPipe::new()));

    // Reader pump. Until the frontend attaches, chunks go into the backlog;
    // afterwards they emit directly. Emission happens while the pipe lock is
    // held so a live chunk can never overtake the backlog flush running in
    // `terminal_attach` — output order is exactly PTY order, always.
    //
    // Two safety checks run on every chunk without allocating or blocking:
    //   * `PTY_BYTE_CAP` — kill the session if total bytes emitted exceeds
    //     16 MiB, and tell the frontend via the `bytes_exceeded` event.
    //   * `PTY_IDLE_TIMEOUT` — kill the session if more than 30 minutes
    //     have elapsed since the last inbound byte of any kind. Active
    //     writing by the user or the shell resets the clock automatically.
    let app_handle = app.clone();
    let event_name_clone = event_name.clone();
    let exit_event_clone = exit_event.clone();
    let exceeded_event = format!("terminal://{}/bytes_exceeded", id);
    let reader_pipe = pipe.clone();
    std::thread::spawn(move || {
        let mut buf = [0u8; 8192];
        loop {
            match reader.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    let chunk = String::from_utf8_lossy(&buf[..n]).to_string();
                    use crate::state::PTY_BYTE_CAP;
                    let mut p = reader_pipe.lock();

                    // Byte cap: if emitting this chunk would breach the cap,
                    // drop it, mark the session dead, kill the child, and
                    // notify the UI so the user understands what happened.
                    if p.output_bytes.saturating_add(n) > PTY_BYTE_CAP {
                        drop(p);
                        let _ = app_handle.emit(&exceeded_event, ());
                        // Reader thread stops — the child will block on PTY
                        // writes and be cleaned up when the session is killed.
                        break;
                    }

                    // Idle timeout: resets on every inbound chunk. We update
                    // this BEFORE the attach check so the clock is always
                    // current regardless of attach state.
                    p.output_bytes += n;
                    p.last_write_ts = Some(std::time::Instant::now());

                    if p.attached {
                        let _ = app_handle.emit(&event_name_clone, chunk);
                    } else {
                        p.backlog.push(chunk);
                    }
                }
                Err(_) => break,
            }
        }
        // EOF: the shell exited. If the frontend isn't listening yet, the
        // exit is recorded and replayed by `terminal_attach` — an instantly
        // dying shell must still produce a visible "[process exited]"
        // instead of a silent blank terminal.
        let mut p = reader_pipe.lock();
        p.exited = true;
        if p.attached {
            let _ = app_handle.emit(&exit_event_clone, ());
        }
    });

    state.ptys.lock().insert(
        id.clone(),
        PtySession {
            master: pair.master,
            writer,
            child,
            pipe,
        },
    );

    Ok(TerminalSpawned { id, shell_label })
}

/// Attach handshake: called by the frontend once its `terminal://{id}/data`
/// and `terminal://{id}/exit` listeners are registered. Returns everything
/// the shell produced since spawn (in order) and switches the session to
/// live emission for subsequent chunks.
///
/// Backlog bytes are returned in the invoke payload — NOT emitted as events.
/// Emitting the backlog used to race React StrictMode: the first effect
/// mount would attach, flush into listeners, then unmount and dispose xterm
/// before the surviving mount registered — leaving a blank terminal with only
/// xterm's default cursor.
///
/// Idempotent: a second attach finds an empty backlog and `exited: false`.
#[tauri::command]
pub async fn terminal_attach(
    _app: AppHandle,
    state: State<'_, AppState>,
    id: String,
) -> Result<TerminalAttached, String> {
    let pipe = {
        let mut guard = state.ptys.lock();
        let session = guard.get_mut(&id).ok_or("unknown terminal id")?;
        use std::io::Write;
        if let Err(e) = session.writer.flush() {
            tracing::warn!("attach writer flush[{}]: {}", id, e);
        }
        session.pipe.clone()
    };

    let mut p = pipe.lock();
    let first_attach = !p.attached;
    let chunks: Vec<String> = p.backlog.drain(..).collect();
    let replay_exit = first_attach && p.exited;
    p.attached = true;
    tracing::debug!(
        "terminal_attach[{}]: first={} backlog_chunks={} exited={}",
        id,
        first_attach,
        chunks.len(),
        replay_exit
    );
    Ok(TerminalAttached {
        chunks,
        exited: replay_exit,
    })
}

/// Force-flush the PTY writer. Optional safety net the frontend can call
/// right after attach so the first keystroke isn't stuck in a buffer.
#[tauri::command]
pub async fn terminal_flush(state: State<'_, AppState>, id: String) -> Result<(), String> {
    let mut guard = state.ptys.lock();
    let session = guard.get_mut(&id).ok_or("unknown terminal id")?;
    use std::io::Write;
    session.writer.flush().map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn terminal_write(
    state: State<'_, AppState>,
    id: String,
    data: String,
) -> Result<(), String> {
    let mut guard = state.ptys.lock();
    let session = guard.get_mut(&id).ok_or("unknown terminal id")?;
    use std::io::Write;
    session
        .writer
        .write_all(data.as_bytes())
        .map_err(|e| e.to_string())?;
    session.writer.flush().map_err(|e| e.to_string())?;
    // Frontend write resets the idle clock — user typing keeps the session
    // alive even when the shell hasn't produced output for a while.
    session.pipe.lock().last_write_ts = Some(std::time::Instant::now());
    Ok(())
}

#[tauri::command]
pub async fn terminal_resize(
    state: State<'_, AppState>,
    id: String,
    cols: u16,
    rows: u16,
) -> Result<(), String> {
    let guard = state.ptys.lock();
    let session = guard.get(&id).ok_or("unknown terminal id")?;
    session
        .master
        .resize(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        })
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn terminal_kill(state: State<'_, AppState>, id: String) -> Result<(), String> {
    let mut session = state.ptys.lock().remove(&id).ok_or("unknown terminal id")?;
    let _ = session.child.kill();
    let _ = session.child.wait();
    Ok(())
}
