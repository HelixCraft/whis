//! Transcription of an audio buffer.
//!
//! Used for re-transcribing the stored recording: the WAV is loaded once and
//! sent through the currently configured provider, using the same chunking and
//! overlap merging as live transcription.

use anyhow::{Context, Result};

use crate::config::TranscriptionProvider;
use crate::http::get_http_client;
use crate::provider::{TranscriptionRequest, registry};

/// Chunk length for cloud uploads.
const CHUNK_SECS: usize = 90;
/// Audio overlap between consecutive chunks, used to avoid cutting words.
const OVERLAP_SECS: usize = 1;
/// Sample rate of all stored recordings.
const SAMPLE_RATE: usize = 16_000;

/// Transcribe 16 kHz mono samples with the given provider.
///
/// `api_key_or_model_path` is the API key for cloud providers and the model
/// path for local providers, matching [`crate::state::TranscriptionConfig`].
pub async fn transcribe_samples(
    provider: &TranscriptionProvider,
    api_key_or_model_path: &str,
    language: Option<&str>,
    samples: Vec<f32>,
) -> Result<String> {
    if samples.is_empty() {
        anyhow::bail!("No audio to transcribe");
    }

    if provider.is_local() {
        #[cfg(feature = "local-transcription")]
        {
            let model_path = api_key_or_model_path.to_string();
            let language = language.map(str::to_string);
            return tokio::task::spawn_blocking(move || {
                crate::provider::transcribe_local(&model_path, samples, language.as_deref())
            })
            .await
            .context("Transcription task panicked")?
            .map(|result| result.text);
        }
        #[cfg(not(feature = "local-transcription"))]
        {
            anyhow::bail!("Local transcription is not enabled in this build");
        }
    }

    transcribe_cloud_samples(provider, api_key_or_model_path, language, samples).await
}

async fn transcribe_cloud_samples(
    provider: &TranscriptionProvider,
    api_key: &str,
    language: Option<&str>,
    samples: Vec<f32>,
) -> Result<String> {
    // Realtime providers have no file API. Fall back to their batch sibling.
    let effective = match provider {
        TranscriptionProvider::OpenAIRealtime => TranscriptionProvider::OpenAI,
        TranscriptionProvider::DeepgramRealtime => TranscriptionProvider::Deepgram,
        other => other.clone(),
    };

    let provider_impl = registry().get_by_kind(&effective)?;
    let client = get_http_client()?;

    let chunk_len = CHUNK_SECS * SAMPLE_RATE;
    let step = chunk_len - OVERLAP_SECS * SAMPLE_RATE;

    let mut texts = Vec::new();
    let mut start = 0;

    while start < samples.len() {
        let end = (start + chunk_len).min(samples.len());
        let mp3 = super::transcribe::samples_to_mp3(&samples[start..end])?;
        let index = texts.len();

        let request = TranscriptionRequest {
            audio_data: mp3,
            language: language.map(str::to_string),
            filename: format!("audio_chunk_{index}.mp3"),
            mime_type: "audio/mpeg".to_string(),
            progress: None,
        };

        let result = provider_impl
            .transcribe_async(client, api_key, request)
            .await
            .with_context(|| format!("Failed to transcribe chunk {index}"))?;
        texts.push(result.text);

        if end == samples.len() {
            break;
        }
        start += step;
    }

    Ok(merge_texts(texts))
}

/// Merge chunk transcripts, removing words repeated because of the overlap.
fn merge_texts(texts: Vec<String>) -> String {
    let mut merged = String::new();

    for text in texts {
        let text = text.trim();
        if text.is_empty() {
            continue;
        }
        if merged.is_empty() {
            merged.push_str(text);
            continue;
        }

        let cleaned = super::transcribe::remove_overlap(&merged, text);
        if cleaned.trim().is_empty() {
            continue;
        }
        if !merged.ends_with(' ') {
            merged.push(' ');
        }
        merged.push_str(cleaned.trim());
    }

    merged
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merge_removes_overlap() {
        let merged = merge_texts(vec![
            "hello world this is a test".to_string(),
            "this is a test of the system".to_string(),
        ]);
        assert_eq!(merged, "hello world this is a test of the system");
    }

    #[test]
    fn merge_keeps_disjoint_text() {
        let merged = merge_texts(vec!["hello".to_string(), "world".to_string()]);
        assert_eq!(merged, "hello world");
    }
}
