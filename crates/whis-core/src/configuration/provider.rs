//! Transcription provider configuration.
//!
//! Defines the available speech-to-text providers that whis can use.
//! Each provider has different characteristics:
//!
//! # Cloud Providers (require API key)
//!
//! - **OpenAI** - Whisper API, reliable and accurate
//! - **OpenAI Realtime** - WebSocket streaming for live transcription
//! - **Deepgram** - Fast, good for long recordings
//! - **Deepgram Realtime** - WebSocket streaming
//! - **Mistral** - EU-based alternative
//! - **Groq** - Fast inference
//! - **ElevenLabs** - High quality
//!
//! # Local Provider (no API key, requires model download)
//!
//! - **Local** - Any model from the local catalog (Whisper, Parakeet, GGUF)
//!
//! # Helper Methods
//!
//! - `as_str()` - Get provider identifier string
//! - `display_name()` - Human-readable name
//! - `api_key_env_var()` - Environment variable for API key
//! - `requires_api_key()` - Whether cloud API key is needed
//! - `is_local()` - Whether provider runs locally

use serde::{Deserialize, Serialize};
use std::fmt;

/// Available transcription providers
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum TranscriptionProvider {
    OpenAI,
    #[serde(rename = "openai-realtime")]
    OpenAIRealtime,
    Mistral,
    Groq,
    Deepgram,
    #[serde(rename = "deepgram-realtime")]
    DeepgramRealtime,
    ElevenLabs,
    /// Local transcription with a model from the catalog.
    ///
    /// Legacy names (`local-whisper`, `local-parakeet`) deserialize to this
    /// variant so old settings keep working.
    #[serde(alias = "local-whisper", alias = "local-parakeet")]
    Local,
}

impl Default for TranscriptionProvider {
    fn default() -> Self {
        super::DEFAULT_PROVIDER
    }
}

impl TranscriptionProvider {
    /// Get the string identifier for this provider
    pub fn as_str(&self) -> &'static str {
        match self {
            TranscriptionProvider::OpenAI => "openai",
            TranscriptionProvider::OpenAIRealtime => "openai-realtime",
            TranscriptionProvider::Mistral => "mistral",
            TranscriptionProvider::Groq => "groq",
            TranscriptionProvider::Deepgram => "deepgram",
            TranscriptionProvider::DeepgramRealtime => "deepgram-realtime",
            TranscriptionProvider::ElevenLabs => "elevenlabs",
            TranscriptionProvider::Local => "local",
        }
    }

    /// Get the environment variable name for this provider's API key (or path for local)
    pub fn api_key_env_var(&self) -> &'static str {
        match self {
            TranscriptionProvider::OpenAI | TranscriptionProvider::OpenAIRealtime => {
                "OPENAI_API_KEY"
            }
            TranscriptionProvider::Mistral => "MISTRAL_API_KEY",
            TranscriptionProvider::Groq => "GROQ_API_KEY",
            TranscriptionProvider::Deepgram | TranscriptionProvider::DeepgramRealtime => {
                "DEEPGRAM_API_KEY"
            }
            TranscriptionProvider::ElevenLabs => "ELEVENLABS_API_KEY",
            TranscriptionProvider::Local => "LOCAL_MODEL_PATH",
        }
    }

    /// List all available providers in recommended order.
    ///
    /// Ordering: Deepgram first (generous free tier, real-time optimized),
    /// then OpenAI (widely used), followed by other cloud options.
    pub fn all() -> &'static [TranscriptionProvider] {
        &[
            TranscriptionProvider::Deepgram,
            TranscriptionProvider::DeepgramRealtime,
            TranscriptionProvider::OpenAI,
            TranscriptionProvider::OpenAIRealtime,
            TranscriptionProvider::Mistral,
            TranscriptionProvider::Groq,
            TranscriptionProvider::ElevenLabs,
            TranscriptionProvider::Local,
        ]
    }

    /// List cloud providers for UI dropdowns (ordered by recommendation).
    ///
    /// Excludes local providers and realtime variants (realtime is typically
    /// toggled separately in the UI rather than shown as a separate provider).
    pub fn cloud_providers() -> impl Iterator<Item = &'static TranscriptionProvider> {
        Self::all()
            .iter()
            .filter(|p| !p.is_local() && !p.as_str().contains("realtime"))
    }

    /// Human-readable display name for this provider
    pub fn display_name(&self) -> &'static str {
        match self {
            TranscriptionProvider::OpenAI => "OpenAI",
            TranscriptionProvider::OpenAIRealtime => "OpenAI Realtime",
            TranscriptionProvider::Mistral => "Mistral",
            TranscriptionProvider::Groq => "Groq",
            TranscriptionProvider::Deepgram => "Deepgram",
            TranscriptionProvider::DeepgramRealtime => "Deepgram Realtime",
            TranscriptionProvider::ElevenLabs => "ElevenLabs",
            TranscriptionProvider::Local => "Local",
        }
    }

    /// Whether this provider requires an API key (vs a model path for local)
    pub fn requires_api_key(&self) -> bool {
        !self.is_local()
    }

    /// Whether this is a local provider (no cloud API)
    pub fn is_local(&self) -> bool {
        matches!(self, TranscriptionProvider::Local)
    }

    /// Get the API key name for this provider.
    ///
    /// Realtime variants share API keys with their base providers:
    /// - `OpenAIRealtime` → "openai"
    /// - `DeepgramRealtime` → "deepgram"
    pub fn api_key_name(&self) -> &'static str {
        match self {
            Self::OpenAIRealtime => "openai",
            Self::DeepgramRealtime => "deepgram",
            _ => self.as_str(),
        }
    }

    /// Configure model memory behavior for local models.
    ///
    /// When `keep` is true, models stay loaded in memory between
    /// transcriptions for faster subsequent runs. When false, they are
    /// unloaded after each transcription to free memory.
    ///
    /// No-op for cloud providers (they don't have local model memory).
    #[cfg(feature = "local-transcription")]
    pub fn set_keep_loaded(&self, keep: bool) {
        if self.is_local() {
            // Only one engine is used at a time, so configure all caches.
            crate::provider::local_cpp_set_keep_loaded(keep);
            crate::provider::parakeet_set_keep_loaded(keep);
        }
    }
}

impl fmt::Display for TranscriptionProvider {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl std::str::FromStr for TranscriptionProvider {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "openai" => Ok(TranscriptionProvider::OpenAI),
            "openai-realtime" | "openairealttime" | "realtime" => {
                Ok(TranscriptionProvider::OpenAIRealtime)
            }
            "mistral" => Ok(TranscriptionProvider::Mistral),
            "groq" => Ok(TranscriptionProvider::Groq),
            "deepgram" => Ok(TranscriptionProvider::Deepgram),
            "deepgram-realtime" | "deepgramrealtime" => Ok(TranscriptionProvider::DeepgramRealtime),
            "elevenlabs" => Ok(TranscriptionProvider::ElevenLabs),
            "local" | "local-whisper" | "localwhisper" | "whisper" | "local-parakeet"
            | "localparakeet" | "parakeet" => Ok(TranscriptionProvider::Local),
            _ => Err(format!(
                "Unknown provider: {}. Available: openai, openai-realtime, mistral, groq, deepgram, deepgram-realtime, elevenlabs, local",
                s
            )),
        }
    }
}
