//! Conservative audio cleanup before transcription.
//!
//! The chain is split into two groups with very different risk profiles:
//!
//! - Always-on clean-up ([`EnhanceMode::Light`]): DC/rumble high-pass, narrow
//!   notches for detected mains hum, and level normalisation with peak
//!   protection. It only removes energy below the speech band and only
//!   amplifies quiet speech, so a recording from a good microphone is not made
//!   worse.
//! - Optional spectral denoising ([`EnhanceMode::AutoDenoise`] /
//!   [`EnhanceMode::StrongDenoise`]). This is the only stage that can introduce
//!   artifacts, so it is opt-in or gated by a measured SNR.
//!
//! Everything is designed for speed: the light chain is a handful of biquads
//! plus a level analysis and stays in the low single-digit milliseconds for a
//! 90 second chunk. The denoiser uses a small radix-2 FFT and needs no extra
//! dependency.
//!
//! The stored recording is never modified. Callers hand in a sample buffer,
//! receive a processed copy (unless the mode is off), and keep the original.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;

use crate::audio::ProgressiveChunk;
use crate::resample::WHISPER_SAMPLE_RATE;

/// Sample rate of the pipeline (16 kHz mono).
const FS: f32 = WHISPER_SAMPLE_RATE as f32;

/// Level that speech should reach after normalisation.
const TARGET_SPEECH_DBFS: f32 = -20.0;
/// Hard ceiling for automatic amplification.
const MAX_GAIN_DB: f32 = 18.0;
/// Never let normalisation push the peak above -1 dBFS.
const PEAK_CEILING: f32 = 0.891;
/// Below this peak and RMS a chunk contains no usable audio and is skipped.
/// These values are far below any audible speech, even after amplification, so
/// the gate can never cut off words.
const GATE_PEAK_DBFS: f32 = -70.0;
const GATE_RMS_DBFS: f32 = -75.0;
/// Auto mode only denoises when the measured SNR is below this value.
const AUTO_DENOISE_SNR_DB: f32 = 15.0;
/// Do not consider audio quieter than this as speech when recommending denoise.
const MIN_SPEECH_DBFS: f32 = -60.0;
/// High-pass corner. Speech fundamentals are above this, rumble and DC below.
const HIGH_PASS_HZ: f32 = 60.0;

/// Level analysis frame (20 ms).
const ANALYSIS_FRAME: usize = 320;

/// Denoiser STFT settings. 32 ms window, 8 ms hop, Hann analysis/synthesis.
const FFT_SIZE: usize = 512;
const FFT_HOP: usize = FFT_SIZE / 4;
/// Frames per noise-estimate block and blocks kept in the minimum tracker.
const NOISE_BLOCK_FRAMES: usize = 20;
const NOISE_BLOCKS: usize = 16;

/// How aggressively the cleanup chain works.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum EnhanceMode {
    /// No processing, metrics are still measured.
    #[serde(rename = "off")]
    Off,
    /// High-pass, detected hum notches, and level normalisation.
    #[default]
    #[serde(rename = "light")]
    Light,
    /// Light, plus denoising when the recording is measured as noisy.
    #[serde(rename = "auto")]
    AutoDenoise,
    /// Light, plus denoising on every recording.
    #[serde(rename = "strong")]
    StrongDenoise,
}

impl EnhanceMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Light => "light",
            Self::AutoDenoise => "auto",
            Self::StrongDenoise => "strong",
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Self::Off => "Off",
            Self::Light => "Light",
            Self::AutoDenoise => "Auto denoise",
            Self::StrongDenoise => "Strong denoise",
        }
    }

    fn uses_basic_chain(&self) -> bool {
        !matches!(self, Self::Off)
    }
}

/// Settings for the audio cleanup chain.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct AudioEnhanceSettings {
    pub mode: EnhanceMode,
}

/// Snapshot of the cleanup configuration.
#[derive(Debug, Clone, Copy, Default)]
pub struct EnhanceConfig {
    pub mode: EnhanceMode,
}

impl From<AudioEnhanceSettings> for EnhanceConfig {
    fn from(settings: AudioEnhanceSettings) -> Self {
        Self {
            mode: settings.mode,
        }
    }
}

impl From<&AudioEnhanceSettings> for EnhanceConfig {
    fn from(settings: &AudioEnhanceSettings) -> Self {
        Self {
            mode: settings.mode,
        }
    }
}

/// Measurements for one processed chunk.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct EnhanceStats {
    pub peak_dbfs: f32,
    pub rms_dbfs: f32,
    pub speech_dbfs: f32,
    pub noise_dbfs: f32,
    pub snr_db: f32,
    pub gain_db: f32,
    pub hum_hz: Option<f32>,
    pub clipping: bool,
    pub denoised: bool,
    pub gated: bool,
    pub duration_secs: f32,
}

/// Aggregated measurements for a whole recording.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct AudioQuality {
    pub peak_dbfs: f32,
    pub rms_dbfs: f32,
    pub speech_dbfs: f32,
    pub noise_dbfs: f32,
    pub snr_db: f32,
    pub gain_db: f32,
    pub hum_hz: Option<f32>,
    pub clipping: bool,
    pub denoised: bool,
    pub chunks: usize,
    pub gated_chunks: usize,
    pub duration_secs: f32,
}

impl AudioQuality {
    /// Add one chunk, weighting the averages by duration.
    pub fn add(&mut self, stats: &EnhanceStats) {
        let weight = stats.duration_secs.max(0.0);
        if weight <= 0.0 {
            return;
        }

        let total = self.duration_secs + weight;
        if total > 0.0 {
            self.peak_dbfs =
                (self.peak_dbfs * self.duration_secs + stats.peak_dbfs * weight) / total;
            self.rms_dbfs = (self.rms_dbfs * self.duration_secs + stats.rms_dbfs * weight) / total;
            self.speech_dbfs =
                (self.speech_dbfs * self.duration_secs + stats.speech_dbfs * weight) / total;
            self.noise_dbfs =
                (self.noise_dbfs * self.duration_secs + stats.noise_dbfs * weight) / total;
            self.snr_db = (self.snr_db * self.duration_secs + stats.snr_db * weight) / total;
        }
        self.duration_secs = total;

        self.gain_db = self.gain_db.max(stats.gain_db);
        if self.hum_hz.is_none() {
            self.hum_hz = stats.hum_hz;
        }
        self.clipping |= stats.clipping;
        self.denoised |= stats.denoised;
        self.chunks += 1;
        if stats.gated {
            self.gated_chunks += 1;
        }
    }

    /// Whether the recording looks noisy enough that denoising is worth trying.
    pub fn is_noisy(&self) -> bool {
        self.chunks > 0
            && self.speech_dbfs > MIN_SPEECH_DBFS
            // A steady signal with no quieter frames has no usable noise floor
            // estimate, so it is not reported as noisy.
            && self.snr_db > 3.0
            && self.snr_db < AUTO_DENOISE_SNR_DB
            && !self.denoised
    }
}

/// Result of processing one buffer.
pub struct Enhanced {
    pub samples: Vec<f32>,
    pub stats: EnhanceStats,
}

/// Process a borrowed buffer.
pub fn enhance(samples: &[f32], config: &EnhanceConfig) -> Enhanced {
    enhance_owned(samples.to_vec(), config)
}

/// Process an owned buffer without an extra copy when the mode is off.
pub fn enhance_owned(samples: Vec<f32>, config: &EnhanceConfig) -> Enhanced {
    let duration_secs = samples.len() as f32 / FS;

    if samples.is_empty() {
        return Enhanced {
            samples,
            stats: EnhanceStats {
                peak_dbfs: -120.0,
                rms_dbfs: -120.0,
                speech_dbfs: -120.0,
                noise_dbfs: -120.0,
                duration_secs,
                ..Default::default()
            },
        };
    }

    let hum = detect_hum(&samples);
    let analysis = analyze(&samples, hum.as_ref().map(|h| h.base_hz));

    if !config.mode.uses_basic_chain() {
        return Enhanced {
            samples,
            stats: stats_from_analysis(&analysis, duration_secs, 0.0, false, false),
        };
    }

    let mut samples = samples;

    // Detect hum before filtering: the high-pass already attenuates the base.
    if let Some(hum) = &hum {
        for frequency in &hum.notches {
            let mut notch = Biquad::notch(FS, *frequency, 25.0);
            notch.run(&mut samples);
        }
    }

    let mut high_pass = Biquad::high_pass(FS, HIGH_PASS_HZ, 0.707);
    high_pass.run(&mut samples);

    let denoised = match config.mode {
        EnhanceMode::AutoDenoise => {
            // Auto mode needs evidence of a separate noise floor: quiet frames
            // between speech. Without that gap a steady clean signal (singing,
            // a held tone) would be mistaken for noise and attenuated.
            analysis.snr_db > 3.0
                && analysis.snr_db < AUTO_DENOISE_SNR_DB
                && analysis.speech_dbfs > MIN_SPEECH_DBFS
        }
        EnhanceMode::StrongDenoise => analysis.speech_dbfs > MIN_SPEECH_DBFS,
        _ => false,
    };

    let denoised = denoised
        && match config.mode {
            EnhanceMode::StrongDenoise => spectral_denoise(&mut samples, DenoiseStrength::Strong),
            _ => spectral_denoise(&mut samples, DenoiseStrength::Auto),
        };

    // Normalise after filtering so the measured speech level matches the audio
    // that is actually sent to the model.
    let post_levels = frame_levels(&samples);
    let peak = post_levels.peak;
    let gain_db = normalisation_gain(post_levels.speech_dbfs, peak);
    if gain_db > 0.0 {
        let ratio = 10f32.powf(gain_db / 20.0);
        for sample in &mut samples {
            *sample *= ratio;
        }
    }

    let output_peak = peak_of(&samples);
    let output_rms = rms_of(&samples);
    let gated = to_db(output_peak) < GATE_PEAK_DBFS && to_db(output_rms) < GATE_RMS_DBFS;

    let stats = stats_from_analysis(&analysis, duration_secs, gain_db, denoised, gated);
    Enhanced { samples, stats }
}

fn stats_from_analysis(
    analysis: &Analysis,
    duration_secs: f32,
    gain_db: f32,
    denoised: bool,
    gated: bool,
) -> EnhanceStats {
    EnhanceStats {
        peak_dbfs: analysis.peak_dbfs,
        rms_dbfs: analysis.rms_dbfs,
        speech_dbfs: analysis.speech_dbfs,
        noise_dbfs: analysis.noise_dbfs,
        snr_db: analysis.snr_db,
        gain_db,
        hum_hz: analysis.hum_hz,
        clipping: analysis.clipping,
        denoised,
        gated,
        duration_secs,
    }
}

// ---------------------------------------------------------------------------
// Level analysis
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy)]
struct Analysis {
    peak_dbfs: f32,
    rms_dbfs: f32,
    speech_dbfs: f32,
    noise_dbfs: f32,
    snr_db: f32,
    clipping: bool,
    hum_hz: Option<f32>,
}

impl Default for Analysis {
    fn default() -> Self {
        Self {
            peak_dbfs: -120.0,
            rms_dbfs: -120.0,
            speech_dbfs: -120.0,
            noise_dbfs: -120.0,
            snr_db: 0.0,
            clipping: false,
            hum_hz: None,
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct Levels {
    peak: f32,
    rms_dbfs: f32,
    noise_dbfs: f32,
    speech_dbfs: f32,
}

fn analyze(samples: &[f32], hum_hz: Option<f32>) -> Analysis {
    if samples.is_empty() {
        return Analysis::default();
    }

    let levels = frame_levels(samples);
    let snr_db = (levels.speech_dbfs - levels.noise_dbfs).clamp(0.0, 60.0);

    Analysis {
        peak_dbfs: to_db(levels.peak),
        rms_dbfs: levels.rms_dbfs,
        speech_dbfs: levels.speech_dbfs,
        noise_dbfs: levels.noise_dbfs,
        snr_db,
        clipping: levels.peak >= 0.999,
        hum_hz,
    }
}

fn frame_levels(samples: &[f32]) -> Levels {
    let mut peak = 0.0f32;
    let mut sumsq = 0.0f64;
    let mut levels = Vec::with_capacity(samples.len().div_ceil(ANALYSIS_FRAME));

    for frame in samples.chunks(ANALYSIS_FRAME) {
        let mut frame_sumsq = 0.0f64;
        for &sample in frame {
            let absolute = sample.abs();
            if absolute > peak {
                peak = absolute;
            }
            frame_sumsq += (sample as f64) * (sample as f64);
        }
        sumsq += frame_sumsq;

        let rms = (frame_sumsq / frame.len() as f64).sqrt() as f32;
        if rms > 0.0 {
            levels.push(to_db(rms));
        }
    }

    let rms_dbfs = to_db((sumsq / samples.len() as f64).sqrt() as f32);
    if levels.is_empty() {
        return Levels {
            peak,
            rms_dbfs,
            noise_dbfs: -120.0,
            speech_dbfs: -120.0,
        };
    }

    levels.sort_by(|a, b| a.total_cmp(b));
    Levels {
        peak,
        rms_dbfs,
        noise_dbfs: percentile_sorted(&levels, 0.10),
        speech_dbfs: percentile_sorted(&levels, 0.90),
    }
}

fn percentile_sorted(sorted: &[f32], fraction: f32) -> f32 {
    if sorted.is_empty() {
        return -120.0;
    }
    let index = ((sorted.len() - 1) as f32 * fraction.clamp(0.0, 1.0)).round() as usize;
    sorted[index.min(sorted.len() - 1)]
}

/// Automatic gain to bring quiet speech to [`TARGET_SPEECH_DBFS`].
///
/// The gain is never negative and never pushes the peak above
/// [`PEAK_CEILING`], so a loud recording is left untouched and no clipping is
/// introduced.
fn normalisation_gain(speech_dbfs: f32, peak: f32) -> f32 {
    if !speech_dbfs.is_finite() || speech_dbfs <= -119.0 || peak <= f32::MIN_POSITIVE {
        return 0.0;
    }

    let target = (TARGET_SPEECH_DBFS - speech_dbfs).clamp(0.0, MAX_GAIN_DB);
    let max_for_peak = 20.0 * (PEAK_CEILING / peak).log10();
    target.min(max_for_peak).clamp(0.0, MAX_GAIN_DB)
}

fn peak_of(samples: &[f32]) -> f32 {
    samples.iter().fold(0.0f32, |peak, s| peak.max(s.abs()))
}

fn rms_of(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    let sumsq: f64 = samples.iter().map(|s| (*s as f64) * (*s as f64)).sum();
    (sumsq / samples.len() as f64).sqrt() as f32
}

fn to_db(value: f32) -> f32 {
    if value <= 1e-9 {
        -120.0
    } else {
        20.0 * value.log10()
    }
}

// ---------------------------------------------------------------------------
// Hum detection and biquads
// ---------------------------------------------------------------------------

struct HumAnalysis {
    base_hz: f32,
    /// Base frequency and harmonics that should be removed.
    notches: Vec<f32>,
}

/// Detect mains hum at 50 or 60 Hz by comparing the tone with nearby bins.
///
/// Only very clear, narrow peaks are accepted, and only harmonics that are
/// themselves prominent are notched. This keeps a normal voice untouched.
fn detect_hum(samples: &[f32]) -> Option<HumAnalysis> {
    let window_len = samples.len().min((8.0 * FS) as usize);
    if window_len < FS as usize {
        return None;
    }
    let window = &samples[..window_len];

    let mut best: Option<(f32, HumAnalysis)> = None;

    for base in [50.0f32, 60.0] {
        let base_amp = goertzel_amplitude(window, base);
        let base_prominence = base_amp / (reference_level(window, base) + 1e-9);
        if base_amp < 0.001 || base_prominence < 6.0 {
            continue;
        }

        let mut notches = vec![base];
        let mut score = base_amp * base_prominence;

        for harmonic in 2..=4 {
            let frequency = base * harmonic as f32;
            if frequency >= 400.0 || frequency >= FS / 2.0 {
                break;
            }
            let amplitude = goertzel_amplitude(window, frequency);
            let prominence = amplitude / (reference_level(window, frequency) + 1e-9);
            if amplitude >= 0.0005 && prominence >= 3.0 {
                notches.push(frequency);
                score += amplitude * prominence;
            }
        }

        let strong_single = base_amp >= 0.002 && base_prominence >= 12.0;
        if notches.len() < 2 && !strong_single {
            continue;
        }

        if best
            .as_ref()
            .is_none_or(|(best_score, _)| score > *best_score)
        {
            best = Some((
                score,
                HumAnalysis {
                    base_hz: base,
                    notches,
                },
            ));
        }
    }

    best.map(|(_, analysis)| analysis)
}

/// Average amplitude at a few offsets around a frequency, used as the local
/// noise reference for the prominence test.
fn reference_level(samples: &[f32], frequency: f32) -> f32 {
    let offsets = [-10.0f32, -6.0, 6.0, 10.0];
    let mut sum = 0.0;
    let mut count = 0;
    for offset in offsets {
        let f = frequency + offset;
        if f > 0.0 && f < FS / 2.0 {
            sum += goertzel_amplitude(samples, f);
            count += 1;
        }
    }
    if count == 0 { 0.0 } else { sum / count as f32 }
}

/// Amplitude of a sinusoid at `frequency`, computed with the Goertzel algorithm.
fn goertzel_amplitude(samples: &[f32], frequency: f32) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }

    let omega = 2.0 * std::f32::consts::PI * frequency / FS;
    let coeff = 2.0 * omega.cos();
    let (mut s1, mut s2) = (0.0f32, 0.0f32);

    for &sample in samples {
        let s0 = sample + coeff * s1 - s2;
        s2 = s1;
        s1 = s0;
    }

    let real = s1 - s2 * omega.cos();
    let imag = s2 * omega.sin();
    (2.0 * (real * real + imag * imag).sqrt()) / samples.len() as f32
}

/// Direct-form II transposed biquad.
#[derive(Debug, Clone, Copy)]
struct Biquad {
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
    z1: f32,
    z2: f32,
}

impl Biquad {
    fn high_pass(fs: f32, frequency: f32, q: f32) -> Self {
        let omega = 2.0 * std::f32::consts::PI * frequency / fs;
        let (sin, cos) = omega.sin_cos();
        let alpha = sin / (2.0 * q);

        let b0 = (1.0 + cos) / 2.0;
        let b1 = -(1.0 + cos);
        let b2 = (1.0 + cos) / 2.0;
        let a0 = 1.0 + alpha;
        let a1 = -2.0 * cos;
        let a2 = 1.0 - alpha;

        Self::normalised(b0, b1, b2, a0, a1, a2)
    }

    fn notch(fs: f32, frequency: f32, q: f32) -> Self {
        let omega = 2.0 * std::f32::consts::PI * frequency / fs;
        let (sin, cos) = omega.sin_cos();
        let alpha = sin / (2.0 * q);

        let b0 = 1.0;
        let b1 = -2.0 * cos;
        let b2 = 1.0;
        let a0 = 1.0 + alpha;
        let a1 = -2.0 * cos;
        let a2 = 1.0 - alpha;

        Self::normalised(b0, b1, b2, a0, a1, a2)
    }

    fn normalised(b0: f32, b1: f32, b2: f32, a0: f32, a1: f32, a2: f32) -> Self {
        Self {
            b0: b0 / a0,
            b1: b1 / a0,
            b2: b2 / a0,
            a1: a1 / a0,
            a2: a2 / a0,
            z1: 0.0,
            z2: 0.0,
        }
    }

    #[inline]
    fn process(&mut self, input: f32) -> f32 {
        let output = self.b0 * input + self.z1;
        self.z1 = self.b1 * input - self.a1 * output + self.z2;
        self.z2 = self.b2 * input - self.a2 * output;
        output
    }

    fn run(&mut self, samples: &mut [f32]) {
        for sample in samples {
            *sample = self.process(*sample);
        }
    }
}

// ---------------------------------------------------------------------------
// Spectral denoising
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy)]
enum DenoiseStrength {
    Auto,
    Strong,
}

/// Reduce stationary noise with a smoothed Wiener-style spectral gain.
///
/// Returns false when the buffer is too short to process. The attenuation is
/// floored (8 dB in auto mode, 14 dB in strong mode), so speech is never gated
/// to zero and no words can disappear.
fn spectral_denoise(samples: &mut [f32], strength: DenoiseStrength) -> bool {
    if samples.len() < FFT_SIZE {
        return false;
    }

    let (oversubtraction, floor_db) = match strength {
        DenoiseStrength::Auto => (1.2f32, -8.0f32),
        DenoiseStrength::Strong => (2.0f32, -14.0f32),
    };
    let floor = 10f32.powf(floor_db / 20.0);
    let bins = FFT_SIZE / 2 + 1;

    let window: Vec<f32> = (0..FFT_SIZE)
        .map(|i| 0.5 - 0.5 * (2.0 * std::f32::consts::PI * i as f32 / FFT_SIZE as f32).cos())
        .collect();

    let frames = samples.len().div_ceil(FFT_HOP);
    let output_len = (frames - 1) * FFT_HOP + FFT_SIZE;
    let mut accumulated = vec![0.0f32; output_len];
    let mut window_sum = vec![0.0f32; output_len];

    let mut noise = vec![0.0f32; bins];
    let mut block_min = vec![f32::INFINITY; bins];
    let mut noise_ring = vec![f32::INFINITY; bins * NOISE_BLOCKS];
    let mut ring_pos = 0usize;
    let mut block_frames = 0usize;
    let mut initiated = false;

    let mut raw_gain = vec![1.0f32; bins];
    let mut frequency_smoothed = vec![1.0f32; bins];
    let mut smooth_gain = vec![1.0f32; bins];
    let mut spectrum_gain = vec![1.0f32; FFT_SIZE];

    let mut real = vec![0.0f32; FFT_SIZE];
    let mut imaginary = vec![0.0f32; FFT_SIZE];
    let mut power = vec![0.0f32; bins];

    for frame in 0..frames {
        let start = frame * FFT_HOP;

        for i in 0..FFT_SIZE {
            let sample = samples.get(start + i).copied().unwrap_or(0.0);
            real[i] = sample * window[i];
            imaginary[i] = 0.0;
        }

        fft(&mut real, &mut imaginary, false);

        for bin in 0..bins {
            power[bin] = real[bin] * real[bin] + imaginary[bin] * imaginary[bin];
        }

        if !initiated {
            noise.copy_from_slice(&power);
            initiated = true;
        }

        for bin in 0..bins {
            block_min[bin] = block_min[bin].min(power[bin]);
        }
        block_frames += 1;
        if block_frames >= NOISE_BLOCK_FRAMES {
            for bin in 0..bins {
                noise_ring[ring_pos * bins + bin] = block_min[bin];
                block_min[bin] = f32::INFINITY;
            }
            ring_pos = (ring_pos + 1) % NOISE_BLOCKS;
            block_frames = 0;

            for bin in 0..bins {
                let mut minimum = f32::INFINITY;
                for block in 0..NOISE_BLOCKS {
                    minimum = minimum.min(noise_ring[block * bins + bin]);
                }
                if minimum.is_finite() {
                    noise[bin] = minimum;
                }
            }
        }

        for bin in 0..bins {
            let p = power[bin].max(1e-12);
            let n = noise[bin].max(1e-12);
            let clean = (p - oversubtraction * n).max(0.0);
            raw_gain[bin] = (clean / p).sqrt().max(floor);
        }

        // Smooth across frequency, then over time, to avoid musical noise.
        for bin in 0..bins {
            let left = raw_gain[bin.saturating_sub(1)];
            let right = raw_gain[(bin + 1).min(bins - 1)];
            frequency_smoothed[bin] = (left + 2.0 * raw_gain[bin] + right) * 0.25;
        }
        for bin in 0..bins {
            smooth_gain[bin] = 0.6 * smooth_gain[bin] + 0.4 * frequency_smoothed[bin];
            spectrum_gain[bin] = smooth_gain[bin];
        }
        for bin in 1..FFT_SIZE / 2 {
            spectrum_gain[FFT_SIZE - bin] = spectrum_gain[bin];
        }

        for i in 0..FFT_SIZE {
            real[i] *= spectrum_gain[i];
            imaginary[i] *= spectrum_gain[i];
        }

        fft(&mut real, &mut imaginary, true);

        for i in 0..FFT_SIZE {
            let index = start + i;
            if index >= output_len {
                break;
            }
            accumulated[index] += real[i] * window[i];
            window_sum[index] += window[i] * window[i];
        }
    }

    for (index, sample) in samples.iter_mut().enumerate() {
        let normalisation = window_sum[index];
        if normalisation > 1e-3 {
            *sample = accumulated[index] / normalisation;
        }
        // At the very first and last samples the Hann window sum is near zero.
        // Keeping the original sample there avoids a click at the edges.
    }

    true
}

/// In-place iterative radix-2 FFT. `inverse` also applies the 1/N scaling.
fn fft(real: &mut [f32], imaginary: &mut [f32], inverse: bool) {
    let n = real.len();
    debug_assert_eq!(n, imaginary.len());
    if n <= 1 {
        return;
    }
    debug_assert!(n.is_power_of_two());

    let mut j = 0usize;
    for i in 1..n {
        let mut bit = n >> 1;
        while j & bit != 0 {
            j ^= bit;
            bit >>= 1;
        }
        j ^= bit;
        if i < j {
            real.swap(i, j);
            imaginary.swap(i, j);
        }
    }

    let sign = if inverse { 1.0 } else { -1.0 };
    let mut length = 2usize;
    while length <= n {
        let angle = sign * 2.0 * std::f32::consts::PI / length as f32;
        let (step_imag, step_real) = angle.sin_cos();
        let half = length / 2;

        for start in (0..n).step_by(length) {
            let (mut twiddle_real, mut twiddle_imag) = (1.0f32, 0.0f32);
            for offset in 0..half {
                let even = start + offset;
                let odd = even + half;

                let odd_real = real[odd] * twiddle_real - imaginary[odd] * twiddle_imag;
                let odd_imag = real[odd] * twiddle_imag + imaginary[odd] * twiddle_real;

                let even_real = real[even];
                let even_imag = imaginary[even];

                real[even] = even_real + odd_real;
                imaginary[even] = even_imag + odd_imag;
                real[odd] = even_real - odd_real;
                imaginary[odd] = even_imag - odd_imag;

                let next_real = twiddle_real * step_real - twiddle_imag * step_imag;
                twiddle_imag = twiddle_real * step_imag + twiddle_imag * step_real;
                twiddle_real = next_real;
            }
        }

        length <<= 1;
    }

    if inverse {
        let scale = 1.0 / n as f32;
        for value in real.iter_mut() {
            *value *= scale;
        }
        for value in imaginary.iter_mut() {
            *value *= scale;
        }
    }
}

// ---------------------------------------------------------------------------
// Progressive and realtime integration
// ---------------------------------------------------------------------------

/// Wrap a progressive chunk stream with the cleanup chain.
///
/// Returns the processed stream and a handle with the aggregated quality. Chunks
/// that contain no usable audio are dropped instead of being sent to the model.
pub fn enhance_chunk_stream(
    mut rx: mpsc::UnboundedReceiver<ProgressiveChunk>,
    config: EnhanceConfig,
) -> (
    mpsc::UnboundedReceiver<ProgressiveChunk>,
    Arc<Mutex<AudioQuality>>,
) {
    let (tx, out_rx) = mpsc::unbounded_channel();
    let quality = Arc::new(Mutex::new(AudioQuality::default()));
    let quality_handle = Arc::clone(&quality);

    tokio::spawn(async move {
        while let Some(chunk) = rx.recv().await {
            let enhanced = enhance_owned(chunk.samples, &config);
            quality_handle.lock().unwrap().add(&enhanced.stats);
            if enhanced.stats.gated {
                continue;
            }

            let processed = ProgressiveChunk {
                index: chunk.index,
                samples: enhanced.samples,
                has_leading_overlap: chunk.has_leading_overlap,
            };
            if tx.send(processed).is_err() {
                break;
            }
        }
    });

    (out_rx, quality)
}

/// Wrap a realtime block stream with the stateful cleanup chain.
///
/// Realtime audio is processed block by block, so no denoising or hum detection
/// is applied: only the stateful high-pass and a slow level normalisation. No
/// block is ever dropped.
pub fn enhance_audio_stream(
    rx: mpsc::UnboundedReceiver<Vec<f32>>,
    config: EnhanceConfig,
) -> mpsc::UnboundedReceiver<Vec<f32>> {
    let (tx, out_rx) = mpsc::unbounded_channel();

    if !config.mode.uses_basic_chain() {
        tokio::spawn(async move {
            let mut rx = rx;
            while let Some(block) = rx.recv().await {
                if tx.send(block).is_err() {
                    break;
                }
            }
        });
        return out_rx;
    }

    let mut enhancer = StreamEnhancer::new();
    tokio::spawn(async move {
        let mut rx = rx;
        while let Some(mut block) = rx.recv().await {
            enhancer.process(&mut block);
            if tx.send(block).is_err() {
                break;
            }
        }
    });

    out_rx
}

/// Stateful, block-oriented variant for realtime providers.
pub struct StreamEnhancer {
    high_pass: Biquad,
    levels: VecDeque<f32>,
    gain_db: f32,
}

impl StreamEnhancer {
    pub fn new() -> Self {
        Self {
            high_pass: Biquad::high_pass(FS, HIGH_PASS_HZ, 0.707),
            levels: VecDeque::with_capacity(64),
            gain_db: 0.0,
        }
    }

    pub fn process(&mut self, samples: &mut [f32]) {
        self.high_pass.run(samples);

        let level = to_db(rms_of(samples));
        if level > -119.0 {
            self.levels.push_back(level);
            while self.levels.len() > 64 {
                self.levels.pop_front();
            }
        }

        if self.levels.is_empty() {
            return;
        }

        let mut sorted: Vec<f32> = self.levels.iter().copied().collect();
        sorted.sort_by(|a, b| a.total_cmp(b));
        let speech = percentile_sorted(&sorted, 0.90);
        let target = (TARGET_SPEECH_DBFS - speech).clamp(0.0, MAX_GAIN_DB);
        self.gain_db += 0.05 * (target - self.gain_db);

        let peak = peak_of(samples);
        if peak <= f32::MIN_POSITIVE {
            return;
        }

        let limit_db = 20.0 * (PEAK_CEILING / peak).log10();
        let applied = self.gain_db.min(limit_db).max(0.0);
        if applied > 0.0 {
            let ratio = 10f32.powf(applied / 20.0);
            for sample in samples {
                *sample *= ratio;
            }
        }
    }
}

impl Default for StreamEnhancer {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: f32 = 16_000.0;

    fn sine(frequency: f32, amplitude: f32, seconds: f32) -> Vec<f32> {
        let count = (RATE * seconds) as usize;
        (0..count)
            .map(|i| amplitude * (2.0 * std::f32::consts::PI * frequency * i as f32 / RATE).sin())
            .collect()
    }

    fn light() -> EnhanceConfig {
        EnhanceConfig {
            mode: EnhanceMode::Light,
        }
    }

    fn amplitude_at(samples: &[f32], frequency: f32) -> f32 {
        goertzel_amplitude(samples, frequency)
    }

    #[test]
    fn silence_is_gated() {
        let enhanced = enhance(&vec![0.0; RATE as usize], &light());
        assert!(enhanced.stats.gated);
        assert!(enhanced.samples.iter().all(|s| *s == 0.0));
    }

    #[test]
    fn dc_offset_is_removed() {
        let mut samples = sine(440.0, 0.2, 1.0);
        for sample in &mut samples {
            *sample += 0.3;
        }

        let enhanced = enhance(&samples, &light());
        let mean: f32 = enhanced.samples.iter().sum::<f32>() / enhanced.samples.len() as f32;
        assert!(mean.abs() < 0.01, "mean after high-pass: {mean}");
    }

    #[test]
    fn hum_is_notched_without_touching_speech() {
        let mut fixture: Vec<f32> = sine(1000.0, 0.3, 2.0);
        for (index, sample) in fixture.iter_mut().enumerate() {
            *sample += 0.05 * (2.0 * std::f32::consts::PI * 50.0 * index as f32 / RATE).sin();
        }

        let before = amplitude_at(&fixture, 50.0);
        let enhanced = enhance(&fixture, &light());
        let after = amplitude_at(&enhanced.samples, 50.0);
        let tone_before = amplitude_at(&fixture, 1000.0);
        let tone_after = amplitude_at(&enhanced.samples, 1000.0);

        assert!(after < before * 0.1, "hum before {before}, after {after}");
        assert!(
            (tone_after - tone_before).abs() < tone_before * 0.05,
            "tone changed: {tone_before} -> {tone_after}"
        );
    }

    #[test]
    fn quiet_speech_is_amplified_without_clipping() {
        let samples = sine(440.0, 0.01, 2.0);
        let enhanced = enhance(&samples, &light());

        assert!(enhanced.stats.gain_db > 10.0);
        assert!(peak_of(&enhanced.samples) <= PEAK_CEILING + 1e-4);
        assert!(amplitude_at(&enhanced.samples, 440.0) > amplitude_at(&samples, 440.0));
    }

    #[test]
    fn loud_speech_is_not_reduced() {
        let samples = sine(440.0, 0.6, 2.0);
        let enhanced = enhance(&samples, &light());

        assert_eq!(enhanced.stats.gain_db, 0.0);
        assert!(
            (amplitude_at(&enhanced.samples, 440.0) - amplitude_at(&samples, 440.0)).abs() < 0.01
        );
    }

    #[test]
    fn auto_mode_skips_clean_audio() {
        let samples = sine(440.0, 0.2, 2.0);
        let enhanced = enhance(
            &samples,
            &EnhanceConfig {
                mode: EnhanceMode::AutoDenoise,
            },
        );

        assert!(!enhanced.stats.denoised);
    }

    #[test]
    fn auto_mode_denoises_noisy_audio_with_pauses() {
        let mut samples = Vec::with_capacity(RATE as usize * 3);
        let mut state = 0x9e37_79b9u32;

        // Alternating speech-like bursts and noise-only pauses.
        for block in 0..6 {
            for index in 0..(RATE as usize / 2) {
                state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                let noise = ((state >> 8) as f32 / 8_388_608.0 - 1.0) * 0.04;
                let speech = if block % 2 == 0 {
                    0.15 * (2.0 * std::f32::consts::PI * 440.0 * index as f32 / RATE).sin()
                } else {
                    0.0
                };
                samples.push(speech + noise);
            }
        }

        let enhanced = enhance(
            &samples,
            &EnhanceConfig {
                mode: EnhanceMode::AutoDenoise,
            },
        );

        assert!(enhanced.stats.denoised, "expected auto denoise to trigger");
    }

    #[test]
    fn strong_mode_preserves_length_and_level() {
        let samples = sine(440.0, 0.2, 2.0);
        let enhanced = enhance(
            &samples,
            &EnhanceConfig {
                mode: EnhanceMode::StrongDenoise,
            },
        );

        assert_eq!(enhanced.samples.len(), samples.len());
        assert!(enhanced.stats.denoised);
        assert!(peak_of(&enhanced.samples) > 0.05);
    }

    #[test]
    fn denoise_reduces_stationary_noise() {
        let mut samples = sine(1000.0, 0.15, 3.0);
        let mut state = 0x1234_5678u32;
        for sample in &mut samples {
            // Deterministic pseudo-noise around -40 dBFS.
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let noise = ((state >> 8) as f32 / 8_388_608.0 - 1.0) * 0.01;
            *sample += noise;
        }

        let enhanced = enhance(
            &samples,
            &EnhanceConfig {
                mode: EnhanceMode::StrongDenoise,
            },
        );

        assert!(enhanced.stats.denoised);
        assert_eq!(enhanced.samples.len(), samples.len());
    }

    #[test]
    fn fft_roundtrip_is_lossless() {
        let mut real: Vec<f32> = (0..FFT_SIZE)
            .map(|i| (i as f32 * 0.1).sin() + 0.3 * (i as f32 * 0.7).cos())
            .collect();
        let original = real.clone();
        let mut imaginary = vec![0.0f32; FFT_SIZE];

        fft(&mut real, &mut imaginary, false);
        fft(&mut real, &mut imaginary, true);

        for (a, b) in real.iter().zip(original.iter()) {
            assert!((a - b).abs() < 1e-4, "fft roundtrip mismatch: {a} vs {b}");
        }
    }

    #[test]
    fn quality_aggregates_by_duration() {
        let mut quality = AudioQuality::default();
        quality.add(&EnhanceStats {
            snr_db: 10.0,
            speech_dbfs: -20.0,
            peak_dbfs: -10.0,
            duration_secs: 2.0,
            ..Default::default()
        });
        quality.add(&EnhanceStats {
            snr_db: 20.0,
            speech_dbfs: -20.0,
            peak_dbfs: -10.0,
            duration_secs: 6.0,
            ..Default::default()
        });

        assert!((quality.snr_db - 17.5).abs() < 0.01);
        assert_eq!(quality.chunks, 2);
    }

    #[test]
    fn noisy_quality_recommends_denoise() {
        let mut quality = AudioQuality::default();
        quality.add(&EnhanceStats {
            snr_db: 9.0,
            speech_dbfs: -25.0,
            duration_secs: 4.0,
            ..Default::default()
        });
        assert!(quality.is_noisy());

        quality.denoised = true;
        assert!(!quality.is_noisy());
    }

    #[test]
    fn steady_signal_does_not_recommend_denoise() {
        let mut quality = AudioQuality::default();
        quality.add(&EnhanceStats {
            snr_db: 0.0,
            speech_dbfs: -25.0,
            duration_secs: 4.0,
            ..Default::default()
        });
        assert!(!quality.is_noisy());
    }

    #[test]
    fn silence_does_not_recommend_denoise() {
        let mut quality = AudioQuality::default();
        quality.add(&EnhanceStats {
            snr_db: 0.0,
            speech_dbfs: -120.0,
            duration_secs: 1.0,
            ..Default::default()
        });
        assert!(!quality.is_noisy());
    }
}
