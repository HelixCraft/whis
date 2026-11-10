//! Neural speech denoisers, both running on the CPU from pure Rust.
//!
//! - [`rnnoise`]: Xiph's RNNoise via the pure-Rust `nnnoiseless` port. A small
//!   recurrent network from 2018, very fast, and clearly audible on stationary
//!   noise.
//! - [`deepfilter`]: DeepFilterNet3 via `deep-filter-rt`. Noticeably cleaner
//!   than RNNoise and the current state of the art for real-time speech
//!   enhancement, at roughly three percent of real time on a laptop CPU.
//!
//! Both models are trained on 48 kHz audio, so the 16 kHz input is upsampled
//! and the result is downsampled back.

use super::DenoiseStrength;
use super::resample;
use ndarray::Array2;

/// Samples the models consume per frame.
const FRAME: usize = 480;

/// RNNoise expects samples in the range of 16-bit PCM, not `[-1, 1]`.
const RNNOISE_SCALE: f32 = 32768.0;

/// DeepFilterNet's normalisation is sensitive to very quiet input, so the
/// signal is brought to a fixed RMS before the model and scaled back after.
const DEEPFILTER_TARGET_RMS: f32 = 0.1;

/// Merge the model output with the input, keeping some of the original signal
/// so the result does not sound over-processed.
fn blend(dry: &[f32], wet: &[f32], wet_weight: f32) -> Vec<f32> {
    dry.iter()
        .zip(wet)
        .map(|(dry, wet)| wet * wet_weight + dry * (1.0 - wet_weight))
        .collect()
}

/// Denoise with RNNoise.
pub(super) fn rnnoise(samples: &mut [f32], strength: DenoiseStrength) -> bool {
    use nnnoiseless::DenoiseState;

    if samples.len() < FRAME {
        return false;
    }

    let wet_weight = match strength {
        DenoiseStrength::Balanced => 0.8,
        DenoiseStrength::Strong => 1.0,
    };

    let upsampled = resample::upsample(samples);
    let mut state = DenoiseState::new();

    // A leading silent frame gives the recurrent state something to settle on,
    // and the matching output frame is dropped again below.
    let frames = (upsampled.len() + FRAME) / FRAME;
    let mut input = vec![0.0f32; frames * FRAME];
    for (target, source) in input[FRAME..].iter_mut().zip(upsampled.iter()) {
        *target = source * RNNOISE_SCALE;
    }

    let mut output = vec![0.0f32; frames * FRAME];
    let mut frame_out = vec![0.0f32; DenoiseState::FRAME_SIZE];
    for frame in 0..frames {
        let start = frame * FRAME;
        let end = start + FRAME;
        state.process_frame(&mut frame_out, &input[start..end]);
        output[start..end].copy_from_slice(&frame_out);
    }

    for value in output.iter_mut() {
        *value /= RNNOISE_SCALE;
    }

    let wet = resample::downsample(&output[FRAME..], samples.len());
    let mixed = blend(samples, &wet, wet_weight);
    samples.copy_from_slice(&mixed);
    true
}

/// Denoise with DeepFilterNet3.
pub(super) fn deepfilter(samples: &mut [f32], strength: DenoiseStrength) -> bool {
    use deep_filter_rt::tract::DfTract;

    if samples.len() < FRAME {
        return false;
    }

    let mut model = DfTract::default();
    // The model already mixes the noisy spectrum back in, which is a cleaner
    // way to cap the attenuation than blending in the time domain.
    model.set_atten_lim(match strength {
        DenoiseStrength::Balanced => 20.0,
        DenoiseStrength::Strong => 40.0,
    });

    let hop = model.hop_size;
    let delay = model.delay();

    let upsampled = resample::upsample(samples);
    let level = (upsampled.iter().map(|s| s * s).sum::<f32>() / upsampled.len() as f32).sqrt();
    let gain = if level > 1e-6 {
        (DEEPFILTER_TARGET_RMS / level).max(1.0)
    } else {
        1.0
    };

    let frames = upsampled.len().div_ceil(hop) + delay.div_ceil(hop) + 1;
    let mut output = Vec::with_capacity(frames * hop);
    let mut noisy = Array2::<f32>::zeros((1, hop));
    let mut enhanced = Array2::<f32>::zeros((1, hop));
    for frame in 0..frames {
        let start = frame * hop;
        for i in 0..hop {
            noisy[[0, i]] = upsampled.get(start + i).copied().unwrap_or(0.0) * gain;
        }
        if model.process(noisy.view(), enhanced.view_mut()).is_err() {
            return false;
        }
        output.extend_from_slice(enhanced.row(0).as_slice().unwrap());
    }

    let wet = resample::downsample(&output[delay..], samples.len());
    for (target, value) in samples.iter_mut().zip(wet) {
        // Undo the analysis gain; the cleanup chain normalises afterwards.
        *target = value / gain;
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn white_noise(len: usize, amplitude: f32) -> Vec<f32> {
        let mut state = 0x2545_f491u32;
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

    #[test]
    fn rnnoise_reduces_noise() {
        let mut samples = white_noise(16000, 0.05);
        let before = rms(&samples[4000..]);
        assert!(rnnoise(&mut samples, DenoiseStrength::Strong));
        let after = rms(&samples[4000..]);
        assert!(after < before * 0.5, "{before} -> {after}");
        assert_eq!(samples.len(), 16000);
    }

    #[test]
    fn deepfilter_reduces_noise() {
        let mut samples = white_noise(16000, 0.05);
        let before = rms(&samples[4000..]);
        assert!(deepfilter(&mut samples, DenoiseStrength::Balanced));
        let after = rms(&samples[4000..]);
        assert!(after < before * 0.5, "{before} -> {after}");
        assert_eq!(samples.len(), 16000);
        assert!(samples.iter().all(|s| s.is_finite()));
    }
}
