//! Memory layer — SQLite + sqlite-vec.
//!
//! Kilroy's persistent brain. Kilroy repositories and projects that already
//! opt into `.kilroy/` keep memory beside the code. Other projects use a
//! stable, per-project directory under the operating system's app-data path.
//! A single SQLite file holds:
//!
//! * Conversation history (sessions, messages)
//! * Code chunks + their vector embeddings (semantic search)
//! * Task graph runs + tool call logs (audit trail)
//! * Decision log + decision embeddings (architectural why)
//! * Activity stream (UI feed)
//!
//! Everything is local. No daemon. No cloud. The sqlite-vec extension is
//! statically linked and auto-registered the first time we open a connection.

pub mod actions;
pub mod activity;
pub mod agent_runtime;
pub mod chunks;
pub mod decisions;
pub mod files;
pub mod messages;
pub mod projects;
pub mod schema;
pub mod sessions;
pub mod tasks;

use anyhow::{Context, Result};
use parking_lot::Mutex;
use rusqlite::Connection;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Once};

/// Owns the project-local memory DB connection.
///
/// Held inside `AppState`. Replaced when the user opens a different folder.
pub struct Memory {
    pub root: PathBuf,
    pub conn: Arc<Mutex<Connection>>,
}

/// Determine whether `root` is Kilroy's own repo (has the smartcoder script).
fn is_kilroy_repo(root: &Path) -> bool {
    root.join("smartcoder")
        .join("kilroy_smartcoder.py")
        .is_file()
}

impl Memory {
    /// Open `<root>/.kilroy/memory.db`, applying migrations on first use.
    ///
    /// The memory DB is created in `root/.kilroy/` ONLY when:
    ///   - The folder IS the Kilroy repo (has `smartcoder/kilroy_smartcoder.py`), or
    ///   - The folder already has a `.kilroy/` directory.
    ///
    /// Otherwise the DB is stored in the app-data directory
    /// (`<app config dir>/memories/<root_hash>/`), preventing `.kilroy/`
    /// from leaking into every random folder the user opens.
    pub fn open(root: impl AsRef<Path>, app_data_dir: &Path) -> Result<Self> {
        register_vec_extension();

        let root = root.as_ref().to_path_buf();

        // Determine where the .kilroy directory lives.
        let dir: PathBuf = if is_kilroy_repo(&root) || root.join(".kilroy").is_dir() {
            // Authorised: use the folder-local location.
            root.join(".kilroy")
        } else {
            // Foreign folder: route to app-data so we don't litter the user's FS.
            let hash = format!(
                "{:x}",
                Sha256::digest(root.to_string_lossy().as_bytes())
            );
            let memories_dir = app_data_dir.join("memories");
            memories_dir.join(hash)
        };
        std::fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;

        let db_path = dir.join("memory.db");
        let conn =
            Connection::open(&db_path).with_context(|| format!("opening {}", db_path.display()))?;

        // Standard SQLite tuning for an interactive app.
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        conn.pragma_update(None, "temp_store", "MEMORY")?;
        conn.pragma_update(None, "mmap_size", 268_435_456_i64)?; // 256 MB

        schema::migrate(&conn).context("running migrations")?;
        agent_runtime::recover_interrupted(&conn).context("recovering interrupted agent runs")?;

        tracing::info!(db = %db_path.display(), "memory db ready");

        Ok(Self {
            root,
            conn: Arc::new(Mutex::new(conn)),
        })
    }

    /// A read-only handle that runs `f` with the shared connection.
    pub fn with_conn<R>(&self, f: impl FnOnce(&Connection) -> Result<R>) -> Result<R> {
        let guard = self.conn.lock();
        f(&guard)
    }
}

// ─── sqlite-vec auto-extension registration ─────────────────────────────────
//
// sqlite-vec is a C extension. We use SQLite's auto-extension mechanism so
// every connection opened from this point on automatically has vec0 virtual
// tables available, without each `Connection::open` having to call
// `load_extension` manually.

static REGISTER_ONCE: Once = Once::new();

fn register_vec_extension() {
    REGISTER_ONCE.call_once(|| {
        // SAFETY: sqlite3_auto_extension is a thread-safe one-shot registration
        // call. sqlite_vec::sqlite3_vec_init has the correct ABI for an
        // sqlite3 entry-point function — we just have to spell its signature
        // out in rusqlite's type universe (each crate has its own bindgen'd
        // `sqlite3` / `sqlite3_api_routines`).
        type EntryPoint = unsafe extern "C" fn(
            *mut rusqlite::ffi::sqlite3,
            *mut *mut std::ffi::c_char,
            *const rusqlite::ffi::sqlite3_api_routines,
        ) -> std::ffi::c_int;
        unsafe {
            let init: EntryPoint = std::mem::transmute(sqlite_vec::sqlite3_vec_init as *const ());
            rusqlite::ffi::sqlite3_auto_extension(Some(init));
        }
        tracing::debug!("sqlite-vec extension registered");
    });
}
