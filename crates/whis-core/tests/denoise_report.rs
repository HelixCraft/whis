//! Objective comparison harness for the denoise algorithms.

use whis_core::audio::denoise::{ALGORITHMS, DenoiseStrength, denoise};
use whis_core::audio::enhance::{self, EnhanceConfig, EnhanceMode};

const RATE: f32 = 16000.0;

fn speech(len: usize) -> Vec<f32> {
    let mut state = 0x9e37_79b9u32;
    (0..len)
        .map(|index| {
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let time = index as f32 / RATE;
            let active = (time * 2.3).fract() < 0.55;
            if !active {
                return 0.0;
            }
            let pitch = 125.0 + 20.0 * (2.0 * std::f32::consts::PI * 2.7 * time).sin();
            let mut value = 0.0;
            for harmonic in 1..24 {
                let frequency = pitch * harmonic as f32;
                if frequency > 7600.0 {
                    break;
                }
                let envelope = (-((frequency - 650.0) / 300.0).powi(2)).exp()
                    + 0.7 * (-((frequency - 1150.0) / 420.0).powi(2)).exp()
                    + 0.4 * (-((frequency - 2500.0) / 600.0).powi(2)).exp()
                    + 0.12;
                value += envelope / harmonic as f32
                    * (2.0 * std::f32::consts::PI * frequency * time).sin();
            }
            let modulation = 0.55 + 0.45 * (2.0 * std::f32::consts::PI * 4.1 * time).sin();
            0.12 * modulation * value
        })
        .collect()
}

fn noise(len: usize, amplitude: f32) -> Vec<f32> {
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

fn rms(x: &[f32]) -> f32 {
    (x.iter().map(|v| v * v).sum::<f32>() / x.len().max(1) as f32).sqrt()
}

fn db(value: f32) -> f32 {
    20.0 * value.max(1e-12).log10()
}

fn scale_invariant_sdr(reference: &[f32], output: &[f32]) -> f32 {
    let dot: f32 = reference.iter().zip(output).map(|(r, o)| r * o).sum();
    let energy: f32 = reference.iter().map(|r| r * r).sum();
    if energy <= 0.0 {
        return 0.0;
    }
    let gain = dot / energy;
    let signal: f32 = reference
        .iter()
        .zip(output)
        .map(|(r, _)| {
            let target = gain * r;
            target * target
        })
        .sum();
    let error: f32 = reference
        .iter()
        .zip(output)
        .map(|(r, o)| {
            let diff = o - gain * r;
            diff * diff
        })
        .sum();
    10.0 * (signal / error.max(1e-20)).log10()
}

#[test]
#[ignore = "report only, run with: cargo test -p whis-core --test denoise_report --release -- --ignored --nocapture"]
fn report() {
    let seconds = 6.0;
    let len = (RATE * seconds) as usize;
    let clean = speech(len);

    for input_snr_db in [15.0f32, 8.0] {
        let signal_level = rms(&clean);
        let target_noise = signal_level * 10f32.powf(-input_snr_db / 20.0) * 3.0;
        let noisy: Vec<f32> = clean
            .iter()
            .zip(noise(len, target_noise))
            .map(|(s, n)| s + n)
            .collect();

        println!("\n=== input SNR about {input_snr_db:.0} dB ===");
        println!(
            "{:<22} {:>10} {:>10} {:>10} {:>8}",
            "algorithm", "noise dB", "speech dB", "SI-SDR", "ms"
        );

        for strength in [DenoiseStrength::Balanced, DenoiseStrength::Strong] {
            for algorithm in ALGORITHMS {
                let mut buffer = noisy.clone();
                let started = std::time::Instant::now();
                denoise(&mut buffer, algorithm, strength);
                let elapsed = started.elapsed().as_secs_f32() * 1000.0;

                // Only frames where the clean signal is exactly zero are real
                // pauses, so the reduction is not mixed with speech.
                let mut pause: Vec<usize> = (0..len).filter(|i| clean[*i].abs() < 1e-9).collect();
                pause.truncate(16000);
                let pause_noisy: Vec<f32> = pause.iter().map(|i| noisy[*i]).collect();
                let pause_out: Vec<f32> = pause.iter().map(|i| buffer[*i]).collect();
                let noise_reduction = db(rms(&pause_out) / rms(&pause_noisy));

                let voice: Vec<usize> = (0..len)
                    .filter(|i| clean[*i].abs() > 1e-6 * signal_level)
                    .collect();
                let voice_noisy: Vec<f32> = voice.iter().map(|i| noisy[*i]).collect();
                let voice_clean: Vec<f32> = voice.iter().map(|i| clean[*i]).collect();
                let voice_out: Vec<f32> = voice.iter().map(|i| buffer[*i]).collect();
                let speech_change = db(rms(&voice_out) / rms(&voice_noisy));
                let sdr = scale_invariant_sdr(&voice_clean, &voice_out);

                println!(
                    "{:<22} {noise_reduction:>10.1} {speech_change:>10.1} {sdr:>10.1} {elapsed:>8.1}",
                    format!("{:?} {:?}", algorithm, strength).replace("DenoiseStrength::", "")
                );
            }
        }

        // The numbers the UI shows: the whole cleanup chain, as the app runs it.
        println!(
            "\n{:<22} {:>12} {:>12} {:>12} {:>8}",
            "chain (strong)", "SNR in", "SNR out", "noise out", "ms"
        );
        for algorithm in ALGORITHMS {
            let started = std::time::Instant::now();
            let enhanced = enhance::enhance(
                &noisy,
                &EnhanceConfig {
                    mode: EnhanceMode::StrongDenoise,
                    algorithm,
                },
            );
            let elapsed = started.elapsed().as_secs_f32() * 1000.0;
            println!(
                "{:<22} {:>12.1} {:>12.1} {:>12.1} {elapsed:>8.1}",
                algorithm.as_str(),
                enhanced.stats.snr_db,
                enhanced.stats.output_snr_db,
                enhanced.stats.output_noise_dbfs,
            );
        }
    }
}
