//! Storage for the most recent recording.
//!
//! The audio is written to disk *before* transcription starts, so a failed
//! transcription can never lose what the user said. Only one file is kept;
//! each new recording atomically replaces the previous one.
//!
//! ```text
//! ~/.local/share/whis/last_audio.wav   (16 kHz, mono, 16-bit PCM)
//! ~/.local/share/whis/last_audio.json  (metadata)
//! ```

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs::{self, File};
use std::io::BufWriter;
use std::path::{Path, PathBuf};

use crate::history::{data_dir, now_ms};

/// Sample rate of all stored recordings.
pub const SAMPLE_RATE: u32 = 16_000;

/// File name of the stored recording.
pub const FILE_NAME: &str = "last_audio.wav";

/// Metadata stored next to the recording.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LastAudioMeta {
    #[serde(default)]
    pub created_at_ms: u64,
    #[serde(default)]
    pub duration_secs: f32,
    #[serde(default)]
    pub sample_rate: u32,
    #[serde(default)]
    pub provider: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub language: Option<String>,
    /// History entry created from this recording, when known.
    #[serde(default)]
    pub history_id: Option<String>,
}

/// Information about the stored recording, as exposed to the UI.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LastAudioInfo {
    pub path: String,
    pub created_at_ms: u64,
    pub duration_secs: f32,
    pub size_bytes: u64,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub language: Option<String>,
}

/// Path of the stored recording.
pub fn path() -> PathBuf {
    data_dir().join(FILE_NAME)
}

/// Path of the metadata file.
pub fn meta_path() -> PathBuf {
    data_dir().join("last_audio.json")
}

/// Persist a recording, replacing the previous one.
///
/// Both files are written to a temporary path first and renamed afterwards,
/// so a crash or a full disk never leaves a half-written recording behind.
pub fn save(
    samples: &[f32],
    provider: Option<String>,
    model: Option<String>,
    language: Option<String>,
) -> Result<LastAudioInfo> {
    if samples.is_empty() {
        anyhow::bail!("Refusing to save empty recording");
    }

    let created_at_ms = now_ms();
    let duration_secs = samples.len() as f32 / SAMPLE_RATE as f32;
    let wav_path = path();
    let meta = LastAudioMeta {
        created_at_ms,
        duration_secs,
        sample_rate: SAMPLE_RATE,
        provider,
        model,
        language,
        history_id: None,
    };

    write_wav(&wav_path, samples)?;
    write_meta(&meta_path(), &meta)?;

    Ok(info_from_meta(wav_path, &meta))
}

/// Return information about the stored recording, if one exists.
///
/// Falls back to the file system when the metadata file is missing or
/// malformed, so a recording is never reported as absent just because its
/// sidecar file was lost.
pub fn info() -> Option<LastAudioInfo> {
    let wav_path = path();
    let metadata = fs::metadata(&wav_path).ok()?;
    if metadata.len() == 0 {
        return None;
    }

    let meta = read_meta(&meta_path()).unwrap_or_default();
    let created_at_ms = if meta.created_at_ms > 0 {
        meta.created_at_ms
    } else {
        metadata
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0)
    };
    let duration_secs = if meta.duration_secs > 0.0 {
        meta.duration_secs
    } else {
        duration_of(&wav_path).unwrap_or(0.0)
    };

    Some(LastAudioInfo {
        path: wav_path.to_string_lossy().to_string(),
        created_at_ms,
        duration_secs,
        size_bytes: metadata.len(),
        provider: meta.provider,
        model: meta.model,
        language: meta.language,
    })
}

/// Load the stored recording as 16 kHz mono f32 samples.
pub fn load_samples() -> Result<Vec<f32>> {
    let wav_path = path();
    if !wav_path.exists() {
        anyhow::bail!("No recording stored");
    }
    read_wav(&wav_path)
}

/// Associate the stored recording with a history entry.
pub fn set_history_id(id: &str) {
    let meta_path = meta_path();
    let mut meta = read_meta(&meta_path).unwrap_or_default();
    meta.history_id = Some(id.to_string());
    if let Err(e) = write_meta(&meta_path, &meta) {
        crate::verbose!("Failed to update last audio metadata: {e}");
    }
}

/// History entry created from the stored recording, when known.
pub fn history_id() -> Option<String> {
    read_meta(&meta_path()).and_then(|meta| meta.history_id)
}

/// Delete the stored recording and its metadata.
pub fn delete() -> Result<()> {
    for p in [path(), meta_path()] {
        match fs::remove_file(&p) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e).with_context(|| format!("Failed to delete {}", p.display())),
        }
    }
    Ok(())
}

fn info_from_meta(wav_path: PathBuf, meta: &LastAudioMeta) -> LastAudioInfo {
    let size_bytes = fs::metadata(&wav_path).map(|m| m.len()).unwrap_or(0);
    LastAudioInfo {
        path: wav_path.to_string_lossy().to_string(),
        created_at_ms: meta.created_at_ms,
        duration_secs: meta.duration_secs,
        size_bytes,
        provider: meta.provider.clone(),
        model: meta.model.clone(),
        language: meta.language.clone(),
    }
}

fn write_wav(path: &Path, samples: &[f32]) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).context("Failed to create data directory")?;
    }

    let tmp = path.with_extension("wav.tmp");
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: SAMPLE_RATE,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };

    let file = File::create(&tmp).context("Failed to create recording file")?;
    let mut writer =
        hound::WavWriter::new(BufWriter::new(file), spec).context("Failed to create WAV writer")?;
    for &sample in samples {
        let value = (sample.clamp(-1.0, 1.0) * i16::MAX as f32).round() as i16;
        writer
            .write_sample(value)
            .context("Failed to write audio sample")?;
    }
    writer.finalize().context("Failed to finalize WAV file")?;

    fs::rename(&tmp, path).context("Failed to replace recording file")?;
    Ok(())
}

fn read_wav(path: &Path) -> Result<Vec<f32>> {
    let mut reader = hound::WavReader::open(path).context("Failed to open recording")?;
    let spec = reader.spec();
    if spec.channels != 1 || spec.sample_rate != SAMPLE_RATE {
        anyhow::bail!(
            "Unexpected recording format: {} Hz, {} channel(s)",
            spec.sample_rate,
            spec.channels
        );
    }

    let samples = match spec.sample_format {
        hound::SampleFormat::Int => {
            let max = (1i64 << (spec.bits_per_sample - 1)) as f32;
            reader
                .samples::<i32>()
                .map(|s| s.map(|v| v as f32 / max))
                .collect::<std::result::Result<Vec<_>, _>>()
                .context("Failed to read audio samples")?
        }
        hound::SampleFormat::Float => reader
            .samples::<f32>()
            .collect::<std::result::Result<Vec<_>, _>>()
            .context("Failed to read audio samples")?,
    };

    Ok(samples)
}

fn duration_of(path: &Path) -> Option<f32> {
    let reader = hound::WavReader::open(path).ok()?;
    let spec = reader.spec();
    if spec.sample_rate == 0 || spec.channels == 0 {
        return None;
    }
    let frames = reader.len() as f32 / u32::from(spec.channels) as f32;
    Some(frames / spec.sample_rate as f32)
}

fn read_meta(path: &Path) -> Option<LastAudioMeta> {
    let content = fs::read_to_string(path).ok()?;
    serde_json::from_str(&content).ok()
}

fn write_meta(path: &Path, meta: &LastAudioMeta) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).context("Failed to create data directory")?;
    }
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, serde_json::to_vec_pretty(meta)?).context("Failed to write audio metadata")?;
    fs::rename(&tmp, path).context("Failed to replace audio metadata")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_path(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "whis-last-audio-{}-{}-{}",
            std::process::id(),
            name,
            now_ms()
        ))
    }

    #[test]
    fn wav_roundtrip_is_close_to_original() {
        let path = temp_path("roundtrip.wav");
        let samples: Vec<f32> = (0..1600)
            .map(|i| (i as f32 / 1600.0 * std::f32::consts::TAU).sin() * 0.5)
            .collect();

        write_wav(&path, &samples).unwrap();
        let read = read_wav(&path).unwrap();

        assert_eq!(read.len(), samples.len());
        for (a, b) in samples.iter().zip(read.iter()) {
            assert!((a - b).abs() < 0.001, "sample mismatch: {a} vs {b}");
        }
        let duration = duration_of(&path).unwrap();
        assert!((duration - 0.1).abs() < 0.01);
        let _ = fs::remove_file(path);
    }
}
