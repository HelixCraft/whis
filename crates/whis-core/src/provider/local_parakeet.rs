//! Local transcription using NVIDIA Parakeet via ONNX
//!
//! This provider enables offline transcription using Parakeet models.
//! Requires a Parakeet model directory containing ONNX files.
//!
//! Parakeet models offer high accuracy and speed for speech-to-text.

use anyhow::Result;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};

use super::TranscriptionResult;

/// Transcribe raw f32 samples directly.
///
/// Use this for local recordings where samples are already 16kHz mono.
///
/// # Arguments
/// * `model_path` - Path to the Parakeet model directory
/// * `samples` - Raw f32 audio samples (must be 16kHz mono)
pub fn transcribe_raw(model_path: &str, samples: Vec<f32>) -> Result<TranscriptionResult> {
    transcribe_samples(model_path, samples)
}

/// Internal function to transcribe PCM samples using Parakeet
///
/// ONNX Runtime has memory constraints with long audio in Parakeet models.
/// This function automatically chunks audio longer than 90 seconds to avoid ORT errors.
fn transcribe_samples(model_path: &str, samples: Vec<f32>) -> Result<TranscriptionResult> {
    // Empirically tested: Parakeet works well up to ~90 seconds
    // Beyond that, ONNX Runtime can hit memory limits (ORT error)
    const CHUNK_SIZE: usize = 1_440_000; // 90 seconds at 16kHz
    const OVERLAP: usize = 16_000; // 1 second overlap for context at chunk boundaries

    // Load engine if not already cached
    get_or_load_engine(model_path)?;

    // Get the cache and lock the engine
    let mut cache = get_cache().lock().unwrap();
    let cached = cache.as_mut().ok_or_else(|| {
        anyhow::anyhow!("Parakeet engine not loaded (cache empty after get_or_load_engine)")
    })?;

    // If audio is short, transcribe directly (no chunking needed)
    let result = if samples.len() <= CHUNK_SIZE {
        transcribe_chunk_with_engine(&mut cached.engine, &samples)?
    } else {
        // Split long audio into chunks with overlap
        let mut chunks = Vec::new();
        let mut start = 0;
        while start < samples.len() {
            let end = (start + CHUNK_SIZE).min(samples.len());
            chunks.push(&samples[start..end]);
            start += CHUNK_SIZE - OVERLAP;
        }

        // Transcribe each chunk using the same engine instance
        let mut results = Vec::new();
        for (i, chunk) in chunks.iter().enumerate() {
            crate::verbose!(
                "Transcribing chunk {}/{} ({:.1}s)...",
                i + 1,
                chunks.len(),
                chunk.len() as f32 / 16000.0
            );

            let chunk_result = transcribe_chunk_with_engine(&mut cached.engine, chunk)?;
            results.push(chunk_result.text);
        }

        // Concatenate chunk results with space separator
        TranscriptionResult {
            text: results.join(" "),
        }
    };

    // Release the lock before maybe_unload
    drop(cache);

    // Conditionally unload based on KEEP_LOADED flag
    maybe_unload();

    Ok(result)
}

/// Transcribe a single chunk of audio using an already-loaded engine
///
/// This function is used internally by `transcribe_samples()` to reuse the same
/// engine instance across multiple chunks, avoiding repeated model loading.
fn transcribe_chunk_with_engine(
    model: &mut transcribe_rs::onnx::parakeet::ParakeetModel,
    samples: &[f32],
) -> Result<TranscriptionResult> {
    use transcribe_rs::{SpeechModel, TranscribeOptions};

    // Transcribe the audio samples using the pre-loaded engine
    // transcribe-rs expects 16kHz mono samples
    let result = model
        .transcribe(samples, &TranscribeOptions::default())
        .map_err(|e| anyhow::anyhow!("Parakeet transcription failed: {}", e))?;

    Ok(TranscriptionResult {
        text: result.text.trim().to_string(),
    })
}

// ============================================================================
// Engine Caching (matches local_whisper.rs pattern)
// ============================================================================

/// Global shared Parakeet engine (can be unloaded unlike OnceCell)
static PARAKEET_ENGINE: OnceLock<Mutex<Option<CachedParakeetEngine>>> = OnceLock::new();

/// Keep the engine loaded after transcription (for desktop recording mode)
static KEEP_LOADED: AtomicBool = AtomicBool::new(false);

struct CachedParakeetEngine {
    engine: transcribe_rs::onnx::parakeet::ParakeetModel,
    path: String,
}

fn get_cache() -> &'static Mutex<Option<CachedParakeetEngine>> {
    PARAKEET_ENGINE.get_or_init(|| Mutex::new(None))
}

/// Get or load the shared Parakeet engine
///
/// This function ensures the model is loaded only once and then cached globally.
/// All subsequent calls reuse the same engine instance, reducing memory usage
/// and eliminating repeated model loading overhead.
fn get_or_load_engine(model_path: &str) -> Result<()> {
    use std::path::Path;
    use transcribe_rs::onnx::Quantization;
    use transcribe_rs::onnx::parakeet::ParakeetModel;

    let mut cache = get_cache().lock().unwrap();

    // Check if already loaded with same path
    if let Some(ref cached) = *cache
        && cached.path == model_path
    {
        return Ok(()); // Already loaded
    }

    // Validate model path
    if model_path.is_empty() {
        anyhow::bail!("Parakeet model path not configured. Run 'whis setup local'.");
    }

    if !Path::new(model_path).exists() {
        anyhow::bail!(
            "Parakeet model not found at: {}\n\
             Download a model using: whis setup local",
            model_path
        );
    }

    crate::verbose!("Loading Parakeet model: {}", model_path);

    let engine = ParakeetModel::load(Path::new(model_path), &Quantization::Int8)
        .map_err(|e| anyhow::anyhow!("Failed to load Parakeet model: {}", e))?;

    crate::verbose!("Parakeet model loaded");

    *cache = Some(CachedParakeetEngine {
        engine,
        path: model_path.to_string(),
    });

    Ok(())
}

/// Preload Parakeet model in background to reduce first-transcription latency
///
/// This function spawns a background thread that loads the Parakeet model
/// into memory. This reduces the latency when transcription actually starts,
/// as the model will already be loaded.
///
/// The preloaded model is cached in a static variable and reused for all
/// subsequent transcription calls.
///
/// # Arguments
/// * `model_path` - Path to the Parakeet model directory
///
/// # Example
/// ```no_run
/// use whis_core::preload_parakeet;
/// preload_parakeet("/path/to/parakeet/model");
/// // Model loads in background while recording...
/// ```
pub fn preload_parakeet(model_path: &str) {
    // Check if model is already loaded
    {
        let cache = get_cache().lock().unwrap();
        if let Some(ref cached) = *cache
            && cached.path == model_path
        {
            crate::verbose!("Engine already cached, skipping preload");
            return;
        }
    }

    let model_path = model_path.to_string();
    std::thread::spawn(move || {
        crate::verbose!("Preloading Parakeet model: {}", model_path);

        // Load into shared static cache using get_or_load_engine
        if let Err(e) = get_or_load_engine(&model_path) {
            eprintln!("Warning: Failed to preload Parakeet model: {}", e);
            return;
        }

        crate::verbose!("Parakeet model preloaded");
    });
}

/// Set whether to keep the model loaded after transcription.
///
/// When `true`, the model stays in memory for faster subsequent transcriptions.
/// When `false` (default), the model is unloaded after each use.
///
/// # Arguments
/// * `keep` - Whether to keep the model loaded
pub fn set_keep_loaded(keep: bool) {
    KEEP_LOADED.store(keep, Ordering::SeqCst);
    crate::verbose!("Parakeet engine keep_loaded set to: {}", keep);
}

/// Check if models should be kept loaded.
pub fn should_keep_loaded() -> bool {
    KEEP_LOADED.load(Ordering::SeqCst)
}

/// Unload the cached model (if any).
///
/// This frees the memory used by the model. Call this when you're done
/// with transcription and don't expect more requests soon.
pub fn unload_parakeet() {
    let mut cache = get_cache().lock().unwrap();
    if cache.is_some() {
        crate::verbose!("Unloading Parakeet engine from cache");
        *cache = None;
    }
}

/// Called after transcription to conditionally unload the model.
fn maybe_unload() {
    if !should_keep_loaded() {
        unload_parakeet();
    }
}
