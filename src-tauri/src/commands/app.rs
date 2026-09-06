//! Misc app-level commands.

use serde::Serialize;

#[derive(Serialize)]
pub struct AppInfo {
    pub name: &'static str,
    pub version: &'static str,
    pub commit: Option<String>,
}

#[tauri::command]
pub fn app_info() -> AppInfo {
    AppInfo {
        name: "Kilroy",
        version: env!("CARGO_PKG_VERSION"),
        commit: option_env!("GIT_COMMIT").map(|s| s.to_string()),
    }
}
