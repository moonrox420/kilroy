//! Smart Coder subprocess discovery, launch construction, and streaming.

use serde::Serialize;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter};

#[derive(Clone, Debug)]
pub struct ResolvedPython {
    pub executable: PathBuf,
    pub source: String,
    pub venv_root: Option<PathBuf>,
}

#[derive(Clone, Debug)]
pub struct SmartCoderLaunch {
    pub argv: Vec<String>,
    pub workdir: Option<PathBuf>,
}

#[derive(Clone, Debug)]
pub struct SmartCoderProcessResult {
    pub code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
}

#[derive(Clone)]
pub enum StreamSink {
    SmartCoderPanel { app: AppHandle },
}

#[derive(Clone, Serialize)]
struct OutputEvent {
    stream: String,
    line: String,
}

impl StreamSink {
    pub(crate) fn emit(&self, stream: &str, line: &str) {
        match self {
            Self::SmartCoderPanel { app } => {
                let _ = app.emit(
                    "smartcoder://output",
                    OutputEvent {
                        stream: stream.to_string(),
                        line: line.to_string(),
                    },
                );
            }
        }
    }
}

fn python_in_venv(root: &Path) -> PathBuf {
    if cfg!(windows) {
        root.join("Scripts").join("python.exe")
    } else {
        root.join("bin").join("python")
    }
}

fn kilroy_roots(project_root: Option<&Path>) -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Some(root) = project_root {
        roots.push(root.to_path_buf());
    }
    if let Some(root) = Path::new(env!("CARGO_MANIFEST_DIR")).parent() {
        roots.push(root.to_path_buf());
    }
    if let Ok(cwd) = std::env::current_dir() {
        roots.push(cwd);
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            roots.push(parent.to_path_buf());
            roots.push(parent.join("resources"));
            roots.push(parent.join("_up_"));
            roots.push(parent.join("resources").join("_up_"));
        }
    }
    roots.sort();
    roots.dedup();
    roots
}

pub fn vendored_script(project_root: Option<&Path>) -> Option<PathBuf> {
    kilroy_roots(project_root)
        .into_iter()
        .map(|root| root.join("smartcoder").join("kilroy_smartcoder.py"))
        .find(|path| path.is_file())
}

pub fn resolve_python(project_root: Option<&Path>) -> Option<ResolvedPython> {
    let script = vendored_script(project_root);
    let kilroy_root = script
        .as_deref()
        .and_then(Path::parent)
        .and_then(Path::parent)
        .map(Path::to_path_buf);

    let mut venvs = Vec::new();
    if let Some(root) = kilroy_root.as_deref() {
        venvs.push(root.join(".venv"));
        venvs.push(root.join("smartcoder").join(".venv"));
    }
    // The opened repository's virtualenv belongs to that repository, not to
    // Kilroy. Running the backend from it made Smart Coder's availability
    // depend on whichever project happened to be open.
    venvs.sort();
    venvs.dedup();

    for venv_root in venvs {
        let executable = python_in_venv(&venv_root);
        if executable.is_file() && python_version_at(&executable).is_some() {
            return Some(ResolvedPython {
                executable,
                source: "project_venv".to_string(),
                venv_root: Some(venv_root),
            });
        }
    }

    for executable in ["python", "python3"].map(PathBuf::from) {
        if python_version_at(&executable).is_some() {
            return Some(ResolvedPython {
                executable,
                source: "system".to_string(),
                venv_root: None,
            });
        }
    }
    None
}

pub fn python_version_at(executable: &Path) -> Option<String> {
    let output = Command::new(executable).arg("--version").output().ok()?;
    if !output.status.success() {
        return None;
    }
    let text = if output.stdout.is_empty() {
        String::from_utf8_lossy(&output.stderr)
    } else {
        String::from_utf8_lossy(&output.stdout)
    };
    Some(text.trim().to_string())
}

pub fn smartcoder_launchable(project_root: Option<&Path>) -> bool {
    vendored_script(project_root).is_some() && resolve_python(project_root).is_some()
}

pub fn smartcoder_deps_ready(project_root: Option<&Path>) -> bool {
    let Some(python) = resolve_python(project_root) else {
        return false;
    };
    let Some(script) = vendored_script(project_root) else {
        return false;
    };
    let Some(repo_root) = script.parent().and_then(Path::parent) else {
        return false;
    };
    Command::new(&python.executable)
        .arg("-c")
        .arg(
            "import ollama, smolagents; from smartcoder.controllers.maestro import SmartCoderController",
        )
        .current_dir(repo_root)
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

pub fn smartcoder_setup_hint(_project_root: Option<&Path>) -> String {
    let python = if cfg!(windows) {
        ".venv\\Scripts\\python.exe"
    } else {
        ".venv/bin/python"
    };
    format!(
        "From the Kilroy repository root, run `uv venv .venv`, \
         `uv pip install --python {python} -r requirements.txt`, then \
         `uv pip install --python {python} --no-deps smartcoder`."
    )
}

#[allow(clippy::too_many_arguments)]
pub fn build_smartcoder_argv(
    subcommand: &str,
    args: &[String],
    project_root: Option<&Path>,
    ollama_host: &str,
    chat_model: &str,
    context_file: Option<&Path>,
    workdir: Option<&Path>,
    project_grounded: bool,
    task_role: Option<&str>,
    task_type: Option<&str>,
) -> Result<SmartCoderLaunch, String> {
    let python = resolve_python(project_root)
        .ok_or_else(|| format!("Python is unavailable. {}", smartcoder_setup_hint(project_root)))?;
    let script = vendored_script(project_root).ok_or_else(|| {
        "The bundled Smart Coder source tree could not be located.".to_string()
    })?;

    let mut argv = vec![
        python.executable.to_string_lossy().to_string(),
        script.to_string_lossy().to_string(),
        "--backend".to_string(),
        "ollama".to_string(),
        "--ollama-host".to_string(),
        ollama_host.to_string(),
        "--model".to_string(),
        chat_model.to_string(),
    ];
    if project_grounded {
        argv.push("--no-dataset-rag".to_string());
    }
    if let Some(path) = context_file {
        argv.push("--context-file".to_string());
        argv.push(path.to_string_lossy().to_string());
    }
    if let Some(root) = project_root {
        argv.push("--project-root".to_string());
        argv.push(root.to_string_lossy().to_string());
    }
    if let Some(role) = task_role {
        argv.push("--task-role".to_string());
        argv.push(role.to_string());
    }
    if let Some(kind) = task_type {
        argv.push("--task-type".to_string());
        argv.push(kind.to_string());
    }
    argv.push(subcommand.to_string());
    argv.extend(args.iter().cloned());

    let process_workdir = workdir
        .map(Path::to_path_buf)
        .or_else(|| script.parent().and_then(Path::parent).map(Path::to_path_buf));
    Ok(SmartCoderLaunch {
        argv,
        workdir: process_workdir,
    })
}

pub fn write_context_temp_file<T: Serialize>(context: &T) -> Result<PathBuf, String> {
    let path = std::env::temp_dir().join(format!(
        "kilroy-smartcoder-context-{}.json",
        uuid::Uuid::new_v4().simple()
    ));
    let payload = serde_json::to_vec(context).map_err(|error| error.to_string())?;
    std::fs::write(&path, payload).map_err(|error| {
        format!("write Smart Coder context {}: {error}", path.display())
    })?;
    Ok(path)
}

pub fn run_smartcoder_ask_blocking(
    argv: &[String],
    workdir: Option<&Path>,
    sink: &StreamSink,
    timeout_seconds: Option<u64>,
) -> Result<SmartCoderProcessResult, String> {
    let (program, arguments) = argv
        .split_first()
        .ok_or_else(|| "Smart Coder launch arguments are empty.".to_string())?;
    let mut command = Command::new(program);
    command
        .args(arguments)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .stdin(Stdio::null());
    if let Some(directory) = workdir {
        command.current_dir(directory);
    }
    if timeout_seconds.is_some() {
        command.env("SMARTCODER_SUPERVISED", "1");
    }
    let mut child = command
        .spawn()
        .map_err(|error| format!("launch Smart Coder: {error}"))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "Smart Coder stdout pipe was unavailable.".to_string())?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| "Smart Coder stderr pipe was unavailable.".to_string())?;

    let stdout_sink = sink.clone();
    let stdout_thread = thread::spawn(move || {
        let mut collected = String::new();
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            stdout_sink.emit("stdout", &line);
            collected.push_str(&line);
            collected.push('\n');
        }
        collected
    });
    let stderr_sink = sink.clone();
    let stderr_thread = thread::spawn(move || {
        let mut collected = String::new();
        for line in BufReader::new(stderr).lines().map_while(Result::ok) {
            stderr_sink.emit("stderr", &line);
            collected.push_str(&line);
            collected.push('\n');
        }
        collected
    });

    let started = Instant::now();
    let mut timed_out = false;
    let status = loop {
        match child
            .try_wait()
            .map_err(|error| format!("poll Smart Coder process: {error}"))?
        {
            Some(status) => break status,
            None => {
                let deadline_reached = timeout_seconds
                    .map(|seconds| started.elapsed() >= Duration::from_secs(seconds))
                    .unwrap_or(false);
                if deadline_reached {
                    timed_out = true;
                    let _ = child.kill();
                    break child
                        .wait()
                        .map_err(|error| format!("terminate Smart Coder after timeout: {error}"))?;
                }
                thread::sleep(Duration::from_millis(50));
            }
        }
    };

    let stdout = stdout_thread
        .join()
        .map_err(|_| "Smart Coder stdout reader panicked.".to_string())?;
    let stderr = stderr_thread
        .join()
        .map_err(|_| "Smart Coder stderr reader panicked.".to_string())?;

    if timed_out {
        let seconds = timeout_seconds.unwrap_or_default();
        let mut detail = format!(
            "Smart Coder exceeded its {seconds}s maximum runtime and was terminated."
        );
        if !stderr.trim().is_empty() {
            detail.push_str("\n\nSmart Coder stderr:\n");
            detail.push_str(stderr.trim());
        }
        if !stdout.trim().is_empty() {
            detail.push_str("\n\nSmart Coder stdout:\n");
            detail.push_str(stdout.trim());
        }
        return Err(detail);
    }

    Ok(SmartCoderProcessResult {
        code: status.code(),
        stdout,
        stderr,
    })
}
