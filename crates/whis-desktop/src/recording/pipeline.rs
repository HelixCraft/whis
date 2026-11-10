//! Transcription Pipeline
//!
//! Orchestrates the full transcription pipeline:
//! 1. Finalize recording and persist the audio
//! 2. Transcribe audio (single or parallel chunks)
//! 3. Post-process transcription (optional)
//! 4. Append to history
//! 5. Copy to clipboard / autotype
//! 6. Emit completion event

use crate::state::{AppState, RecordingState};
use std::path::Path;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};
use whis_core::audio::last_recording;
use whis_core::history::{self, HistoryEntry, HistorySource};
use whis_core::{
    AutotypeBackend, ClipboardMethod, DEFAULT_POST_PROCESSING_PROMPT, OutputMethod,
    PostProcessConfig, PostProcessor, TranscriptionProvider, autotype_text, copy_to_clipboard,
    ollama, post_process, warn,
};
#[cfg(feature = "local-transcription")]
use whis_core::{local_cpp_unload_model, unload_parakeet};

/// Truncate text for console output without risking a UTF-8 boundary panic.
fn short(text: &str) -> String {
    text.chars().take(50).collect()
}

/// Output text based on configured output method
fn output_text(
    text: &str,
    output_method: &OutputMethod,
    clipboard_method: &ClipboardMethod,
    autotype_backend: &AutotypeBackend,
    autotype_delay_ms: Option<u32>,
) -> Result<(), String> {
    match output_method {
        OutputMethod::Clipboard => {
            copy_to_clipboard(text, clipboard_method.clone()).map_err(|e| e.to_string())?;
        }
        OutputMethod::Autotype => {
            autotype_text(text, autotype_backend.clone(), autotype_delay_ms)
                .map_err(|e| e.to_string())?;
        }
        OutputMethod::Both => {
            copy_to_clipboard(text, clipboard_method.clone()).map_err(|e| e.to_string())?;
            autotype_text(text, autotype_backend.clone(), autotype_delay_ms)
                .map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

/// Stop recording and run the full transcription pipeline (progressive mode)
/// Guarantees state cleanup on both success and failure
pub async fn stop_and_transcribe(app: &AppHandle) -> Result<(), String> {
    let state = app.state::<AppState>();

    // Stop recording. The samples are kept: they are persisted before
    // transcription so a failed transcription can never lose the audio.
    let stop_result = {
        let recorder = state.recorder.lock().unwrap().take();
        match recorder {
            Some(mut rec) => rec
                .stop_recording()
                .map(|data| data.finalize_raw())
                .map_err(|e| e.to_string()),
            None => Err("No active recording".to_string()),
        }
    };

    let samples = match stop_result {
        Ok(samples) => samples,
        Err(message) => {
            state.set_state(RecordingState::Idle);
            let _ = app.emit("transcription-error", &message);
            return Err(message);
        }
    };

    let duration_secs = samples.len() as f32 / 16_000.0;
    persist_last_audio(app, &state, &samples);

    // Update state to transcribing
    state.set_state(RecordingState::Transcribing);
    println!("Transcribing...");

    // Run transcription with guaranteed state cleanup on any error
    let result = do_progressive_transcription(app, &state, duration_secs).await;

    // Always reset state, regardless of success or failure
    state.set_state(RecordingState::Idle);

    if let Err(ref e) = result {
        let _ = app.emit("transcription-error", e.clone());
    }

    result
}

/// Save the most recent recording as a WAV file.
///
/// Failures are non-fatal: the transcription still runs, the UI only shows a
/// warning.
fn persist_last_audio(app: &AppHandle, state: &AppState, samples: &[f32]) {
    let enabled = state.with_settings(|s| s.ui.last_audio.enabled);
    if !enabled {
        return;
    }

    let (provider, model, language) = transcription_identity(state);

    match last_recording::save(samples, Some(provider), model, language) {
        Ok(info) => {
            let _ = app.emit("last-audio-updated", &info);
        }
        Err(e) => {
            let message = format!("Could not save the last recording: {e}");
            warn!("{message}");
            let _ = app.emit("last-audio-warning", &message);
        }
    }
}

/// Provider, model and language currently in effect.
///
/// Falls back to settings when no recording cached a config yet (for example
/// after an app restart before re-transcribing).
fn transcription_identity(state: &AppState) -> (String, Option<String>, Option<String>) {
    {
        let config = state.transcription_config.lock().unwrap();
        if let Some(c) = config.as_ref() {
            return (
                c.provider.as_str().to_string(),
                model_label(&c.provider, &c.api_key),
                c.language.clone(),
            );
        }
    }

    let settings = state.settings.lock().unwrap();
    let provider = settings.transcription.provider.clone();
    let model_source = if provider.is_local() {
        settings.transcription.local_model_path()
    } else {
        settings.transcription.api_key_from_settings()
    };
    (
        provider.as_str().to_string(),
        model_source
            .as_deref()
            .and_then(|value| model_label(&provider, value)),
        settings.transcription.language.clone(),
    )
}

/// Human readable model label for local models.
fn model_label(provider: &TranscriptionProvider, api_key: &str) -> Option<String> {
    if !provider.is_local() {
        return None;
    }
    if let Some(id) = whis_core::model::id_for_path(api_key) {
        return Some(id.to_string());
    }
    Path::new(api_key)
        .file_stem()
        .and_then(|s| s.to_str())
        .map(str::to_string)
}

/// Transcript produced by one transcription run, before it is stored.
struct Transcript<'a> {
    raw: &'a str,
    final_text: &'a str,
    post_processed: bool,
}

/// Persist a history entry. Never fails the pipeline: history is a safety net,
/// not a requirement for the transcription itself.
fn record_history(
    app: &AppHandle,
    state: &AppState,
    transcript: Transcript<'_>,
    duration_secs: f32,
    source: HistorySource,
    parent_id: Option<String>,
) {
    let (enabled, max_entries) =
        state.with_settings(|s| (s.ui.history.enabled, s.ui.history.max_entries));
    if !enabled || transcript.final_text.trim().is_empty() {
        return;
    }

    let (provider, model, language) = transcription_identity(state);

    let mut entry = HistoryEntry::new(transcript.final_text, provider);
    entry.raw_text = transcript.raw.to_string();
    entry.post_processed = transcript.post_processed;
    entry.model = model;
    entry.language = language;
    entry.duration_secs = duration_secs;
    entry.source = source;
    entry.parent_id = parent_id;

    match history::append(&entry, max_entries) {
        Ok(()) => {
            if last_recording::path().exists() {
                last_recording::set_history_id(&entry.id);
            }
            let _ = app.emit("history-updated", ());
        }
        Err(e) => {
            warn!("Failed to save transcription history: {e}");
            let _ = app.emit("history-warning", e.to_string());
        }
    }
}

/// Apply the configured post-processing to a transcript.
///
/// Post-processing failures are non-fatal and return the raw text, matching
/// the behaviour of the recording pipeline.
pub(crate) async fn post_process_text(
    app: &AppHandle,
    state: &AppState,
    text: &str,
) -> (String, bool) {
    let config = {
        let settings = state.settings.lock().unwrap();
        if settings.post_processing.enabled
            && settings.post_processing.processor != PostProcessor::None
        {
            let processor = settings.post_processing.processor.clone();
            let prompt = settings
                .post_processing
                .prompt
                .clone()
                .unwrap_or_else(|| DEFAULT_POST_PROCESSING_PROMPT.to_string());
            let ollama_model = settings.services.ollama.model.clone();
            let ollama_keep_alive = settings.services.ollama.keep_alive();

            let api_key_or_url = if processor.requires_api_key() {
                settings
                    .post_processing
                    .api_key_from_settings(&settings.transcription.api_keys)
            } else if processor == PostProcessor::Ollama {
                let ollama_url = settings
                    .services
                    .ollama
                    .url()
                    .unwrap_or_else(|| ollama::DEFAULT_OLLAMA_URL.to_string());
                Some(ollama_url)
            } else {
                None
            };

            api_key_or_url.map(|key_or_url| PostProcessConfig {
                processor,
                prompt,
                api_key_or_url: key_or_url,
                ollama_model,
                ollama_keep_alive,
            })
        } else {
            None
        }
    };

    let Some(config) = config else {
        return (text.to_string(), false);
    };

    if config.processor == PostProcessor::Ollama {
        let url_for_check = config.api_key_or_url.clone();
        let ollama_result = tauri::async_runtime::spawn_blocking(move || {
            ollama::ensure_ollama_running(&url_for_check)
        })
        .await;

        match ollama_result {
            Ok(Ok(_)) => {}
            Ok(Err(e)) => {
                let warning = format!("Ollama: {e}");
                warn!("Post-processing: {warning}");
                let _ = app.emit("post-process-warning", &warning);
                return (text.to_string(), false);
            }
            Err(e) => {
                let warning = format!("Post-processing task failed: {e}");
                warn!("{warning}");
                let _ = app.emit("post-process-warning", &warning);
                return (text.to_string(), false);
            }
        }

        // Re-warm Ollama model (in case it unloaded during long recording > keep_alive timeout)
        if config.ollama_model.is_some() {
            {
                let settings = state.settings.lock().unwrap();
                settings.services.ollama.preload();
            }
            // Brief pause to allow warmup to complete (runs in background thread)
            tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;
        }
    }

    println!("Post-processing...");
    let _ = app.emit("post-process-started", ());

    let model = if config.processor == PostProcessor::Ollama {
        config.ollama_model.as_deref()
    } else {
        None
    };

    match post_process(
        text,
        &config.processor,
        &config.api_key_or_url,
        &config.prompt,
        model,
    )
    .await
    {
        Ok(processed) => (processed, true),
        Err(e) => {
            let warning = e.to_string();
            warn!("Post-processing: {warning}");
            let _ = app.emit("post-process-warning", &warning);
            (text.to_string(), false)
        }
    }
}

/// Write history, emit the result, and handle output failures as warnings.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn finalize_transcription(
    app: &AppHandle,
    state: &AppState,
    raw_text: &str,
    final_text: &str,
    post_processed: bool,
    duration_secs: f32,
    source: HistorySource,
    parent_id: Option<String>,
) -> Result<(), String> {
    // Persist before output: a clipboard failure must not lose the text.
    record_history(
        app,
        state,
        Transcript {
            raw: raw_text,
            final_text,
            post_processed,
        },
        duration_secs,
        source,
        parent_id,
    );

    let (clipboard_method, output_method, autotype_backend, autotype_delay_ms) = {
        let settings = state.settings.lock().unwrap();
        (
            settings.ui.clipboard_backend.clone(),
            settings.ui.output_method.clone(),
            settings.ui.autotype_backend.clone(),
            settings.ui.autotype_delay_ms,
        )
    };

    if let Err(e) = output_text(
        final_text,
        &output_method,
        &clipboard_method,
        &autotype_backend,
        autotype_delay_ms,
    ) {
        warn!("Output failed: {e}");
        let _ = app.emit("output-warning", e);
    }

    println!("Done: {}", short(final_text));

    let _ = app.emit("transcription-complete", final_text);

    // Schedule idle model unload (if configured)
    schedule_idle_model_unload(app, state);

    Ok(())
}

/// Store the quality of a finished recording and notify the UI.
pub(crate) fn publish_audio_quality(
    app: &AppHandle,
    state: &AppState,
    quality: whis_core::audio::AudioQuality,
) {
    if quality.chunks == 0 {
        return;
    }

    *state.last_audio_quality.lock().unwrap() = Some(quality.clone());
    let _ = app.emit("audio-quality-updated", &quality);

    let mode = state.with_settings(|s| s.ui.audio_enhance.mode);
    if quality.is_noisy()
        && matches!(
            mode,
            whis_core::audio::EnhanceMode::Off | whis_core::audio::EnhanceMode::Light
        )
    {
        let _ = app.emit("audio-quality-warning", &quality);
    }
}

/// Publish the measured quality of the finished recording.
///
/// When the recording looks noisy and denoising is off, the UI also gets a
/// one-time recommendation. The raw audio is never changed by this.
fn report_audio_quality(app: &AppHandle, state: &AppState) {
    let quality = state.active_quality.lock().unwrap().take();
    let Some(quality) = quality else {
        return;
    };
    let quality = quality.lock().unwrap().clone();
    publish_audio_quality(app, state, quality);
}

/// Progressive transcription logic - receives result from background task
async fn do_progressive_transcription(
    app: &AppHandle,
    state: &AppState,
    duration_secs: f32,
) -> Result<(), String> {
    // Receive transcription result from background task
    let rx = {
        let mut rx_guard = state.transcription_rx.lock().unwrap();
        rx_guard
            .take()
            .ok_or("No progressive transcription in progress")?
    };

    // Wait for transcription to complete (rx_guard dropped, so this is Send-safe)
    let transcription = rx
        .await
        .map_err(|_| "Transcription task dropped unexpectedly".to_string())?;

    // Report quality even when transcription failed: the audio was measured
    // and the user can still act on a noisy-recording recommendation.
    report_audio_quality(app, state);

    let raw_text = transcription.map_err(|e| format!("Transcription failed: {e}"))?;

    let (final_text, post_processed) = post_process_text(app, state, &raw_text).await;

    finalize_transcription(
        app,
        state,
        &raw_text,
        &final_text,
        post_processed,
        duration_secs,
        HistorySource::Recording,
        None,
    )
    .await
}

/// Schedule automatic model unload after idle timeout
///
/// If keep_model_loaded is true and unload_after_minutes > 0, spawns a background
/// task that will unload the model after the configured idle period.
/// The task can be cancelled if a new recording starts.
fn schedule_idle_model_unload(app: &AppHandle, state: &AppState) {
    // Read settings to determine if we should schedule an unload
    let (keep_loaded, unload_minutes, provider) = {
        let settings = state.settings.lock().unwrap();
        let config = state.transcription_config.lock().unwrap();
        let provider = config.as_ref().map(|c| c.provider.clone());
        (
            settings.ui.model_memory.keep_model_loaded,
            settings.ui.model_memory.unload_after_minutes,
            provider,
        )
    };

    // Only schedule if:
    // 1. keep_model_loaded is true (otherwise model is already unloaded)
    // 2. unload_minutes > 0 (0 means "never auto-unload")
    // 3. We have a local provider
    #[cfg(feature = "local-transcription")]
    if keep_loaded && unload_minutes > 0 && provider.as_ref().is_some_and(|p| p.is_local()) {
        let duration = Duration::from_secs(u64::from(unload_minutes) * 60);
        let app_handle = app.clone();

        // Spawn the delayed unload task
        let task = tauri::async_runtime::spawn(async move {
            tokio::time::sleep(duration).await;

            // After sleep, unload the model
            let state = app_handle.state::<AppState>();

            // Re-check settings in case user changed them while we were waiting
            let should_unload = {
                let settings = state.settings.lock().unwrap();
                settings.ui.model_memory.keep_model_loaded
                    && settings.ui.model_memory.unload_after_minutes > 0
            };

            if should_unload {
                println!(
                    "Auto-unloading model after {} minutes of inactivity",
                    unload_minutes
                );
                // Only one local engine is active; clearing all caches is cheap.
                local_cpp_unload_model();
                unload_parakeet();
            }
        });

        // Store the task handle so we can cancel if a new recording starts
        state.set_idle_unload_handle(task);
    }

    // Suppress unused variable warnings when local-transcription feature is disabled
    #[cfg(not(feature = "local-transcription"))]
    {
        let _ = (keep_loaded, unload_minutes, provider);
        let _ = app;
    }
}
