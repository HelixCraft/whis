<script setup lang="ts">
import type { TranscriptionMode } from '../components/settings/ModeCards.vue'
import type { DenoiseAlgorithm, EnhanceMode, OutputMethod, PostProcessor, Provider, SelectOption } from '../types'
import { invoke } from '@tauri-apps/api/core'
import { computed, onMounted, ref } from 'vue'
import AppSelect from '../components/AppSelect.vue'
import AppSlider from '../components/AppSlider.vue'
import AudioTestCard from '../components/audio/AudioTestCard.vue'
import CloudProviderConfig from '../components/settings/CloudProviderConfig.vue'
import LocalModelsConfig from '../components/settings/LocalModelsConfig.vue'
import ModeCards from '../components/settings/ModeCards.vue'
import PostProcessingConfig from '../components/settings/PostProcessingConfig.vue'
import ToggleSwitch from '../components/settings/ToggleSwitch.vue'
import { settingsStore } from '../stores/settings'
import { isLocalProvider, normalizeProvider } from '../types'

const helpOpen = ref(false)
const advancedOpen = ref(false)

// Settings from store
const provider = computed(() => settingsStore.state.transcription.provider)
const language = computed(() => settingsStore.state.transcription.language)
const apiKeys = computed(() => settingsStore.state.transcription.api_keys)
const postProcessor = computed(() => settingsStore.state.post_processing.processor)
const postProcessingEnabled = computed(() => settingsStore.state.post_processing.enabled)

// Transcription mode: cloud vs local
const transcriptionMode = ref<TranscriptionMode>(
  isLocalProvider(provider.value) ? 'local' : 'cloud',
)

// Streaming mode (for OpenAI and DeepGram)
const isStreaming = computed(() =>
  provider.value === 'openai-realtime' || provider.value === 'deepgram-realtime',
)

// Normalize provider for dropdown display (realtime variants show as base provider)
const baseProvider = computed(() => normalizeProvider(provider.value))

// Whether to show streaming toggle (cloud mode + provider that supports streaming)
const showStreamingToggle = computed(() => {
  if (transcriptionMode.value !== 'cloud')
    return false
  return baseProvider.value === 'openai' || baseProvider.value === 'deepgram'
})

// Cloud provider options for dropdown (loaded from backend for correct order)
const cloudProviderOptions = ref<SelectOption[]>([])

// Load cloud providers from backend (ordered by recommendation from whis-core)
onMounted(async () => {
  try {
    const providers = await invoke<{ value: string, label: string }[]>('get_cloud_providers')
    cloudProviderOptions.value = providers
  }
  catch (error) {
    console.error('Failed to load cloud providers:', error)
    // No fallback - backend should always work
  }
})

// Filter providers based on streaming mode
const filteredProviderOptions = computed(() => {
  if (isStreaming.value) {
    // When streaming enabled, only show providers that support it
    return cloudProviderOptions.value.filter(p =>
      p.value === 'openai' || p.value === 'deepgram',
    )
  }
  return cloudProviderOptions.value
})

// Common language codes for the dropdown
const languageOptions: SelectOption[] = [
  { value: null, label: 'Auto-detect' },
  { value: 'en', label: 'English' },
  { value: 'de', label: 'German' },
  { value: 'fr', label: 'French' },
  { value: 'es', label: 'Spanish' },
  { value: 'it', label: 'Italian' },
  { value: 'pt', label: 'Portuguese' },
  { value: 'nl', label: 'Dutch' },
  { value: 'pl', label: 'Polish' },
  { value: 'ru', label: 'Russian' },
  { value: 'ja', label: 'Japanese' },
  { value: 'ko', label: 'Korean' },
  { value: 'zh', label: 'Chinese' },
]

function handleModeChange(mode: TranscriptionMode) {
  transcriptionMode.value = mode
  if (mode === 'cloud') {
    // Switch to default cloud provider if currently on local
    if (isLocalProvider(provider.value)) {
      const defaultProvider = settingsStore.getDefaultProvider()
      settingsStore.setProvider(defaultProvider)
      // Auto-sync post-processor to match (if user has cloud post-processor enabled)
      if (postProcessor.value !== 'none' && postProcessor.value !== 'ollama') {
        // Only set post-processor if default provider supports it
        if (defaultProvider === 'openai' || defaultProvider === 'mistral') {
          settingsStore.setPostProcessor(defaultProvider)
        }
      }
    }
  }
  else {
    // Switch to local transcription if currently on cloud
    if (!isLocalProvider(provider.value)) {
      settingsStore.setProvider('local')
    }
  }
}

function handleProviderUpdate(value: string | null) {
  if (!value)
    return

  // Use realtime variant if streaming mode is enabled for supported providers
  const effectiveProvider = isStreaming.value && (value === 'openai' || value === 'deepgram')
    ? `${value}-realtime` as Provider
    : value as Provider

  settingsStore.setProvider(effectiveProvider)

  // Auto-sync post-processor to match provider (if cloud post-processor enabled)
  const shouldSyncProcessor = (value === 'openai' || value === 'mistral')
    && postProcessor.value !== 'none'
    && postProcessor.value !== 'ollama'

  if (shouldSyncProcessor) {
    settingsStore.setPostProcessor(value as PostProcessor)
  }
}

function handleApiKeyUpdate(providerKey: string, value: string) {
  settingsStore.setApiKey(providerKey, value)
}

function handleStreamingToggle(enabled: boolean) {
  // Toggle between standard and realtime variant of current provider
  const base = baseProvider.value
  if (base === 'openai') {
    settingsStore.setProvider(enabled ? 'openai-realtime' : 'openai')
    // Keep post-processor as openai (both methods use same API)
    if (postProcessor.value !== 'none' && postProcessor.value !== 'ollama') {
      settingsStore.setPostProcessor('openai')
    }
  }
  else if (base === 'deepgram') {
    settingsStore.setProvider(enabled ? 'deepgram-realtime' : 'deepgram')
  }
}

function handleLanguageChange(value: string | null) {
  settingsStore.setLanguage(value)
}

// Audio devices
interface AudioDevice {
  name: string
  display_name: string | null
  is_default: boolean
}

const audioDevices = ref<AudioDevice[]>([])
const microphoneDevice = computed(() => settingsStore.state.ui.microphone_device)

// Load available audio devices
onMounted(async () => {
  try {
    audioDevices.value = await invoke<AudioDevice[]>('list_audio_devices')
  }
  catch (error) {
    console.error('Failed to load audio devices:', error)
  }
})

// Convert audio devices to select options
const microphoneOptions = computed<SelectOption[]>(() => {
  const options: SelectOption[] = [
    { value: null, label: 'System Default' },
  ]

  for (const device of audioDevices.value) {
    // Use display_name if available, otherwise fall back to raw name
    const displayName = device.display_name ?? device.name
    options.push({
      value: device.name, // Store raw name for device lookup
      label: displayName,
    })
  }

  return options
})

function handleMicrophoneChange(value: string | null) {
  settingsStore.setMicrophoneDevice(value)
}

// Audio cleanup before transcription
const enhanceMode = computed(() => settingsStore.state.ui.audio_enhance.mode)

const enhanceModeOptions: SelectOption[] = [
  { value: 'off', label: 'Off' },
  { value: 'light', label: 'Light (recommended)' },
  { value: 'auto', label: 'Auto denoise (only when noisy)' },
  { value: 'denoise', label: 'Denoise always' },
  { value: 'strong', label: 'Strong denoise' },
]

function handleEnhanceModeChange(value: string | null) {
  if (value) {
    settingsStore.setEnhanceMode(value as EnhanceMode)
  }
}

// Denoise algorithm, only relevant while denoising is enabled
const denoiseAlgorithm = computed(() => settingsStore.state.ui.audio_enhance.algorithm)
const denoiseEnabled = computed(() =>
  ['auto', 'denoise', 'strong'].includes(enhanceMode.value),
)

const denoiseAlgorithmOptions: SelectOption[] = [
  { value: 'subtraction', label: 'Spectral subtraction (fastest)' },
  { value: 'wiener', label: 'Wiener filter' },
  { value: 'mmse-lsa', label: 'MMSE-LSA (recommended)' },
  { value: 'rnnoise', label: 'RNNoise (neural)' },
  { value: 'deepfilter', label: 'DeepFilterNet3 (strongest)' },
]

const denoiseAlgorithmHints: Record<DenoiseAlgorithm, string> = {
  'subtraction': 'Classic spectral subtraction. Fastest, but can leave a light ringing on noise.',
  'wiener': 'Wiener gain with smoother noise tracking. A good balance of speed and quality.',
  'mmse-lsa': 'Ephraim-Malah estimator. Best classical result, keeps quiet consonants intact.',
  'rnnoise': 'Small neural network trained on speech. Very fast, clear on steady noise.',
  'deepfilter': 'DeepFilterNet3. Strongest removal of background noise, slightly more CPU.',
}

const denoiseAlgorithmHint = computed(() => denoiseAlgorithmHints[denoiseAlgorithm.value])

function handleDenoiseAlgorithmChange(value: string | null) {
  if (value) {
    settingsStore.setDenoiseAlgorithm(value as DenoiseAlgorithm)
  }
}

// Chunk duration for progressive transcription
const chunkDuration = computed(() => settingsStore.state.ui.chunk_duration_secs)

function handleChunkDurationChange(value: number) {
  settingsStore.setChunkDuration(value)
}

// Custom model path override (falls back to the catalog selection when empty)
const customModelPath = computed(() => settingsStore.state.transcription.local_models.custom_path)
const modelPathUnlocked = ref(false)

function handleModelPathChange(event: Event) {
  const value = (event.target as HTMLInputElement).value
  settingsStore.setLocalModelPath(value.length > 0 ? value : null)
}

// Config file path
const configPath = '~/.config/whis/settings.json'
const configPathCopied = ref(false)

async function copyConfigPath() {
  try {
    await navigator.clipboard.writeText(configPath)
    configPathCopied.value = true
    setTimeout(() => {
      configPathCopied.value = false
    }, 2000)
  }
  catch (error) {
    console.error('Failed to copy path:', error)
  }
}

// Bubble settings
const bubbleEnabled = computed(() => settingsStore.state.ui.bubble.enabled)
const bubbleSupportsDrag = computed(() => settingsStore.state.bubbleSupportsDrag)

function handleBubbleEnabledChange(value: boolean) {
  settingsStore.setBubbleEnabled(value)
}

// Output method settings (clipboard, autotype, both)
const outputMethod = computed(() => settingsStore.state.ui.output_method)

const outputMethodOptions: SelectOption[] = [
  { value: 'clipboard', label: 'Clipboard' },
  { value: 'autotype', label: 'Autotype' },
  { value: 'both', label: 'Both' },
]

function handleOutputMethodChange(value: string | null) {
  if (value) {
    settingsStore.setOutputMethod(value as OutputMethod)
  }
}

// Autotype tool status
const needsAutotypeTools = computed(() =>
  outputMethod.value === 'autotype' || outputMethod.value === 'both',
)

const autotypeToolsAvailable = computed(() => {
  const status = settingsStore.state.autotypeToolStatus
  // Tools are available if:
  // - status is null (non-Linux, uses enigo)
  // - or there are available tools
  // - or no recommended tool is needed (meaning we already have what we need)
  if (!status)
    return true
  return status.available.length > 0 || status.recommended === null
})

const autotypeInstallHint = computed(() => {
  const status = settingsStore.state.autotypeToolStatus
  if (!status || !status.install_hint)
    return 'Install an autotype tool (ydotool recommended)'
  return `Install ${status.recommended}: ${status.install_hint}`
})

// Model memory settings
const keepModelLoaded = computed(() => settingsStore.state.ui.model_memory.keep_model_loaded)
const unloadAfterMinutes = computed(() => settingsStore.state.ui.model_memory.unload_after_minutes)
const ollamaKeepAlive = computed(() => settingsStore.state.services.ollama.keep_alive)

// Storage settings
const historyEnabled = computed(() => settingsStore.state.ui.history.enabled)
const historyMaxEntries = computed(() => settingsStore.state.ui.history.max_entries)
const lastAudioEnabled = computed(() => settingsStore.state.ui.last_audio.enabled)

// Conditional visibility states
const isLocalMode = computed(() => transcriptionMode.value === 'local')
const isOllamaPostProcessor = computed(() =>
  postProcessingEnabled.value && postProcessor.value === 'ollama',
)

const unloadTimeoutOptions: SelectOption[] = [
  { value: '5', label: '5 min' },
  { value: '10', label: '10 min' },
  { value: '30', label: '30 min' },
  { value: '60', label: '60 min' },
  { value: '0', label: 'Never' },
]

const ollamaKeepAliveOptions: SelectOption[] = [
  { value: '0', label: 'Immediate' },
  { value: '5m', label: '5 min' },
  { value: '10m', label: '10 min' },
  { value: '30m', label: '30 min' },
  { value: '-1', label: 'Forever' },
]

const historyLimitOptions: SelectOption[] = [
  { value: '100', label: '100 entries' },
  { value: '500', label: '500 entries' },
  { value: '1000', label: '1000 entries' },
  { value: '0', label: 'Unlimited' },
]

function handleHistoryLimitChange(value: string | null) {
  if (value !== null)
    settingsStore.setHistoryMaxEntries(Number.parseInt(value, 10))
}

function handleKeepModelLoadedChange(value: boolean) {
  settingsStore.setKeepModelLoaded(value)
}

function handleUnloadAfterMinutesChange(value: string | null) {
  if (value !== null) {
    settingsStore.setUnloadAfterMinutes(Number.parseInt(value, 10))
  }
}

function handleOllamaKeepAliveChange(value: string | null) {
  settingsStore.setOllamaKeepAlive(value)
}
</script>

<template>
  <section class="section settings-section">
    <header class="section-header">
      <div class="header-content">
        <h1>Settings</h1>
        <p>Configure transcription</p>
      </div>
      <button class="help-btn" :aria-label="helpOpen ? 'Close help' : 'Open help'" @click="helpOpen = !helpOpen">
        {{ helpOpen ? 'Close' : 'Help' }}
      </button>
    </header>

    <div class="settings-layout">
      <div class="section-content">
        <!-- Mode Cards (Cloud/Local) -->
        <ModeCards
          :model-value="transcriptionMode"
          @update:model-value="handleModeChange"
        />

        <!-- Cloud Provider Config (API key only) -->
        <CloudProviderConfig
          v-if="transcriptionMode === 'cloud'"
          :provider="provider"
          :api-keys="apiKeys"
          :show-config-card="true"
          @update:api-key="handleApiKeyUpdate"
        />

        <!-- Local model picker and download -->
        <LocalModelsConfig
          v-if="transcriptionMode === 'local'"
          :show-config-card="true"
        />

        <!-- Transcription Section -->
        <div class="settings-section">
          <p class="section-label">
            transcription
          </p>

          <!-- Cloud: Service selector -->
          <div v-if="transcriptionMode === 'cloud'" class="field-row">
            <label>Service</label>
            <AppSelect
              :model-value="baseProvider"
              :options="filteredProviderOptions"
              @update:model-value="handleProviderUpdate"
            />
          </div>

          <!-- Cloud: Streaming toggle (OpenAI and DeepGram) -->
          <div v-if="showStreamingToggle" class="field-row">
            <label>Streaming</label>
            <ToggleSwitch
              :model-value="isStreaming"
              @update:model-value="handleStreamingToggle"
            />
          </div>

          <!-- Language -->
          <div class="field-row">
            <label>Language</label>
            <AppSelect
              :key="`language-${settingsStore.state.loaded}`"
              :model-value="language"
              :options="languageOptions"
              @update:model-value="handleLanguageChange"
            />
          </div>

          <!-- Microphone Device -->
          <div class="field-row">
            <label>Microphone</label>
            <AppSelect
              :key="`microphone-${settingsStore.state.loaded}`"
              :model-value="microphoneDevice"
              :options="microphoneOptions"
              @update:model-value="handleMicrophoneChange"
            />
          </div>
        </div>

        <!-- Audio Cleanup Section -->
        <div class="settings-section">
          <p class="section-label">
            audio cleanup
          </p>

          <div class="field-row">
            <label>Cleanup</label>
            <AppSelect
              :model-value="enhanceMode"
              :options="enhanceModeOptions"
              @update:model-value="handleEnhanceModeChange"
            />
          </div>

          <div v-if="denoiseEnabled" class="field-row">
            <label>Denoise algorithm</label>
            <AppSelect
              :model-value="denoiseAlgorithm"
              :options="denoiseAlgorithmOptions"
              @update:model-value="handleDenoiseAlgorithmChange"
            />
          </div>

          <p class="env-hint">
            <span class="hint-marker">[i]</span>
            Light removes rumble and mains hum and lifts quiet speech without changing loud audio.
            Denoise also has to remove noise, which can leave artifacts, so it is opt-in.
          </p>

          <p v-if="denoiseEnabled" class="env-hint">
            <span class="hint-marker">[i]</span>
            {{ denoiseAlgorithmHint }}
          </p>

          <AudioTestCard />
        </div>

        <!-- Post-Processing Section -->
        <div class="settings-section">
          <p class="section-label">
            post-processing
          </p>
          <PostProcessingConfig />
        </div>

        <!-- Recording Indicator Section -->
        <div class="settings-section">
          <p class="section-label">
            recording indicator
          </p>

          <div class="field-row">
            <label>Show Indicator</label>
            <ToggleSwitch
              :model-value="bubbleEnabled"
              @update:model-value="handleBubbleEnabledChange"
            />
          </div>

          <p v-if="!bubbleSupportsDrag && bubbleEnabled" class="env-hint">
            <span class="hint-marker">[i]</span>
            On Wayland, indicator position stays centered
          </p>
        </div>

        <!-- Performance Section (only when local mode or Ollama enabled) -->
        <div v-if="isLocalMode || isOllamaPostProcessor" class="settings-section">
          <p class="section-label">
            performance
          </p>

          <!-- Model Memory (only in local mode) -->
          <div v-if="isLocalMode" class="field-row">
            <label>Keep Model in Memory</label>
            <ToggleSwitch
              :model-value="keepModelLoaded"
              @update:model-value="handleKeepModelLoadedChange"
            />
          </div>

          <div v-if="isLocalMode && keepModelLoaded" class="field-row">
            <label>Release Model After</label>
            <AppSelect
              :model-value="String(unloadAfterMinutes)"
              :options="unloadTimeoutOptions"
              @update:model-value="handleUnloadAfterMinutesChange"
            />
          </div>

          <!-- Ollama Keep Alive (only when using Ollama) -->
          <div v-if="isOllamaPostProcessor" class="field-row">
            <label>Release Ollama After</label>
            <AppSelect
              :model-value="ollamaKeepAlive"
              :options="ollamaKeepAliveOptions"
              @update:model-value="handleOllamaKeepAliveChange"
            />
          </div>
        </div>

        <!-- Storage Section -->
        <div class="settings-section">
          <p class="section-label">
            storage
          </p>

          <div class="field-row">
            <label>Save Transcripts</label>
            <ToggleSwitch
              :model-value="historyEnabled"
              @update:model-value="settingsStore.setHistoryEnabled"
            />
          </div>

          <div v-if="historyEnabled" class="field-row">
            <label>History Entries</label>
            <AppSelect
              :model-value="String(historyMaxEntries)"
              :options="historyLimitOptions"
              @update:model-value="handleHistoryLimitChange"
            />
          </div>

          <div class="field-row">
            <label>Save Last Recording</label>
            <ToggleSwitch
              :model-value="lastAudioEnabled"
              @update:model-value="settingsStore.setLastAudioEnabled"
            />
          </div>

          <p class="env-hint">
            <span class="hint-marker">[i]</span>
            Transcripts and the last recording are stored as files in the whis data folder.
            <router-link to="/history">
              Open history
            </router-link>
          </p>
        </div>

        <!-- Advanced Section (collapsed by default) -->
        <details class="settings-section advanced-section" :open="advancedOpen" @toggle="advancedOpen = ($event.target as HTMLDetailsElement).open">
          <summary class="section-label clickable">
            advanced
            <span class="toggle-indicator">{{ advancedOpen ? '−' : '+' }}</span>
          </summary>

          <div class="advanced-content">
            <!-- Transcription Interval -->
            <div class="field-row">
              <label>Transcription Interval</label>
              <AppSlider
                :model-value="chunkDuration"
                :min="10"
                :max="300"
                :step="10"
                unit="sec"
                aria-label="Transcription interval in seconds"
                @update:model-value="handleChunkDurationChange"
              />
            </div>

            <!-- Model Location (only in local mode) -->
            <div v-if="isLocalMode" class="field-row">
              <label>Model Location</label>
              <div class="locked-input" :class="{ locked: !modelPathUnlocked }">
                <input
                  type="text"
                  class="text-input"
                  :value="customModelPath ?? ''"
                  placeholder="Use the model selected above"
                  :disabled="!modelPathUnlocked"
                  spellcheck="false"
                  @input="handleModelPathChange"
                >
                <button
                  class="lock-btn"
                  :title="modelPathUnlocked ? 'Lock' : 'Unlock to edit'"
                  @click="modelPathUnlocked = !modelPathUnlocked"
                >
                  {{ modelPathUnlocked ? '[-]' : '[=]' }}
                </button>
              </div>
            </div>

            <!-- Output Method -->
            <div class="field-row">
              <label>Output Method</label>
              <AppSelect
                :model-value="outputMethod"
                :options="outputMethodOptions"
                @update:model-value="handleOutputMethodChange"
              />
            </div>

            <!-- Autotype tool installation hint -->
            <p v-if="needsAutotypeTools && !autotypeToolsAvailable" class="env-hint">
              <span class="hint-marker">[i]</span>
              {{ autotypeInstallHint }}
            </p>

            <!-- Config File Path -->
            <div class="field-row">
              <label>Config File</label>
              <div class="locked-input locked">
                <input
                  type="text"
                  class="text-input"
                  :value="configPath"
                  disabled
                  readonly
                  spellcheck="false"
                >
                <button
                  class="lock-btn"
                  :title="configPathCopied ? 'Copied!' : 'Copy path'"
                  @click="copyConfigPath"
                >
                  {{ configPathCopied ? '[ok]' : '[cp]' }}
                </button>
              </div>
            </div>
          </div>
        </details>
      </div>

      <!-- Help Panel -->
      <div class="help-panel slide-panel" :class="{ open: helpOpen }">
        <div class="slide-panel-content">
          <div class="slide-panel-header">
            <h2>Help</h2>
          </div>

          <div class="help-section">
            <h3>transcription mode</h3>
            <p>Choose how to transcribe audio. Cloud is fast and easy (requires API key and internet). Local is private and free (requires model download, slower on older hardware).</p>
          </div>

          <div class="help-section">
            <h3>transcription service</h3>
            <p>Choose which cloud service performs speech-to-text. Each has different pricing, speed, and language support. Requires a separate API account.</p>
          </div>

          <div class="help-section">
            <h3>streaming</h3>
            <p>Available for OpenAI and Deepgram. When enabled, audio streams during recording for lower latency. When disabled, audio is uploaded after recording (works with files too).</p>
          </div>

          <div class="help-section">
            <h3>api key</h3>
            <p>Your API key authenticates with the provider. Get it from your provider's website. Keys are stored locally on your device.</p>
          </div>

          <div class="help-section">
            <h3>local transcription</h3>
            <p><strong>Parakeet:</strong> Fast and accurate, best on European languages. <strong>Whisper:</strong> Broad language coverage in several sizes. <strong>Qwen3-ASR, Moonshine:</strong> compact GGUF models for quick notes. Everything runs offline. Larger models are more accurate but slower.</p>
          </div>

          <div class="help-section">
            <h3>language</h3>
            <p>Auto-detect works for most recordings. Set a specific language if you're getting poor results with accents, technical terms, or mixed languages.</p>
          </div>

          <div class="help-section">
            <h3>post-processing</h3>
            <p>Clean up transcripts with AI. Fixes grammar, punctuation, and can add structure. Works with cloud providers or local Ollama. Optional—leave off for verbatim transcripts.</p>
          </div>

          <div class="help-section">
            <h3>presets</h3>
            <p>Pre-configured post-processing instructions. Choose a style that matches your use case, or create custom presets in the Presets page.</p>
          </div>

          <div class="help-section">
            <h3>ollama</h3>
            <p>Use local AI models for post-processing without cloud APIs or costs. Requires Ollama running on your machine.</p>
            <div class="help-steps">
              <p><strong>Setup:</strong></p>
              <ol>
                <li>Install from <a href="https://ollama.com/download" target="_blank">ollama.com</a></li>
                <li>Download a model: <code>ollama pull llama3.2:3b</code></li>
                <li>Click "ping" below to test connection</li>
              </ol>
            </div>
          </div>

          <div class="help-section">
            <h3>recording indicator</h3>
            <p>Shows a floating indicator during recording. Drag to reposition. The bubble remembers its last position.</p>
          </div>

          <div class="help-section">
            <h3>output method</h3>
            <p>How transcribed text is delivered. <strong>Clipboard</strong> copies text for pasting. <strong>Autotype</strong> types directly into the active window (requires wtype/xdotool on Linux). <strong>Both</strong> does both.</p>
          </div>

          <div class="help-section">
            <h3>keep model in memory</h3>
            <p>When enabled, local transcription models stay in memory between recordings for faster response (~2GB RAM). Disable to free memory after each recording (slower startup for next recording).</p>
          </div>

          <div class="help-section">
            <h3>release model after</h3>
            <p>Auto-unload the model after this many minutes of inactivity to free memory. "Never" keeps the model loaded until the app closes. Default: 10 minutes.</p>
          </div>

          <div class="help-section">
            <h3>release ollama after</h3>
            <p>How long Ollama keeps its model in GPU memory after post-processing. "Immediate" unloads right away. "Forever" keeps it loaded until Ollama restarts. Default: 5 minutes.</p>
          </div>

          <div class="help-section">
            <h3>microphone</h3>
            <p>Select which audio input device to use. "System Default" uses your system's current default microphone.</p>
          </div>

          <div class="help-section">
            <h3>audio cleanup</h3>
            <p>Processing applied before the audio is sent to the model. The stored recording stays untouched.</p>
            <p><strong>Light</strong> applies a high-pass filter at 60 Hz, removes detected 50/60 Hz mains hum, and amplifies quiet speech up to -20 dBFS without ever clipping. Loud audio is not attenuated, so a good microphone is not made worse.</p>
            <p><strong>Auto denoise</strong> additionally removes steady background noise, but only when the measured signal-to-noise ratio is between 3 and 15 dB and the recording contains speech pauses. <strong>Strong denoise</strong> always denoises; use it only for consistently noisy setups.</p>
            <p>The test button records 6 seconds and lets you compare the original with the cleaned audio at the same level.</p>
          </div>

          <div class="help-section">
            <h3>transcription interval</h3>
            <p>How often audio is sent for transcription during recording. Smaller values (30-60s) feel more responsive but may reduce accuracy. Larger values (90-180s) give the model more context for better accuracy. Default: 90 seconds.</p>
          </div>

          <div class="help-section">
            <h3>model location</h3>
            <p>Override the default model location. Only change this if you've downloaded models to a custom directory. Leave empty to use the default location.</p>
          </div>

          <div class="help-section">
            <h3>config file</h3>
            <p>Settings are stored locally at this path. You can backup or edit this file directly if needed.</p>
          </div>
        </div>
      </div>
    </div>
  </section>
</template>

<style scoped>
/* Settings Sections */
.settings-section {
  display: flex;
  flex-direction: column;
  gap: 12px;
}

.section-label {
  font-size: 10px;
  text-transform: uppercase;
  color: var(--text-weak);
  letter-spacing: 0.05em;
  margin: 0;
  padding-bottom: 8px;
  border-bottom: 1px solid var(--border);
}

.settings-layout {
  display: flex;
  flex: 1;
  overflow: hidden;
  position: relative;
}

.section-content {
  flex: 1;
  overflow-y: auto;
  padding-right: 16px;
}

/* Header with help button */
.section-header {
  display: flex;
  justify-content: space-between;
  align-items: flex-start;
}

.header-content {
  flex: 1;
}

.help-btn {
  background: none;
  border: none;
  font-family: var(--font);
  font-size: 12px;
  color: var(--text-weak);
  cursor: pointer;
  padding: 4px 8px;
  transition: color 0.15s ease;
}

.help-btn:hover {
  color: var(--accent);
}

/* Help Panel - uses global .slide-panel styles */
.help-panel .slide-panel-header {
  padding-bottom: 12px;
  border-bottom: 1px solid var(--border);
}

.help-section {
  display: flex;
  flex-direction: column;
  gap: 4px;
}

.help-section h3 {
  font-size: 11px;
  font-weight: 400;
  color: var(--text-weak);
  margin: 0;
  text-transform: lowercase;
}

.help-section p {
  font-size: 12px;
  color: var(--text);
  line-height: 1.5;
  margin: 0;
}

.help-steps {
  margin-top: 8px;
}

.help-steps ol {
  margin: 4px 0 0 0;
  padding-left: 16px;
  font-size: 11px;
  color: var(--text);
  line-height: 1.6;
}

.help-steps li {
  margin-bottom: 2px;
}

.help-steps code {
  background: var(--bg-weak);
  padding: 1px 4px;
  border-radius: 2px;
  font-size: 10px;
}

.help-steps a {
  color: var(--accent);
  text-decoration: none;
}

.help-steps a:hover {
  text-decoration: underline;
}

/* Text Input */
.text-input {
  padding: 10px 12px;
  background: var(--bg-weak);
  border: 1px solid var(--border);
  border-radius: 4px;
  font-family: var(--font);
  font-size: 12px;
  color: var(--text);
  transition: border-color 0.15s ease;
  min-width: 0;
  flex: 1;
}

.text-input:focus {
  outline: none;
  border-color: var(--accent);
}

.text-input::placeholder {
  color: var(--text-weak);
}

/* Locked Input */
.locked-input {
  display: flex;
  flex: 1;
  gap: 8px;
  align-items: center;
}

.locked-input.locked .text-input {
  opacity: 0.5;
  cursor: not-allowed;
}

.lock-btn {
  background: none;
  border: none;
  cursor: pointer;
  font-family: monospace;
  font-size: 12px;
  padding: 4px;
  color: var(--text-weak);
  transition: color 0.15s;
}

.lock-btn:hover:not(:disabled) {
  color: var(--accent);
}

.lock-btn:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}

.locked-input.disabled .text-input {
  opacity: 0.5;
  cursor: not-allowed;
}

/* Advanced Section (collapsible) */
.advanced-section {
  border: none;
}

.advanced-section summary {
  cursor: pointer;
  user-select: none;
  display: flex;
  justify-content: space-between;
  align-items: center;
}

.advanced-section summary:hover {
  color: var(--text);
}

.advanced-section summary::-webkit-details-marker {
  display: none;
}

.advanced-section summary::marker {
  display: none;
  content: '';
}

.toggle-indicator {
  font-size: 12px;
  color: var(--text-weak);
  font-weight: 400;
}

.advanced-content {
  display: flex;
  flex-direction: column;
  gap: 12px;
  padding-top: 12px;
}

/* Environment hint for tool installation */
.env-hint {
  display: flex;
  align-items: flex-start;
  gap: 6px;
  margin: 0;
  font-size: 11px;
  color: var(--text-weak);
  line-height: 1.4;
}

.hint-marker {
  color: var(--text-weak);
  opacity: 0.7;
  flex-shrink: 0;
}
</style>
