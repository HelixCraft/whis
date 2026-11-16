//! Audio test recording for the settings page.
//!
//! Recording is explicit: the UI starts it, the user stops it. Both the raw
//! and the cleaned audio are returned as data URLs so the card can play them
//! in the window instead of opening an external player. Nothing here touches
//! the stored last recording.

use crate::state::{AppState, RecordingState};
use base64::Engine;
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use tauri::State;
use whis_core::audio::DenoiseAlgorithm;
use whis_core::audio::enhance::{self, AudioQuality, EnhanceConfig, EnhanceMode};
use whis_core::audio::last_recording::{read_wav, wav_bytes, write_wav};

/// Shortest accepted test recording.
const MIN_TEST_SAMPLES: usize = whis_core::resample::WHISPER_SAMPLE_RATE as usize / 2;

/// Result of a test recording or reprocessing run.
#[derive(serde::Serialize)]
pub struct AudioTestResult {
    pub duration_secs: f32,
    pub quality: AudioQuality,
    pub mode: String,
    pub algorithm: String,
    pub strength: f32,
    /// Raw sample as a WAV data URL, for in-window playback.
    pub raw_audio: String,
    /// Cleaned sample as a WAV data URL.
    pub clean_audio: String,
}

fn test_paths() -> (PathBuf, PathBuf) {
    let dir = whis_core::history::data_dir();
    (
        dir.join("audio_test_raw.wav"),
        dir.join("audio_test_clean.wav"),
    )
}

fn data_url(samples: &[f32]) -> Result<String, String> {
    let bytes = wav_bytes(samples).map_err(|e| e.to_string())?;
    Ok(format!(
        "data:audio/wav;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(bytes)
    ))
}

/// Apply a cleanup configuration to a test sample and write the clean WAV.
///
/// The raw preview gets the same gain that the chain applies, so the two files
/// only differ by the filtering itself. The files are still written because
/// the transcription buttons read them from disk.
fn process_test(
    state: &AppState,
    samples: Vec<f32>,
    config: EnhanceConfig,
) -> Result<AudioTestResult, String> {
    let enhanced = enhance::enhance(&samples, &config);

    let raw = if enhanced.stats.gain_db > 0.0 {
        let ratio = 10f32.powf(enhanced.stats.gain_db / 20.0);
        samples.iter().map(|s| s * ratio).collect()
    } else {
        samples
    };

    let (raw_path, clean_path) = test_paths();
    write_wav(&raw_path, &raw).map_err(|e| e.to_string())?;
    write_wav(&clean_path, &enhanced.samples).map_err(|e| e.to_string())?;

    let raw_audio = data_url(&raw)?;
    let clean_audio = data_url(&enhanced.samples)?;

    let mut quality = AudioQuality::default();
    quality.add(&enhanced.stats);
    *state.last_audio_quality.lock().unwrap() = Some(quality.clone());

    let applied_strength = match config.mode {
        EnhanceMode::StrongDenoise => 1.0,
        _ => config.strength.clamp(0.0, 1.0),
    };

    Ok(AudioTestResult {
        duration_secs: enhanced.stats.duration_secs,
        quality,
        mode: config.mode.as_str().to_string(),
        algorithm: config.algorithm.as_str().to_string(),
        strength: applied_strength,
        raw_audio,
        clean_audio,
    })
}

/// Start recording a test sample with the configured microphone.
///
/// The recorder stays in the app state until [`stop_audio_test_recording`] or
/// [`cancel_audio_test_recording`] is called.
#[tauri::command]
pub fn start_audio_test_recording(state: State<'_, AppState>) -> Result<(), String> {
    let state = state.inner();

    if state.get_state() != RecordingState::Idle {
        return Err("Finish the current recording first".to_string());
    }
    if state
        .testing_audio
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return Err("A test recording is already running".to_string());
    }

    let device = state.with_settings(|s| s.ui.microphone_device.clone());
    let started = (|| -> Result<(), String> {
        let mut recorder = whis_core::AudioRecorder::new().map_err(|e| e.to_string())?;
        recorder.set_vad(false, 0.0);
        recorder
            .start_recording_with_device(device.as_deref())
            .map_err(|e| format!("Failed to start microphone: {e}"))?;
        *state.recorder.lock().unwrap() = Some(recorder);
        Ok(())
    })();

    if started.is_err() {
        state.testing_audio.store(false, Ordering::SeqCst);
    }
    started
}

/// Stop a running test recording and process the sample.
#[tauri::command]
pub fn stop_audio_test_recording(state: State<'_, AppState>) -> Result<AudioTestResult, String> {
    let state = state.inner();

    if !state.testing_audio.swap(false, Ordering::SeqCst) {
        return Err("No test recording is running".to_string());
    }

    let recorder = state.recorder.lock().unwrap().take();
    let mut recorder = recorder.ok_or("The microphone is not recording")?;
    let samples = recorder
        .stop_recording()
        .map(|data| data.finalize_raw())
        .map_err(|e| format!("Failed to stop microphone: {e}"))?;

    if samples.len() < MIN_TEST_SAMPLES {
        return Err("The test recording was too short".to_string());
    }

    let config = EnhanceConfig::from(state.with_settings(|s| s.ui.audio_enhance));
    process_test(state, samples, config)
}

/// Abort a running test recording and discard the audio.
#[tauri::command]
pub fn cancel_audio_test_recording(state: State<'_, AppState>) -> Result<(), String> {
    let state = state.inner();
    state.testing_audio.store(false, Ordering::SeqCst);

    let recorder = state.recorder.lock().unwrap().take();
    if let Some(mut recorder) = recorder {
        let _ = recorder.stop_recording();
    }
    Ok(())
}

/// Re-process the stored test sample with another mode, algorithm, and strength.
///
/// This is what makes an A/B comparison easy: record once, then apply every
/// configuration to exactly the same audio. It never changes the saved
/// settings.
#[tauri::command]
pub fn process_audio_test(
    state: State<'_, AppState>,
    mode: EnhanceMode,
    algorithm: DenoiseAlgorithm,
    strength: f32,
) -> Result<AudioTestResult, String> {
    let state = state.inner();

    if state.get_state() != RecordingState::Idle {
        return Err("Finish the current recording first".to_string());
    }

    let (raw_path, _) = test_paths();
    if !raw_path.exists() {
        return Err("Record a test sample first".to_string());
    }
    let samples = read_wav(&raw_path).map_err(|e| e.to_string())?;

    let config = EnhanceConfig {
        mode,
        algorithm,
        strength: strength.clamp(0.0, 1.0),
    };
    process_test(state, samples, config)
}

/// Transcribe one of the test recordings and return the text.
#[tauri::command]
pub async fn transcribe_audio_test(
    state: State<'_, AppState>,
    kind: String,
) -> Result<String, String> {
    let state_handle = state.inner();
    if state_handle.get_state() != RecordingState::Idle {
        return Err("Finish the current recording first".to_string());
    }

    let (raw_path, clean_path) = test_paths();
    let path = match kind.as_str() {
        "raw" => raw_path,
        "clean" => clean_path,
        other => return Err(format!("Unknown test audio kind: {other}")),
    };
    if !path.exists() {
        return Err("Record a test sample first".to_string());
    }

    let config = crate::recording::load_transcription_config(state_handle)?;
    let samples = read_wav(&path).map_err(|e| e.to_string())?;

    // The sample is already processed, so no further enhancement is applied.
    whis_core::transcribe_samples(
        &config.provider,
        &config.api_key,
        config.language.as_deref(),
        samples,
    )
    .await
    .map_err(|e| format!("Transcription failed: {e}"))
}

/// Quality metrics of the most recent transcription (or test recording).
#[tauri::command]
pub fn get_last_audio_quality(state: State<'_, AppState>) -> Option<AudioQuality> {
    state.last_audio_quality.lock().unwrap().clone()
}
