<script setup lang="ts">
import type { AudioTestResult } from '../../types'
import { invoke } from '@tauri-apps/api/core'
import { ref } from 'vue'

const recording = ref(false)
const transcribing = ref<'raw' | 'clean' | null>(null)
const result = ref<AudioTestResult | null>(null)
const rawText = ref<string | null>(null)
const cleanText = ref<string | null>(null)
const error = ref<string | null>(null)

function formatSnr(value: number): string {
  return Number.isFinite(value) ? `${value.toFixed(1)} dB` : 'n/a'
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

async function record() {
  recording.value = true
  error.value = null
  rawText.value = null
  cleanText.value = null
  try {
    result.value = await invoke<AudioTestResult>('record_audio_test')
  }
  catch (e) {
    error.value = String(e)
  }
  finally {
    recording.value = false
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

    <template v-if="result">
      <div class="test-metrics">
        <span class="metric">
          <span class="metric-label">Signal</span>
          <span class="metric-value">{{ qualityLabel(result.quality.snr_db) }}</span>
        </span>
        <span class="metric">
          <span class="metric-label">SNR</span>
          <span class="metric-value">{{ formatSnr(result.quality.snr_db) }}</span>
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
          <span class="metric-label">Mode</span>
          <span class="metric-value">{{ result.mode }}</span>
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
  font-size: 10px;
  color: var(--text-weak);
}

.test-actions,
.compare-actions {
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
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
