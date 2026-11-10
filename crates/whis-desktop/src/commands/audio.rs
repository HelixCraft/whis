//! Audio test recording for the settings page.
//!
//! Records a short sample with the configured microphone and returns both the
//! raw audio and the cleaned version, level-matched so the comparison is fair.
//! Nothing here touches the stored last recording.

use crate::state::{AppState, RecordingState};
use std::sync::atomic::Ordering;
use std::time::Duration;
use tauri::State;
use whis_core::audio::DenoiseAlgorithm;
use whis_core::audio::enhance::{self, AudioQuality, EnhanceConfig, EnhanceMode};
use whis_core::audio::last_recording::{read_wav, write_wav};

/// Length of the test sample.
const TEST_DURATION: Duration = Duration::from_secs(6);

/// Result of a test recording.
#[derive(serde::Serialize)]
pub struct AudioTestResult {
    pub raw_path: String,
    pub clean_path: String,
    pub duration_secs: f32,
    pub quality: AudioQuality,
    pub mode: String,
    pub algorithm: String,
}

fn test_paths() -> (std::path::PathBuf, std::path::PathBuf) {
    let dir = whis_core::history::data_dir();
    (
        dir.join("audio_test_raw.wav"),
        dir.join("audio_test_clean.wav"),
    )
}

/// Apply a cleanup configuration to a test sample and write the clean WAV.
///
/// The raw preview gets the same gain that the chain applies, so the two files
/// only differ by the filtering itself.
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

    let mut quality = AudioQuality::default();
    quality.add(&enhanced.stats);
    *state.last_audio_quality.lock().unwrap() = Some(quality.clone());

    Ok(AudioTestResult {
        raw_path: raw_path.to_string_lossy().to_string(),
        clean_path: clean_path.to_string_lossy().to_string(),
        duration_secs: enhanced.stats.duration_secs,
        quality,
        mode: config.mode.as_str().to_string(),
        algorithm: config.algorithm.as_str().to_string(),
    })
}

/// The preview always shows something, so an off chain is previewed as Light.
fn preview_config(state: &AppState) -> EnhanceConfig {
    let mut config = EnhanceConfig::from(state.with_settings(|s| s.ui.audio_enhance));
    if config.mode == EnhanceMode::Off {
        config.mode = EnhanceMode::Light;
    }
    config
}

/// Record a short sample and write a raw and a cleaned, level-matched WAV.
///
/// The current cleanup settings are used. When the mode is off, the light chain
/// is used for the preview so the user can hear what Light would change.
#[tauri::command]
pub async fn record_audio_test(state: State<'_, AppState>) -> Result<AudioTestResult, String> {
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
    // Clears the flag on every exit path.
    let _guard = TestGuard { state };

    let device = state.with_settings(|s| s.ui.microphone_device.clone());

    let samples = tauri::async_runtime::spawn_blocking(move || {
        let mut recorder = whis_core::AudioRecorder::new().map_err(|e| e.to_string())?;
        recorder.set_vad(false, 0.0);
        recorder
            .start_recording_with_device(device.as_deref())
            .map_err(|e| format!("Failed to start microphone: {e}"))?;
        std::thread::sleep(TEST_DURATION);
        recorder
            .stop_recording()
            .map(|data| data.finalize_raw())
            .map_err(|e| format!("Failed to stop microphone: {e}"))
    })
    .await
    .map_err(|e| format!("Test recording task panicked: {e}"))??;

    if samples.len() < whis_core::resample::WHISPER_SAMPLE_RATE as usize / 2 {
        return Err("Test recording was too short".to_string());
    }

    let config = preview_config(state);
    process_test(state, samples, config)
}

/// Re-process the stored test sample with another mode and algorithm.
///
/// This is what makes an A/B comparison easy: record once, then listen to every
/// denoiser on exactly the same audio.
#[tauri::command]
pub fn process_audio_test(
    state: State<'_, AppState>,
    mode: EnhanceMode,
    algorithm: DenoiseAlgorithm,
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

    let config = EnhanceConfig { mode, algorithm };
    process_test(state, samples, config)
}

/// Play one of the test recordings. `kind` is `raw` or `clean`.
#[tauri::command]
pub fn play_audio_test(kind: String) -> Result<(), String> {
    let (raw_path, clean_path) = test_paths();
    let path = match kind.as_str() {
        "raw" => raw_path,
        "clean" => clean_path,
        other => return Err(format!("Unknown test audio kind: {other}")),
    };
    if !path.exists() {
        return Err("Record a test sample first".to_string());
    }
    crate::commands::open_external_path(&path)
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

struct TestGuard<'a> {
    state: &'a AppState,
}

impl Drop for TestGuard<'_> {
    fn drop(&mut self) {
        self.state.testing_audio.store(false, Ordering::SeqCst);
    }
}
