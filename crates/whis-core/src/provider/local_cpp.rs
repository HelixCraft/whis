//! Local transcription via transcribe.cpp.
//!
//! Covers every GGML-based model whis ships: Whisper (`ggml-*.bin`), Qwen3-ASR,
//! Parakeet Ultra, and Moonshine (`.gguf`). transcribe.cpp detects the model
//! family from the file itself, so callers only pass the path.
//!
//! Like the other local engines, the loaded model is cached so repeated
//! transcriptions do not pay the load cost again.

use anyhow::Result;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use transcribe_cpp::{Model, RunOptions, Session};

use super::TranscriptionResult;

/// Transcribe raw f32 samples directly.
///
/// Use this for local recordings where samples are already 16kHz mono.
///
/// # Arguments
/// * `model_path` - Path to a GGML/GGUF model file (.bin or .gguf)
/// * `samples` - Raw f32 audio samples (must be 16kHz mono)
/// * `language` - Optional language hint (ISO code)
pub fn transcribe_raw(
    model_path: &str,
    samples: &[f32],
    language: Option<&str>,
) -> Result<TranscriptionResult> {
    if samples.is_empty() {
        anyhow::bail!("No audio to transcribe");
    }

    get_or_load_model(model_path)?;

    let mut cache = get_cache().lock().unwrap();
    let cached = cache
        .as_mut()
        .ok_or_else(|| anyhow::anyhow!("Local model not loaded after load request"))?;

    let options = RunOptions {
        language: language.map(str::to_string),
        ..Default::default()
    };

    // transcribe.cpp writes device/scheduler lines straight to stderr.
    let _stderr_guard = stderr_suppression::suppress();
    let transcript = cached
        .session
        .run(samples, &options)
        .map_err(|e| anyhow::anyhow!("Local transcription failed: {e}"))?;
    drop(_stderr_guard);

    let text = transcript.text.trim().to_string();
    drop(cache);

    maybe_unload();

    Ok(TranscriptionResult { text })
}

// ============================================================================
// stderr suppression for native GGML output
// ============================================================================

/// Temporarily suppresses stderr during closure execution.
///
/// The native library writes device detection and scheduler lines directly to
/// stderr, bypassing Rust logging.
mod stderr_suppression {
    /// RAII guard that restores stderr when dropped.
    #[cfg(unix)]
    pub struct StderrGuard {
        saved_fd: i32,
        stderr_fd: i32,
    }

    #[cfg(unix)]
    impl Drop for StderrGuard {
        fn drop(&mut self) {
            unsafe {
                libc::dup2(self.saved_fd, self.stderr_fd);
                libc::close(self.saved_fd);
            }
        }
    }

    #[cfg(unix)]
    pub fn suppress() -> Option<StderrGuard> {
        use std::os::unix::io::AsRawFd;

        let stderr_fd = std::io::stderr().as_raw_fd();
        let saved_fd = unsafe { libc::dup(stderr_fd) };
        if saved_fd == -1 {
            return None;
        }

        let devnull = std::fs::File::open("/dev/null").ok()?;
        let result = unsafe { libc::dup2(devnull.as_raw_fd(), stderr_fd) };
        if result == -1 {
            unsafe { libc::close(saved_fd) };
            return None;
        }

        Some(StderrGuard {
            saved_fd,
            stderr_fd,
        })
    }

    #[cfg(windows)]
    pub struct StderrGuard {
        saved_handle: *mut std::ffi::c_void,
    }

    #[cfg(windows)]
    impl Drop for StderrGuard {
        fn drop(&mut self) {
            const STD_ERROR_HANDLE: u32 = 0xFFFF_FFF4; // -12 as u32
            unsafe extern "system" {
                fn SetStdHandle(nStdHandle: u32, hHandle: *mut std::ffi::c_void) -> i32;
            }
            unsafe {
                SetStdHandle(STD_ERROR_HANDLE, self.saved_handle);
            }
        }
    }

    #[cfg(windows)]
    pub fn suppress() -> Option<StderrGuard> {
        use std::os::windows::io::AsRawHandle;
        const STD_ERROR_HANDLE: u32 = 0xFFFF_FFF4; // -12 as u32

        unsafe extern "system" {
            fn GetStdHandle(nStdHandle: u32) -> *mut std::ffi::c_void;
            fn SetStdHandle(nStdHandle: u32, hHandle: *mut std::ffi::c_void) -> i32;
        }

        let saved_handle = unsafe { GetStdHandle(STD_ERROR_HANDLE) };

        let nul = std::fs::OpenOptions::new().write(true).open("NUL").ok()?;

        let result = unsafe { SetStdHandle(STD_ERROR_HANDLE, nul.as_raw_handle() as *mut _) };
        if result == 0 {
            return None;
        }

        // Keep nul open; the handle is restored when the guard drops.
        std::mem::forget(nul);

        Some(StderrGuard { saved_handle })
    }
}

// ============================================================================
// Engine caching
// ============================================================================

/// Global shared model and its session.
static LOCAL_MODEL: OnceLock<Mutex<Option<CachedLocalModel>>> = OnceLock::new();

/// Keep the model loaded after transcription.
static KEEP_LOADED: AtomicBool = AtomicBool::new(false);

struct CachedLocalModel {
    // The session holds its own handle to the model, so both stay alive here.
    #[allow(dead_code)]
    model: Model,
    session: Session,
    path: String,
}

fn get_cache() -> &'static Mutex<Option<CachedLocalModel>> {
    LOCAL_MODEL.get_or_init(|| Mutex::new(None))
}

fn get_or_load_model(model_path: &str) -> Result<()> {
    use std::path::Path;

    let mut cache = get_cache().lock().unwrap();

    if let Some(ref cached) = *cache
        && cached.path == model_path
    {
        return Ok(());
    }

    if model_path.is_empty() {
        anyhow::bail!("Local model path not configured. Run 'whis setup local'.");
    }

    let path = Path::new(model_path);
    if !path.is_file() {
        anyhow::bail!(
            "Local model not found at: {}\n\
             Download a model using: whis setup local",
            model_path
        );
    }

    crate::verbose!("Loading local model: {}", model_path);

    let _stderr_guard = stderr_suppression::suppress();
    let model =
        Model::load(path).map_err(|e| anyhow::anyhow!("Failed to load local model: {e}"))?;
    let session = model
        .session()
        .map_err(|e| anyhow::anyhow!("Failed to open local model session: {e}"))?;
    drop(_stderr_guard);

    crate::verbose!("Local model loaded (arch: {})", model.arch());

    *cache = Some(CachedLocalModel {
        model,
        session,
        path: model_path.to_string(),
    });

    Ok(())
}

/// Preload a model in the background to reduce first-transcription latency.
pub fn preload_model(model_path: &str) {
    {
        let cache = get_cache().lock().unwrap();
        if let Some(ref cached) = *cache
            && cached.path == model_path
        {
            crate::verbose!("Local model already cached, skipping preload");
            return;
        }
    }

    let model_path = model_path.to_string();
    std::thread::spawn(move || {
        crate::verbose!("Preloading local model: {}", model_path);
        if let Err(e) = get_or_load_model(&model_path) {
            eprintln!("Warning: Failed to preload local model: {e}");
        }
    });
}

/// Set whether to keep the model loaded after transcription.
pub fn set_keep_loaded(keep: bool) {
    KEEP_LOADED.store(keep, Ordering::SeqCst);
    crate::verbose!("Local model keep_loaded set to: {}", keep);
}

pub fn should_keep_loaded() -> bool {
    KEEP_LOADED.load(Ordering::SeqCst)
}

/// Unload the cached model and free its memory.
pub fn unload_model() {
    let mut cache = get_cache().lock().unwrap();
    if cache.is_some() {
        crate::verbose!("Unloading local model from cache");
        *cache = None;
    }
}

fn maybe_unload() {
    if !should_keep_loaded() {
        unload_model();
    }
}
