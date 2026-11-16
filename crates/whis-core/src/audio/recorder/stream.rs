//! Unified audio stream building with platform-specific handling.

use anyhow::Result;
use cpal::traits::DeviceTrait;
use cpal::{Device, Stream, StreamConfig};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use super::AudioStreamSender;
use super::processor::SampleProcessor;

/// Global counter for stream errors (reset per recording session)
/// Used to provide rate-limited, user-friendly error reporting
static STREAM_ERROR_COUNT: AtomicU64 = AtomicU64::new(0);

/// Reset the stream error counter (call at start of new recording)
pub fn reset_stream_error_count() {
    STREAM_ERROR_COUNT.store(0, Ordering::Relaxed);
}

/// Get total stream errors from last recording session
pub fn get_stream_error_count() -> u64 {
    STREAM_ERROR_COUNT.load(Ordering::Relaxed)
}

/// Build a unified audio input stream that works with or without VAD.
///
/// This function eliminates the code duplication between VAD and non-VAD builds
/// by using the SampleProcessor abstraction.
pub(super) fn build_stream<T>(
    device: &Device,
    config: &StreamConfig,
    samples: Arc<Mutex<Vec<f32>>>,
    processor: SampleProcessor,
    stream_tx: Option<Arc<AudioStreamSender>>,
) -> Result<Stream>
where
    T: cpal::Sample + cpal::SizedSample,
    f32: cpal::FromSample<T>,
{
    // Wrap processor in Arc<Mutex> for sharing with audio callback
    let processor = Arc::new(Mutex::new(processor));

    // Rate-limited error handler for ALSA stream errors
    // These are common on Linux (especially with USB audio) and non-fatal
    let err_fn = |err| {
        let count = STREAM_ERROR_COUNT.fetch_add(1, Ordering::Relaxed);

        // Log first error with helpful explanation
        if count == 0 {
            crate::verbose!(
                "Audio stream error (common on Linux, non-fatal): {err}\n\
                 This is usually caused by audio buffer timing and doesn't affect recording quality.\n\
                 Subsequent similar errors will be suppressed."
            );
        }
        // Log periodically to show it's ongoing (every 1000 errors)
        else if count.is_multiple_of(1000) {
            crate::verbose!(
                "Audio stream: {count} non-fatal errors (recording continues normally)"
            );
        }
    };

    let stream = device.build_input_stream(
        config,
        move |data: &[T], _: &cpal::InputCallbackInfo| {
            // Convert to f32
            let f32_samples: Vec<f32> =
                data.iter().map(|&s| cpal::Sample::from_sample(s)).collect();

            // Process through resampler and VAD (if enabled)
            let processed_samples = processor.lock().unwrap().process(&f32_samples);

            // Store processed samples (speech only if VAD enabled)
            if !processed_samples.is_empty() {
                samples
                    .lock()
                    .unwrap()
                    .extend_from_slice(&processed_samples);

                // Stream samples if channel is configured (for real-time transcription).
                // Unbounded, so this never blocks the audio callback and never drops audio.
                if let Some(ref tx) = stream_tx {
                    let _ = tx.send(processed_samples);
                }
            }
        },
        err_fn,
        None,
    )?;

    Ok(stream)
}
