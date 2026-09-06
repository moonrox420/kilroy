//! Project compiler / linter gate.
//!
//! Detects project toolchains (Cargo for Rust, npm/tsc for TypeScript/Node,
//! go for Golang, ruff for Python) and executes compiler checks against
//! the project root after changes are applied.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const MAX_OUTPUT_BYTES: usize = 512 * 1024;
const DEFAULT_TIMEOUT_SECS: u64 = 45;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompilerCheckResult {
    pub success: bool,
    pub toolchain: String,
    pub command: String,
    pub stdout: String,
    pub stderr: String,
    pub exit_code: Option<i32>,
    pub duration_ms: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectToolchain {
    Rust,
    TypeScript,
    Go,
    Python,
}

impl ProjectToolchain {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Rust => "rust",
            Self::TypeScript => "typescript",
            Self::Go => "go",
            Self::Python => "python",
        }
    }
}

/// Detect the primary toolchain for the project by inspecting marker files.
pub fn detect_toolchain(root: &Path) -> Option<ProjectToolchain> {
    if root.join("Cargo.toml").is_file() {
        Some(ProjectToolchain::Rust)
    } else if root.join("tsconfig.json").is_file() || root.join("package.json").is_file() {
        Some(ProjectToolchain::TypeScript)
    } else if root.join("go.mod").is_file() {
        Some(ProjectToolchain::Go)
    } else if root.join("pyproject.toml").is_file() || root.join("ruff.toml").is_file() {
        Some(ProjectToolchain::Python)
    } else {
        None
    }
}

/// Run the compiler or linter check for the given project root.
pub fn run_compiler_check(root: &Path) -> Result<Option<CompilerCheckResult>> {
    let toolchain = match detect_toolchain(root) {
        Some(t) => t,
        None => return Ok(None),
    };

    let start = Instant::now();

    #[cfg(windows)]
    let (program, args, command_str) = match toolchain {
        ProjectToolchain::Rust => (
            "cargo".to_string(),
            vec!["check".to_string(), "--message-format=short".to_string()],
            "cargo check --message-format=short".to_string(),
        ),
        ProjectToolchain::TypeScript => {
            let cmd = if root.join("tsconfig.json").is_file() {
                "npx.cmd --no-install tsc --noEmit"
            } else {
                "npm.cmd test -- --watchAll=false"
            };
            (
                "cmd.exe".to_string(),
                vec!["/C".to_string(), cmd.to_string()],
                cmd.to_string(),
            )
        }
        ProjectToolchain::Go => (
            "go".to_string(),
            vec!["vet".to_string(), "./...".to_string()],
            "go vet ./...".to_string(),
        ),
        ProjectToolchain::Python => (
            "cmd.exe".to_string(),
            vec!["/C".to_string(), "ruff check .".to_string()],
            "ruff check .".to_string(),
        ),
    };

    #[cfg(not(windows))]
    let (program, args, command_str) = match toolchain {
        ProjectToolchain::Rust => (
            "cargo".to_string(),
            vec!["check".to_string(), "--message-format=short".to_string()],
            "cargo check --message-format=short".to_string(),
        ),
        ProjectToolchain::TypeScript => {
            let cmd = if root.join("tsconfig.json").is_file() {
                "npx --no-install tsc --noEmit"
            } else {
                "npm test -- --watchAll=false"
            };
            (
                "sh".to_string(),
                vec!["-c".to_string(), cmd.to_string()],
                cmd.to_string(),
            )
        }
        ProjectToolchain::Go => (
            "go".to_string(),
            vec!["vet".to_string(), "./...".to_string()],
            "go vet ./...".to_string(),
        ),
        ProjectToolchain::Python => (
            "sh".to_string(),
            vec!["-c".to_string(), "ruff check .".to_string()],
            "ruff check .".to_string(),
        ),
    };

    let mut cmd = Command::new(&program);
    cmd.args(&args)
        .current_dir(root)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }

    let mut child = cmd
        .spawn()
        .with_context(|| format!("failed to spawn compiler check: {command_str}"))?;

    let out_pipe = child.stdout.take().context("stdout pipe missing")?;
    let err_pipe = child.stderr.take().context("stderr pipe missing")?;

    let out_handle = std::thread::spawn(move || drain_stream(out_pipe));
    let err_handle = std::thread::spawn(move || drain_stream(err_pipe));

    let deadline = Instant::now() + Duration::from_secs(DEFAULT_TIMEOUT_SECS);
    let status = loop {
        if let Some(status) = child.try_wait().context("poll compiler child")? {
            break status;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            return Err(anyhow::anyhow!(
                "Compiler check '{command_str}' timed out after {DEFAULT_TIMEOUT_SECS}s and was killed"
            ));
        }
        std::thread::sleep(Duration::from_millis(50));
    };

    let stdout_bytes = out_handle
        .join()
        .map_err(|_| anyhow::anyhow!("stdout reader thread panicked"))??;
    let stderr_bytes = err_handle
        .join()
        .map_err(|_| anyhow::anyhow!("stderr reader thread panicked"))??;

    let stdout = String::from_utf8_lossy(&stdout_bytes).to_string();
    let stderr = String::from_utf8_lossy(&stderr_bytes).to_string();
    let exit_code = status.code();
    let success = status.success();
    let duration_ms = start.elapsed().as_millis() as u64;

    Ok(Some(CompilerCheckResult {
        success,
        toolchain: toolchain.as_str().to_string(),
        command: command_str,
        stdout,
        stderr,
        exit_code,
        duration_ms,
    }))
}

fn drain_stream(mut reader: impl std::io::Read) -> std::io::Result<Vec<u8>> {
    let mut output = Vec::new();
    let mut buffer = [0u8; 8192];
    loop {
        let count = reader.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        let keep = count.min(MAX_OUTPUT_BYTES.saturating_sub(output.len()));
        output.extend_from_slice(&buffer[..keep]);
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn temp_test_dir() -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("kilroy-compiler-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn detects_rust_toolchain() {
        let dir = temp_test_dir();
        std::fs::write(dir.join("Cargo.toml"), "[package]\nname = \"test\"").unwrap();
        assert_eq!(detect_toolchain(&dir), Some(ProjectToolchain::Rust));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn detects_typescript_toolchain() {
        let dir = temp_test_dir();
        std::fs::write(dir.join("tsconfig.json"), "{}").unwrap();
        assert_eq!(detect_toolchain(&dir), Some(ProjectToolchain::TypeScript));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn returns_none_when_no_marker() {
        let dir = temp_test_dir();
        assert_eq!(detect_toolchain(&dir), None);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
