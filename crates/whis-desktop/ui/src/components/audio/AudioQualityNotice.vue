<script setup lang="ts">
import type { UnlistenFn } from '@tauri-apps/api/event'
import type { AudioQuality } from '../../types'
import { listen } from '@tauri-apps/api/event'
import { onMounted, onUnmounted, ref } from 'vue'
import { settingsStore } from '../../stores/settings'
import NoticeCard from '../NoticeCard.vue'

const quality = ref<AudioQuality | null>(null)
let unlisten: UnlistenFn | null = null

function dismiss() {
  quality.value = null
}

function enableAutoDenoise() {
  settingsStore.setEnhanceMode('auto')
  dismiss()
}

onMounted(async () => {
  unlisten = await listen<AudioQuality>('audio-quality-warning', (event) => {
    quality.value = event.payload
  })
})

onUnmounted(() => {
  unlisten?.()
  unlisten = null
})
</script>

<template>
  <div v-if="quality" class="quality-notice">
    <NoticeCard marker="[!]" variant="warning">
      <p>
        <strong>Noisy audio detected</strong>
        (SNR {{ quality.snr_db.toFixed(1) }} dB). Auto denoise can remove steady background noise
        and usually improves transcription accuracy.
      </p>
      <div class="quality-actions">
        <button class="btn btn-secondary btn-small" @click="enableAutoDenoise">
          Enable Auto denoise
        </button>
        <button class="btn btn-secondary btn-small" @click="dismiss">
          Dismiss
        </button>
      </div>
    </NoticeCard>
  </div>
</template>

<style scoped>
.quality-notice {
  padding: 0 24px 16px;
}

.quality-actions {
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
  margin-top: 8px;
}

.btn-small {
  padding: 6px 10px;
  font-size: 11px;
}
</style>
