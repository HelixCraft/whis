//! Catalog of downloadable local transcription models.
//!
//! Every entry describes where a model comes from, how it is stored on disk,
//! and which engine runs it. The desktop UI and the CLI both derive their model
//! lists from here, so adding a model is a data change rather than code.

use std::path::{Path, PathBuf};

/// Engine that runs a model.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelEngine {
    /// Whisper GGML files via transcribe.cpp.
    WhisperCpp,
    /// ONNX Runtime via transcribe-rs (model directories).
    Onnx,
    /// Other model families (Qwen3-ASR, Moonshine, ...) as GGUF via transcribe.cpp.
    Gguf,
    /// Known model without a supported runtime.
    Unsupported,
}

impl ModelEngine {
    /// Whether this build can run the engine.
    pub fn is_built(&self) -> bool {
        match self {
            Self::WhisperCpp | Self::Onnx | Self::Gguf => cfg!(feature = "local-transcription"),
            Self::Unsupported => false,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::WhisperCpp => "whisper-cpp",
            Self::Onnx => "onnx",
            Self::Gguf => "gguf",
            Self::Unsupported => "unsupported",
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Self::WhisperCpp => "Whisper GGML",
            Self::Onnx => "ONNX",
            Self::Gguf => "GGUF",
            Self::Unsupported => "Not supported",
        }
    }
}

/// On-disk layout of a model.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelFormat {
    /// Single GGML file.
    WhisperGgml,
    /// tar.gz archive containing a model directory.
    OnnxArchive,
    /// Single GGUF file.
    Gguf,
    /// Nothing to download.
    None,
}

impl ModelFormat {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::WhisperGgml => "ggml",
            Self::OnnxArchive => "onnx",
            Self::Gguf => "gguf",
            Self::None => "none",
        }
    }
}

/// One downloadable model.
#[derive(Debug, Clone, Copy)]
pub struct LocalModelSpec {
    /// Stable identifier stored in settings.
    pub id: &'static str,
    /// Human-readable name.
    pub name: &'static str,
    pub engine: ModelEngine,
    pub format: ModelFormat,
    /// Supported languages, human-readable.
    pub languages: &'static str,
    /// Approximate download size.
    pub size_mb: u64,
    pub license: &'static str,
    pub description: &'static str,
    /// Download URL. Empty for models without a runtime.
    pub url: &'static str,
    /// Subdirectory below the models directory.
    pub subdir: &'static str,
    /// File name (files) or directory name (archives).
    pub file_name: &'static str,
    /// Files that must exist inside an archive model directory.
    pub required_files: &'static [&'static str],
    /// Whether this model can be run at all.
    pub supported: bool,
}

impl LocalModelSpec {
    /// Absolute path where the model is stored.
    pub fn path(&self) -> PathBuf {
        let base = models_dir();
        if self.file_name.is_empty() {
            return base.join(self.id);
        }
        if self.subdir.is_empty() {
            base.join(self.file_name)
        } else {
            base.join(self.subdir).join(self.file_name)
        }
    }

    /// Whether this build can download and run the model.
    pub fn is_available(&self) -> bool {
        self.supported && self.engine.is_built()
    }

    /// Check that the model is fully present at the given path.
    pub fn verify(&self, path: &Path) -> bool {
        match self.format {
            ModelFormat::WhisperGgml | ModelFormat::Gguf => {
                path.is_file() && path.metadata().map(|m| m.len() > 0).unwrap_or(false)
            }
            ModelFormat::OnnxArchive => {
                path.is_dir()
                    && self
                        .required_files
                        .iter()
                        .all(|file| path.join(file).is_file())
            }
            ModelFormat::None => false,
        }
    }

    /// Whether the model is installed at its default location.
    pub fn is_installed(&self) -> bool {
        self.supported && self.verify(&self.path())
    }

    pub fn size_label(&self) -> String {
        if self.size_mb >= 1000 {
            format!("{:.1} GB", self.size_mb as f64 / 1000.0)
        } else {
            format!("{} MB", self.size_mb)
        }
    }
}

/// Root directory for downloaded models.
pub fn models_dir() -> PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("whis")
        .join("models")
}

/// Default model for new local configurations.
pub const DEFAULT_MODEL_ID: &str = "parakeet-v3";

const WHISPER_TINY_URL: &str =
    "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-tiny.bin";
const WHISPER_BASE_URL: &str =
    "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-base.bin";
const WHISPER_SMALL_URL: &str =
    "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-small.bin";
const WHISPER_MEDIUM_URL: &str =
    "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-medium.bin";

const PARAKEET_V3_URL: &str = "https://blob.handy.computer/parakeet-v3-int8.tar.gz";
const PARAKEET_V2_URL: &str = "https://blob.handy.computer/parakeet-v2-int8.tar.gz";

const PARAKEET_REQUIRED_FILES: &[&str] = &[
    "encoder-model.int8.onnx",
    "decoder_joint-model.int8.onnx",
    "nemo128.onnx",
    "vocab.txt",
];

const CATALOG: &[LocalModelSpec] = &[
    // Whisper via whisper.cpp
    LocalModelSpec {
        id: "whisper-tiny",
        name: "Whisper Tiny",
        engine: ModelEngine::WhisperCpp,
        format: ModelFormat::WhisperGgml,
        languages: "99 languages",
        size_mb: 75,
        license: "MIT",
        description: "Fastest Whisper model, lowest accuracy.",
        url: WHISPER_TINY_URL,
        subdir: "",
        file_name: "ggml-tiny.bin",
        required_files: &[],
        supported: true,
    },
    LocalModelSpec {
        id: "whisper-base",
        name: "Whisper Base",
        engine: ModelEngine::WhisperCpp,
        format: ModelFormat::WhisperGgml,
        languages: "99 languages",
        size_mb: 142,
        license: "MIT",
        description: "Small and quick, good for clear speech.",
        url: WHISPER_BASE_URL,
        subdir: "",
        file_name: "ggml-base.bin",
        required_files: &[],
        supported: true,
    },
    LocalModelSpec {
        id: "whisper-small",
        name: "Whisper Small",
        engine: ModelEngine::WhisperCpp,
        format: ModelFormat::WhisperGgml,
        languages: "99 languages",
        size_mb: 466,
        license: "MIT",
        description: "Balanced accuracy and speed.",
        url: WHISPER_SMALL_URL,
        subdir: "",
        file_name: "ggml-small.bin",
        required_files: &[],
        supported: true,
    },
    LocalModelSpec {
        id: "whisper-medium",
        name: "Whisper Medium",
        engine: ModelEngine::WhisperCpp,
        format: ModelFormat::WhisperGgml,
        languages: "99 languages",
        size_mb: 1500,
        license: "MIT",
        description: "High accuracy, needs more memory.",
        url: WHISPER_MEDIUM_URL,
        subdir: "",
        file_name: "ggml-medium.bin",
        required_files: &[],
        supported: true,
    },
    // Parakeet via ONNX
    LocalModelSpec {
        id: "parakeet-v3",
        name: "Parakeet TDT 0.6B v3",
        engine: ModelEngine::Onnx,
        format: ModelFormat::OnnxArchive,
        languages: "25 European languages",
        size_mb: 478,
        license: "CC-BY-4.0",
        description: "Fast multilingual model, strong on European languages.",
        url: PARAKEET_V3_URL,
        subdir: "parakeet",
        file_name: "parakeet-tdt-0.6b-v3-int8",
        required_files: PARAKEET_REQUIRED_FILES,
        supported: true,
    },
    LocalModelSpec {
        id: "parakeet-v2",
        name: "Parakeet TDT 0.6B v2",
        engine: ModelEngine::Onnx,
        format: ModelFormat::OnnxArchive,
        languages: "English",
        size_mb: 478,
        license: "CC-BY-4.0",
        description: "English-only predecessor of v3.",
        url: PARAKEET_V2_URL,
        subdir: "parakeet",
        file_name: "parakeet-tdt-0.6b-v2-int8",
        required_files: PARAKEET_REQUIRED_FILES,
        supported: true,
    },
    // GGUF via transcribe.cpp
    LocalModelSpec {
        id: "parakeet-ultra-q4",
        name: "Parakeet Ultra Q4",
        engine: ModelEngine::Gguf,
        format: ModelFormat::Gguf,
        languages: "25 European languages",
        size_mb: 486,
        license: "CC-BY-4.0",
        description: "Quantized Parakeet with low memory use.",
        url: "https://huggingface.co/handy-computer/parakeet-ultra-gguf/resolve/main/parakeet-ultra-Q4_K_M.gguf",
        subdir: "gguf",
        file_name: "parakeet-ultra-Q4_K_M.gguf",
        required_files: &[],
        supported: true,
    },
    LocalModelSpec {
        id: "parakeet-ultra-q8",
        name: "Parakeet Ultra Q8",
        engine: ModelEngine::Gguf,
        format: ModelFormat::Gguf,
        languages: "25 European languages",
        size_mb: 740,
        license: "CC-BY-4.0",
        description: "Higher-precision Parakeet.",
        url: "https://huggingface.co/handy-computer/parakeet-ultra-gguf/resolve/main/parakeet-ultra-Q8_0.gguf",
        subdir: "gguf",
        file_name: "parakeet-ultra-Q8_0.gguf",
        required_files: &[],
        supported: true,
    },
    LocalModelSpec {
        id: "qwen3-asr-0.6b",
        name: "Qwen3-ASR 0.6B",
        engine: ModelEngine::Gguf,
        format: ModelFormat::Gguf,
        languages: "30 languages",
        size_mb: 590,
        license: "Apache-2.0",
        description: "Compact multilingual model with language detection.",
        url: "https://huggingface.co/handy-computer/Qwen3-ASR-0.6B-gguf/resolve/main/Qwen3-ASR-0.6B-Q4_K_M.gguf",
        subdir: "gguf",
        file_name: "Qwen3-ASR-0.6B-Q4_K_M.gguf",
        required_files: &[],
        supported: true,
    },
    LocalModelSpec {
        id: "qwen3-asr-1.7b",
        name: "Qwen3-ASR 1.7B",
        engine: ModelEngine::Gguf,
        format: ModelFormat::Gguf,
        languages: "30 languages",
        size_mb: 1320,
        license: "Apache-2.0",
        description: "Larger multilingual model for difficult audio.",
        url: "https://huggingface.co/handy-computer/Qwen3-ASR-1.7B-gguf/resolve/main/Qwen3-ASR-1.7B-Q4_K_M.gguf",
        subdir: "gguf",
        file_name: "Qwen3-ASR-1.7B-Q4_K_M.gguf",
        required_files: &[],
        supported: true,
    },
    LocalModelSpec {
        id: "moonshine-tiny",
        name: "Moonshine Tiny",
        engine: ModelEngine::Gguf,
        format: ModelFormat::Gguf,
        languages: "English",
        size_mb: 35,
        license: "MIT",
        description: "Very small English model for quick notes.",
        url: "https://huggingface.co/handy-computer/moonshine-tiny-gguf/resolve/main/moonshine-tiny-Q8_0.gguf",
        subdir: "gguf",
        file_name: "moonshine-tiny-Q8_0.gguf",
        required_files: &[],
        supported: true,
    },
    LocalModelSpec {
        id: "moonshine-base",
        name: "Moonshine Base",
        engine: ModelEngine::Gguf,
        format: ModelFormat::Gguf,
        languages: "English",
        size_mb: 77,
        license: "MIT",
        description: "Small English model, better accuracy than Tiny.",
        url: "https://huggingface.co/handy-computer/moonshine-base-gguf/resolve/main/moonshine-base-Q8_0.gguf",
        subdir: "gguf",
        file_name: "moonshine-base-Q8_0.gguf",
        required_files: &[],
        supported: true,
    },
    // Known models without a supported runtime. They stay in the catalog so the
    // UI can explain why they are missing instead of silently ignoring requests.
    LocalModelSpec {
        id: "bark-3.0",
        name: "Underdog Bark 3.0",
        engine: ModelEngine::Unsupported,
        format: ModelFormat::None,
        languages: "English",
        size_mb: 163,
        license: "CC-BY-4.0",
        description: "Speech cleanup model with a custom runtime; it does not transcribe.",
        url: "",
        subdir: "",
        file_name: "",
        required_files: &[],
        supported: false,
    },
    LocalModelSpec {
        id: "phonon-2.5",
        name: "Phonon 2.5",
        engine: ModelEngine::Unsupported,
        format: ModelFormat::None,
        languages: "English",
        size_mb: 156,
        license: "CC-BY-4.0",
        description: "Packed MLX/CoreML model without a desktop runtime.",
        url: "",
        subdir: "",
        file_name: "",
        required_files: &[],
        supported: false,
    },
];

/// All known local models, including unsupported ones.
pub fn catalog() -> &'static [LocalModelSpec] {
    CATALOG
}

/// Look up a model by its stable identifier.
pub fn find(id: &str) -> Option<&'static LocalModelSpec> {
    CATALOG.iter().find(|model| model.id == id)
}

/// Model id for an installed path, when it matches a catalog entry.
pub fn id_for_path(path: &str) -> Option<&'static str> {
    let path = Path::new(path);
    let file_name = path.file_name().and_then(|name| name.to_str());
    CATALOG
        .iter()
        .find(|model| {
            if model.file_name.is_empty() {
                return false;
            }
            // Settings may come from another user or machine, so the file name
            // is enough to recognize a catalog model inside a foreign models dir.
            model.path() == path || file_name == Some(model.file_name)
        })
        .map(|model| model.id)
}

/// Which engine should run the model at this path.
///
/// Used by the transcription dispatch so callers only need the resolved path.
pub fn engine_for_path(path: &str) -> Option<ModelEngine> {
    let path = Path::new(path);
    // Directories are ONNX bundles. Check this before the extension: Parakeet
    // directory names contain dots, so `extension()` would not be empty.
    if path.is_dir() {
        return Some(ModelEngine::Onnx);
    }

    if let Some(ext) = path.extension().and_then(|ext| ext.to_str()) {
        match ext.to_ascii_lowercase().as_str() {
            "gguf" => return Some(ModelEngine::Gguf),
            "bin" => return Some(ModelEngine::WhisperCpp),
            _ => {}
        }
    }

    // Not downloaded yet: fall back to the catalog so the engine is still known.
    CATALOG
        .iter()
        .find(|model| model.path() == path)
        .map(|model| model.engine)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_unique() {
        let mut ids: Vec<_> = catalog().iter().map(|model| model.id).collect();
        let count = ids.len();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), count);
    }

    #[test]
    fn whisper_paths_keep_legacy_layout() {
        let model = find("whisper-small").unwrap();
        assert!(model.path().ends_with("models/ggml-small.bin"));
    }

    #[test]
    fn paths_from_another_data_dir_are_recognized() {
        assert_eq!(
            id_for_path("/home/other/.local/share/whis/models/ggml-small.bin"),
            Some("whisper-small")
        );
        assert_eq!(
            id_for_path("/opt/models/parakeet-tdt-0.6b-v3-int8"),
            Some("parakeet-v3")
        );
        assert_eq!(id_for_path("/opt/models/custom.bin"), None);
    }

    #[test]
    fn engine_is_inferred_from_path() {
        assert_eq!(
            engine_for_path("/models/ggml-small.bin"),
            Some(ModelEngine::WhisperCpp)
        );
        assert_eq!(
            engine_for_path("/models/model.gguf"),
            Some(ModelEngine::Gguf)
        );
        assert_eq!(engine_for_path("/models/unknown-file.txt"), None);
    }

    #[test]
    fn not_yet_downloaded_parakeet_uses_onnx() {
        let model = find("parakeet-v3").unwrap();
        let path = model.path().to_string_lossy().to_string();
        assert!(path.contains('.'));
        assert_eq!(engine_for_path(&path), Some(ModelEngine::Onnx));
    }
}
