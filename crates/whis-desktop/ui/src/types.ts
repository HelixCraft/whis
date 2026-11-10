// Generic select option for dropdowns
export interface SelectOption<T = string | null> {
  value: T
  label: string
  disabled?: boolean
}

// Transcription providers
export type Provider
  = | 'openai'
    | 'openai-realtime'
    | 'mistral'
    | 'groq'
    | 'deepgram'
    | 'deepgram-realtime'
    | 'elevenlabs'
    | 'local'

// OpenAI transcription method
export type TranscriptionMethod = 'standard' | 'streaming'

// Text post-processing providers
export type PostProcessor = 'none' | 'openai' | 'mistral' | 'ollama'

// CLI shortcut mode
export type CliShortcutMode = 'system' | 'direct'

// All settings from the backend (nested structure)
export interface Settings {
  transcription: {
    provider: Provider
    language: string | null
    api_keys: Record<string, string>
    local_models: {
      selected: string | null
      custom_path: string | null
    }
  }
  post_processing: {
    enabled: boolean
    processor: PostProcessor
    prompt: string | null
  }
  services: {
    ollama: {
      url: string | null
      model: string | null
      keep_alive: string | null
    }
  }
  shortcuts: {
    cli_mode: CliShortcutMode
    cli_key: string
    cli_push_to_talk: boolean
    desktop_key: string
  }
  ui: {
    clipboard_backend: string
    microphone_device: string | null
    audio_enhance: {
      mode: EnhanceMode
      algorithm: DenoiseAlgorithm
    }
    chunk_duration_secs: number
    output_method: OutputMethod
    autotype_backend: AutotypeBackend
    autotype_delay_ms: number | null
    vad: {
      enabled: boolean
      threshold: number
    }
    active_preset: string | null
    bubble: {
      enabled: boolean
    }
    model_memory: {
      keep_model_loaded: boolean
      unload_after_minutes: number
    }
    history: {
      enabled: boolean
      max_entries: number
    }
    last_audio: {
      enabled: boolean
    }
  }
}

// How transcribed text should be output
export type OutputMethod = 'clipboard' | 'autotype' | 'both'

// Cleanup applied to the audio before transcription
export type EnhanceMode = 'off' | 'light' | 'auto' | 'denoise' | 'strong'

// Noise suppression algorithm used when denoising is enabled
export type DenoiseAlgorithm = 'subtraction' | 'wiener' | 'mmse-lsa' | 'rnnoise' | 'deepfilter'

// Which backend to use for autotyping (when OutputMethod includes autotype)
export type AutotypeBackend = 'auto' | 'tools' | 'enigo'

// Status of autotyping tool availability
export interface AutotypeToolStatus {
  available: string[]
  recommended: string | null
  install_hint: string | null
}

// Shortcut backend information
export interface BackendInfo {
  backend: string
  requires_restart: boolean
  compositor: string
  portal_version: number
  is_flatpak: boolean
}

// Shortcut path mismatch information
export interface ShortcutPathMismatch {
  configured_command: string
  current_command: string
}

// Status response from backend
export interface StatusResponse {
  state: 'Idle' | 'Recording' | 'Transcribing'
  config_valid: boolean
}

// Response when saving settings
export interface SaveSettingsResponse {
  needs_restart: boolean
}

// Downloadable local model from the backend catalog
export interface LocalModelInfo {
  id: string
  name: string
  engine: string
  engine_label: string
  format: string
  languages: string
  size_mb: number
  size_label: string
  license: string
  description: string
  installed: boolean
  available: boolean
  supported: boolean
  selected: boolean
  path: string
  note: string
}

// A persisted transcription
export interface HistoryEntry {
  id: string
  created_at_ms: number
  text: string
  raw_text: string
  post_processed: boolean
  provider: string
  model: string | null
  language: string | null
  duration_secs: number
  source: 'recording' | 'retranscription'
  parent_id: string | null
}

// The most recent recording kept on disk
export interface LastAudioInfo {
  path: string
  created_at_ms: number
  duration_secs: number
  size_bytes: number
  provider: string | null
  model: string | null
  language: string | null
}

// Measured audio quality of a recording
export interface AudioQuality {
  peak_dbfs: number
  rms_dbfs: number
  speech_dbfs: number
  noise_dbfs: number
  snr_db: number
  gain_db: number
  hum_hz: number | null
  clipping: boolean
  denoised: boolean
  denoise_algorithm: DenoiseAlgorithm | null
  denoise_ms: number
  output_noise_dbfs: number
  output_snr_db: number
  chunks: number
  gated_chunks: number
  duration_secs: number
}

// Result of a settings test recording
export interface AudioTestResult {
  raw_path: string
  clean_path: string
  duration_secs: number
  quality: AudioQuality
  mode: EnhanceMode
  algorithm: DenoiseAlgorithm
}

// Preset info from backend
export interface PresetInfo {
  name: string
  description: string
  is_builtin: boolean
}

// Full preset details
export interface PresetDetails {
  name: string
  description: string
  prompt: string
  post_processor: string | null
  model: string | null
  is_builtin: boolean
}

// Cloud provider configuration
export interface CloudProviderInfo {
  value: Provider
  label: string
  keyUrl: string
  placeholder: string
}

// Helper to check if provider is local
export function isLocalProvider(provider: Provider): boolean {
  return provider === 'local'
}

// Normalize provider to base variant (realtime variants → base)
export function normalizeProvider(provider: Provider): Provider {
  switch (provider) {
    case 'openai-realtime':
      return 'openai'
    case 'deepgram-realtime':
      return 'deepgram'
    default:
      return provider
  }
}
