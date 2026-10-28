import type { HistoryEntry } from '../types'
import { invoke } from '@tauri-apps/api/core'
import { readonly, ref } from 'vue'

const entries = ref<HistoryEntry[]>([])
const loading = ref(false)
const error = ref<string | null>(null)

async function load() {
  loading.value = true
  error.value = null
  try {
    entries.value = await invoke<HistoryEntry[]>('list_history')
  }
  catch (e) {
    console.error('Failed to load history:', e)
    error.value = String(e)
  }
  finally {
    loading.value = false
  }
}

async function remove(id: string) {
  try {
    await invoke('delete_history_entry', { id })
    entries.value = entries.value.filter(entry => entry.id !== id)
  }
  catch (e) {
    console.error('Failed to delete history entry:', e)
    error.value = String(e)
  }
}

async function clear() {
  try {
    await invoke('clear_history')
    entries.value = []
  }
  catch (e) {
    console.error('Failed to clear history:', e)
    error.value = String(e)
  }
}

async function copy(text: string) {
  await invoke('copy_text', { text })
}

export const historyStore = {
  entries: readonly(entries),
  loading: readonly(loading),
  error: readonly(error),
  load,
  remove,
  clear,
  copy,
}
