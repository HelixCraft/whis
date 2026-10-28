//! Transcription history commands.
//!
//! Read/delete operations plus the clipboard helper used by the history view.
//! The clipboard goes through `whis-core` so it honors the configured
//! backend (X11, Wayland, Flatpak) instead of relying on browser APIs.

use crate::state::AppState;
use tauri::State;
use whis_core::history::{self, HistoryEntry};

/// List history entries, newest first.
#[tauri::command]
pub fn list_history(state: State<'_, AppState>) -> Result<Vec<HistoryEntry>, String> {
    let max = state.with_settings(|s| s.ui.history.max_entries);
    history::list(max).map_err(|e| e.to_string())
}

/// Delete a single entry. Returns whether an entry was removed.
#[tauri::command]
pub fn delete_history_entry(id: String) -> Result<bool, String> {
    history::delete(&id).map_err(|e| e.to_string())
}

/// Remove all history entries.
#[tauri::command]
pub fn clear_history() -> Result<(), String> {
    history::clear().map_err(|e| e.to_string())
}

/// Copy arbitrary text using the configured clipboard backend.
#[tauri::command]
pub fn copy_text(text: String, state: State<'_, AppState>) -> Result<(), String> {
    let method = state.with_settings(|s| s.ui.clipboard_backend.clone());
    whis_core::copy_to_clipboard(&text, method).map_err(|e| e.to_string())
}

/// Directory containing the history file.
#[tauri::command]
pub fn history_storage_path() -> String {
    history::history_dir().to_string_lossy().to_string()
}

/// Open the history directory in the system file manager.
#[tauri::command]
pub fn open_history_folder() -> Result<(), String> {
    crate::commands::open_external_path(&history::history_dir())
}
