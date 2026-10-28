<script setup lang="ts">
import type { UnlistenFn } from '@tauri-apps/api/event'
import type { LastAudioInfo } from '../types'
import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import { onMounted, onUnmounted, ref } from 'vue'

const info = ref<LastAudioInfo | null>(null)
const busy = ref(false)
const message = ref<string | null>(null)
const error = ref<string | null>(null)
const confirmingDelete = ref(false)
let unlisten: UnlistenFn[] = []

function formatDate(ms: number): string {
  if (!ms)
    return 'unknown'
  return new Date(ms).toLocaleString(undefined, { dateStyle: 'medium', timeStyle: 'short' })
}

function formatDuration(seconds: number): string {
  const total = Math.round(seconds)
  const minutes = Math.floor(total / 60)
  const rest = total % 60
  return minutes > 0 ? `${minutes} min ${rest} s` : `${rest} s`
}

async function refresh() {
  info.value = await invoke<LastAudioInfo | null>('get_last_audio_info')
  if (!info.value)
    confirmingDelete.value = false
}

async function retranscribe() {
  busy.value = true
  message.value = null
  error.value = null
  try {
    await invoke<string>('retranscribe_last_audio')
    message.value = 'Re-transcribed with the current settings.'
  }
  catch (e) {
    error.value = String(e)
  }
  finally {
    busy.value = false
    await refresh()
  }
}

async function play() {
  try {
    await invoke('play_last_audio')
  }
  catch (e) {
    error.value = String(e)
  }
}

async function remove() {
  if (!confirmingDelete.value) {
    confirmingDelete.value = true
    return
  }
  confirmingDelete.value = false
  try {
    await invoke('delete_last_audio')
    await refresh()
  }
  catch (e) {
    error.value = String(e)
  }
}

onMounted(async () => {
  await refresh()
  unlisten.push(await listen('last-audio-updated', () => refresh()))
  unlisten.push(await listen('retranscription-started', () => {
    busy.value = true
    message.value = null
    error.value = null
  }))
  unlisten.push(await listen('transcription-complete', () => {
    busy.value = false
  }))
  unlisten.push(await listen<string>('transcription-error', (event) => {
    busy.value = false
    error.value = event.payload
  }))
})

onUnmounted(() => {
  unlisten.forEach(fn => fn())
  unlisten = []
})
</script>

<template>
  <div v-if="info" class="last-audio-card">
    <div class="card-header">
      <span class="card-title">Last recording</span>
      <span class="card-meta">
        {{ formatDate(info.created_at_ms) }} · {{ formatDuration(info.duration_secs) }}
        <template v-if="info.model"> · {{ info.model }}</template>
      </span>
    </div>

    <div class="card-actions">
      <button class="btn btn-secondary btn-small" :disabled="busy" @click="retranscribe">
        {{ busy ? 'Transcribing...' : 'Re-transcribe' }}
      </button>
      <button class="btn btn-secondary btn-small" :disabled="busy" @click="play">
        Play
      </button>
      <button
        class="btn btn-secondary btn-small"
        :class="{ danger: confirmingDelete }"
        :disabled="busy"
        @click="remove"
      >
        {{ confirmingDelete ? 'Confirm Delete' : 'Delete' }}
      </button>
    </div>

    <p v-if="message" class="card-message">
      {{ message }}
    </p>
    <p v-if="error" class="card-error">
      {{ error }}
    </p>
  </div>
</template>

<style scoped>
.last-audio-card {
  padding: 12px;
  background: var(--bg-weak);
  border: 1px solid var(--border);
  border-radius: 4px;
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.card-header {
  display: flex;
  flex-direction: column;
  gap: 2px;
}

.card-title {
  font-size: 11px;
  color: var(--text-strong);
}

.card-meta {
  font-size: 10px;
  color: var(--text-weak);
}

.card-actions {
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
}

.btn-small {
  padding: 6px 10px;
  font-size: 11px;
}

.btn.danger {
  border-color: var(--recording);
  color: var(--recording);
}

.card-message {
  margin: 0;
  font-size: 11px;
  color: var(--text-weak);
}

.card-error {
  margin: 0;
  font-size: 11px;
  color: var(--recording);
}
</style>
