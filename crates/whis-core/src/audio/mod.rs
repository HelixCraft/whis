//! Audio Recording Module
//!
//! This module provides cross-platform audio recording with the following features:
//! - Real-time resampling to 16kHz mono
//! - Voice Activity Detection (optional, via `vad` feature)
//! - MP3 encoding via embedded encoder
//!
//! # Architecture
//!
//! ```text
//! AudioRecorder
//!   ├── Stream (cpal) - Platform-specific audio capture
//!   ├── Resampler     - Real-time 16kHz conversion
//!   ├── VAD (optional)- Voice activity detection
//!   └── Encoder       - MP3 encoding
//! ```
//!
//! # Usage
//!
//! ```rust,no_run
//! use whis_core::audio::AudioRecorder;
//!
//! let mut recorder = AudioRecorder::new()?;
//! recorder.start_recording()?;
//! // ... wait for input ...
//! let recording_data = recorder.stop_recording()?;
//! let samples = recording_data.finalize_raw();
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```
//!
//! # Platform Notes
//!
//! - **macOS**: Uses message passing architecture to avoid `Send` issues with cpal::Stream
//! - **Linux**: ALSA stderr suppression via safe FFI wrapper

pub mod chunker;
mod devices;
mod encoder;
pub mod enhance;
pub mod error;
#[cfg(feature = "history")]
pub mod last_recording;
mod recorder;
mod types;
mod vad;

// PulseAudio device enumeration with rich metadata (Linux only)
#[cfg(all(target_os = "linux", feature = "pulse-metadata"))]
mod pulse;

// Re-export public types
pub use chunker::{AudioChunk as ProgressiveChunk, ChunkerConfig, ProgressiveChunker};
pub use devices::list_audio_devices;
pub use encoder::{AudioEncoder, create_encoder};
pub use enhance::{
    AudioEnhanceSettings, AudioQuality, EnhanceConfig, EnhanceMode, EnhanceStats, Enhanced,
    StreamEnhancer, enhance, enhance_audio_stream, enhance_chunk_stream, enhance_owned,
};
pub use error::AudioError;
pub use recorder::{AudioRecorder, AudioStreamSender, RecorderConfig, RecordingData};
pub use types::AudioDeviceInfo;

// Re-export VAD types (always available - no-op when feature disabled)
pub use vad::{VadConfig, VadProcessor, VadState};
