import type { AutotypeBackend, AutotypeToolStatus, BackendInfo, CliShortcutMode, EnhanceMode, OutputMethod, PostProcessor, Provider, Settings, ShortcutPathMismatch } from '../types'
import { invoke } from '@tauri-apps/api/core'
import { nextTick, reactive, readonly, watch } from 'vue'

// Defaults fetched from backend (single source of truth: whis-core/src/defaults.rs)
interface Defaults {
  provider: Provider
  post_processor: PostProcessor
  ollama_url: string
  ollama_model: string
  desktop_key: string
  vad_enabled: boolean
  vad_threshold: number
}

// Debounce utility with cancel support
function debounce<T extends (...args: unknown[]) => unknown>(fn: T, ms: number) {
  let timeoutId: ReturnType<typeof setTimeout> | null = null

  const debounced = (...args: Parameters<T>) => {
    if (timeoutId)
      clearTimeout(timeoutId)
    timeoutId = setTimeout(() => {
      timeoutId = null
      fn(...args)
    }, ms)
  }

  debounced.cancel = () => {
    if (timeoutId) {
      clearTimeout(timeoutId)
      timeoutId = null
    }
  }

  return debounced
}

// Cached defaults from backend (populated on init)
// These are fallback values until get_defaults() is called
let defaults: Defaults = {
  provider: 'deepgram',
  post_processor: 'openai',
  ollama_url: 'http://localhost:11434',
  ollama_model: 'qwen2.5:1.5b',
  desktop_key: 'Ctrl+Alt+W',
  vad_enabled: false,
  vad_threshold: 0.5,
}

// Get default settings using cached defaults
function getDefaultSettings(): Settings {
  return {
    transcription: {
      provider: defaults.provider,
      language: null,
      api_keys: {},
      local_models: {
        selected: null,
        custom_path: null,
      },
    },
    post_processing: {
      enabled: false,
      processor: defaults.post_processor,
      prompt: null,
    },
    services: {
      ollama: {
        url: defaults.ollama_url,
        model: defaults.ollama_model,
        keep_alive: '5m',
      },
    },
    shortcuts: {
      cli_mode: 'system' as CliShortcutMode,
      cli_key: defaults.desktop_key,
      cli_push_to_talk: false,
      desktop_key: defaults.desktop_key,
    },
    ui: {
      clipboard_backend: 'auto',
      microphone_device: null,
      audio_enhance: {
        mode: 'light' as EnhanceMode,
      },
      chunk_duration_secs: 90,
      output_method: 'clipboard' as OutputMethod,
      autotype_backend: 'auto' as AutotypeBackend,
      autotype_delay_ms: null,
      vad: {
        enabled: defaults.vad_enabled,
        threshold: defaults.vad_threshold,
      },
      active_preset: null,
      bubble: {
        enabled: false,
      },
      model_memory: {
        keep_model_loaded: true,
        unload_after_minutes: 10,
      },
      history: {
        enabled: true,
        max_entries: 500,
      },
      last_audio: {
        enabled: true,
      },
    },
  }
}

// Internal mutable state
const state = reactive({
  // Settings (initialized with defaults, updated from backend)
  ...getDefaultSettings(),

  // Backend info
  backendInfo: null as BackendInfo | null,
  portalShortcut: null as string | null,
  portalBindError: null as string | null,
  rdevGrabError: null as string | null,
  isInInputGroup: false,
  systemShortcut: null as string | null, // GNOME custom shortcut (RdevGrab backend)
  shortcutPathMismatch: null as ShortcutPathMismatch | null, // Path mismatch warning

  // Autotype tool status
  autotypeToolStatus: null as AutotypeToolStatus | null,

  // Loading state
  loaded: false,

  // Platform capability state
  bubbleSupportsDrag: true,

  // Window visibility state (for smooth show/hide transitions)
  windowVisible: false,

  // Download state for local models (not persisted to disk)
  modelDownload: {
    active: false,
    modelId: null as string | null,
    progress: null as { downloaded: number, total: number } | null,
    error: null as string | null,
  },
})

// Build settings payload for save_settings command
function buildSettingsPayload(): Settings {
  return {
    transcription: state.transcription,
    post_processing: state.post_processing,
    services: state.services,
    shortcuts: state.shortcuts,
    ui: state.ui,
  }
}

// Debounced auto-save (500ms delay)
const debouncedSave = debounce(async () => {
  try {
    await invoke<{ needs_restart: boolean }>('save_settings', {
      settings: buildSettingsPayload(),
    })
  }
  catch (e) {
    console.error('Auto-save failed:', e)
  }
}, 500)

// Watch settings and auto-save on change
watch(
  () => [
    state.transcription,
    state.post_processing,
    state.services,
    state.shortcuts,
    state.ui,
  ],
  () => {
    if (state.loaded)
      debouncedSave()
  },
  { deep: true },
)

// Actions
async function load() {
  try {
    const settings = await invoke<Settings>('get_settings')
    // Deep copy nested settings, using cached defaults for fallbacks
    state.transcription = {
      provider: settings.transcription.provider || defaults.provider,
      language: settings.transcription.language,
      api_keys: settings.transcription.api_keys || {},
      local_models: {
        selected: settings.transcription.local_models.selected ?? null,
        custom_path: settings.transcription.local_models.custom_path ?? null,
      },
    }
    state.post_processing = {
      enabled: settings.post_processing.enabled ?? false,
      processor: settings.post_processing.processor || defaults.post_processor,
      prompt: settings.post_processing.prompt,
    }
    state.services = {
      ollama: {
        url: settings.services.ollama.url || defaults.ollama_url,
        model: settings.services.ollama.model || defaults.ollama_model,
        keep_alive: settings.services.ollama.keep_alive || '5m',
      },
    }
    state.shortcuts = {
      cli_mode: settings.shortcuts?.cli_mode || 'system',
      cli_key: settings.shortcuts?.cli_key || defaults.desktop_key,
      cli_push_to_talk: settings.shortcuts?.cli_push_to_talk ?? false,
      desktop_key: settings.shortcuts?.desktop_key || defaults.desktop_key,
    }
    state.ui = {
      clipboard_backend: settings.ui.clipboard_backend,
      microphone_device: settings.ui.microphone_device,
      audio_enhance: {
        mode: settings.ui.audio_enhance?.mode ?? 'light',
      },
      chunk_duration_secs: Math.max(10, Math.min(300, settings.ui.chunk_duration_secs ?? 90)),
      output_method: settings.ui.output_method ?? 'clipboard',
      autotype_backend: settings.ui.autotype_backend ?? 'auto',
      autotype_delay_ms: settings.ui.autotype_delay_ms ?? null,
      vad: {
        enabled: settings.ui.vad.enabled ?? defaults.vad_enabled,
        threshold: settings.ui.vad.threshold ?? defaults.vad_threshold,
      },
      active_preset: settings.ui.active_preset,
      bubble: {
        enabled: settings.ui.bubble?.enabled ?? false,
      },
      model_memory: {
        keep_model_loaded: settings.ui.model_memory?.keep_model_loaded ?? true,
        unload_after_minutes: settings.ui.model_memory?.unload_after_minutes ?? 10,
      },
      history: {
        enabled: settings.ui.history?.enabled ?? true,
        max_entries: Math.max(0, settings.ui.history?.max_entries ?? 500),
      },
      last_audio: {
        enabled: settings.ui.last_audio?.enabled ?? true,
      },
    }
  }
  catch (e) {
    console.error('Failed to load settings:', e)
  }
}

async function save(): Promise<boolean> {
  try {
    const result = await invoke<{ needs_restart: boolean }>('save_settings', {
      settings: buildSettingsPayload(),
    })
    return result.needs_restart
  }
  catch (e) {
    console.error('Failed to save settings:', e)
    throw e
  }
}

async function loadBackendInfo() {
  try {
    state.backendInfo = await invoke<BackendInfo>('shortcut_backend')

    // For portal backend, fetch actual binding and any errors
    if (state.backendInfo?.backend === 'PortalGlobalShortcuts') {
      state.portalShortcut = await invoke<string | null>('portal_shortcut')
      state.portalBindError = await invoke<string | null>('portal_bind_error')
      // Also read GNOME custom shortcut for display (Flatpak uses custom keybindings)
      state.systemShortcut = await invoke<string | null>('system_shortcut_from_dconf')
    }

    // For RdevGrab backend, fetch any grab errors and check input group
    if (state.backendInfo?.backend === 'RdevGrab') {
      state.rdevGrabError = await invoke<string | null>('rdev_grab_error')
      state.isInInputGroup = await invoke<boolean>('check_input_group_membership')

      // Try to read configured system shortcut from GNOME dconf
      state.systemShortcut = await invoke<string | null>('system_shortcut_from_dconf')

      // Check for shortcut path mismatch (configured vs current binary)
      state.shortcutPathMismatch = await invoke<ShortcutPathMismatch | null>('check_shortcut_path_mismatch')
    }
  }
  catch (e) {
    console.error('Failed to get backend info:', e)
  }
}

async function loadDefaults() {
  try {
    defaults = await invoke<Defaults>('get_defaults')
  }
  catch (e) {
    console.error('Failed to load defaults from backend:', e)
    // Keep using fallback defaults
  }
}

async function loadAutotypeToolStatus() {
  try {
    state.autotypeToolStatus = await invoke<AutotypeToolStatus>('get_autotype_tool_status_cmd')
  }
  catch (e) {
    console.error('Failed to load autotype tool status:', e)
  }
}

async function initialize() {
  // Load canonical defaults from backend first (single source of truth)
  await loadDefaults()
  await loadBackendInfo()
  await loadAutotypeToolStatus()
  await load()

  // Check if bubble drag is supported on this platform
  try {
    state.bubbleSupportsDrag = await invoke<boolean>('bubble_supports_drag')
  }
  catch {
    state.bubbleSupportsDrag = true // Default to true on error
  }

  // Query backend for active download state (survives window close/reopen)
  try {
    const activeDownload = await invoke<{
      model_name: string
      model_type: string
      downloaded: number
      total: number
    } | null>('get_active_download')

    if (activeDownload) {
      state.modelDownload.active = true
      state.modelDownload.modelId = activeDownload.model_name
      state.modelDownload.progress = {
        downloaded: activeDownload.downloaded,
        total: activeDownload.total,
      }
    }
  }
  catch {
    // Command not yet implemented or error - ignore
  }

  state.loaded = true
  await nextTick() // Ensure reactive updates propagate before watchers fire

  // Sync detected system shortcut to settings.json
  // Must be after state.loaded = true so watcher triggers auto-save
  if (state.backendInfo?.backend === 'RdevGrab' && state.systemShortcut) {
    if (state.systemShortcut !== state.shortcuts.desktop_key) {
      state.shortcuts.desktop_key = state.systemShortcut
      // Watcher automatically saves since state.loaded is true
    }
  }
}

async function waitForLoaded(): Promise<void> {
  if (state.loaded)
    return

  return new Promise((resolve) => {
    const unwatch = watch(
      () => state.loaded,
      (loaded) => {
        if (loaded) {
          unwatch()
          resolve()
        }
      },
      { immediate: true },
    )
  })
}

// Flush pending settings immediately (no debounce delay)
// Called before window close to ensure settings are persisted
async function flush(): Promise<void> {
  if (!state.loaded)
    return

  // Cancel any pending debounced save to avoid race condition
  debouncedSave.cancel()

  try {
    await invoke<{ needs_restart: boolean }>('save_settings', {
      settings: buildSettingsPayload(),
    })
  }
  catch (e) {
    console.error('Failed to flush settings:', e)
  }
}

// Setters for individual fields (for v-model binding)
function setProvider(value: Provider) {
  state.transcription.provider = value
}

function setLanguage(value: string | null) {
  state.transcription.language = value
}

function setApiKey(provider: string, key: string) {
  state.transcription.api_keys = { ...state.transcription.api_keys, [provider]: key }
}

function setLocalModel(value: string | null) {
  state.transcription.local_models.selected = value
}

function setLocalModelPath(value: string | null) {
  state.transcription.local_models.custom_path = value
}

function setPostProcessor(value: PostProcessor) {
  state.post_processing.processor = value
}

function setOllamaUrl(value: string | null) {
  state.services.ollama.url = value
}

function setOllamaModel(value: string | null) {
  state.services.ollama.model = value
}

function setPostProcessingPrompt(value: string | null) {
  state.post_processing.prompt = value
}

function setDesktopKey(value: string) {
  state.shortcuts.desktop_key = value
}

function setPortalShortcut(value: string | null) {
  state.portalShortcut = value
}

function setMicrophoneDevice(value: string | null) {
  state.ui.microphone_device = value
}

function setEnhanceMode(value: EnhanceMode) {
  state.ui.audio_enhance.mode = value
}

function setBubbleEnabled(value: boolean) {
  state.ui.bubble.enabled = value
}

function setChunkDuration(value: number) {
  // Clamp to valid range (10-300 seconds)
  state.ui.chunk_duration_secs = Math.max(10, Math.min(300, value))
}

function setKeepModelLoaded(value: boolean) {
  state.ui.model_memory.keep_model_loaded = value
}

function setUnloadAfterMinutes(value: number) {
  state.ui.model_memory.unload_after_minutes = value
}

function setHistoryEnabled(value: boolean) {
  state.ui.history.enabled = value
}

function setHistoryMaxEntries(value: number) {
  // 0 disables pruning, so only guard against negative values
  state.ui.history.max_entries = Math.max(0, Math.floor(value))
}

function setLastAudioEnabled(value: boolean) {
  state.ui.last_audio.enabled = value
}

function setOllamaKeepAlive(value: string | null) {
  state.services.ollama.keep_alive = value
}

function setOutputMethod(value: OutputMethod) {
  state.ui.output_method = value
}

// Post-processing orchestration methods
function enablePostProcessing() {
  state.post_processing.enabled = true
}

function disablePostProcessing() {
  state.post_processing.enabled = false
  // Keep active_preset - user can re-enable without losing preset selection
}

async function applyPreset(name: string): Promise<void> {
  // Backend updates prompt, processor, and active_preset
  await invoke('apply_preset', { name })

  // Reload settings to get backend changes
  await load()

  // Enable post-processing when applying a preset
  state.post_processing.enabled = true
}

async function clearPreset(): Promise<void> {
  // Clear active preset in backend
  await invoke('set_active_preset', { name: null })

  // Reload settings
  await load()

  // Disable post-processing when clearing preset
  state.post_processing.enabled = false
}

// Window visibility state (for smooth show/hide transitions on Wayland)
function setWindowVisible(visible: boolean) {
  state.windowVisible = visible
}

// Download state management for local models
function startModelDownload(modelId: string) {
  state.modelDownload.active = true
  state.modelDownload.modelId = modelId
  state.modelDownload.progress = null
  state.modelDownload.error = null
}

function updateModelDownloadProgress(downloaded: number, total: number) {
  if (state.modelDownload.active)
    state.modelDownload.progress = { downloaded, total }
}

function completeModelDownload() {
  state.modelDownload.active = false
  state.modelDownload.progress = null
  state.modelDownload.modelId = null
}

function failModelDownload(error: string) {
  state.modelDownload.active = false
  state.modelDownload.error = error
}

// Getter for default provider (used by SettingsView when switching modes)
function getDefaultProvider(): Provider {
  return defaults.provider
}

// Export reactive state and actions
export const settingsStore = {
  // Readonly state for reading (prevents accidental mutation)
  state: readonly(state),

  // Mutable state for v-model (use sparingly)
  mutableState: state,

  // Actions
  load,
  save,
  initialize,
  waitForLoaded,
  flush,
  getDefaultProvider,

  // Setters
  setProvider,
  setLanguage,
  setApiKey,
  setLocalModel,
  setLocalModelPath,
  setPostProcessor,
  setOllamaUrl,
  setOllamaModel,
  setPostProcessingPrompt,
  setDesktopKey,
  setPortalShortcut,
  setMicrophoneDevice,
  setEnhanceMode,
  setBubbleEnabled,
  setChunkDuration,
  setKeepModelLoaded,
  setUnloadAfterMinutes,
  setHistoryEnabled,
  setHistoryMaxEntries,
  setLastAudioEnabled,
  setOllamaKeepAlive,
  setOutputMethod,
  setWindowVisible,

  // Post-processing orchestration
  enablePostProcessing,
  disablePostProcessing,
  applyPreset,
  clearPreset,

  // Download state management
  startModelDownload,
  updateModelDownloadProgress,
  completeModelDownload,
  failModelDownload,
}
