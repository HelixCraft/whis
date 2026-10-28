//! Progressive audio transcription using provider registry.
//!
//! All audio inputs (microphone, file, stdin) use progressive transcription:
//! - Cloud: `progressive_transcribe_cloud()` - sequential processing
//! - Local: `progressive_transcribe_local()` - sequential with shared model cache
//!
//! Supports overlap merging for seamless chunk boundaries.

use anyhow::{Context, Result};

use crate::config::TranscriptionProvider;
use crate::http::get_http_client;
use crate::provider::{TranscriptionRequest, registry};

/// Maximum words to search for overlap between chunks
const MAX_OVERLAP_WORDS: usize = 15;

/// Result of transcribing a single chunk
struct ChunkTranscription {
    index: usize,
    text: String,
    has_leading_overlap: bool,
}

/// Progress callback: `(chunks_done, total_chunks)`. Total is 0 while unknown.
pub type ChunkProgressCallback = Box<dyn Fn(usize, usize) + Send + Sync>;

/// Live preview callback, called with the merged transcript so far.
pub type PartialTextCallback = Box<dyn Fn(&str) + Send + Sync>;

/// Merge transcription results, handling overlaps.
///
/// Takes a slice so the same function can produce an in-progress preview while
/// chunks are still arriving.
fn merge_transcriptions(transcriptions: &[ChunkTranscription]) -> String {
    if transcriptions.is_empty() {
        return String::new();
    }

    if transcriptions.len() == 1 {
        return transcriptions[0].text.clone();
    }

    let mut merged = String::new();

    for (i, transcription) in transcriptions.iter().enumerate() {
        let text = transcription.text.trim();

        if i == 0 {
            // First chunk - use as-is
            merged.push_str(text);
        } else if transcription.has_leading_overlap {
            // This chunk has overlap - try to find and remove duplicate words
            let cleaned_text = remove_overlap(&merged, text);

            // Skip completely deduplicated chunks to avoid extra whitespace
            if cleaned_text.trim().is_empty() {
                crate::verbose!(
                    "Chunk {} completely deduplicated after overlap removal",
                    transcription.index
                );
                continue;
            }

            if !merged.ends_with(' ') && !cleaned_text.is_empty() && !cleaned_text.starts_with(' ')
            {
                merged.push(' ');
            }
            merged.push_str(&cleaned_text);
        } else {
            // No overlap - just append with space
            if !merged.ends_with(' ') && !text.is_empty() && !text.starts_with(' ') {
                merged.push(' ');
            }
            merged.push_str(text);
        }
    }

    merged
}

/// Remove overlapping text from the beginning of new_text that matches end of existing_text
pub(crate) fn remove_overlap(existing: &str, new_text: &str) -> String {
    let existing_words: Vec<&str> = existing.split_whitespace().collect();
    let new_words: Vec<&str> = new_text.split_whitespace().collect();

    if existing_words.is_empty() || new_words.is_empty() {
        return new_text.to_string();
    }

    // Look for overlap in the last N words of existing and first N words of new
    // ~2 seconds of audio overlap = roughly 5-15 words
    let search_end = existing_words.len().min(MAX_OVERLAP_WORDS);
    let search_new = new_words.len().min(MAX_OVERLAP_WORDS);

    // Find the longest matching overlap
    let mut best_overlap = 0;

    for overlap_len in 1..=search_end.min(search_new) {
        let end_slice = &existing_words[existing_words.len() - overlap_len..];
        let start_slice = &new_words[..overlap_len];

        // Case-insensitive comparison
        let matches = end_slice
            .iter()
            .zip(start_slice.iter())
            .all(|(a, b)| a.eq_ignore_ascii_case(b));

        if matches {
            best_overlap = overlap_len;
        }
    }

    if best_overlap > 0 {
        // Skip the overlapping words
        new_words[best_overlap..].join(" ")
    } else {
        new_text.to_string()
    }
}

//
// Progressive Transcription Functions
//

use crate::audio::chunker::AudioChunk as ProgressiveChunk;

/// Progressive transcription for cloud providers
///
/// Transcribes audio chunks DURING recording (true progressive). As each 90-second
/// chunk is produced, it's immediately sent to the API for transcription sequentially.
/// Results are collected and merged when recording ends.
///
/// # Arguments
/// * `provider` - The transcription provider to use
/// * `api_key` - API key for the provider
/// * `language` - Optional language hint
/// * `chunk_rx` - Channel receiving audio chunks during recording
/// * `progress_callback` - Optional progress reporting
/// * `partial_callback` - Optional live preview, called with the merged text so far
pub async fn progressive_transcribe_cloud(
    provider: &TranscriptionProvider,
    api_key: &str,
    language: Option<&str>,
    mut chunk_rx: tokio::sync::mpsc::UnboundedReceiver<ProgressiveChunk>,
    progress_callback: Option<ChunkProgressCallback>,
    partial_callback: Option<PartialTextCallback>,
) -> Result<String> {
    let client = get_http_client()?;
    let provider_impl = registry().get_by_kind(provider)?;
    let mut transcriptions = Vec::new();
    let mut chunk_count = 0;

    // Process chunks sequentially as they arrive (true progressive)
    while let Some(chunk) = chunk_rx.recv().await {
        chunk_count += 1;
        let chunk_index = chunk.index;
        let has_leading_overlap = chunk.has_leading_overlap;

        // Convert samples to MP3
        let mp3_data =
            samples_to_mp3(&chunk.samples).context("Failed to encode audio chunk to MP3")?;

        let request = TranscriptionRequest {
            audio_data: mp3_data,
            language: language.map(|s| s.to_string()),
            filename: format!("audio_chunk_{chunk_index}.mp3"),
            mime_type: "audio/mpeg".to_string(),
            progress: None,
        };

        let result = provider_impl
            .transcribe_async(client, api_key, request)
            .await
            .with_context(|| format!("Failed to transcribe chunk {chunk_index}"))?;

        transcriptions.push(ChunkTranscription {
            index: chunk_index,
            text: result.text,
            has_leading_overlap,
        });

        // Progress reporting (total unknown until channel closes)
        if let Some(ref callback) = progress_callback {
            callback(chunk_count, 0); // Total is 0 since we don't know how many more chunks will arrive
        }
        if let Some(ref callback) = partial_callback {
            callback(&merge_transcriptions(&transcriptions));
        }
    }

    // Results are already in correct order (sequential processing, no sorting needed)
    Ok(merge_transcriptions(&transcriptions))
}

/// Progressive transcription for local models.
///
/// Transcribes audio chunks DURING recording (true progressive). As each 90-second
/// chunk is produced, it's immediately transcribed using the shared cached model
/// (sequential processing). The model is loaded once and reused to minimize memory
/// usage (constant 2GB, compared to 6GB with the previous parallel worker architecture).
///
/// The engine is inferred from the model path.
///
/// # Arguments
/// * `model_path` - Path to the local model file or directory
/// * `language` - Optional language hint (used by Whisper and Qwen3-ASR)
/// * `chunk_rx` - Channel receiving audio chunks during recording
/// * `progress_callback` - Optional progress reporting
/// * `partial_callback` - Optional live preview, called with the merged text so far
#[cfg(feature = "local-transcription")]
pub async fn progressive_transcribe_local(
    model_path: &str,
    language: Option<&str>,
    mut chunk_rx: tokio::sync::mpsc::UnboundedReceiver<ProgressiveChunk>,
    progress_callback: Option<ChunkProgressCallback>,
    partial_callback: Option<PartialTextCallback>,
) -> Result<String> {
    let language = language.map(str::to_string);
    let mut transcriptions = Vec::new();
    let mut chunk_count = 0;

    // Process chunks sequentially as they arrive (true progressive)
    while let Some(chunk) = chunk_rx.recv().await {
        chunk_count += 1;
        let chunk_index = chunk.index;
        let has_leading_overlap = chunk.has_leading_overlap;
        let samples = chunk.samples;
        let model_path_owned = model_path.to_string();

        // Run transcription in blocking task (CPU-bound work)
        let language = language.clone();
        let result = tokio::task::spawn_blocking(move || {
            crate::provider::transcribe_local(&model_path_owned, samples, language.as_deref())
        })
        .await
        .context("Transcription task panicked")?
        .context("Transcription failed")?;

        transcriptions.push(ChunkTranscription {
            index: chunk_index,
            text: result.text,
            has_leading_overlap,
        });

        // Progress reporting (total unknown until channel closes)
        if let Some(ref callback) = progress_callback {
            callback(chunk_count, 0); // Total is 0 since we don't know how many more chunks will arrive
        }
        if let Some(ref callback) = partial_callback {
            callback(&merge_transcriptions(&transcriptions));
        }
    }

    // Results are already in correct order (sequential processing, no sorting needed)
    Ok(merge_transcriptions(&transcriptions))
}

/// Convert f32 samples to MP3 bytes
pub(crate) fn samples_to_mp3(samples: &[f32]) -> Result<Vec<u8>> {
    use crate::audio::create_encoder;
    let encoder = create_encoder();
    encoder
        .encode_samples(samples, crate::resample::WHISPER_SAMPLE_RATE)
        .context("Failed to encode audio to MP3")
}
