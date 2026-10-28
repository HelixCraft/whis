<script setup lang="ts">
import type { UnlistenFn } from '@tauri-apps/api/event'
import type { HistoryEntry } from '../types'
import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import { onMounted, onUnmounted, ref } from 'vue'
import { historyStore } from '../stores/history'

// Unwrap the store refs once: nested refs are not auto-unwrapped in templates.
const entries = historyStore.entries
const loading = historyStore.loading
const error = historyStore.error

const pendingDelete = ref<string | null>(null)
const confirmingClear = ref(false)
const copiedId = ref<string | null>(null)
const expandedId = ref<string | null>(null)
const storagePath = ref('')
let unlistenHistory: UnlistenFn | null = null
let copiedTimer: number | null = null

function formatDate(ms: number): string {
  if (!ms)
    return 'unknown'
  return new Date(ms).toLocaleString(undefined, { dateStyle: 'medium', timeStyle: 'short' })
}

function formatDuration(seconds: number): string {
  if (!seconds || seconds < 1)
    return ''
  const total = Math.round(seconds)
  const minutes = Math.floor(total / 60)
  const rest = total % 60
  return minutes > 0 ? `${minutes}m ${rest}s` : `${rest}s`
}

function firstLine(entry: HistoryEntry): string {
  const line = entry.text.split('\n').find(l => l.trim().length > 0) ?? ''
  return line.length > 140 ? `${line.slice(0, 140)}...` : line
}

function toggleExpand(id: string) {
  expandedId.value = expandedId.value === id ? null : id
  pendingDelete.value = null
}

async function copyEntry(entry: HistoryEntry) {
  try {
    await historyStore.copy(entry.text)
    copiedId.value = entry.id
    if (copiedTimer)
      clearTimeout(copiedTimer)
    copiedTimer = window.setTimeout(() => {
      copiedId.value = null
    }, 1500)
  }
  catch (e) {
    console.error('Failed to copy text:', e)
  }
}

async function requestDelete(id: string) {
  if (pendingDelete.value !== id) {
    pendingDelete.value = id
    return
  }
  pendingDelete.value = null
  await historyStore.remove(id)
}

async function clearAll() {
  if (!confirmingClear.value) {
    confirmingClear.value = true
    return
  }
  confirmingClear.value = false
  await historyStore.clear()
}

async function openFolder() {
  try {
    await invoke('open_history_folder')
  }
  catch (e) {
    console.error('Failed to open history folder:', e)
  }
}

onMounted(async () => {
  await historyStore.load()
  storagePath.value = await invoke<string>('history_storage_path')
  unlistenHistory = await listen('history-updated', () => {
    historyStore.load()
  })
})

onUnmounted(() => {
  unlistenHistory?.()
  if (copiedTimer)
    clearTimeout(copiedTimer)
})
</script>

<template>
  <section class="section history-section">
    <header class="section-header">
      <h1>History</h1>
      <p>Every transcription is saved to disk</p>
    </header>

    <div class="section-content">
      <div class="history-toolbar">
        <span class="entry-count">
          {{ entries.length }} {{ entries.length === 1 ? 'entry' : 'entries' }}
        </span>
        <div class="toolbar-actions">
          <button class="btn btn-secondary btn-small" @click="openFolder">
            Open Folder
          </button>
          <button
            class="btn btn-secondary btn-small"
            :class="{ danger: confirmingClear }"
            :disabled="entries.length === 0"
            @click="clearAll"
          >
            {{ confirmingClear ? 'Confirm Clear All' : 'Clear All' }}
          </button>
        </div>
      </div>

      <p v-if="storagePath" class="storage-path">
        {{ storagePath }}
      </p>

      <div v-if="error" class="config-notice error">
        <span class="notice-marker">[!]</span>
        <p>{{ error }}</p>
      </div>

      <div v-if="loading" class="empty-state">
        Loading history...
      </div>

      <div v-else-if="entries.length === 0" class="empty-state">
        No transcriptions yet. The next recording will show up here.
      </div>

      <ul v-else class="history-list">
        <li
          v-for="entry in entries"
          :key="entry.id"
          class="history-entry"
          :class="{ expanded: expandedId === entry.id }"
          @click="toggleExpand(entry.id)"
        >
          <div class="entry-header">
            <div class="entry-meta">
              <span class="entry-date">{{ formatDate(entry.created_at_ms) }}</span>
              <span v-if="entry.source === 'retranscription'" class="entry-badge">re-transcribed</span>
              <span v-if="formatDuration(entry.duration_secs)" class="entry-duration">{{ formatDuration(entry.duration_secs) }}</span>
            </div>
            <div class="entry-actions">
              <button
                class="icon-btn"
                :aria-label="copiedId === entry.id ? 'Copied' : 'Copy text'"
                :title="copiedId === entry.id ? 'Copied' : 'Copy'"
                @click.stop="copyEntry(entry)"
              >
                {{ copiedId === entry.id ? '[ok]' : '[copy]' }}
              </button>
              <button
                class="icon-btn"
                :class="{ danger: pendingDelete === entry.id }"
                :aria-label="pendingDelete === entry.id ? 'Confirm delete' : 'Delete entry'"
                :title="pendingDelete === entry.id ? 'Click again to delete' : 'Delete'"
                @click.stop="requestDelete(entry.id)"
              >
                {{ pendingDelete === entry.id ? '[del?]' : '[x]' }}
              </button>
            </div>
          </div>

          <p class="entry-preview">
            {{ firstLine(entry) }}
          </p>

          <pre v-if="expandedId === entry.id" class="entry-full">{{ entry.text }}</pre>
        </li>
      </ul>
    </div>
  </section>
</template>

<style scoped>
.history-section {
  height: 100%;
  display: flex;
  flex-direction: column;
}

.section-content {
  flex: 1;
  overflow-y: auto;
  min-height: 0;
}

.history-toolbar {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  margin-bottom: 8px;
}

.entry-count {
  font-size: 11px;
  color: var(--text-weak);
}

.toolbar-actions {
  display: flex;
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

.storage-path {
  margin: 0 0 16px;
  font-size: 10px;
  color: var(--text-weak);
  opacity: 0.6;
  word-break: break-all;
}

.empty-state {
  padding: 24px 12px;
  text-align: center;
  font-size: 12px;
  color: var(--text-weak);
}

.history-list {
  list-style: none;
  padding: 0;
  margin: 0;
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.history-entry {
  padding: 10px 12px;
  background: var(--bg-weak);
  border: 1px solid var(--border);
  border-radius: 4px;
  cursor: pointer;
  transition: border-color 0.15s ease;
}

.history-entry:hover {
  border-color: var(--accent);
}

.history-entry.expanded {
  border-color: var(--accent);
}

.entry-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  margin-bottom: 4px;
}

.entry-meta {
  display: flex;
  align-items: center;
  gap: 8px;
  font-size: 10px;
  color: var(--text-weak);
  min-width: 0;
}

.entry-badge {
  padding: 1px 5px;
  border: 1px solid var(--border);
  border-radius: 3px;
  text-transform: uppercase;
  letter-spacing: 0.04em;
}

.entry-actions {
  display: flex;
  gap: 4px;
  flex-shrink: 0;
}

.icon-btn {
  padding: 2px 6px;
  font-family: var(--font);
  font-size: 10px;
  color: var(--text-weak);
  background: transparent;
  border: 1px solid transparent;
  border-radius: 3px;
  cursor: pointer;
}

.icon-btn:hover {
  color: var(--accent);
  border-color: var(--border);
}

.icon-btn.danger {
  color: var(--recording);
  border-color: var(--recording);
}

.entry-preview {
  margin: 0;
  font-size: 12px;
  color: var(--text);
  line-height: 1.4;
  word-break: break-word;
}

.entry-full {
  margin: 10px 0 0;
  padding: 10px;
  font-family: var(--font);
  font-size: 12px;
  line-height: 1.5;
  color: var(--text);
  background: var(--bg);
  border: 1px solid var(--border);
  border-radius: 3px;
  white-space: pre-wrap;
  word-break: break-word;
}

.config-notice {
  display: flex;
  align-items: flex-start;
  gap: 10px;
  padding: 12px;
  margin-bottom: 12px;
  background: var(--bg-weak);
  border: 1px solid var(--border);
  border-radius: 4px;
  font-size: 12px;
}

.config-notice p {
  margin: 0;
}

.config-notice.error .notice-marker {
  color: #f87171;
}
</style>
