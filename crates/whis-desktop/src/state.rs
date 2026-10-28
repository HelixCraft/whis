use std::sync::Mutex;
use std::sync::atomic::AtomicBool;
use tauri::menu::MenuItem;
use tokio::sync::oneshot;
pub use whis_core::RecordingState;
use whis_core::{AudioRecorder, Settings, TranscriptionProvider};

#[cfg(target_os = "linux")]
use crate::shortcuts::RdevGrabGuard;

/// Cached transcription configuration (provider + API key + language)
#[derive(Clone)]
pub struct TranscriptionConfig {
    pub provider: TranscriptionProvider,
    pub api_key: String,
    pub language: Option<String>,
}

/// Active model download state (persists across window close/reopen)
#[derive(Clone, Debug)]
pub struct DownloadState {
    pub model_name: String,
    pub model_type: String, // "whisper" or "parakeet"
    pub downloaded: u64,
    pub total: u64,
}

pub struct AppState {
    pub state: Mutex<RecordingState>,
    pub recorder: Mutex<Option<AudioRecorder>>,
    pub transcription_config: Mutex<Option<TranscriptionConfig>>,
    pub record_menu_item: Mutex<Option<MenuItem<tauri::Wry>>>,
    pub settings: Mutex<Settings>,
    /// The actual shortcut binding from the XDG Portal (Wayland only)
    pub portal_shortcut: Mutex<Option<String>>,
    /// Error message if portal shortcut binding failed
    pub portal_bind_error: Mutex<Option<String>>,
    /// Whether system tray is available
    pub tray_available: Mutex<bool>,
    /// Active model download (if any)
    pub active_download: Mutex<Option<DownloadState>>,
    /// Progressive transcription result receiver (if progressive mode active)
    pub transcription_rx: Mutex<Option<oneshot::Receiver<Result<String, String>>>>,
    /// JoinHandle for pending idle model unload task (if any)
    /// Used to cancel the unload when a new recording starts
    pub idle_unload_handle: Mutex<Option<tauri::async_runtime::JoinHandle<()>>>,
    /// True while the last recording is being re-transcribed
    pub retranscribing: AtomicBool,
    /// Guard for rdev::grab() keyboard listener (Linux only)
    #[cfg(target_os = "linux")]
    pub rdev_guard: Mutex<Option<RdevGrabGuard>>,
    /// Error message if rdev grab failed (Linux only)
    #[cfg(target_os = "linux")]
    pub rdev_grab_error: Mutex<Option<String>>,
}

impl AppState {
    pub fn new(settings: Settings, tray_available: bool) -> Self {
        Self {
            state: Mutex::new(RecordingState::Idle),
            recorder: Mutex::new(None),
            transcription_config: Mutex::new(None),
            record_menu_item: Mutex::new(None),
            settings: Mutex::new(settings),
            portal_shortcut: Mutex::new(None),
            portal_bind_error: Mutex::new(None),
            tray_available: Mutex::new(tray_available),
            active_download: Mutex::new(None),
            transcription_rx: Mutex::new(None),
            idle_unload_handle: Mutex::new(None),
            retranscribing: AtomicBool::new(false),
            #[cfg(target_os = "linux")]
            rdev_guard: Mutex::new(None),
            #[cfg(target_os = "linux")]
            rdev_grab_error: Mutex::new(None),
        }
    }

    // ─── Helper methods to reduce .lock().unwrap() boilerplate ───

    /// Get the current recording state
    pub fn get_state(&self) -> RecordingState {
        *self.state.lock().unwrap()
    }

    /// Set the recording state
    pub fn set_state(&self, new_state: RecordingState) {
        *self.state.lock().unwrap() = new_state;
    }

    /// Read settings with a closure
    pub fn with_settings<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&Settings) -> R,
    {
        f(&self.settings.lock().unwrap())
    }

    /// Modify settings with a closure
    pub fn with_settings_mut<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&mut Settings) -> R,
    {
        f(&mut self.settings.lock().unwrap())
    }

    /// Check if tray is available
    pub fn is_tray_available(&self) -> bool {
        *self.tray_available.lock().unwrap()
    }

    /// Cancel any pending idle model unload task
    pub fn cancel_idle_unload(&self) {
        if let Some(handle) = self.idle_unload_handle.lock().unwrap().take() {
            handle.abort();
        }
    }

    /// Set the idle unload task handle
    pub fn set_idle_unload_handle(&self, handle: tauri::async_runtime::JoinHandle<()>) {
        // Cancel any existing unload task first
        self.cancel_idle_unload();
        *self.idle_unload_handle.lock().unwrap() = Some(handle);
    }
}

impl Default for AppState {
    fn default() -> Self {
        Self::new(Settings::default(), false)
    }
}
