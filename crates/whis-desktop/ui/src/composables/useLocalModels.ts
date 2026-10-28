import type { UnlistenFn } from '@tauri-apps/api/event'
import type { LocalModelInfo } from '../types'
import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import { computed, onMounted, onUnmounted, ref } from 'vue'
import { settingsStore } from '../stores/settings'

interface DownloadProgressEvent {
  model_id: string
  downloaded: number
  total: number
}

/**
 * Composable for the local model catalog: listing, downloading, deleting, and
 * selecting models. The catalog itself lives in whis-core, so this only talks
 * to the backend commands.
 */
export function useLocalModels() {
  const models = ref<LocalModelInfo[]>([])
  const loading = ref(false)
  const status = ref('')
  let downloadUnlisten: UnlistenFn | null = null

  const selectedId = computed(() => settingsStore.state.transcription.local_models.selected)
  const customPath = computed(() => settingsStore.state.transcription.local_models.custom_path)
  const download = computed(() => settingsStore.state.modelDownload)

  const selectedModel = computed(() =>
    models.value.find(model => model.id === selectedId.value) ?? null,
  )
  const downloadingId = computed(() => download.value.active ? download.value.modelId : null)
  const progress = computed(() => download.value.progress)
  const progressPercent = computed(() => {
    const p = progress.value
    if (!p || p.total === 0)
      return 0
    return Math.min(100, Math.round((p.downloaded / p.total) * 100))
  })
  const progressText = computed(() => {
    const p = progress.value
    if (!p)
      return ''
    const mb = (p.downloaded / 1_000_000).toFixed(1)
    if (p.total === 0)
      return `${mb} MB downloaded`
    const totalMb = (p.total / 1_000_000).toFixed(0)
    return `${mb} of ${totalMb} MB (${progressPercent.value}%)`
  })
  const isSelectedInstalled = computed(() => selectedModel.value?.installed ?? false)

  async function loadModels() {
    loading.value = true
    try {
      models.value = await invoke<LocalModelInfo[]>('get_local_models')
    }
    catch (e) {
      console.error('Failed to load local models:', e)
    }
    finally {
      loading.value = false
    }
  }

  async function downloadModel(id: string) {
    const model = models.value.find(entry => entry.id === id)
    if (!model || !model.available)
      return

    status.value = ''
    settingsStore.startModelDownload(id)
    try {
      await invoke<string>('download_local_model', { modelId: id })
      status.value = `${model.name} installed`
      await loadModels()
    }
    catch (e) {
      settingsStore.failModelDownload(String(e))
      status.value = `${model.name} download failed: ${e}`
    }
    finally {
      settingsStore.completeModelDownload()
    }
  }

  async function deleteModel(id: string) {
    const model = models.value.find(entry => entry.id === id)
    if (!model)
      return

    status.value = ''
    try {
      await invoke('delete_local_model', { modelId: id })
      status.value = `${model.name} deleted`
      await loadModels()
    }
    catch (e) {
      status.value = `Could not delete ${model.name}: ${e}`
    }
  }

  function selectModel(id: string | null) {
    if (!id)
      return
    const model = models.value.find(entry => entry.id === id)
    // Unavailable entries are listed so the reason is visible, but they cannot
    // be selected as the active model.
    if (!model || !model.available)
      return
    settingsStore.setLocalModel(id)
  }

  function setCustomPath(path: string | null) {
    settingsStore.setLocalModelPath(path && path.length > 0 ? path : null)
  }

  onMounted(async () => {
    await loadModels()
    downloadUnlisten = await listen<DownloadProgressEvent>('download-progress', (event) => {
      settingsStore.updateModelDownloadProgress(event.payload.downloaded, event.payload.total)
    })
  })

  onUnmounted(() => {
    downloadUnlisten?.()
  })

  return {
    models,
    loading,
    status,
    selectedId,
    selectedModel,
    customPath,
    downloadingId,
    progress,
    progressPercent,
    progressText,
    isSelectedInstalled,
    loadModels,
    downloadModel,
    deleteModel,
    selectModel,
    setCustomPath,
  }
}
