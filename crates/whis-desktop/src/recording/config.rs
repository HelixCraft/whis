//! Recording Configuration
//!
//! Handles loading and validation of transcription configuration from settings.

use crate::state::{AppState, TranscriptionConfig};

/// Load transcription configuration from settings
/// Returns error if required API key or model path is missing
pub fn load_transcription_config(state: &AppState) -> Result<TranscriptionConfig, String> {
    let settings = state.settings.lock().unwrap();
    let provider = settings.transcription.provider.clone();

    // Get API key/model path based on provider type
    let api_key = if provider.is_local() {
        settings
            .transcription
            .local_model_path()
            .ok_or_else(|| "No local model configured. Add one in Settings.".to_string())?
    } else {
        settings
            .transcription
            .api_key_from_settings()
            .ok_or_else(|| format!("No {} API key configured. Add it in Settings.", provider))?
    };

    let language = settings.transcription.language.clone();

    Ok(TranscriptionConfig {
        provider,
        api_key,
        language,
    })
}
