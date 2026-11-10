<script setup lang="ts">
import type { AudioTestResult, DenoiseAlgorithm, SelectOption } from '../../types'
import { invoke } from '@tauri-apps/api/core'
import { computed, ref } from 'vue'

const recording = ref(false)
const processing = ref(false)
const transcribing = ref<'raw' | 'clean' | null>(null)
const result = ref<AudioTestResult | null>(null)
const rawText = ref<string | null>(null)
const cleanText = ref<string | null>(null)
const error = ref<string | null>(null)

// Settings used for re-processing a stored sample. Both start from whatever the
// recording used, so the first comparison matches the current configuration.
const algorithm = ref<DenoiseAlgorithm>('mmse-lsa')
const strength = ref<'denoise' | 'strong'>('denoise')

const algorithmOptions: SelectOption[] = [
  { value: 'subtraction', label: 'Spectral subtraction' },
  { value: 'wiener', label: 'Wiener filter' },
  { value: 'mmse-lsa', label: 'MMSE-LSA' },
  { value: 'rnnoise', label: 'RNNoise' },
  { value: 'deepfilter', label: 'DeepFilterNet3' },
]

const strengthOptions: SelectOption[] = [
  { value: 'denoise', label: 'Balanced' },
  { value: 'strong', label: 'Strong' },
]

function formatSnr(value: number): string {
  return Number.isFinite(value) ? `${value.toFixed(1)} dB` : 'n/a'
}

function formatLevel(value: number): string {
  return Number.isFinite(value) ? `${value.toFixed(1)} dBFS` : 'n/a'
}

const snrChange = computed(() => {
  if (!result.value)
    return ''
  const input = result.value.quality.snr_db
  const output = result.value.quality.output_snr_db
  if (!result.value.quality.denoised)
    return formatSnr(input)
  return `${input.toFixed(1)} to ${output.toFixed(1)} dB`
})

function qualityLabel(snr: number): string {
  if (snr >= 25)
    return 'Clean'
  if (snr >= 15)
    return 'Good'
  if (snr >= 8)
    return 'Noisy'
  return 'Very noisy'
}

function formatDuration(milliseconds: number): string {
  if (milliseconds <= 0)
    return 'none'
  return milliseconds >= 1000
    ? `${(milliseconds / 1000).toFixed(2)} s`
    : `${milliseconds.toFixed(0)} ms`
}

const appliedAlgorithm = computed(() => {
  const applied = result.value?.quality.denoise_algorithm
  if (!applied)
    return 'none'
  return algorithmOptions.find(option => option.value === applied)?.label ?? applied
})

function handleAlgorithmChange(value: string | null) {
  if (value)
    algorithm.value = value as DenoiseAlgorithm
}

function handleStrengthChange(value: string | null) {
  if (value)
    strength.value = value as 'denoise' | 'strong'
}

async function record() {
  recording.value = true
  error.value = null
  rawText.value = null
  cleanText.value = null
  try {
    const recorded = await invoke<AudioTestResult>('record_audio_test')
    result.value = recorded
    algorithm.value = recorded.algorithm
    strength.value = recorded.mode === 'strong' ? 'strong' : 'denoise'
  }
  catch (e) {
    error.value = String(e)
  }
  finally {
    recording.value = false
  }
}

async function reprocess() {
  processing.value = true
  error.value = null
  cleanText.value = null
  try {
    result.value = await invoke<AudioTestResult>('process_audio_test', {
      mode: strength.value,
      algorithm: algorithm.value,
    })
  }
  catch (e) {
    error.value = String(e)
  }
  finally {
    processing.value = false
  }
}

async function play(kind: 'raw' | 'clean') {
  error.value = null
  try {
    await invoke('play_audio_test', { kind })
  }
  catch (e) {
    error.value = String(e)
  }
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
</script>

<template>
  <div class="audio-test-card">
    <div class="test-header">
      <span class="test-title">Test your microphone</span>
      <span class="test-hint">Records 6 seconds and compares the original with the cleaned audio at the same level.</span>
    </div>

    <div class="test-actions">
      <button class="btn btn-secondary btn-small" :disabled="recording" @click="record">
        {{ recording ? 'Recording...' : 'Record test' }}
      </button>
    </div>

    <div v-if="result" class="test-apply">
      <div class="apply-field">
        <label>Algorithm</label>
        <AppSelect
          :model-value="algorithm"
          :options="algorithmOptions"
          @update:model-value="handleAlgorithmChange"
        />
      </div>
      <div class="apply-field">
        <label>Strength</label>
        <AppSelect
          :model-value="strength"
          :options="strengthOptions"
          @update:model-value="handleStrengthChange"
        />
      </div>
      <button class="btn btn-secondary btn-small" :disabled="processing" @click="reprocess">
        {{ processing ? 'Processing...' : 'Apply to sample' }}
      </button>
    </div>

    <p v-if="result" class="test-hint">
      Record once, then apply every algorithm to the same sample to compare them. This does not change the settings above.
    </p>

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
          <span class="metric-value">{{ appliedAlgorithm }}</span>
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
            <button class="btn btn-secondary btn-small" @click="play('raw')">
              Play
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
            <button class="btn btn-secondary btn-small" @click="play('clean')">
              Play
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

.test-error {
  margin: 0;
  font-size: 11px;
  color: var(--recording);
}
</style>
