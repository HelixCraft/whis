//! Classical single-channel speech denoisers.
//!
//! All three share one STFT front end and one noise-PSD tracker (a simplified
//! MCRA, minima controlled recursive averaging) and differ only in how the
//! per-bin gain is derived:
//!
//! - [`Method::Subtraction`]: Berouti-style spectral subtraction, the classic
//!   approach from 1979. Cheap and easy to hear, but it leaves musical noise.
//! - [`Method::Wiener`]: Wiener gain driven by a decision-directed a priori SNR
//!   estimate, the 1984 Ephraim-Malah baseline.
//! - [`Method::MmseLsa`]: Ephraim-Malah log-spectral-amplitude estimator, which
//!   adds a correction term on top of the Wiener gain and is noticeably better
//!   at keeping quiet consonants intact.

use super::DenoiseStrength;

/// 32 ms analysis window at 16 kHz.
const FFT_SIZE: usize = 512;
/// 75 percent overlap keeps the overlap-add artifacts below audibility.
const HOP: usize = FFT_SIZE / 4;
const BINS: usize = FFT_SIZE / 2 + 1;

/// Floor for the a priori SNR, -25 dB as in the original publications.
const XI_MIN: f32 = 0.0032;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Method {
    Subtraction,
    Wiener,
    MmseLsa,
}

#[derive(Debug, Clone, Copy)]
struct Params {
    /// Over-subtraction factor for spectral subtraction.
    oversubtraction: f32,
    /// Spectral floor for spectral subtraction, applied to the power gain.
    beta: f32,
    /// Weight of the previous frame in the decision-directed SNR estimate.
    dd_alpha: f32,
    /// Hard limit on how much a bin may be attenuated.
    gain_floor_db: f32,
}

impl Method {
    fn params(self, strength: DenoiseStrength) -> Params {
        match (self, strength) {
            (Method::Subtraction, DenoiseStrength::Balanced) => Params {
                oversubtraction: 2.2,
                beta: 0.04,
                dd_alpha: 0.0,
                gain_floor_db: -18.0,
            },
            (Method::Subtraction, DenoiseStrength::Strong) => Params {
                oversubtraction: 4.0,
                beta: 0.01,
                dd_alpha: 0.0,
                gain_floor_db: -32.0,
            },
            (Method::Wiener, DenoiseStrength::Balanced) => Params {
                oversubtraction: 1.0,
                beta: 0.0,
                dd_alpha: 0.98,
                gain_floor_db: -18.0,
            },
            (Method::Wiener, DenoiseStrength::Strong) => Params {
                oversubtraction: 1.0,
                beta: 0.0,
                dd_alpha: 0.96,
                gain_floor_db: -32.0,
            },
            (Method::MmseLsa, DenoiseStrength::Balanced) => Params {
                oversubtraction: 1.0,
                beta: 0.0,
                dd_alpha: 0.98,
                gain_floor_db: -18.0,
            },
            (Method::MmseLsa, DenoiseStrength::Strong) => Params {
                oversubtraction: 1.0,
                beta: 0.0,
                dd_alpha: 0.96,
                gain_floor_db: -32.0,
            },
        }
    }
}

/// Denoise in place. Returns false when the buffer is too short to transform.
pub(super) fn denoise(samples: &mut [f32], method: Method, strength: DenoiseStrength) -> bool {
    if samples.len() < FFT_SIZE {
        return false;
    }

    let params = method.params(strength);
    let gain_floor = 10f32.powf(params.gain_floor_db / 20.0);

    let window: Vec<f32> = (0..FFT_SIZE)
        .map(|i| 0.5 - 0.5 * (2.0 * std::f32::consts::PI * i as f32 / FFT_SIZE as f32).cos())
        .collect();

    // One extra frame so the tail is reconstructed instead of being left dry.
    let frames = samples.len().div_ceil(HOP) + 1;
    let output_len = (frames - 1) * HOP + FFT_SIZE;
    let mut accumulated = vec![0.0f32; output_len];
    let mut window_sum = vec![0.0f32; output_len];

    let mut tracker = NoiseTracker::new(BINS);
    let mut power = vec![0.0f32; BINS];
    let mut gain = vec![1.0f32; BINS];
    let mut previous_gain = vec![1.0f32; BINS];
    let mut previous_gamma = vec![1.0f32; BINS];
    let mut smoothed_gain = vec![1.0f32; BINS];
    let mut spectrum_gain = vec![1.0f32; FFT_SIZE];
    let mut real = vec![0.0f32; FFT_SIZE];
    let mut imaginary = vec![0.0f32; FFT_SIZE];

    for frame in 0..frames {
        let start = frame * HOP;
        for i in 0..FFT_SIZE {
            let sample = samples.get(start + i).copied().unwrap_or(0.0);
            real[i] = sample * window[i];
            imaginary[i] = 0.0;
        }

        fft(&mut real, &mut imaginary, false);

        for bin in 0..BINS {
            power[bin] = real[bin] * real[bin] + imaginary[bin] * imaginary[bin];
        }

        tracker.update(&power);

        for bin in 0..BINS {
            let gamma = power[bin] / tracker.noise[bin].max(1e-20);
            let gamma = gamma.max(1e-6);

            let a_priori = if method == Method::Subtraction {
                (gamma - 1.0).max(0.0)
            } else {
                (params.dd_alpha * previous_gain[bin] * previous_gain[bin] * previous_gamma[bin]
                    + (1.0 - params.dd_alpha) * (gamma - 1.0).max(0.0))
                .max(XI_MIN)
            };

            let raw = match method {
                Method::Subtraction => (1.0 - params.oversubtraction / gamma)
                    .max(params.beta)
                    .sqrt(),
                Method::Wiener => a_priori / (1.0 + a_priori),
                Method::MmseLsa => {
                    let nu = a_priori * gamma / (1.0 + a_priori);
                    let correction = (0.5 * exp_int1(nu)).exp();
                    (a_priori / (1.0 + a_priori)) * correction
                }
            };

            gain[bin] = raw.clamp(gain_floor, 1.0);
            previous_gamma[bin] = gamma;
            previous_gain[bin] = gain[bin];
        }

        // Frequency smoothing removes isolated bins that would otherwise be
        // heard as musical noise.
        for bin in 0..BINS {
            let left = gain[bin.saturating_sub(1)];
            let right = gain[(bin + 1).min(BINS - 1)];
            smoothed_gain[bin] = 0.25 * left + 0.5 * gain[bin] + 0.25 * right;
        }

        // Smooth the gain over time, but only slowly where no speech is
        // present: fast enough to keep onsets, slow enough to hide noise.
        for bin in 0..BINS {
            let alpha = 0.9 - 0.6 * tracker.speech_probability[bin];
            let smoothed = alpha * spectrum_gain[bin] + (1.0 - alpha) * smoothed_gain[bin];
            spectrum_gain[bin] = smoothed.clamp(gain_floor, 1.0);
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
        // Where the window sum is close to zero the original sample is kept,
        // which avoids a fade at the very first and last samples.
    }

    true
}

/// In-place iterative radix-2 FFT. `inverse` also applies the 1/N scaling.
fn fft(real: &mut [f32], imaginary: &mut [f32], inverse: bool) {
    let n = real.len();
    if n <= 1 {
        return;
    }
    debug_assert_eq!(n, imaginary.len());
    debug_assert!(n.is_power_of_two());

    let mut j = 0usize;
    for i in 1..n {
        let mut bit = n >> 1;
        while j & bit != 0 {
            j ^= bit;
            bit >>= 1;
        }
        j |= bit;
        if i < j {
            real.swap(i, j);
            imaginary.swap(i, j);
        }
    }

    let mut length = 2usize;
    while length <= n {
        let angle = 2.0 * std::f32::consts::PI / length as f32 * if inverse { 1.0 } else { -1.0 };
        let (sin, cos) = angle.sin_cos();
        let mut start = 0usize;
        while start < n {
            let mut w_real = 1.0f32;
            let mut w_imaginary = 0.0f32;
            for k in 0..length / 2 {
                let even = start + k;
                let odd = even + length / 2;
                let odd_real = real[odd] * w_real - imaginary[odd] * w_imaginary;
                let odd_imaginary = real[odd] * w_imaginary + imaginary[odd] * w_real;
                real[odd] = real[even] - odd_real;
                imaginary[odd] = imaginary[even] - odd_imaginary;
                real[even] += odd_real;
                imaginary[even] += odd_imaginary;
                let next_real = w_real * cos - w_imaginary * sin;
                w_imaginary = w_real * sin + w_imaginary * cos;
                w_real = next_real;
            }
            start += length;
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

/// Exponential integral E1, needed by the log-spectral-amplitude gain.
///
/// A series is used for small arguments and a continued fraction for larger
/// ones; both are evaluated in `f64` so the result stays accurate near zero,
/// where the gain correction is largest.
fn exp_int1(x: f32) -> f32 {
    if !x.is_finite() || x <= 0.0 {
        // E1 tends to infinity as x approaches zero; the gain clamps it anyway.
        return 30.0;
    }
    let x = x as f64;
    if x < 1.0 {
        let mut sum = 0.0f64;
        let mut term = 1.0f64;
        for n in 1..=40 {
            term *= -x / n as f64;
            sum += term / n as f64;
        }
        (-std::f64::consts::EULER_GAMMA - x.ln() - sum) as f32
    } else {
        let mut b = x + 1.0;
        let mut c = 1e30f64;
        let mut d = 1.0 / b;
        let mut h = d;
        for i in 1..=100 {
            let a = -(i as f64) * (i as f64);
            b += 2.0;
            d = 1.0 / (a * d + b);
            c = b + a / c;
            let delta = d * c;
            h *= delta;
            if (delta - 1.0).abs() < 1e-12 {
                break;
            }
        }
        ((-x).exp() * h) as f32
    }
}

/// Minima-controlled recursive averaging noise tracker (Cohen, 2001).
///
/// Keeps a recursive noise PSD plus a speech-presence probability per bin. The
/// probability slows the noise update while speech is present, which is what
/// keeps a continuous voice from being learned as noise.
struct NoiseTracker {
    /// Fast recursive power average.
    smooth: Vec<f32>,
    /// `smooth` after a short frequency average.
    smooth_frequency: Vec<f32>,
    /// Minimum of `smooth` inside the current sub-window.
    block_minimum: Vec<f32>,
    /// Minima of the completed sub-windows, used as a ring buffer.
    ring: Vec<f32>,
    ring_position: usize,
    block_frames: usize,
    /// Estimated noise PSD per bin.
    noise: Vec<f32>,
    /// Smoothed speech presence probability per bin.
    speech_probability: Vec<f32>,
    initiated: bool,
}

const SMOOTH_ALPHA: f32 = 0.8;
const SUBWINDOW_FRAMES: usize = 8;
const SUBWINDOWS: usize = 8;
const MINIMUM_BIAS: f32 = 1.5;
const GAMMA_HIGH: f32 = 4.6;
const GAMMA_LOW: f32 = 1.67;
const PRESENCE_ALPHA: f32 = 0.2;
const NOISE_ALPHA: f32 = 0.85;

impl NoiseTracker {
    fn new(bins: usize) -> Self {
        Self {
            smooth: vec![0.0; bins],
            smooth_frequency: vec![0.0; bins],
            block_minimum: vec![f32::INFINITY; bins],
            ring: vec![f32::INFINITY; bins * SUBWINDOWS],
            ring_position: 0,
            block_frames: 0,
            noise: vec![1e-20; bins],
            speech_probability: vec![0.0; bins],
            initiated: false,
        }
    }

    fn update(&mut self, power: &[f32]) {
        let bins = self.noise.len();

        if !self.initiated {
            self.smooth.copy_from_slice(power);
            self.noise.copy_from_slice(power);
            for value in self.noise.iter_mut() {
                *value = value.max(1e-20);
            }
            self.initiated = true;
        }

        for (smooth, value) in self.smooth.iter_mut().zip(power) {
            *smooth = SMOOTH_ALPHA * *smooth + (1.0 - SMOOTH_ALPHA) * value;
        }
        for bin in 0..bins {
            let left = self.smooth[bin.saturating_sub(1)];
            let right = self.smooth[(bin + 1).min(bins - 1)];
            self.smooth_frequency[bin] = 0.25 * left + 0.5 * self.smooth[bin] + 0.25 * right;
        }

        for (minimum, smooth) in self.block_minimum.iter_mut().zip(&self.smooth) {
            *minimum = minimum.min(*smooth);
        }
        self.block_frames += 1;
        if self.block_frames >= SUBWINDOW_FRAMES {
            let offset = self.ring_position * bins;
            self.ring[offset..offset + bins].copy_from_slice(&self.block_minimum);
            for value in self.block_minimum.iter_mut() {
                *value = f32::INFINITY;
            }
            self.ring_position = (self.ring_position + 1) % SUBWINDOWS;
            self.block_frames = 0;
        }

        for (bin, value) in power.iter().enumerate() {
            let mut minimum = f32::INFINITY;
            for block in 0..SUBWINDOWS {
                minimum = minimum.min(self.ring[block * bins + bin]);
            }
            let minimum = if minimum.is_finite() {
                minimum * MINIMUM_BIAS
            } else {
                self.noise[bin]
            };
            let ratio = self.smooth_frequency[bin] / minimum.max(1e-20);
            let presence = if ratio > GAMMA_HIGH {
                1.0
            } else if ratio < GAMMA_LOW {
                0.0
            } else {
                (ratio - GAMMA_LOW) / (GAMMA_HIGH - GAMMA_LOW)
            };
            self.speech_probability[bin] =
                PRESENCE_ALPHA * self.speech_probability[bin] + (1.0 - PRESENCE_ALPHA) * presence;

            let alpha = NOISE_ALPHA + (1.0 - NOISE_ALPHA) * self.speech_probability[bin];
            self.noise[bin] = (alpha * self.noise[bin] + (1.0 - alpha) * value).max(1e-20);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn white_noise(len: usize, amplitude: f32, seed: u32) -> Vec<f32> {
        let mut state = seed | 1;
        (0..len)
            .map(|_| {
                state ^= state << 13;
                state ^= state >> 17;
                state ^= state << 5;
                (state as f32 / u32::MAX as f32 * 2.0 - 1.0) * amplitude
            })
            .collect()
    }

    fn rms(samples: &[f32]) -> f32 {
        (samples.iter().map(|s| s * s).sum::<f32>() / samples.len().max(1) as f32).sqrt()
    }

    fn db(value: f32) -> f32 {
        20.0 * value.max(1e-9).log10()
    }

    #[test]
    fn exponential_integral_matches_known_values() {
        for (x, expected) in [
            (0.1f32, 1.822_924f32),
            (0.5, 0.559_773_6),
            (1.0, 0.219_383_9),
            (2.0, 0.048_900_5),
            (5.0, 0.001_148_3),
        ] {
            let actual = exp_int1(x);
            assert!(
                (actual - expected).abs() < expected.abs() * 0.02 + 1e-4,
                "E1({x}) = {actual}, expected {expected}"
            );
        }
    }

    #[test]
    fn fft_round_trip_is_lossless() {
        let original = white_noise(FFT_SIZE, 0.5, 7);
        let mut real = original.clone();
        let mut imaginary = vec![0.0f32; FFT_SIZE];
        fft(&mut real, &mut imaginary, false);
        fft(&mut real, &mut imaginary, true);
        for (a, b) in real.iter().zip(original.iter()) {
            assert!((a - b).abs() < 1e-5, "{a} != {b}");
        }
    }

    #[test]
    fn every_method_reduces_stationary_noise() {
        for method in [Method::Subtraction, Method::Wiener, Method::MmseLsa] {
            for strength in [DenoiseStrength::Balanced, DenoiseStrength::Strong] {
                let mut samples = white_noise(16000, 0.05, 42);
                let before = rms(&samples[4000..]);
                assert!(denoise(&mut samples, method, strength));
                let after = rms(&samples[4000..]);
                let reduction = db(before) - db(after);
                assert!(
                    reduction > 6.0,
                    "{method:?} {strength:?} only reduced noise by {reduction:.1} dB"
                );
            }
        }
    }

    #[test]
    fn output_stays_finite_and_bounded() {
        for method in [Method::Subtraction, Method::Wiener, Method::MmseLsa] {
            let mut samples = white_noise(8000, 0.9, 11);
            denoise(&mut samples, method, DenoiseStrength::Strong);
            assert!(samples.iter().all(|s| s.is_finite()));
            assert!(samples.iter().all(|s| s.abs() < 1.5));
            assert_eq!(samples.len(), 8000);
        }
    }

    #[test]
    fn short_buffer_is_ignored() {
        let mut samples = vec![0.1f32; 100];
        assert!(!denoise(
            &mut samples,
            Method::MmseLsa,
            DenoiseStrength::Balanced
        ));
    }
}
