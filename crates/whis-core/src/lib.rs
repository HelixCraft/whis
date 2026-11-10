// Domain modules (organized by concern)
pub mod audio;
pub mod configuration;
pub mod provider;
pub mod settings;
pub mod transcription;

// Model management
pub mod model;

// Utility modules (cross-cutting concerns)
#[cfg(feature = "autotyping")]
pub mod autotyping;
#[cfg(feature = "clipboard")]
pub mod clipboard;
pub mod error;
#[cfg(feature = "hotkey")]
pub mod hotkey;
pub mod http;
pub mod platform;
pub mod resample;
pub mod state;
pub mod verbose;

// Re-export audio types
#[cfg(feature = "history")]
pub use audio::last_recording::{LastAudioInfo, LastAudioMeta};
pub use audio::{
    AudioDeviceInfo, AudioRecorder, ChunkerConfig, ProgressiveChunk, ProgressiveChunker,
    RecordingData, VadConfig, list_audio_devices,
};

// Persistent transcription history
#[cfg(feature = "history")]
pub mod history;

// Re-export configuration types
pub use configuration::{
    DEFAULT_LANGUAGE, DEFAULT_OLLAMA_MODEL, DEFAULT_OLLAMA_URL, DEFAULT_POST_PROCESSOR,
    DEFAULT_PROVIDER, DEFAULT_SHORTCUT, DEFAULT_SHORTCUT_MODE, DEFAULT_VAD_ENABLED,
    DEFAULT_VAD_THRESHOLD,
};
pub use configuration::{Preset, PresetSource, TranscriptionProvider};

// Re-export transcription types
#[cfg(feature = "local-transcription")]
pub use transcription::progressive_transcribe_local;
pub use transcription::{
    DEFAULT_POST_PROCESSING_PROMPT, PostProcessConfig, PostProcessor, WarmupConfig,
    clear_warmup_cache, post_process, preload_ollama, progressive_transcribe_cloud,
    resolve_post_processor_config, transcribe_samples, transcribe_samples_enhanced,
    warmup_configured,
};

// Re-export provider types
#[cfg(feature = "realtime")]
pub use provider::DeepgramRealtimeProvider;
#[cfg(feature = "realtime")]
pub use provider::OpenAIRealtimeProvider;
pub use provider::is_realtime_provider;
#[cfg(feature = "local-transcription")]
pub use provider::preload_parakeet;
#[cfg(feature = "local-transcription")]
pub use provider::transcribe_local;
pub use provider::{
    DEFAULT_TIMEOUT_SECS, ProgressCallback, TranscriptionBackend, TranscriptionRequest,
    TranscriptionResult, TranscriptionStage, registry,
};
#[cfg(feature = "realtime")]
pub use provider::{RealtimeTranscriptionBackend, get_realtime_backend};
#[cfg(feature = "local-transcription")]
pub use provider::{local_cpp_preload_model, local_cpp_set_keep_loaded, local_cpp_unload_model};
#[cfg(feature = "local-transcription")]
pub use provider::{parakeet_set_keep_loaded, unload_parakeet};

// Re-export other utility types
#[cfg(feature = "autotyping")]
pub use autotyping::{
    AutotypeBackend, AutotypeToolStatus, OutputMethod, autotype_text, get_autotype_tool_status,
};
#[cfg(feature = "clipboard")]
pub use clipboard::{ClipboardMethod, copy_to_clipboard};
pub use error::{AudioError, ProviderError, Result, WhisError};
pub use http::{get_http_client, is_http_client_ready, warmup_http_client};
pub use settings::Settings;
pub use state::RecordingState;
pub use verbose::set_verbose;

#[cfg(feature = "hotkey")]
pub use hotkey::{Hotkey, HotkeyParseError, key_to_string, lock_or_recover, parse_key};
pub use platform::{Compositor, Platform, PlatformInfo, detect_platform, is_flatpak};

// Legacy module aliases for backward compatibility
#[doc(hidden)]
pub mod config {
    pub use crate::configuration::TranscriptionProvider;
}

#[doc(hidden)]
pub mod defaults {
    pub use crate::configuration::{
        DEFAULT_LANGUAGE, DEFAULT_OLLAMA_MODEL, DEFAULT_OLLAMA_URL, DEFAULT_POST_PROCESSOR,
        DEFAULT_PROVIDER, DEFAULT_SHORTCUT, DEFAULT_SHORTCUT_MODE, DEFAULT_VAD_ENABLED,
        DEFAULT_VAD_THRESHOLD,
    };
}

#[doc(hidden)]
pub mod preset {
    pub use crate::configuration::{Preset, PresetSource};
}

#[doc(hidden)]
pub mod post_processing {
    pub use crate::transcription::{
        DEFAULT_POST_PROCESSING_PROMPT, PostProcessConfig, PostProcessor, post_process,
        resolve_post_processor_config,
    };
}

#[doc(hidden)]
pub mod transcribe {
    pub use crate::transcription::progressive_transcribe_cloud;
    #[cfg(feature = "local-transcription")]
    pub use crate::transcription::progressive_transcribe_local;
    pub use crate::transcription::transcribe_samples;
}

#[doc(hidden)]
pub mod ollama {
    pub use crate::transcription::{
        DEFAULT_OLLAMA_MODEL, DEFAULT_OLLAMA_URL, OLLAMA_MODEL_OPTIONS, OllamaModel,
        ensure_ollama_ready, ensure_ollama_running, has_model, is_ollama_installed,
        is_ollama_running, list_models, pull_model, pull_model_with_progress,
    };
}

#[doc(hidden)]
pub mod ollama_manager {
    pub use crate::transcription::{clear_warmup_cache, preload_ollama};
}

#[doc(hidden)]
pub mod warmup {
    pub use crate::transcription::{WarmupConfig, warmup_configured};
}
