//! Noise suppression for speech recordings.
//!
//! The chain in [`super::enhance`] handles the always-safe clean-up (hum
//! notches, high-pass, level normalisation). Everything that can change the
//! character of a recording lives here, behind an explicit choice of
//! algorithm.
//!
//! The algorithms are ordered by strength and cost:
//!
//! 1. [`DenoiseAlgorithm::SpectralSubtraction`] - Berouti spectral
//!    subtraction. Fastest, and the classic sound with some musical noise.
//! 2. [`DenoiseAlgorithm::Wiener`] - decision-directed Wiener gain.
//! 3. [`DenoiseAlgorithm::MmseLsa`] - Ephraim-Malah log-spectral amplitude,
//!    the best of the classical estimators.
//! 4. [`DenoiseAlgorithm::RnNoise`] - RNNoise, a small recurrent network.
//! 5. [`DenoiseAlgorithm::DeepFilter`] - DeepFilterNet3, the current state of
//!    the art for real-time speech enhancement.

mod classical;
mod neural;
mod resample;

use serde::{Deserialize, Serialize};

/// Selectable noise suppression algorithm.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum DenoiseAlgorithm {
    /// Berouti spectral subtraction, the 1979 classic.
    #[serde(rename = "subtraction")]
    SpectralSubtraction,
    /// Wiener gain with a decision-directed a priori SNR.
    #[serde(rename = "wiener")]
    Wiener,
    /// Ephraim-Malah log-spectral amplitude estimator.
    #[default]
    #[serde(rename = "mmse-lsa")]
    MmseLsa,
    /// RNNoise, a small recurrent neural network.
    #[serde(rename = "rnnoise")]
    RnNoise,
    /// DeepFilterNet3, a larger neural network.
    #[serde(rename = "deepfilter")]
    DeepFilter,
}

impl DenoiseAlgorithm {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::SpectralSubtraction => "subtraction",
            Self::Wiener => "wiener",
            Self::MmseLsa => "mmse-lsa",
            Self::RnNoise => "rnnoise",
            Self::DeepFilter => "deepfilter",
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Self::SpectralSubtraction => "Spectral subtraction",
            Self::Wiener => "Wiener filter",
            Self::MmseLsa => "MMSE-LSA",
            Self::RnNoise => "RNNoise",
            Self::DeepFilter => "DeepFilterNet3",
        }
    }

    /// One sentence for the settings page and the CLI help.
    pub fn description(&self) -> &'static str {
        match self {
            Self::SpectralSubtraction => {
                "Classic spectral subtraction. Fastest, but can leave a light ringing on noise."
            }
            Self::Wiener => {
                "Wiener gain with smoother noise tracking. A good balance of speed and quality."
            }
            Self::MmseLsa => {
                "Ephraim-Malah estimator. Best classical result, keeps quiet consonants intact."
            }
            Self::RnNoise => {
                "Small neural network trained on speech. Very fast, clear on steady noise."
            }
            Self::DeepFilter => {
                "DeepFilterNet3. Strongest removal of background noise, slightly more CPU."
            }
        }
    }

    pub fn is_neural(&self) -> bool {
        matches!(self, Self::RnNoise | Self::DeepFilter)
    }
}

/// How hard the selected algorithm works.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DenoiseStrength {
    /// Default; keeps speech natural.
    Balanced,
    /// Deeper attenuation, for very noisy recordings.
    Strong,
}

/// Run one denoiser in place.
///
/// Returns false when the buffer was too short to process, in which case the
/// samples are unchanged.
pub fn denoise(
    samples: &mut [f32],
    algorithm: DenoiseAlgorithm,
    strength: DenoiseStrength,
) -> bool {
    match algorithm {
        DenoiseAlgorithm::SpectralSubtraction => {
            classical::denoise(samples, classical::Method::Subtraction, strength)
        }
        DenoiseAlgorithm::Wiener => {
            classical::denoise(samples, classical::Method::Wiener, strength)
        }
        DenoiseAlgorithm::MmseLsa => {
            classical::denoise(samples, classical::Method::MmseLsa, strength)
        }
        DenoiseAlgorithm::RnNoise => neural::rnnoise(samples, strength),
        DenoiseAlgorithm::DeepFilter => neural::deepfilter(samples, strength),
    }
}

/// All algorithms, in the order they are shown in the settings.
pub const ALGORITHMS: [DenoiseAlgorithm; 5] = [
    DenoiseAlgorithm::SpectralSubtraction,
    DenoiseAlgorithm::Wiener,
    DenoiseAlgorithm::MmseLsa,
    DenoiseAlgorithm::RnNoise,
    DenoiseAlgorithm::DeepFilter,
];
