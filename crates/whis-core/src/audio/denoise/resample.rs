//! 16 kHz to 48 kHz conversion for the neural denoisers.
//!
//! Both RNNoise and DeepFilterNet are trained on 48 kHz audio, so the cleaned
//! chain upsamples before the model and downsamples afterwards. A polyphase
//! Kaiser-windowed sinc is used instead of a generic resampler because it is
//! integer-ratio, allocation-light and has no additional algorithmic delay.

use std::sync::OnceLock;

/// Upsampling factor from the whisper rate to the model rate.
pub(super) const FACTOR: usize = 3;
/// Filter length, a compromise between edge steepness and cost.
const TAPS: usize = 241;
/// Passband edge in cycles per sample at 48 kHz (about 7.9 kHz).
const CUTOFF: f32 = 0.165;
const KAISER_BETA: f32 = 9.0;

fn bessel_i0(x: f32) -> f32 {
    let mut sum = 1.0f32;
    let mut term = 1.0f32;
    let quarter = x * x * 0.25;
    for k in 1..=32 {
        term *= quarter / (k as f32 * k as f32);
        sum += term;
        if term < sum * 1e-9 {
            break;
        }
    }
    sum
}

fn coefficients() -> &'static [f32] {
    static COEFFS: OnceLock<Vec<f32>> = OnceLock::new();
    COEFFS.get_or_init(|| {
        let center = (TAPS - 1) as f32 * 0.5;
        let scale = bessel_i0(KAISER_BETA);
        (0..TAPS)
            .map(|n| {
                let offset = n as f32 - center;
                let normalized = offset / center;
                let window =
                    bessel_i0(KAISER_BETA * (1.0 - normalized * normalized).max(0.0).sqrt())
                        / scale;
                let angle = 2.0 * std::f32::consts::PI * CUTOFF * offset;
                let sinc = if angle.abs() < 1e-6 {
                    1.0
                } else {
                    angle.sin() / angle
                };
                2.0 * CUTOFF * sinc * window
            })
            .collect()
    })
}

/// Upsample 16 kHz to 48 kHz.
pub(super) fn upsample(input: &[f32]) -> Vec<f32> {
    let coeffs = coefficients();
    let pad = TAPS / FACTOR + 1;
    let mut padded = vec![0.0f32; input.len() + 2 * pad];
    padded[pad..pad + input.len()].copy_from_slice(input);

    let start = pad * FACTOR;
    // Inserting zeros divides the level by the factor, so the interpolation
    // filter has to compensate for it.
    let gain = FACTOR as f32;
    let mut output = Vec::with_capacity(input.len() * FACTOR);
    for m in start..start + input.len() * FACTOR {
        let mut accumulator = 0.0f32;
        let mut tap = m % FACTOR;
        while tap < TAPS {
            let index = (m - tap) / FACTOR;
            if index < padded.len() {
                accumulator += padded[index] * coeffs[tap];
            }
            tap += FACTOR;
        }
        output.push(accumulator * gain);
    }
    output
}

/// Downsample 48 kHz to 16 kHz, returning exactly `output_len` samples.
pub(super) fn downsample(input: &[f32], output_len: usize) -> Vec<f32> {
    let coeffs = coefficients();
    let half = (TAPS - 1) / 2;
    let mut output = Vec::with_capacity(output_len);
    for n in 0..output_len {
        let center = (n * FACTOR) as isize;
        let mut accumulator = 0.0f32;
        for (tap, coefficient) in coeffs.iter().enumerate() {
            let index = center + tap as isize - half as isize;
            if index >= 0 && (index as usize) < input.len() {
                accumulator += input[index as usize] * coefficient;
            }
        }
        output.push(accumulator);
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(frequency: f32, seconds: f32) -> Vec<f32> {
        (0..(16000.0 * seconds) as usize)
            .map(|i| (2.0 * std::f32::consts::PI * frequency * i as f32 / 16000.0).sin() * 0.5)
            .collect()
    }

    fn rms(samples: &[f32]) -> f32 {
        (samples.iter().map(|s| s * s).sum::<f32>() / samples.len() as f32).sqrt()
    }

    #[test]
    fn round_trip_preserves_speech_band() {
        for frequency in [300.0, 1000.0, 3000.0, 6000.0] {
            let input = sine(frequency, 0.25);
            let output = downsample(&upsample(&input), input.len());
            let ratio = rms(&output[2000..]) / rms(&input[2000..]);
            assert!(
                (0.9..1.1).contains(&ratio),
                "{frequency} Hz lost level: {ratio}"
            );
        }
    }

    #[test]
    fn round_trip_removes_images() {
        // 16 kHz content must not fold back as 8 kHz mirror images.
        let input = sine(7000.0, 0.25);
        let up = upsample(&input);
        let above_band: f32 = up
            .iter()
            .skip(2000 * FACTOR)
            .map(|s| s * s)
            .sum::<f32>()
            .sqrt();
        let reference: f32 = input.iter().skip(2000).map(|s| s * s).sum::<f32>().sqrt();
        assert!(
            above_band < reference * 3.0,
            "upsampling produced too much energy: {above_band} vs {reference}"
        );
    }

    #[test]
    fn round_trip_length_is_exact() {
        let input = sine(440.0, 0.1);
        assert_eq!(upsample(&input).len(), input.len() * FACTOR);
        assert_eq!(downsample(&input, 1234).len(), 1234);
    }
}
