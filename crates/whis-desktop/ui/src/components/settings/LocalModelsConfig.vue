<!-- LocalModelsConfig: catalog-driven local model selection, download, and removal -->
<script setup lang="ts">
import type { SelectOption } from '../../types'
import { computed } from 'vue'
import { useLocalModels } from '../../composables/useLocalModels'
import AppSelect from '../AppSelect.vue'

const props = defineProps<{
  showConfigCard?: boolean
}>()

const {
  models,
  status,
  selectedId,
  selectedModel,
  downloadingId,
  progress,
  progressPercent,
  progressText,
  downloadModel,
  deleteModel,
  selectModel,
} = useLocalModels()

const modelOptions = computed<SelectOption[]>(() =>
  models.value
    .filter(model => model.available)
    .map(model => ({
      value: model.id,
      label: `${model.name}${model.installed ? ' [installed]' : ''} - ${model.engine_label} - ${model.size_label}`,
    })),
)

// Models that cannot be used are listed separately instead of in the dropdown,
// so their reason stays visible without being selectable.
const unavailableModels = computed(() =>
  models.value.filter(model => !model.available),
)

const isDownloading = computed(() => downloadingId.value !== null)

const actionLabel = computed(() => {
  if (downloadingId.value && downloadingId.value === selectedId.value)
    return `${progressPercent.value}%`
  if (selectedModel.value?.installed)
    return 'Delete'
  return 'Download'
})

const actionDisabled = computed(() => {
  if (isDownloading.value)
    return true
  return false
})

function handleModelChange(value: string | null) {
  selectModel(value)
}

function handleAction() {
  const id = selectedId.value
  if (!id)
    return
  if (selectedModel.value?.installed)
    deleteModel(id)
  else
    downloadModel(id)
}
</script>

<template>
  <div v-if="props.showConfigCard" class="config-card">
    <div class="model-selector">
      <AppSelect
        :model-value="selectedId"
        :options="modelOptions"
        :disabled="isDownloading"
        aria-label="Local model"
        @update:model-value="handleModelChange"
      />
      <button
        class="btn-primary"
        :disabled="actionDisabled || !selectedId"
        @click="handleAction"
      >
        {{ actionLabel }}
      </button>
    </div>

    <p v-if="progress && downloadingId === selectedId" class="hint">
      {{ progressText }}
    </p>
    <p v-else-if="status" class="hint" :class="{ error: status.includes('failed') || status.includes('Could not') }">
      {{ status }}
    </p>

    <p v-if="!selectedId" class="hint">
      Choose a model, then download it.
    </p>

    <p v-if="selectedModel" class="model-meta">
      {{ selectedModel.engine_label }} · {{ selectedModel.languages }} · {{ selectedModel.size_label }} · {{ selectedModel.license }}
    </p>
    <p v-if="selectedModel" class="hint">
      {{ selectedModel.description }}
    </p>

    <div v-if="unavailableModels.length" class="unavailable">
      <p class="hint">
        Not usable here:
      </p>
      <p v-for="model in unavailableModels" :key="model.id" class="hint">
        {{ model.name }} - {{ model.note }}
      </p>
    </div>
  </div>
</template>

<style scoped>
.config-card {
  display: flex;
  flex-direction: column;
  gap: 8px;
  margin-bottom: 20px;
}

.model-selector {
  display: flex;
  gap: 8px;
}

.model-selector :deep(.custom-select) {
  flex: 1;
  min-width: 0;
}

.model-selector .btn-primary {
  flex-shrink: 0;
  min-width: 80px;
}

.btn-primary {
  padding: 10px 20px;
  background: var(--accent);
  border: none;
  border-radius: 4px;
  font-family: var(--font);
  font-size: 12px;
  color: var(--bg);
  cursor: pointer;
  transition: all 0.15s ease;
}

.btn-primary:hover:not(:disabled) {
  filter: brightness(1.1);
}

.btn-primary:focus-visible {
  outline: 2px solid var(--accent);
  outline-offset: 2px;
}

.btn-primary:disabled {
  opacity: 0.6;
  cursor: not-allowed;
}

.model-meta {
  font-size: 11px;
  color: var(--text);
  margin: 0;
}

.hint {
  font-size: 11px;
  color: var(--text-weak);
  margin: 0;
}

.hint.error {
  color: #f87171;
}

.unavailable {
  display: flex;
  flex-direction: column;
  gap: 2px;
  margin-top: 4px;
}
</style>
