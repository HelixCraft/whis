//! Audio recording with real-time resampling and optional VAD.

mod config;
mod processor;
mod stream;

pub use config::RecorderConfig;
pub use stream::{get_stream_error_count, reset_stream_error_count};

use anyhow::{Context, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::sync::{Arc, Mutex};

use super::devices;
use super::vad::{VadConfig, VadProcessor};
use crate::resample::{FrameResampler, WHISPER_SAMPLE_RATE};

use processor::SampleProcessor;

/// Sender type for streaming audio samples during recording.
///
/// The channel is unbounded on purpose: the audio callback must never block and
/// must never drop a block. A bounded channel with `try_send` silently loses
/// audio whenever the consumer falls behind (denoising, transcription, a busy
/// runtime), which truncates both the transcript and the chunk tail.
pub type AudioStreamSender = tokio::sync::mpsc::UnboundedSender<Vec<f32>>;

/// Audio recorder with real-time resampling to 16kHz mono.
///
/// # Platform Notes
/// - **macOS**: Contains unsafe Send impl due to cpal::Stream limitations
/// - **Linux**: ALSA error suppression is automatically initialized
pub struct AudioRecorder {
    samples: Arc<Mutex<Vec<f32>>>,
    /// Output sample rate (always 16kHz after resampling)
    sample_rate: u32,
    /// Output channels (always 1/mono after resampling)
    channels: u16,
    stream: Option<cpal::Stream>,
    /// Real-time resampler (converts device rate to 16kHz mono)
    /// Created when recording starts (needs device sample rate)
    resampler: Option<Arc<Mutex<FrameResampler>>>,
    /// Sample processor (combines resampler and VAD)
    processor: Option<Arc<Mutex<SampleProcessor>>>,
    /// Voice Activity Detection processor (optional, filters silence)
    vad: Option<Arc<Mutex<VadProcessor>>>,
    /// VAD configuration for next recording
    vad_config: VadConfig,
    /// Optional sender for streaming samples during recording
    stream_tx: Option<Arc<AudioStreamSender>>,
}

// SAFETY: AudioRecorder is always used behind a Mutex in AppState, ensuring
// single-threaded access. The cpal::Stream on macOS contains non-Send types
// (AudioObjectPropertyListener with FnMut), but we never move the AudioRecorder
// between threads - it's created, used, and dropped on the same thread.
// The Mutex only protects against concurrent access within the Tauri async runtime.
//
// TODO(macos): Refactor to eliminate this unsafe impl. Possible approaches:
// 1. Channel-based: Move Stream to dedicated audio thread, communicate via mpsc channels
// 2. Actor pattern: AudioRecorder sends commands to an actor that owns the Stream
// Requires macOS hardware for testing.
#[cfg(target_os = "macos")]
unsafe impl Send for AudioRecorder {}

impl AudioRecorder {
    /// Create a new audio recorder.
    pub fn new() -> Result<Self> {
        Ok(AudioRecorder {
            samples: Arc::new(Mutex::new(Vec::new())),
            sample_rate: WHISPER_SAMPLE_RATE, // Output is always 16kHz
            channels: 1,                      // Output is always mono
            stream: None,
            resampler: None,
            processor: None,
            vad: None,
            vad_config: VadConfig::default(),
            stream_tx: None,
        })
    }

    /// Configure Voice Activity Detection for the next recording.
    /// VAD filters out silence to reduce audio size and improve transcription.
    pub fn set_vad(&mut self, enabled: bool, threshold: f32) {
        self.vad_config = VadConfig {
            enabled,
            threshold: threshold.clamp(0.0, 1.0),
        };
    }

    /// Start recording with the default input device.
    pub fn start_recording(&mut self) -> Result<()> {
        self.start_recording_with_device(None)
    }

    /// Start recording with a specific device name.
    ///
    /// # Parameters
    /// - `device_name`: Name of the device to use (None = system default)
    pub fn start_recording_with_device(&mut self, device_name: Option<&str>) -> Result<()> {
        // Reset stream error counter for new recording session
        reset_stream_error_count();

        devices::init_platform();
        let host = cpal::default_host();

        let device = if let Some(name) = device_name {
            // Try exact match first
            let exact_match = host.input_devices()?.find(|d| {
                d.description()
                    .map(|n| n.to_string() == name)
                    .unwrap_or(false)
            });

            if let Some(device) = exact_match {
                device
            } else {
                // Fallback: fuzzy match using word containment
                // This handles PulseAudio technical names vs CPAL human-readable names
                host.input_devices()?
                    .find(|d| {
                        d.description()
                            .map(|desc| devices::fuzzy_device_match(name, &desc.to_string()))
                            .unwrap_or(false)
                    })
                    .with_context(|| format!("Audio device '{}' not found", name))?
            }
        } else {
            // Use default device
            host.default_input_device()
                .context("No input device available")?
        };

        let actual_device_name = device
            .description()
            .map(|d| d.to_string())
            .unwrap_or_else(|_| "<unknown>".to_string());
        crate::verbose!("Audio device: {}", actual_device_name);

        let config = device
            .default_input_config()
            .context("Failed to get default input config")?;

        // Force mono on Android - emulators and some devices don't support stereo input
        #[cfg(target_os = "android")]
        let device_channels = 1u16;
        #[cfg(not(target_os = "android"))]
        let device_channels = config.channels();

        let device_sample_rate = config.sample_rate();

        crate::verbose!(
            "Audio device: {} Hz, {} channel(s) -> resampling to {} Hz mono",
            device_sample_rate,
            device_channels,
            WHISPER_SAMPLE_RATE
        );

        // Create real-time resampler (device rate -> 16kHz mono)
        let resampler = FrameResampler::new(device_sample_rate, device_channels)
            .context("Failed to create resampler")?;
        let resampler = Arc::new(Mutex::new(resampler));
        self.resampler = Some(resampler.clone());

        // Create sample processor
        let processor = self.create_processor(resampler.clone())?;
        self.processor = Some(Arc::new(Mutex::new(processor)));

        // Output is always 16kHz mono after resampling
        self.sample_rate = WHISPER_SAMPLE_RATE;
        self.channels = 1;

        let stream_config = cpal::StreamConfig {
            channels: device_channels,
            sample_rate: config.sample_rate(),
            buffer_size: cpal::BufferSize::Default,
        };

        let samples = self.samples.clone();
        samples.lock().unwrap().clear();

        // Build stream using unified builder (no duplication!)
        let stream = match config.sample_format() {
            cpal::SampleFormat::F32 => {
                self.build_stream_typed::<f32>(&device, &stream_config, samples)?
            }
            cpal::SampleFormat::I16 => {
                self.build_stream_typed::<i16>(&device, &stream_config, samples)?
            }
            cpal::SampleFormat::U16 => {
                self.build_stream_typed::<u16>(&device, &stream_config, samples)?
            }
            _ => anyhow::bail!("Unsupported sample format"),
        };

        stream.play()?;

        // Store stream to keep it alive; dropping it will release the microphone
        self.stream = Some(stream);

        Ok(())
    }

    /// Create a sample processor with the appropriate VAD configuration.
    fn create_processor(
        &mut self,
        resampler: Arc<Mutex<FrameResampler>>,
    ) -> Result<SampleProcessor> {
        if self.vad_config.enabled {
            crate::verbose!("VAD enabled (threshold: {:.2})", self.vad_config.threshold);
            let vad_processor = VadProcessor::new(true, self.vad_config.threshold)
                .context("Failed to create VAD processor")?;
            let vad = Arc::new(Mutex::new(vad_processor));
            self.vad = Some(vad.clone());
            Ok(SampleProcessor::with_vad(resampler, vad))
        } else {
            self.vad = None;
            Ok(SampleProcessor::new(resampler))
        }
    }

    /// Build a typed audio stream (unified implementation, no duplication).
    fn build_stream_typed<T>(
        &self,
        device: &cpal::Device,
        config: &cpal::StreamConfig,
        samples: Arc<Mutex<Vec<f32>>>,
    ) -> Result<cpal::Stream>
    where
        T: cpal::Sample + cpal::SizedSample,
        f32: cpal::FromSample<T>,
    {
        // Get the processor - clone it since it's shared with self
        let processor = self.processor.as_ref().unwrap().lock().unwrap().clone();

        stream::build_stream::<T>(device, config, samples, processor, self.stream_tx.clone())
    }

    /// Start recording and stream samples to a channel for real-time processing.
    ///
    /// Returns a receiver that receives chunks of resampled 16kHz mono f32 samples
    /// as they are recorded. The receiver should be consumed in a separate async task.
    ///
    /// Used by streaming transcription providers like OpenAI Realtime API.
    pub fn start_recording_streaming(
        &mut self,
    ) -> Result<tokio::sync::mpsc::UnboundedReceiver<Vec<f32>>> {
        self.start_recording_streaming_with_device(None)
    }

    /// Start recording with streaming and a specific device name.
    pub fn start_recording_streaming_with_device(
        &mut self,
        device_name: Option<&str>,
    ) -> Result<tokio::sync::mpsc::UnboundedReceiver<Vec<f32>>> {
        // Create channel for streaming samples
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        self.stream_tx = Some(Arc::new(tx));

        // Start recording normally
        self.start_recording_with_device(device_name)?;

        Ok(rx)
    }

    /// Stop recording and return the recording data.
    /// The stream is dropped here, making the returned RecordingData Send-safe.
    pub fn stop_recording(&mut self) -> Result<RecordingData> {
        // Drop the stream first to release the microphone
        self.stream = None;

        // Drop the streaming sender to signal end of audio to receivers
        self.stream_tx = None;

        // Flush the processor to get any remaining buffered samples
        let flushed_samples = if let Some(processor) = &self.processor {
            processor.lock().unwrap().flush()
        } else {
            Vec::new()
        };
        self.processor = None;
        self.resampler = None;

        {
            self.vad = None;
        }

        // Take ownership of samples and append flushed samples
        let mut samples: Vec<f32> = {
            let mut guard = self.samples.lock().unwrap();
            std::mem::take(&mut *guard)
        };
        samples.extend_from_slice(&flushed_samples);

        if samples.is_empty() {
            crate::verbose!("No audio samples captured");
            anyhow::bail!("No audio data recorded");
        }

        // Output is always 16kHz mono
        let duration_secs = samples.len() as f32 / self.sample_rate as f32;
        crate::verbose!(
            "Recorded {} samples ({:.1}s at {} Hz mono)",
            samples.len(),
            duration_secs,
            self.sample_rate
        );

        // Log stream error summary if there were any
        let error_count = get_stream_error_count();
        if error_count > 0 {
            crate::verbose!(
                "Recording completed with {} non-fatal audio stream errors",
                error_count
            );
        }

        Ok(RecordingData { samples })
    }
}

impl Default for AudioRecorder {
    fn default() -> Self {
        Self::new().expect("Failed to create AudioRecorder")
    }
}

/// Recording data extracted from AudioRecorder after stopping.
/// This struct is Send-safe (unlike AudioRecorder on macOS where cpal::Stream isn't Send).
/// Contains f32 samples at 16kHz mono, ready for progressive transcription.
pub struct RecordingData {
    samples: Vec<f32>,
}

impl RecordingData {
    /// Finalize the recording and return raw f32 samples (16kHz mono).
    ///
    /// The samples are already resampled to 16kHz mono during recording.
    /// Use this with ProgressiveChunker for transcription.
    pub fn finalize_raw(self) -> Vec<f32> {
        self.samples
    }
}
