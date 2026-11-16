<script setup lang="ts">
import type { AudioTestResult, DenoiseAlgorithm } from '../../types'
import { invoke } from '@tauri-apps/api/core'
import { computed, onBeforeUnmount, ref } from 'vue'
import AppSelect from '../AppSelect.vue'
import AppSlider from '../AppSlider.vue'

const recording = ref(false)
const recordingSeconds = ref(0)
const processing = ref(false)
const transcribing = ref<'raw' | 'clean' | null>(null)
const result = ref<AudioTestResult | null>(null)
const rawText = ref<string | null>(null)
const cleanText = ref<string | null>(null)
const error = ref<string | null>(null)
const playing = ref<'raw' | 'clean' | null>(null)

// Comparison controls, independent of the saved settings.
const algorithm = ref<DenoiseAlgorithm>('mmse-lsa')
const strengthPercent = ref(50)

const rawPlayer = ref<HTMLAudioElement | null>(null)
const cleanPlayer = ref<HTMLAudioElement | null>(null)
let timer: ReturnType<typeof setInterval> | null = null

const algorithmOptions = [
  { value: 'subtraction', label: 'Spectral subtraction' },
  { value: 'wiener', label: 'Wiener filter' },
  { value: 'mmse-lsa', label: 'MMSE-LSA' },
  { value: 'rnnoise', label: 'RNNoise' },
  { value: 'deepfilter', label: 'DeepFilterNet3' },
]

function formatSnr(value: number): string {
  return Number.isFinite(value) ? `${value.toFixed(1)} dB` : 'n/a'
}

function formatLevel(value: number): string {
  return Number.isFinite(value) ? `${value.toFixed(1)} dBFS` : 'n/a'
}

function formatDuration(milliseconds: number): string {
  if (milliseconds <= 0)
    return 'none'
  return milliseconds >= 1000
    ? `${(milliseconds / 1000).toFixed(2)} s`
    : `${milliseconds.toFixed(0)} ms`
}

function qualityLabel(snr: number): string {
  if (snr >= 25)
    return 'Clean'
  if (snr >= 15)
    return 'Good'
  if (snr >= 8)
    return 'Noisy'
  return 'Very noisy'
}

const snrChange = computed(() => {
  if (!result.value)
    return ''
  const input = result.value.quality.snr_db
  if (!result.value.quality.denoised)
    return formatSnr(input)
  return `${input.toFixed(1)} to ${result.value.quality.output_snr_db.toFixed(1)} dB`
})

const appliedAlgorithm = computed(() => {
  const applied = result.value?.quality.denoise_algorithm
  if (!applied)
    return 'none'
  return algorithmOptions.find(option => option.value === applied)?.label ?? applied
})

function stopTimer() {
  if (timer !== null) {
    clearInterval(timer)
    timer = null
  }
}

function stopPlayback() {
  for (const player of [rawPlayer.value, cleanPlayer.value]) {
    if (player) {
      player.pause()
      player.currentTime = 0
    }
  }
  playing.value = null
}

function applyResult(recorded: AudioTestResult) {
  stopPlayback()
  result.value = recorded
  algorithm.value = recorded.algorithm
  strengthPercent.value = Math.round(recorded.strength * 100)
  rawText.value = null
  cleanText.value = null
}

async function startRecording() {
  error.value = null
  rawText.value = null
  cleanText.value = null
  try {
    await invoke('start_audio_test_recording')
    recording.value = true
    recordingSeconds.value = 0
    timer = setInterval(() => {
      recordingSeconds.value += 1
    }, 1000)
  }
  catch (e) {
    error.value = String(e)
  }
}

async function stopRecording() {
  stopTimer()
  recording.value = false
  try {
    applyResult(await invoke<AudioTestResult>('stop_audio_test_recording'))
  }
  catch (e) {
    error.value = String(e)
  }
}

async function reprocess() {
  processing.value = true
  error.value = null
  try {
    applyResult(await invoke<AudioTestResult>('process_audio_test', {
      mode: 'denoise',
      algorithm: algorithm.value,
      strength: strengthPercent.value / 100,
    }))
  }
  catch (e) {
    error.value = String(e)
  }
  finally {
    processing.value = false
  }
}

function togglePlay(kind: 'raw' | 'clean') {
  const player = kind === 'raw' ? rawPlayer.value : cleanPlayer.value
  const other = kind === 'raw' ? cleanPlayer.value : rawPlayer.value
  if (!player)
    return

  if (!player.paused) {
    stopPlayback()
    return
  }

  if (other) {
    other.pause()
    other.currentTime = 0
  }
  player.currentTime = 0
  player.play()
    .then(() => {
      playing.value = kind
    })
    .catch((e) => {
      error.value = String(e)
    })
}

async function transcribe(kind: 'raw' | 'clean') {
  transcribing.value = kind
  error.value = null
  try {
    const text = await invoke<string>('transcribe_audio_test', { kind })
    if (kind === 'raw')
      rawText.value = text
    else
      cleanText.value = text
  }
  catch (e) {
    error.value = String(e)
  }
  finally {
    transcribing.value = null
  }
}

onBeforeUnmount(() => {
  stopTimer()
  stopPlayback()
  if (recording.value)
    void invoke('cancel_audio_test_recording')
})
</script>

<template>
  <div class="audio-test-card">
    <div class="test-header">
      <span class="test-title">Test your microphone</span>
      <span class="test-hint">Record a sample, then compare the original with the cleaned audio at the same level.</span>
    </div>

    <div class="test-actions">
      <button
        class="btn btn-secondary btn-small"
        :class="{ 'btn-recording': recording }"
        @click="recording ? stopRecording() : startRecording()"
      >
        {{ recording ? `Stop (${recordingSeconds} s)` : 'Record' }}
      </button>
    </div>

    <template v-if="result">
      <div class="test-apply">
        <div class="apply-field">
          <label>Algorithm</label>
          <AppSelect
            :model-value="algorithm"
            :options="algorithmOptions"
            @update:model-value="value => value && (algorithm = value as DenoiseAlgorithm)"
          />
        </div>
        <div class="apply-field apply-strength">
          <label>Strength</label>
          <AppSlider
            :model-value="strengthPercent"
            :min="0"
            :max="100"
            :step="5"
            unit="%"
            aria-label="Denoise strength for the test sample"
            @update:model-value="value => (strengthPercent = value)"
          />
        </div>
        <button class="btn btn-secondary btn-small" :disabled="processing || recording" @click="reprocess">
          {{ processing ? 'Processing...' : 'Apply to sample' }}
        </button>
      </div>

      <p class="test-hint">
        Record once, then apply every algorithm and strength to the same sample. This does not change the settings above.
      </p>
    </template>

    <template v-if="result">
      <div class="test-metrics">
        <span class="metric">
          <span class="metric-label">Signal</span>
          <span class="metric-value">{{ qualityLabel(result.quality.snr_db) }}</span>
        </span>
        <span class="metric">
          <span class="metric-label">SNR input to output</span>
          <span class="metric-value">{{ snrChange }}</span>
        </span>
        <span class="metric">
          <span class="metric-label">Noise floor</span>
          <span class="metric-value">{{ formatLevel(result.quality.output_noise_dbfs) }}</span>
        </span>
        <span v-if="result.quality.hum_hz" class="metric">
          <span class="metric-label">Hum</span>
          <span class="metric-value">{{ Math.round(result.quality.hum_hz) }} Hz</span>
        </span>
        <span v-if="result.quality.gain_db > 0.1" class="metric">
          <span class="metric-label">Gain</span>
          <span class="metric-value">+{{ result.quality.gain_db.toFixed(1) }} dB</span>
        </span>
        <span class="metric">
          <span class="metric-label">Denoise</span>
          <span class="metric-value">
            {{ appliedAlgorithm }}
            <template v-if="result.quality.denoised"> · {{ Math.round(result.strength * 100) }} %</template>
          </span>
        </span>
        <span class="metric">
          <span class="metric-label">Processing</span>
          <span class="metric-value">{{ formatDuration(result.quality.denoise_ms) }}</span>
        </span>
      </div>

      <div class="compare-grid">
        <div class="compare-column">
          <span class="compare-title">Original</span>
          <div class="compare-actions">
            <button class="btn btn-secondary btn-small" @click="togglePlay('raw')">
              {{ playing === 'raw' ? 'Stop' : 'Play' }}
            </button>
            <button class="btn btn-secondary btn-small" :disabled="transcribing !== null" @click="transcribe('raw')">
              {{ transcribing === 'raw' ? 'Transcribing...' : 'Transcribe' }}
            </button>
          </div>
          <p v-if="rawText !== null" class="compare-text">
            {{ rawText || '(no text)' }}
          </p>
        </div>

        <div class="compare-column">
          <span class="compare-title">Cleaned</span>
          <div class="compare-actions">
            <button class="btn btn-secondary btn-small" @click="togglePlay('clean')">
              {{ playing === 'clean' ? 'Stop' : 'Play' }}
            </button>
            <button class="btn btn-secondary btn-small" :disabled="transcribing !== null" @click="transcribe('clean')">
              {{ transcribing === 'clean' ? 'Transcribing...' : 'Transcribe' }}
            </button>
          </div>
          <p v-if="cleanText !== null" class="compare-text">
            {{ cleanText || '(no text)' }}
          </p>
        </div>
      </div>

      <audio ref="rawPlayer" :src="result.raw_audio" preload="auto" class="audio-player" @ended="playing = null" />
      <audio ref="cleanPlayer" :src="result.clean_audio" preload="auto" class="audio-player" @ended="playing = null" />
    </template>

    <p v-if="error" class="test-error">
      {{ error }}
    </p>
  </div>
</template>

<style scoped>
.audio-test-card {
  padding: 12px;
  background: var(--bg-weak);
  border: 1px solid var(--border);
  border-radius: 4px;
  display: flex;
  flex-direction: column;
  gap: 10px;
}

.test-header {
  display: flex;
  flex-direction: column;
  gap: 2px;
}

.test-title {
  font-size: 11px;
  color: var(--text-strong);
}

.test-hint {
  margin: 0;
  font-size: 10px;
  color: var(--text-weak);
}

.test-actions,
.compare-actions {
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
}

.test-apply {
  display: flex;
  flex-wrap: wrap;
  align-items: flex-end;
  gap: 8px;
}

.apply-field {
  display: flex;
  flex-direction: column;
  gap: 3px;
}

.apply-strength {
  min-width: 220px;
  flex: 1;
}

.apply-field label {
  font-size: 9px;
  text-transform: uppercase;
  letter-spacing: 0.04em;
  color: var(--text-weak);
}

.btn-small {
  padding: 6px 10px;
  font-size: 11px;
}

.btn-recording {
  border-color: var(--recording);
  color: var(--recording);
}

.test-metrics {
  display: flex;
  flex-wrap: wrap;
  gap: 12px;
}

.metric {
  display: flex;
  flex-direction: column;
  gap: 1px;
}

.metric-label {
  font-size: 9px;
  text-transform: uppercase;
  letter-spacing: 0.04em;
  color: var(--text-weak);
}

.metric-value {
  font-size: 11px;
  color: var(--text-strong);
}

.compare-grid {
  display: grid;
  grid-template-columns: 1fr 1fr;
  gap: 12px;
}

.compare-column {
  display: flex;
  flex-direction: column;
  gap: 6px;
}

.compare-title {
  font-size: 10px;
  color: var(--text-weak);
}

.compare-text {
  margin: 0;
  font-size: 11px;
  color: var(--text-strong);
  line-height: 1.4;
  white-space: pre-wrap;
}

.audio-player {
  display: none;
}

.test-error {
  margin: 0;
  font-size: 11px;
  color: var(--recording);
}
</style>
