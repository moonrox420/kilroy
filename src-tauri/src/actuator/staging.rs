//! Disposable command workspaces. No sandbox receives a mount of the real
//! project. Its resulting bytes become separate snapshot-checked approvals.

use super::{is_protected_path, resolve_safe, ActionPayload};
use anyhow::{anyhow, bail, Context, Result};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

const MAX_FILES: usize = 20_000;
const MAX_FILE_BYTES: u64 = 16 * 1024 * 1024;
const MAX_TOTAL_BYTES: u64 = 128 * 1024 * 1024;
const MAX_CHANGE_BYTES: usize = 16 * 1024 * 1024;

pub struct StagedWorkspace {
    root: PathBuf,
    before: BTreeMap<String, Vec<u8>>,
}

#[derive(Clone)]
pub struct StagedChange {
    pub path: String,
    pub payload: ActionPayload,
    pub diff: Option<String>,
}

impl StagedWorkspace {
    pub fn new(project: &Path) -> Result<Self> {
        let project = project.canonicalize().context("resolve staging source")?;
        let before = snapshot(&project, false)?;
        let root =
            std::env::temp_dir().join(format!("kilroy-stage-{}", uuid::Uuid::new_v4().simple()));
        fs::create_dir(&root).context("create command staging directory")?;
        let workspace = Self { root, before };
        for (relative, bytes) in &workspace.before {
            let destination = workspace.root.join(relative);
            if let Some(parent) = destination.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(&destination, bytes).with_context(|| format!("stage {relative}"))?;
            #[cfg(unix)]
            fs::set_permissions(
                &destination,
                fs::metadata(project.join(relative))?.permissions(),
            )?;
        }
        Ok(workspace)
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn changes(&self) -> Result<Vec<StagedChange>> {
        let after = snapshot(&self.root, true)?;
        let paths: BTreeSet<_> = self.before.keys().chain(after.keys()).collect();
        let mut result = Vec::new();
        let mut changed_bytes = 0usize;
        for path in paths {
            let previous = self.before.get(path);
            let next = after.get(path);
            if previous == next {
                continue;
            }
            changed_bytes += next.map_or(0, Vec::len);
            if changed_bytes > MAX_CHANGE_BYTES || result.len() >= 1000 {
                bail!("sandbox output exceeds the 16 MiB / 1000-file review limit; no project files were changed");
            }
            let previous_text = previous.map_or(Ok(""), |bytes| std::str::from_utf8(bytes));
            let next_text = next.map_or(Ok(""), |bytes| std::str::from_utf8(bytes));
            let diff = match (previous_text, next_text) {
                (Ok(old), Ok(new)) => Some(
                    similar::TextDiff::from_lines(old, new)
                        .unified_diff()
                        .context_radius(3)
                        .header(&format!("a/{path}"), &format!("b/{path}"))
                        .to_string(),
                ),
                _ => None,
            };
            result.push(StagedChange {
                path: path.clone(),
                payload: ActionPayload::FileChange {
                    path: path.clone(),
                    before_sha256: previous.map(|bytes| hash(bytes)),
                    content: next.cloned(),
                },
                diff,
            });
        }
        Ok(result)
    }
}

impl Drop for StagedWorkspace {
    fn drop(&mut self) {
        if let Err(error) = fs::remove_dir_all(&self.root) {
            if error.kind() != std::io::ErrorKind::NotFound {
                tracing::warn!(path = %self.root.display(), "remove command staging directory: {error}");
            }
        }
    }
}

fn excluded(path: &Path) -> bool {
    let name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("");
    matches!(
        name,
        ".git"
            | ".kilroy"
            | ".venv"
            | "venv"
            | "node_modules"
            | "target"
            | "dist"
            | "build"
            | "__pycache__"
            | ".pytest_cache"
            | ".ruff_cache"
            | ".mypy_cache"
    ) || name == ".env"
        || (name.starts_with(".env.") && !matches!(name, ".env.example" | ".env.template"))
}

fn is_link(metadata: &fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        metadata.file_attributes() & 0x400 != 0
    }
    #[cfg(not(windows))]
    {
        metadata.file_type().is_symlink()
    }
}

fn snapshot(root: &Path, strict_links: bool) -> Result<BTreeMap<String, Vec<u8>>> {
    let metadata = fs::symlink_metadata(root).context("read staging root")?;
    if is_link(&metadata) || !metadata.is_dir() {
        bail!("staging root is no longer a regular directory");
    }
    let mut files = BTreeMap::new();
    let mut pending = vec![(root.to_path_buf(), 0usize)];
    let mut total = 0u64;
    let mut entries_seen = 0usize;
    while let Some((directory, depth)) = pending.pop() {
        if depth > 64 {
            bail!("project exceeds staging directory depth limit");
        }
        for entry in
            fs::read_dir(&directory).with_context(|| format!("read {}", directory.display()))?
        {
            let path = entry?.path();
            if excluded(&path) {
                continue;
            }
            entries_seen += 1;
            if entries_seen > MAX_FILES {
                bail!("project exceeds staging file count limit ({MAX_FILES})");
            }
            let metadata = fs::symlink_metadata(&path)?;
            if is_link(&metadata) {
                if strict_links {
                    bail!("sandbox created a symlink/junction: {}", path.display());
                }
                continue;
            }
            if metadata.is_dir() {
                pending.push((path, depth + 1));
            } else if metadata.is_file() {
                total += metadata.len();
                if metadata.len() > MAX_FILE_BYTES || total > MAX_TOTAL_BYTES {
                    bail!(
                        "project exceeds staging size limits (16 MiB/file, 128 MiB total): {}",
                        path.display()
                    );
                }
                let relative = path
                    .strip_prefix(root)?
                    .to_str()
                    .ok_or_else(|| anyhow!("staging requires UTF-8 file names"))?
                    .replace('\\', "/");
                let bytes = fs::read(&path)?;
                if bytes.len() as u64 != metadata.len() {
                    bail!("file changed while staging: {relative}; retry the command");
                }
                files.insert(relative, bytes);
            } else {
                bail!("unsupported special file in staging: {}", path.display());
            }
        }
    }
    Ok(files)
}

pub fn hash(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// Apply exactly the bytes approved by the user, only against the captured base.
pub fn apply_change(
    root: &Path,
    path: &str,
    expected: Option<&str>,
    content: Option<&[u8]>,
) -> Result<()> {
    if is_protected_path(path) {
        bail!("protected application path: {path}");
    }
    let destination = resolve_safe(root, path)?;
    let actual = match fs::read(&destination) {
        Ok(bytes) => Some(hash(&bytes)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => {
            return Err(error)
                .with_context(|| format!("read {path} before applying sandbox output"))
        }
    };
    if actual.as_deref() != expected {
        bail!("{path} changed since the command ran; rerun it before applying its output");
    }
    match content {
        None => {
            if expected.is_none() {
                bail!("cannot delete a file that did not exist: {path}");
            }
            fs::remove_file(&destination)
                .with_context(|| format!("delete approved file {path}"))?;
        }
        Some(bytes) => {
            let parent = destination
                .parent()
                .ok_or_else(|| anyhow!("file has no parent"))?;
            fs::create_dir_all(parent)?;
            let temporary = parent.join(format!(".kilroy-write-{}", uuid::Uuid::new_v4().simple()));
            let result = (|| -> Result<()> {
                use std::io::Write;
                let mut file = fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(&temporary)?;
                file.write_all(bytes)?;
                file.sync_all()?;
                drop(file);
                if let Ok(metadata) = fs::metadata(&destination) {
                    fs::set_permissions(&temporary, metadata.permissions())?;
                }
                fs::rename(&temporary, &destination).with_context(|| format!("replace {path}"))?;
                Ok(())
            })();
            if result.is_err() && temporary.exists() {
                if let Err(error) = fs::remove_file(&temporary) {
                    tracing::warn!("remove failed staged-write temporary file: {error}");
                }
            }
            result?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn captures_creates_updates_binary_and_deletes_without_touching_project() {
        let root = std::env::temp_dir().join(format!(
            "kilroy-stage-test-{}",
            uuid::Uuid::new_v4().simple()
        ));
        fs::create_dir(&root).unwrap();
        fs::write(root.join("old.txt"), "old").unwrap();
        fs::write(root.join("delete.txt"), "delete").unwrap();
        fs::write(root.join(".env"), "SECRET=hidden").unwrap();
        let staged = StagedWorkspace::new(&root).unwrap();
        assert!(!staged.root().join(".env").exists());
        fs::write(staged.root().join("old.txt"), "new").unwrap();
        fs::write(staged.root().join("new.bin"), [0xff, 0, 1]).unwrap();
        fs::remove_file(staged.root().join("delete.txt")).unwrap();
        let changes = staged.changes().unwrap();
        assert_eq!(changes.len(), 3);
        assert_eq!(fs::read_to_string(root.join("old.txt")).unwrap(), "old");
        assert!(root.join("delete.txt").exists());
        for change in changes {
            if let ActionPayload::FileChange {
                path,
                before_sha256,
                content,
            } = change.payload
            {
                apply_change(&root, &path, before_sha256.as_deref(), content.as_deref()).unwrap();
            } else {
                panic!("unexpected staged payload");
            }
        }
        assert_eq!(fs::read(root.join("new.bin")).unwrap(), [0xff, 0, 1]);
        assert!(!root.join("delete.txt").exists());
        assert_eq!(fs::read_to_string(root.join("old.txt")).unwrap(), "new");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn stale_and_protected_changes_fail_closed() {
        let root = std::env::temp_dir().join(format!(
            "kilroy-stage-test-{}",
            uuid::Uuid::new_v4().simple()
        ));
        fs::create_dir(&root).unwrap();
        fs::write(root.join("file"), "current").unwrap();
        assert!(apply_change(&root, "file", Some(&hash(b"old")), Some(b"new")).is_err());
        assert!(apply_change(&root, "file", None, Some(b"new")).is_err());
        assert!(apply_change(&root, ".git/config", None, Some(b"new")).is_err());
        assert_eq!(fs::read_to_string(root.join("file")).unwrap(), "current");
        fs::remove_dir_all(root).unwrap();
    }
}
