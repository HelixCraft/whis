//! Commands for the most recent recording.
//!
//! The recording is written before transcription starts, so these commands
//! let the user inspect, play, delete, or re-transcribe it later.

use crate::recording::pipeline::{
    finalize_transcription, post_process_text, publish_audio_quality,
};
use crate::state::{AppState, RecordingState};
use std::sync::atomic::Ordering;
use tauri::{AppHandle, Emitter, State};
use whis_core::audio::enhance::EnhanceConfig;
use whis_core::audio::last_recording::{self, LastAudioInfo};
use whis_core::history::HistorySource;

/// Metadata of the stored recording, if any.
///
/// Returns `None` while the storage setting is off, even if an older file is
/// still on disk.
#[tauri::command]
pub fn get_last_audio_info(state: State<'_, AppState>) -> Option<LastAudioInfo> {
    if !state.with_settings(|s| s.ui.last_audio.enabled) {
        return None;
    }
    last_recording::info()
}

/// Delete the stored recording.
#[tauri::command]
pub fn delete_last_audio() -> Result<(), String> {
    last_recording::delete().map_err(|e| e.to_string())
}

/// Play the stored recording with the system's default audio player.
#[tauri::command]
pub fn play_last_audio() -> Result<(), String> {
    let path = last_recording::path();
    if !path.exists() {
        return Err("No recording stored".to_string());
    }
    crate::commands::open_external_path(&path)
}

/// Open the whis data directory in the system file manager.
#[tauri::command]
pub fn open_data_folder() -> Result<(), String> {
    crate::commands::open_external_path(&whis_core::history::data_dir())
}

/// Re-run transcription on the stored recording with the current settings.
///
/// Useful after switching models or when the previous transcription failed.
/// The result is appended to history as a new entry that references the
/// original one, so nothing is overwritten.
#[tauri::command]
pub async fn retranscribe_last_audio(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<String, String> {
    let state = state.inner();

    // Never interleave with an active recording: the pipeline owns the state
    // machine (and the model cache) until it is done.
    if state.get_state() != RecordingState::Idle {
        return Err("Finish the current recording first".to_string());
    }

    if state
        .retranscribing
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return Err("A re-transcription is already running".to_string());
    }
    // Resets the flag and the recording state on every exit path.
    let _guard = RetranscribeGuard { state };

    let result = run_retranscription(&app, state).await;

    if let Err(ref e) = result {
        let _ = app.emit("transcription-error", e.clone());
    }
    result
}

struct RetranscribeGuard<'a> {
    state: &'a AppState,
}

impl Drop for RetranscribeGuard<'_> {
    fn drop(&mut self) {
        self.state.retranscribing.store(false, Ordering::SeqCst);
        self.state.set_state(RecordingState::Idle);
    }
}

async fn run_retranscription(app: &AppHandle, state: &AppState) -> Result<String, String> {
    let info = last_recording::info()
        .ok_or_else(|| "No recording stored. Record something first.".to_string())?;
    let parent_id = last_recording::history_id();
    let samples = last_recording::load_samples().map_err(|e| e.to_string())?;

    // Settings may have changed since the recording, so reload them and refresh
    // the cached config used for history metadata.
    let config = crate::recording::load_transcription_config(state)?;
    *state.transcription_config.lock().unwrap() = Some(config.clone());

    state.set_state(RecordingState::Transcribing);
    let _ = app.emit("retranscription-started", ());

    // Re-apply the cleanup chain with the current mode. The stored file stays
    // raw, so a later re-transcription can use a different mode.
    let enhance_config = EnhanceConfig::from(state.with_settings(|s| s.ui.audio_enhance));
    let (raw_text, quality) = whis_core::transcribe_samples_enhanced(
        &config.provider,
        &config.api_key,
        config.language.as_deref(),
        samples,
        &enhance_config,
    )
    .await
    .map_err(|e| format!("Re-transcription failed: {e}"))?;

    publish_audio_quality(app, state, quality);

    if raw_text.trim().is_empty() {
        return Err("Re-transcription returned no text".to_string());
    }

    let (final_text, post_processed) = post_process_text(app, state, &raw_text).await;

    finalize_transcription(
        app,
        state,
        &raw_text,
        &final_text,
        post_processed,
        info.duration_secs,
        HistorySource::Retranscription,
        parent_id,
    )
    .await?;

    let _ = app.emit("last-audio-updated", &info);

    Ok(final_text)
}
