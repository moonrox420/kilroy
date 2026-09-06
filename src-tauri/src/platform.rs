//! OS smart detector — the single source of truth for "which platform
//! are we on, and what does that imply."
//!
//! Two mechanisms work together across the codebase:
//!
//!   * **Compile-time** — OS-specific implementations are gated with
//!     `#[cfg(target_os = "...")]` / `#[cfg(windows)]` / `#[cfg(unix)]`.
//!     The compiler on Windows never even sees the macOS/Linux branches
//!     and vice versa, so foreign-OS code can neither run nor break the
//!     build. The sandbox dispatcher (`actuator::sandbox`) and the
//!     terminal shell detection (`commands::terminal`) already use this.
//!
//!   * **Runtime** — this module reports the live OS and the choices
//!     valid for it through the `platform_info` command, so the React UI
//!     can render only what fits the host (Windows Sandbox appears only
//!     on Windows; keyboard hints show Cmd on macOS; shell labels adapt).
//!
//! Keeping the runtime answers in ONE place means a future macOS/Linux
//! port flips behavior here and at the cfg seams, not scattered across
//! the UI.

use serde::Serialize;

/// The detected operating system. `Other` is a catch-all so the enum is
/// total — we never panic on an unexpected `std::env::consts::OS`.
// `MacOs` trips clippy::enum_variant_names (ends with the enum name `Os`),
// but renaming it would change the serde `snake_case` wire value the
// frontend keys on — so the lint is suppressed rather than the variant
// renamed.
#[allow(clippy::enum_variant_names)]
#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Os {
    Windows,
    MacOs,
    Linux,
    Other,
}

impl Os {
    /// Detect the current OS at runtime from the compiled-in target.
    /// `std::env::consts::OS` is a `&'static str` baked in at build time,
    /// so this is effectively free and always accurate for the binary
    /// that's running.
    pub fn detect() -> Self {
        match std::env::consts::OS {
            "windows" => Os::Windows,
            "macos" => Os::MacOs,
            "linux" => Os::Linux,
            _ => Os::Other,
        }
    }
}

/// Everything the frontend needs to render an OS-appropriate UI without
/// hardcoding platform assumptions in TypeScript.
#[derive(Serialize, Clone, Debug)]
pub struct PlatformInfo {
    pub os: Os,
    /// e.g. "x86_64", "aarch64".
    pub arch: String,
    /// "windows" or "unix".
    pub family: String,
    pub is_windows: bool,
    pub is_macos: bool,
    pub is_linux: bool,
    /// Sandbox kinds that are actually selectable on this host. Windows
    /// Sandbox is Windows-only; Host works everywhere; Docker is offered
    /// as a cross-platform escape hatch.
    pub available_sandboxes: Vec<String>,
    /// The sensible default sandbox for this OS.
    pub default_sandbox: String,
    /// "windows" or "unix" — drives shell-name labelling in the UI.
    pub shell_kind: String,
    /// The primary modifier key users expect: "Cmd" on macOS, otherwise
    /// "Ctrl". Used for keyboard-hint text.
    pub modifier_key: String,
    /// Native path separator.
    pub path_sep: String,
}

/// Build the platform snapshot. Pure and cheap; safe to call on every
/// frontend mount.
pub fn info() -> PlatformInfo {
    let os = Os::detect();
    let is_windows = matches!(os, Os::Windows);
    let is_macos = matches!(os, Os::MacOs);
    let is_linux = matches!(os, Os::Linux);

    // Windows Sandbox is only real on Windows. Everywhere else we offer
    // Host (always works) and Docker (cross-platform escape hatch). This
    // is exactly the "Windows ignores the others and vice versa" rule,
    // surfaced to the UI so it never shows an impossible choice.
    let available_sandboxes = if is_windows {
        vec![
            "host".to_string(),
            "windows_sandbox".to_string(),
            "docker".to_string(),
        ]
    } else {
        vec!["host".to_string(), "docker".to_string()]
    };

    let default_sandbox = if is_windows {
        "windows_sandbox"
    } else {
        "host"
    }
    .to_string();

    PlatformInfo {
        os,
        arch: std::env::consts::ARCH.to_string(),
        family: std::env::consts::FAMILY.to_string(),
        is_windows,
        is_macos,
        is_linux,
        available_sandboxes,
        default_sandbox,
        shell_kind: if is_windows { "windows" } else { "unix" }.to_string(),
        modifier_key: if is_macos { "Cmd" } else { "Ctrl" }.to_string(),
        path_sep: if is_windows { "\\" } else { "/" }.to_string(),
    }
}

/// Tauri command — the frontend's window into the detector.
#[tauri::command]
pub async fn platform_info() -> Result<PlatformInfo, String> {
    Ok(info())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_matches_compiled_target() {
        // The detected OS must agree with the cfg the binary was built
        // under — this is the invariant the whole detector rests on.
        let os = Os::detect();
        if cfg!(target_os = "windows") {
            assert_eq!(os, Os::Windows);
        } else if cfg!(target_os = "macos") {
            assert_eq!(os, Os::MacOs);
        } else if cfg!(target_os = "linux") {
            assert_eq!(os, Os::Linux);
        }
    }

    #[test]
    fn info_is_self_consistent() {
        let p = info();
        // Exactly one of the OS booleans may be true (or none, for Other).
        let trues = [p.is_windows, p.is_macos, p.is_linux]
            .iter()
            .filter(|b| **b)
            .count();
        assert!(trues <= 1);
        // Host is always an available sandbox.
        assert!(p.available_sandboxes.iter().any(|s| s == "host"));
        // Windows Sandbox is offered if and only if we're on Windows.
        assert_eq!(
            p.available_sandboxes.iter().any(|s| s == "windows_sandbox"),
            p.is_windows
        );
        // The default must be one of the available kinds.
        assert!(p.available_sandboxes.contains(&p.default_sandbox));
    }
}
