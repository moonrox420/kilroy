//! Execution sandbox dispatcher — Host vs. Windows Sandbox vs. Docker.
//!
//! Windows Sandbox is the default. We boot it with a generated `.wsb`
//! config, hand it a PowerShell script in a mapped folder, then poll
//! that folder for a `done.flag` and read back stdout/stderr/exit. The
//! sandbox auto-closes when its LogonCommand completes via `shutdown /p /f`.
//!
//! Requirements on the host:
//!   * Windows 11 with the "Windows Sandbox" optional feature enabled
//!     (`Enable-WindowsOptionalFeature -Online -FeatureName
//!     "Containers-DisposableClientVM"` from an elevated PowerShell).
//!   * `WindowsSandbox.exe` on PATH (default location: `C:\Windows\System32`).
//!
//! Limitations:
//!   * The sandbox VM takes ~10–30s to boot. We poll once per second up
//!     to 5 minutes. Long-running commands that need longer should
//!     either bump the timeout (env var KILROY_SANDBOX_TIMEOUT_SECS)
//!     or fall back to `Host`.
//!   * Project files are mapped writable so commands like `npm install`
//!     work. Reject any action that proposes destructive commands.

use crate::actuator::SandboxKind;
use anyhow::{anyhow, Context, Result};
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

#[derive(Serialize, Clone)]
pub struct ShellOutput {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: Option<i32>,
    pub sandbox: String,
}

#[derive(Clone, Copy)]
pub struct RunOpts {
    /// Maximum seconds to wait for a Windows Sandbox `done.flag`. Ignored
    /// for Host execution (it uses the host process timeout via reqwest /
    /// std::process).
    pub timeout_secs: u64,
}

impl Default for RunOpts {
    fn default() -> Self {
        Self { timeout_secs: 300 }
    }
}

pub fn run(command: &str, cwd: &Path, sandbox: &SandboxKind, opts: RunOpts) -> Result<ShellOutput> {
    match sandbox {
        SandboxKind::Host => run_host(command, cwd),
        SandboxKind::WindowsSandbox => run_windows_sandbox(command, cwd, opts),
        SandboxKind::Docker => run_docker(command, cwd, opts),
    }
}

// ─── Docker ────────────────────────────────────────────────────────────────────

/// Run a command inside a disposable Docker container with the project
/// directory bind-mounted at `/work`. Cross-platform: works anywhere the
/// `docker` CLI is on PATH and the daemon is running (Docker Desktop on
/// Windows/macOS, native dockerd on Linux).
///
/// The image defaults to `debian:stable-slim` and is overridable via the
/// `KILROY_DOCKER_IMAGE` env var so users can target a toolchain image that
/// already has their build dependencies.
///
/// Robustness:
///   * stdout/stderr are drained on dedicated threads so a chatty command
///     can never deadlock by filling the OS pipe buffer while we wait.
///   * the run is bounded by `opts.timeout_secs`; on expiry a watchdog
///     `docker kill`s the container by name (which ends `docker run`,
///     unblocking the wait) and a timeout error is returned.
///   * stdin is closed, so a command that tries to prompt fails fast
///     instead of hanging forever.
fn run_docker(command: &str, cwd: &Path, opts: RunOpts) -> Result<ShellOutput> {
    use std::io::Read;
    use std::process::Stdio;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    let image = std::env::var("KILROY_DOCKER_IMAGE")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| "debian:stable-slim".to_string());

    let id = uuid::Uuid::new_v4().to_string().replace('-', "");
    let container = format!("kilroy-{}", &id[..12]);
    let mount = format!("{}:/work", host_path_for_mount(cwd));

    let mut child = Command::new("docker")
        .arg("run")
        .arg("--rm")
        .arg("--name")
        .arg(&container)
        .arg("-v")
        .arg(&mount)
        .arg("-w")
        .arg("/work")
        .arg(&image)
        .arg("sh")
        .arg("-lc")
        .arg(command)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context(
            "could not launch `docker` — is Docker installed and the daemon running? \
             Install Docker Desktop, or switch the sandbox to Host / Windows Sandbox in Settings.",
        )?;

    // Drain both pipes on their own threads — a full pipe buffer would
    // otherwise deadlock against `child.wait()`.
    let mut out_pipe = child.stdout.take();
    let mut err_pipe = child.stderr.take();
    let out_handle = std::thread::spawn(move || {
        let mut buf = Vec::new();
        if let Some(p) = out_pipe.as_mut() {
            let _ = p.read_to_end(&mut buf);
        }
        buf
    });
    let err_handle = std::thread::spawn(move || {
        let mut buf = Vec::new();
        if let Some(p) = err_pipe.as_mut() {
            let _ = p.read_to_end(&mut buf);
        }
        buf
    });

    // Watchdog: kill the container by name once the deadline passes. The
    // `finished` flag cancels it on the normal path so we never kill a
    // container that already exited.
    let finished = Arc::new(AtomicBool::new(false));
    let timed_out = Arc::new(AtomicBool::new(false));
    let watchdog = {
        let finished = finished.clone();
        let timed_out = timed_out.clone();
        let container = container.clone();
        let timeout = opts.timeout_secs.max(1);
        std::thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(timeout);
            while Instant::now() < deadline {
                if finished.load(Ordering::SeqCst) {
                    return;
                }
                std::thread::sleep(Duration::from_millis(200));
            }
            if !finished.load(Ordering::SeqCst) {
                timed_out.store(true, Ordering::SeqCst);
                let _ = Command::new("docker")
                    .arg("kill")
                    .arg(&container)
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .status();
            }
        })
    };

    let status = child.wait().context("wait for docker child")?;
    finished.store(true, Ordering::SeqCst);
    let _ = watchdog.join();

    let stdout = out_handle.join().unwrap_or_default();
    let stderr = err_handle.join().unwrap_or_default();

    if timed_out.load(Ordering::SeqCst) {
        return Err(anyhow!(
            "Docker command timed out after {}s (container {} killed).",
            opts.timeout_secs,
            container
        ));
    }

    Ok(ShellOutput {
        stdout: String::from_utf8_lossy(&stdout).to_string(),
        stderr: String::from_utf8_lossy(&stderr).to_string(),
        exit_code: status.code(),
        sandbox: "docker".into(),
    })
}

/// Normalize a host path for a Docker `-v` bind mount. Strips the Windows
/// `\\?\` extended-length prefix if present; Docker Desktop accepts native
/// Windows drive paths (`C:\Users\me\proj`) and Unix absolute paths as-is.
fn host_path_for_mount(p: &Path) -> String {
    let s = p.to_string_lossy().to_string();
    s.strip_prefix(r"\\?\").map(str::to_string).unwrap_or(s)
}

/// Strip a leading UTF-8 BOM character (U+FEFF) if present. `str::trim`
/// does NOT remove it — U+FEFF is not Unicode whitespace — so it survives
/// into `parse()` and breaks numeric parsing of an otherwise-clean value.
fn strip_bom(s: &str) -> &str {
    s.strip_prefix('\u{feff}').unwrap_or(s)
}

/// Decode bytes written by the in-sandbox PowerShell/cmd wrapper, tolerant
/// of the encodings Windows tooling actually emits:
///   * UTF-16 LE / BE — PowerShell 5.1's `>` / `2>` file redirection default
///     (and PS 5.1 is exactly what runs inside Windows Sandbox),
///   * UTF-8 with a BOM — PowerShell 5.1's `Out-File -Encoding utf8`,
///   * plain UTF-8 — PowerShell 7 and cmd.exe redirection.
///
/// Why this matters: `exit.txt` written by PS 5.1 `Out-File` begins with a
/// UTF-8 BOM, so the old `read_to_string(..).trim().parse::<i32>()` produced
/// `None` for every run — and `accept_action` treats a `None` exit code as
/// success (`unwrap_or(0)`), silently masking FAILED sandbox commands as
/// passing. `stderr.txt` written by `2>` is UTF-16, which `read_to_string`
/// rejects outright, dropping all error text. Decoding here fixes both.
fn decode_console_bytes(bytes: &[u8]) -> String {
    if bytes.len() >= 2 && bytes[0] == 0xFF && bytes[1] == 0xFE {
        // UTF-16 LE (with BOM).
        let units: Vec<u16> = bytes[2..]
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect();
        return strip_bom(&String::from_utf16_lossy(&units)).to_string();
    }
    if bytes.len() >= 2 && bytes[0] == 0xFE && bytes[1] == 0xFF {
        // UTF-16 BE (with BOM).
        let units: Vec<u16> = bytes[2..]
            .chunks_exact(2)
            .map(|c| u16::from_be_bytes([c[0], c[1]]))
            .collect();
        return strip_bom(&String::from_utf16_lossy(&units)).to_string();
    }
    // UTF-8, with or without a BOM.
    strip_bom(&String::from_utf8_lossy(bytes)).to_string()
}

// ─── Host ────────────────────────────────────────────────────────────────────

fn run_host(command: &str, cwd: &Path) -> Result<ShellOutput> {
    #[cfg(windows)]
    let mut cmd = {
        let mut c = Command::new("powershell.exe");
        c.arg("-NoLogo")
            .arg("-NoProfile")
            .arg("-Command")
            .arg(command);
        c
    };
    #[cfg(not(windows))]
    let mut cmd = {
        let mut c = Command::new("/bin/sh");
        c.arg("-c").arg(command);
        c
    };

    cmd.current_dir(cwd);
    let output = cmd.output()?;
    Ok(ShellOutput {
        stdout: String::from_utf8_lossy(&output.stdout).to_string(),
        stderr: String::from_utf8_lossy(&output.stderr).to_string(),
        exit_code: output.status.code(),
        sandbox: "host".into(),
    })
}

// ─── Windows Sandbox ─────────────────────────────────────────────────────────

#[cfg(windows)]
fn run_windows_sandbox(command: &str, cwd: &Path, opts: RunOpts) -> Result<ShellOutput> {
    let mount = SandboxMount::new(command, cwd)?;
    let wsb_path = mount.write_wsb()?;
    mount.launch(&wsb_path)?;
    mount.wait_for_done(opts.timeout_secs)?;
    mount.collect_output()
}

#[cfg(not(windows))]
fn run_windows_sandbox(_command: &str, _cwd: &Path, _opts: RunOpts) -> Result<ShellOutput> {
    Err(anyhow!(
        "Windows Sandbox is only available on Windows hosts."
    ))
}

#[cfg(windows)]
struct SandboxMount {
    /// `%TEMP%\kilroy-sb-<uuid>` — host folder we map into the sandbox.
    host_temp: PathBuf,
    /// Inside-sandbox path that maps 1:1 to `host_temp`.
    sandbox_temp: String,
    /// Inside-sandbox path that maps to the project root (writable).
    sandbox_cwd: String,
    cwd: PathBuf,
}

#[cfg(windows)]
impl SandboxMount {
    fn new(command: &str, cwd: &Path) -> Result<Self> {
        let temp_root = std::env::temp_dir();
        let id = uuid::Uuid::new_v4().to_string().replace('-', "");
        let host_temp = temp_root.join(format!("kilroy-sb-{}", &id[..12]));
        std::fs::create_dir_all(&host_temp)
            .with_context(|| format!("create temp {}", host_temp.display()))?;

        let sandbox_temp = "C:\\Users\\WDAGUtilityAccount\\Desktop\\kilroy".to_string();
        let sandbox_cwd = "C:\\Users\\WDAGUtilityAccount\\Desktop\\work".to_string();

        let me = Self {
            host_temp,
            sandbox_temp,
            sandbox_cwd,
            cwd: cwd.to_path_buf(),
        };
        me.write_run_script(command)?;
        Ok(me)
    }

    fn write_run_script(&self, command: &str) -> Result<()> {
        // SECURITY: Don't inline the command into the PowerShell here-doc via
        // `cmd.exe /c '<cmd>'`. PowerShell's single-quote-doubling rule does
        // not protect against cmd.exe's own tokenization, so any `'`, `&`, `|`
        // in the LLM's command can break out of the quoted argument and run
        // arbitrary additional commands (inside the sandbox, but still).
        //
        // Instead, write the raw command bytes to a .cmd batch file inside
        // the mapped temp dir, then reference that file path from the
        // PowerShell wrapper. cmd.exe interpreting a script file is safe;
        // there's no host-shell quoting layer to escape out of.
        let cmd_script_path = self.host_temp.join("payload.cmd");
        std::fs::write(&cmd_script_path, command)
            .with_context(|| format!("write payload script {}", cmd_script_path.display()))?;
        let sandbox_cmd_script = format!("{}\\payload.cmd", self.sandbox_temp);

        let script = format!(
            r#"$ErrorActionPreference = 'Continue'
$base = '{tmp}'
$work = '{work}'
if (Test-Path $work) {{
    Set-Location $work
}} else {{
    Set-Location $base
}}
$stdoutPath = Join-Path $base 'stdout.txt'
$stderrPath = Join-Path $base 'stderr.txt'
$exitPath   = Join-Path $base 'exit.txt'
$doneFlag   = Join-Path $base 'done.flag'
$payload    = '{payload}'

try {{
    $out = & cmd.exe /c "$payload" 2> $stderrPath
    $out | Out-File -FilePath $stdoutPath -Encoding utf8
    $LASTEXITCODE | Out-File -FilePath $exitPath -Encoding utf8
}} catch {{
    $_ | Out-File -FilePath $stderrPath -Encoding utf8 -Append
    -1 | Out-File -FilePath $exitPath -Encoding utf8
}}

New-Item -Path $doneFlag -ItemType File -Force | Out-Null
Start-Sleep -Milliseconds 500
shutdown.exe /p /f
"#,
            tmp = self.sandbox_temp,
            work = self.sandbox_cwd,
            payload = sandbox_cmd_script,
        );
        let path = self.host_temp.join("run.ps1");
        // PowerShell prefers UTF-8 with BOM for non-ASCII source files;
        // ASCII is fine without. We stick to plain UTF-8.
        std::fs::write(&path, script).with_context(|| format!("write {}", path.display()))?;
        Ok(())
    }

    fn write_wsb(&self) -> Result<PathBuf> {
        let temp_str = path_to_windows(&self.host_temp);
        let cwd_str = path_to_windows(&self.cwd);

        let wsb = format!(
            r#"<Configuration>
  <MappedFolders>
    <MappedFolder>
      <HostFolder>{temp}</HostFolder>
      <SandboxFolder>{sb_tmp}</SandboxFolder>
      <ReadOnly>false</ReadOnly>
    </MappedFolder>
    <MappedFolder>
      <HostFolder>{cwd}</HostFolder>
      <SandboxFolder>{sb_cwd}</SandboxFolder>
      <ReadOnly>false</ReadOnly>
    </MappedFolder>
  </MappedFolders>
  <LogonCommand>
    <Command>powershell.exe -ExecutionPolicy Bypass -WindowStyle Hidden -File {sb_tmp}\run.ps1</Command>
  </LogonCommand>
  <MemoryInMB>4096</MemoryInMB>
  <Networking>Default</Networking>
  <vGPU>Disable</vGPU>
  <AudioInput>Disable</AudioInput>
  <VideoInput>Disable</VideoInput>
  <ProtectedClient>Enable</ProtectedClient>
  <ClipboardRedirection>Disable</ClipboardRedirection>
  <PrinterRedirection>Disable</PrinterRedirection>
</Configuration>
"#,
            temp = temp_str,
            sb_tmp = self.sandbox_temp,
            cwd = cwd_str,
            sb_cwd = self.sandbox_cwd,
        );
        let path = self.host_temp.join("config.wsb");
        std::fs::write(&path, wsb).with_context(|| format!("write {}", path.display()))?;
        Ok(path)
    }

    fn launch(&self, wsb: &Path) -> Result<()> {
        let _child = Command::new("WindowsSandbox.exe")
            .arg(wsb)
            .spawn()
            .with_context(|| {
                "could not launch WindowsSandbox.exe — is the \"Windows Sandbox\" \
                 optional feature installed? \n\
                 Enable-WindowsOptionalFeature -Online -FeatureName \
                 \"Containers-DisposableClientVM\" -All"
            })?;
        Ok(())
    }

    fn wait_for_done(&self, timeout_secs: u64) -> Result<()> {
        let deadline = Instant::now() + Duration::from_secs(timeout_secs);
        let done = self.host_temp.join("done.flag");
        while Instant::now() < deadline {
            if done.exists() {
                return Ok(());
            }
            std::thread::sleep(Duration::from_secs(1));
        }
        Err(anyhow!(
            "Windows Sandbox timed out after {}s. Output collected so far is available in {}",
            timeout_secs,
            self.host_temp.display()
        ))
    }

    fn collect_output(&self) -> Result<ShellOutput> {
        // Read as raw bytes and decode tolerantly — the in-sandbox PS 5.1
        // wrapper emits UTF-16 (redirection) and BOM'd UTF-8 (Out-File),
        // neither of which `read_to_string` + `trim().parse()` handle.
        let read = |name: &str| -> Vec<u8> {
            std::fs::read(self.host_temp.join(name)).unwrap_or_default()
        };
        let stdout = decode_console_bytes(&read("stdout.txt"));
        let stderr = decode_console_bytes(&read("stderr.txt"));
        let exit_code = decode_console_bytes(&read("exit.txt"))
            .trim()
            .parse::<i32>()
            .ok();

        // Best-effort cleanup. The mount dir contains the WSB + script + outputs;
        // we keep it around for forensics and only delete on success-with-output.
        if !stdout.is_empty() || exit_code.is_some() {
            let _ = std::fs::remove_dir_all(&self.host_temp);
        }

        Ok(ShellOutput {
            stdout,
            stderr,
            exit_code,
            sandbox: "windows_sandbox".into(),
        })
    }
}

#[cfg(windows)]
fn path_to_windows(p: &Path) -> String {
    // Convert any forward slashes to backslashes for the WSB consumer
    // and strip a leading `\\?\` extended-prefix if present.
    let s = p.to_string_lossy().to_string();
    let s = s.strip_prefix(r"\\?\").map(str::to_string).unwrap_or(s);
    s.replace('/', "\\")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_utf8_passes_through() {
        assert_eq!(decode_console_bytes(b"hello world"), "hello world");
    }

    #[test]
    fn utf8_bom_is_stripped() {
        // PowerShell 5.1 `Out-File -Encoding utf8` prepends EF BB BF.
        assert_eq!(decode_console_bytes(b"\xEF\xBB\xBFhello"), "hello");
    }

    #[test]
    fn bomed_exit_code_still_parses() {
        // Regression for the masked-failure bug: a BOM'd "1" must parse to
        // Some(1), not None (which accept_action would treat as success).
        let decoded = decode_console_bytes(b"\xEF\xBB\xBF1");
        assert_eq!(decoded.trim().parse::<i32>().ok(), Some(1));
    }

    #[test]
    fn utf16le_with_bom_decodes() {
        // "12" in UTF-16 LE with a byte-order mark.
        let bytes = [0xFF, 0xFE, b'1', 0x00, b'2', 0x00];
        assert_eq!(decode_console_bytes(&bytes), "12");
    }

    #[test]
    fn utf16le_stderr_is_recovered() {
        // PS 5.1 `2>` redirection writes UTF-16; previously read_to_string
        // rejected it and the error text was lost entirely.
        let bytes = [0xFF, 0xFE, b'e', 0, b'r', 0, b'r', 0];
        assert_eq!(decode_console_bytes(&bytes), "err");
    }

    #[test]
    fn utf16be_with_bom_decodes() {
        let bytes = [0xFE, 0xFF, 0x00, b'h', 0x00, b'i'];
        assert_eq!(decode_console_bytes(&bytes), "hi");
    }

    #[test]
    fn empty_input_is_empty_string() {
        assert_eq!(decode_console_bytes(b""), "");
        assert_eq!(decode_console_bytes(b"").trim().parse::<i32>().ok(), None);
    }
}
