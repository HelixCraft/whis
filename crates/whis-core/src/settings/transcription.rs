//! Transcription settings for provider configuration.

use serde::{Deserialize, Deserializer, Serialize};
use std::collections::HashMap;

use crate::config::TranscriptionProvider;

/// Settings for transcription providers and models.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranscriptionSettings {
    /// Active transcription provider
    #[serde(default)]
    pub provider: TranscriptionProvider,

    /// Language hint for transcription (ISO-639-1 code, e.g., "en", "de", "fr")
    /// None = auto-detect, Some("en") = English, etc.
    #[serde(default)]
    pub language: Option<String>,

    /// API keys stored by provider name (e.g., "openai" -> "sk-...")
    #[serde(default)]
    pub api_keys: HashMap<String, String>,

    /// Local model configuration
    #[serde(default)]
    pub local_models: LocalModelsConfig,
}

impl Default for TranscriptionSettings {
    fn default() -> Self {
        Self {
            provider: crate::configuration::DEFAULT_PROVIDER,
            language: crate::configuration::DEFAULT_LANGUAGE.map(String::from),
            api_keys: HashMap::new(),
            local_models: LocalModelsConfig::default(),
        }
    }
}

/// Configuration for local transcription models.
#[derive(Debug, Clone, Serialize)]
pub struct LocalModelsConfig {
    /// Catalog id of the selected model.
    #[serde(default)]
    pub selected: Option<String>,

    /// Optional user-supplied path that overrides the catalog selection.
    #[serde(default)]
    pub custom_path: Option<String>,
}

impl Default for LocalModelsConfig {
    fn default() -> Self {
        Self {
            selected: Some(crate::model::DEFAULT_MODEL_ID.to_string()),
            custom_path: None,
        }
    }
}

/// Legacy shape with per-engine paths, accepted during deserialization.
#[derive(Deserialize)]
struct LocalModelsConfigRaw {
    #[serde(default)]
    selected: Option<String>,
    #[serde(default)]
    custom_path: Option<String>,
    #[serde(default)]
    whisper_path: Option<String>,
    #[serde(default)]
    parakeet_path: Option<String>,
}

impl<'de> Deserialize<'de> for LocalModelsConfig {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = LocalModelsConfigRaw::deserialize(deserializer)?;

        if raw.selected.is_some() || raw.custom_path.is_some() {
            return Ok(Self {
                selected: raw.selected,
                custom_path: raw.custom_path,
            });
        }

        // Older settings stored a path per engine. Map known catalog paths to
        // their id and keep anything else as a custom path so user-provided
        // models keep working.
        let legacy_path = raw.whisper_path.or(raw.parakeet_path);
        Ok(match legacy_path {
            Some(path) => match crate::model::id_for_path(&path) {
                Some(id) => Self {
                    selected: Some(id.to_string()),
                    custom_path: None,
                },
                None => Self {
                    selected: None,
                    custom_path: Some(path),
                },
            },
            None => Self {
                selected: None,
                custom_path: None,
            },
        })
    }
}

impl TranscriptionSettings {
    /// Get the API key for the current provider, falling back to environment variables.
    pub fn api_key(&self) -> Option<String> {
        self.api_key_for(&self.provider)
    }

    /// Get the API key for a specific provider.
    ///
    /// Checks in order:
    /// 1. api_keys map
    /// 2. Environment variable
    pub fn api_key_for(&self, provider: &TranscriptionProvider) -> Option<String> {
        // Check api_keys map first (no env var fallback)
        if let Some(key) = self.api_key_from_settings_for(provider) {
            return Some(key);
        }

        // Fall back to environment variable
        std::env::var(provider.api_key_env_var()).ok()
    }

    /// Get the API key for the current provider from settings only (no env var fallback).
    ///
    /// Used by desktop app which doesn't support env var configuration.
    pub fn api_key_from_settings(&self) -> Option<String> {
        self.api_key_from_settings_for(&self.provider)
    }

    /// Get the API key for a specific provider from settings only (no env var fallback).
    pub fn api_key_from_settings_for(&self, provider: &TranscriptionProvider) -> Option<String> {
        self.api_keys
            .get(provider.api_key_name())
            .filter(|k| !k.is_empty())
            .cloned()
    }

    /// Check if an API key is explicitly configured in settings (not just in environment).
    ///
    /// Returns true only if the key exists in the api_keys HashMap.
    /// Use this to distinguish between:
    /// - [configured]: Key in settings.json
    /// - [available]: Key only in environment variable
    pub fn has_configured_api_key(&self, provider: &TranscriptionProvider) -> bool {
        self.api_keys
            .get(provider.api_key_name())
            .map(|k| !k.is_empty())
            .unwrap_or(false)
    }

    /// Set the API key for a provider.
    pub fn set_api_key(&mut self, provider: &TranscriptionProvider, key: String) {
        self.api_keys
            .insert(provider.api_key_name().to_string(), key);
    }

    /// Check if an API key is configured for the current provider.
    pub fn has_api_key(&self) -> bool {
        self.api_key().is_some()
    }

    /// The selected local catalog model, if any.
    pub fn selected_local_model(&self) -> Option<&'static crate::model::LocalModelSpec> {
        self.local_models
            .selected
            .as_deref()
            .and_then(crate::model::find)
    }

    /// Check if the current provider is properly configured.
    ///
    /// For cloud providers: checks for API key
    /// For local: checks that the selected model exists and is complete
    pub fn is_configured(&self) -> bool {
        if self.provider.is_local() {
            return self
                .local_model_path()
                .is_some_and(|path| match self.selected_local_model() {
                    Some(model) => model.verify(std::path::Path::new(&path)),
                    None => std::path::Path::new(&path).exists(),
                });
        }

        self.has_api_key()
    }

    /// Path of the configured local model.
    ///
    /// A custom path wins over the catalog selection. Falls back to the
    /// `LOCAL_MODEL_PATH` environment variable for CLI users.
    pub fn local_model_path(&self) -> Option<String> {
        if let Some(path) = self
            .local_models
            .custom_path
            .as_ref()
            .filter(|path| !path.is_empty())
        {
            return Some(path.clone());
        }

        if let Some(model) = self.selected_local_model() {
            return Some(model.path().to_string_lossy().to_string());
        }

        std::env::var("LOCAL_MODEL_PATH").ok()
    }

    /// Validate transcription settings.
    pub fn validate(&self) -> anyhow::Result<()> {
        if !self.is_configured() {
            anyhow::bail!(
                "Provider '{}' is not configured. Please run setup.",
                self.provider
            );
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_whisper_path_migrates_to_id() {
        let config: LocalModelsConfig = serde_json::from_str(
            r#"{"whisper_path":"/home/user/.local/share/whis/models/ggml-small.bin"}"#,
        )
        .unwrap();
        let expected = crate::model::find("whisper-small").unwrap().path();
        assert_eq!(config.selected.as_deref(), Some("whisper-small"));
        assert!(config.custom_path.is_none());
        assert!(expected.ends_with("ggml-small.bin"));
    }

    #[test]
    fn unknown_legacy_path_becomes_custom_path() {
        let config: LocalModelsConfig =
            serde_json::from_str(r#"{"parakeet_path":"/opt/models/custom"}"#).unwrap();
        assert!(config.selected.is_none());
        assert_eq!(config.custom_path.as_deref(), Some("/opt/models/custom"));
    }

    #[test]
    fn legacy_provider_names_map_to_local() {
        let provider: TranscriptionProvider = serde_json::from_str("\"local-parakeet\"").unwrap();
        assert_eq!(provider, TranscriptionProvider::Local);
        let provider: TranscriptionProvider = serde_json::from_str("\"local-whisper\"").unwrap();
        assert_eq!(provider, TranscriptionProvider::Local);
    }
}
