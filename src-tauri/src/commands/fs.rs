//! Filesystem commands.
//!
//! These are intentionally small wrappers around `std::fs` that produce
//! shapes the frontend wants directly — a flat list of children for a
//! directory, raw text for a file, etc. Watching is handled separately
//! via the `notify` crate when the agent runtime is wired in.

use serde::Serialize;
use std::path::{Path, PathBuf};
use tauri::AppHandle;
use tauri_plugin_dialog::DialogExt;

#[derive(Serialize)]
pub struct DirEntry {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    pub size: u64,
}

#[tauri::command]
pub async fn list_dir(path: String) -> Result<Vec<DirEntry>, String> {
    let p = PathBuf::from(&path);
    let mut entries = Vec::new();

    let read = std::fs::read_dir(&p).map_err(|e| format!("read_dir({}): {}", path, e))?;
    for ent in read.flatten() {
        let meta = match ent.metadata() {
            Ok(m) => m,
            Err(_) => continue,
        };
        let name = ent.file_name().to_string_lossy().to_string();
        // Skip junk we never want shown in an IDE explorer by default.
        if name == ".DS_Store" || name == "Thumbs.db" {
            continue;
        }
        entries.push(DirEntry {
            name,
            path: ent.path().to_string_lossy().to_string(),
            is_dir: meta.is_dir(),
            size: meta.len(),
        });
    }

    // Folders first, alpha within each group, case-insensitive.
    entries.sort_by(|a, b| match (a.is_dir, b.is_dir) {
        (true, false) => std::cmp::Ordering::Less,
        (false, true) => std::cmp::Ordering::Greater,
        _ => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
    });

    Ok(entries)
}

#[tauri::command]
pub async fn read_file(path: String) -> Result<String, String> {
    let bytes =
        std::fs::read(Path::new(&path)).map_err(|e| format!("read_file({}): {}", path, e))?;
    // Best-effort UTF-8 decode; lossy on invalid bytes so the editor can still open binaries.
    Ok(String::from_utf8_lossy(&bytes).to_string())
}

#[tauri::command]
pub async fn write_file(path: String, contents: String) -> Result<(), String> {
    if let Some(parent) = Path::new(&path).parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("mkdir({}): {}", parent.display(), e))?;
        }
    }
    std::fs::write(Path::new(&path), contents).map_err(|e| format!("write_file({}): {}", path, e))
}

#[tauri::command]
pub async fn file_exists(path: String) -> bool {
    Path::new(&path).exists()
}

#[tauri::command]
pub async fn pick_folder(app: AppHandle) -> Option<String> {
    let (tx, rx) = std::sync::mpsc::channel();
    app.dialog().file().pick_folder(move |p| {
        let _ = tx.send(p.map(|fp| fp.to_string()));
    });
    rx.recv().ok().flatten()
}

/// Save-as dialog. Used for the first save of an Untitled tab so the
/// user gets to pick where the file lands. `default_name` seeds the
/// filename field (e.g. "Untitled 1"); the frontend is free to swap it
/// for something derived from the buffer's first line if you want.
#[tauri::command]
pub async fn pick_save_file(app: AppHandle, default_name: Option<String>) -> Option<String> {
    let (tx, rx) = std::sync::mpsc::channel();
    let mut builder = app.dialog().file();
    if let Some(name) = default_name {
        builder = builder.set_file_name(name);
    }
    builder.save_file(move |p| {
        let _ = tx.send(p.map(|fp| fp.to_string()));
    });
    rx.recv().ok().flatten()
}

/// Open-file dialog. Optional filters tighten the picker — each filter
/// is `(name, [ext1, ext2])`, e.g. `("Datasets", ["json", "jsonl"])`.
/// Pass an empty filters list for any-file mode. Returns absolute path
/// of the chosen file, or None when the user cancels.
#[tauri::command]
pub async fn pick_open_file(app: AppHandle, filters: Option<Vec<FilterSpec>>) -> Option<String> {
    let (tx, rx) = std::sync::mpsc::channel();
    let mut builder = app.dialog().file();
    if let Some(filters) = filters {
        for f in filters {
            let exts: Vec<&str> = f.extensions.iter().map(|s| s.as_str()).collect();
            builder = builder.add_filter(&f.name, &exts);
        }
    }
    builder.pick_file(move |p| {
        let _ = tx.send(p.map(|fp| fp.to_string()));
    });
    rx.recv().ok().flatten()
}

#[derive(serde::Deserialize)]
pub struct FilterSpec {
    pub name: String,
    pub extensions: Vec<String>,
}
