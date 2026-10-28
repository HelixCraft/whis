//! Local model catalog commands.
//!
//! The catalog lives in `whis-core`, so the UI only lists, downloads, and
//! deletes entries by id.

use crate::state::AppState;
use std::sync::{Mutex, OnceLock};
use tauri::{AppHandle, Emitter, Manager, State};
use whis_core::model::{self, LocalModelSpec};

static MODEL_DOWNLOAD_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

fn download_lock() -> &'static Mutex<()> {
    MODEL_DOWNLOAD_LOCK.get_or_init(|| Mutex::new(()))
}

/// Progress event for model downloads.
#[derive(Clone, serde::Serialize)]
pub struct DownloadProgress {
    pub model_id: String,
    pub downloaded: u64,
    pub total: u64,
}

/// Catalog entry as exposed to the UI.
#[derive(serde::Serialize)]
pub struct LocalModelInfo {
    pub id: String,
    pub name: String,
    pub engine: String,
    pub engine_label: String,
    pub format: String,
    pub languages: String,
    pub size_mb: u64,
    pub size_label: String,
    pub license: String,
    pub description: String,
    pub installed: bool,
    pub available: bool,
    pub supported: bool,
    pub selected: bool,
    pub path: String,
    pub note: String,
}

/// Notes explaining why a model cannot be used.
fn note_for(spec: &LocalModelSpec, available: bool) -> String {
    if !spec.supported {
        spec.description.to_string()
    } else if !available {
        format!(
            "This build does not include the {} engine.",
            spec.engine.label()
        )
    } else {
        String::new()
    }
}

/// List every catalog model with its install state.
#[tauri::command]
pub fn get_local_models(state: State<'_, AppState>) -> Vec<LocalModelInfo> {
    let selected = state
        .settings
        .lock()
        .unwrap()
        .transcription
        .local_models
        .selected
        .clone();

    model::all_models()
        .iter()
        .map(|spec| {
            let available = spec.is_available();
            LocalModelInfo {
                id: spec.id.to_string(),
                name: spec.name.to_string(),
                engine: spec.engine.as_str().to_string(),
                engine_label: spec.engine.label().to_string(),
                format: spec.format.as_str().to_string(),
                languages: spec.languages.to_string(),
                size_mb: spec.size_mb,
                size_label: spec.size_label(),
                license: spec.license.to_string(),
                description: spec.description.to_string(),
                installed: spec.is_installed(),
                available,
                supported: spec.supported,
                selected: selected.as_deref() == Some(spec.id),
                path: spec.path().to_string_lossy().to_string(),
                note: note_for(spec, available),
            }
        })
        .collect()
}

/// Download a catalog model. Returns its path.
#[tauri::command]
pub async fn download_local_model(app: AppHandle, model_id: String) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let spec =
            model::find(&model_id).ok_or_else(|| format!("Unknown local model: {model_id}"))?;
        if !spec.is_available() {
            return Err(format!("{} is not available in this build", spec.name));
        }

        // One download at a time keeps progress reporting unambiguous.
        let _guard = download_lock()
            .try_lock()
            .map_err(|_| "A model download is already in progress".to_string())?;

        let state = app.state::<AppState>();
        *state.active_download.lock().unwrap() = Some(crate::state::DownloadState {
            model_name: model_id.clone(),
            model_type: spec.engine.as_str().to_string(),
            downloaded: 0,
            total: 0,
        });

        let result = model::install::install_with_progress(spec, |downloaded, total| {
            if let Some(ref mut dl) = *state.active_download.lock().unwrap() {
                dl.downloaded = downloaded;
                dl.total = total;
            }
            let _ = app.emit(
                "download-progress",
                DownloadProgress {
                    model_id: model_id.clone(),
                    downloaded,
                    total,
                },
            );
        });

        *state.active_download.lock().unwrap() = None;

        result
            .map(|path| path.to_string_lossy().to_string())
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Delete an installed catalog model.
#[tauri::command]
pub fn delete_local_model(model_id: String) -> Result<(), String> {
    let spec = model::find(&model_id).ok_or_else(|| format!("Unknown local model: {model_id}"))?;
    model::install::remove(spec).map_err(|e| e.to_string())
}

/// Whether the configured local model exists and is complete.
#[tauri::command]
pub fn is_local_model_valid(state: State<'_, AppState>) -> bool {
    let settings = state.settings.lock().unwrap();
    let transcription = &settings.transcription;

    // A custom path overrides the catalog selection, so verify that file only.
    if let Some(path) = transcription
        .local_models
        .custom_path
        .as_deref()
        .filter(|path| !path.is_empty())
    {
        return std::path::Path::new(path).exists();
    }

    transcription
        .selected_local_model()
        .is_some_and(|model| model.verify(&model.path()))
}

/// Current active download, used to restore progress after reopening the window.
#[derive(serde::Serialize)]
pub struct ActiveDownloadInfo {
    pub model_name: String,
    pub model_type: String,
    pub downloaded: u64,
    pub total: u64,
}

#[tauri::command]
pub fn get_active_download(state: State<'_, AppState>) -> Option<ActiveDownloadInfo> {
    state
        .active_download
        .lock()
        .unwrap()
        .as_ref()
        .map(|dl| ActiveDownloadInfo {
            model_name: dl.model_name.clone(),
            model_type: dl.model_type.clone(),
            downloaded: dl.downloaded,
            total: dl.total,
        })
}
