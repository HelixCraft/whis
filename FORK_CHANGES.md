# Fork changes

This document lists every change this fork (`HelixCraft/whis`) contains relative
to upstream (`frankdierolf/whis`). It covers implemented features, architecture
changes, removed code, and build/documentation changes, not just the most recent
work.

- Upstream baseline: `0e39d9a` (`feat(settings): separate CLI and desktop config storage`)
- Fork HEAD at the time of writing: `a12f708`
- Fork commits ahead of upstream: 6

| Commit | Summary |
|---|---|
| `ba7b020` | Model catalog, transcription history, last recording, live preview |
| `cbc516a` | Last-recording card moved from the Home tab to the History tab |
| `3500d78` | Roadmap in `TODO.md` |
| `2f7fc74` | Core audio cleanup and enhanced transcription API |
| `84a0dde` | Desktop cleanup settings, quality metrics, and test recording |
| `17027f7` | Fork changes documentation |
| `a12f708` | Release workflow adapted for the CPU-only fork |

---

## 1. Local model catalog and unified local engine

Upstream supported two hard-coded model families (`LocalWhisper` and
`LocalParakeet`) with separate download, storage, and settings code paths. The
fork replaces this with a single catalog that is the source of truth for the UI,
the CLI, and the runtime.

### Catalog

`crates/whis-core/src/model/catalog.rs`

- One `LocalModelSpec` per model: stable id, display name, engine, format,
  languages, size, license, description, download URL, target directory,
  required files, and whether the model is runnable.
- 16 entries, 14 runnable in a normal build:
  - Whisper GGML: Tiny, Base, Small, Medium (`transcribe-cpp`)
  - Parakeet ONNX: TDT 0.6B v3, TDT 0.6B v2 (`transcribe-rs` / ONNX Runtime)
  - Parakeet GGUF: Ultra Q4, Ultra Q8 (`transcribe-cpp`)
  - Qwen3-ASR: 0.6B, 1.7B GGUF (`transcribe-cpp`)
  - Moonshine: Tiny, Base GGUF (`transcribe-cpp`)
  - Bark 3.0 and Phonon 2.5 are listed as known models without a supported
    runtime so the catalog can be extended without code changes.
- Default model for new local configurations: `parakeet-v3`.
- Helpers: `all_models`, `find`, `id_for_path`, `engine_for_path`, `models_dir`,
  per-model `path`, `is_installed`, `verify`, `size_label`.

### Engines

- `ModelEngine::WhisperCpp` (Whisper `.bin`), `ModelEngine::Gguf` (Qwen3-ASR,
  Moonshine, Parakeet Ultra), `ModelEngine::Onnx` (Parakeet), and
  `ModelEngine::Unsupported`.
- New `crates/whis-core/src/provider/local_cpp.rs` runs both Whisper GGML and
  all GGUF models through `transcribe-cpp` 0.4.1 with the CPU backend. Models
  stay loaded between jobs through the existing cache.
- `provider/local_whisper.rs` was removed. `provider/local_parakeet.rs` now only
  handles ONNX and delegates to `transcribe-rs` 0.3.
- The duplicate GGML symbol problem (`ggml_backend_device_register` linked twice)
  is gone because only one GGML implementation is linked.
- Local transcription is CPU-only. Vulkan was evaluated and removed: the
  Debian bookworm Vulkan headers are older than what ggml expects
  (`layer_setting_info` is missing), so a Vulkan build fails.

### Provider model

- `TranscriptionProvider::Local` is the only local provider (serde name
  `local`). Legacy serialized names (`local-whisper`, `local-parakeet`,
  `whisper`, `parakeet`, ...) are aliased to it during deserialization.
- `is_local()` is used everywhere instead of matching two variants.
- Dispatch is by model path: `engine_for_path` decides between the GGML engine
  and the ONNX engine, so callers do not need to know which model is active.

### Settings migration

`crates/whis-core/src/settings/transcription.rs`

- `LocalModelsConfig` now stores:
  - `selected`: catalog id
  - `custom_path`: optional user-provided override
- A custom `Deserialize` implementation accepts the old shape with
  `whisper_path` and `parakeet_path`, maps known catalog paths to their id, and
  keeps unknown paths as `custom_path`. Existing installations keep working
  without re-downloading anything.
- `is_configured`, `local_model_path`, `selected_local_model`, and
  `has_api_key` were updated for the new model.

### Installer

`crates/whis-core/src/model/install.rs`

- Download and install with progress callback.
- Handles single-file models (GGML/GGUF) and `.tar.gz` model directories
  (Parakeet ONNX).
- Verifies required files after extraction, removes models, and reports
  per-model progress.
- Old `model/download.rs`, `model/whisper.rs`, `model/parakeet.rs`, and
  `model/types.rs` were removed.

### Desktop commands and UI

- `commands/models/local.rs` replaces `whisper.rs` and `parakeet.rs`:
  `get_local_models`, `download_local_model`, `delete_local_model`,
  `is_local_model_valid`, `get_active_download`.
- `components/settings/LocalModelsConfig.vue` replaces the separate Whisper and
  Parakeet pickers. `composables/useLocalModels.ts` replaces `useWhisperModel`
  and `useParakeetModel`.
- The picker shows size, languages, license, engine, download state, and
  progress; it supports download, cancel, delete, and selecting a custom path.
- The old `LocalWhisperConfig.vue`, `useWhisperModel.ts`, and
  `useParakeetModel.ts` were removed.

### CLI

- `whis model list` now lists all local models grouped by engine
  (`whisper`, `Parakeet`, `GGUF`) with install status and size instead of two
  separate hard-coded lists.
- `whis setup local` and `whis model` were updated to the catalog.
- Local file and progressive transcription dispatch by path
  (`engine_for_path`) instead of matching a provider variant.

---

## 2. Persistent transcription history

New feature. Every completed transcription can be stored on disk and browsed in
the desktop UI.

### Core

`crates/whis-core/src/history/`

- `types.rs`: `HistoryEntry` with id, timestamp, final text, raw text,
  `post_processed`, provider, model, language, duration, source
  (`recording` / `retranscription`), and `parent_id` for re-transcriptions.
- `store.rs`: append-only JSONL store at `<data dir>/whis/entries.jsonl`.
  - `list(max)`, `append(entry, max_entries)`, `delete(id)`, `clear()`, `count()`.
  - Pruning keeps the newest entries; `max_entries = 0` disables pruning.
  - Corrupt lines are skipped instead of failing the whole list.
- Entries are only text and metadata, not audio.

### Settings

`HistorySettings` in `settings/ui.rs`:

- `enabled` (default on)
- `max_entries` (default 500, `0` = unlimited)

### Desktop

- Commands in `commands/history.rs`: `list_history`, `delete_history_entry`,
  `clear_history`, `copy_text`, `history_storage_path`, `open_history_folder`.
- The pipeline writes history before output so a clipboard failure cannot lose
  the transcript (`record_history` in `recording/pipeline.rs`).
- New `HistoryView.vue` with search, copy, delete, clear, and a detail view that
  shows raw vs. post-processed text and the originating recording.
- `stores/history.ts` and the new route/navigation entry.

---

## 3. Last recording storage

New feature. The most recent recording is persisted before transcription starts,
so a failed transcription or a later model switch never loses the audio.

### Core

`crates/whis-core/src/audio/last_recording.rs`

- `last_audio.wav` (16 kHz mono, 16-bit PCM) and `last_audio.json` (metadata) in
  the whis data directory.
- Atomic writes (`.tmp` + rename), so a crash or full disk cannot leave a
  half-written file.
- Metadata: creation time, duration, sample rate, provider, model, language,
  and the related history entry id.
- `save`, `info`, `delete`, `load_samples`, `set_history_id`.
- One slot only: each new recording replaces the previous one.

### Settings

`LastAudioSettings` with `enabled` (default on). Disabling it deletes the stored
file immediately and hides the card.

### Desktop

- Commands in `commands/last_audio.rs`:
  - `get_last_audio_info` (returns nothing while the setting is off)
  - `play_last_audio` (opens the file with the system player)
  - `delete_last_audio`
  - `retranscribe_last_audio`: reloads the WAV, uses the current provider/model
    settings, writes a new history entry with `source = retranscription` and
    `parent_id` of the original entry, and never overwrites the original.
  - `open_data_folder`
- The audio is persisted in `stop_and_transcribe` before transcription.
- The card (`LastAudioCard.vue`) lives on the **History** tab, not the Home tab.
  The Home tab keeps only the failed-transcription retry action.

---

## 4. Live transcription preview

New feature. Progressive transcription now exposes text while the recording is
still running.

- `PartialTextCallback` in `crates/whis-core/src/transcription/transcribe.rs`.
  Both the cloud and local progressive paths call it after each finished chunk
  with the merged text so far.
- The desktop transcription task emits a `partial-transcription` Tauri event.
- `HomeView.vue` listens and shows the growing text below the record button.
- The local progressive path now also forwards the language hint, which was
  previously ignored for local models.

---

## 5. Batch transcription and audio chunk plumbing

`crates/whis-core/src/transcription/audio_input.rs` was added for
re-transcribing stored recordings and other in-memory audio:

- `transcribe_samples(provider, key_or_path, language, samples)`.
- 90-second chunks with a 1-second overlap for cloud providers.
- Overlap-aware merge (`remove_overlap`) so repeated words at chunk boundaries
  are removed.
- Local models receive the raw f32 samples; cloud providers get MP3 chunks.
- Realtime providers fall back to their batch sibling because they have no file
  API.

`transcribe.rs` also gained `samples_to_mp3` and the shared chunk/merge helpers
used by both the progressive and batch paths.

---

## 6. Desktop UI changes

- New `history` route and sidebar entry.
- `HistoryView.vue`: list, search, detail, copy, delete, clear, storage actions.
- `LastAudioCard.vue`: date, duration, model, re-transcribe, play, delete.
- New local model picker and download progress UI.
- Settings changes:
  - Storage section: "Save Transcripts", history limit, "Save Last Recording".
  - Local model section derived from the catalog.
  - Help texts for the new features.
- The transcription retry action stays on the Home tab; the last-recording card
  was moved to History in `cbc516a`.

---

## 7. CLI and mobile changes

- CLI model listing, setup, and local transcription were updated for the
  catalog and the single local provider.
- The CLI reads the same settings as the desktop app, so it automatically uses
  the selected catalog model and the new history/last-audio settings where
  applicable.
- The mobile crate was updated for the changed progressive transcription
  signatures and provider handling.

---

## 8. Dependencies and build

`crates/whis-core/Cargo.toml`

- Added `transcribe-cpp = 0.4.1` with `default-features = false` (portable CPU
  backend) for Whisper GGML and all GGUF models.
- `transcribe-rs = 0.3` is now only used with the `onnx` feature for Parakeet.
- The `local-gguf` cargo feature was removed; GGUF support is part of
  `local-transcription`.
- Added `tar`, `flate2`, `tempfile`, and `libc` (feature-gated) for archive
  extraction, temp files, and stderr suppression.
- Removed the Vulkan requirement from `justfile`, `README.md`, and
  `CONTRIBUTING.md`, including `libvulkan-dev` and the Vulkan SDK checks.

### Notes

- Builds without a GPU. Local inference is CPU-only in this fork.
- The AppImage used during development is built inside a Docker image
  (`whis-build:appimage`); that workflow is local tooling, not a committed
  repository change.
- The `__TAURI_BUNDLE_TYPE variable not found` message during AppImage bundling
  is harmless updater metadata noise.

### Release CI

`.github/workflows/release.yml` is upstream's multi-platform release workflow,
adapted for this fork:

- All Vulkan SDK steps were removed; the fork builds with the portable CPU
  backend.
- Android is optional. The APK build step uses `continue-on-error`, and a
  separate `android-status` gate always succeeds, so a failed APK build does not
  block the desktop and CLI artifacts. The release still waits for Android to
  finish, so a successful APK is included.
- The workflow builds CLI tarballs for Linux x86_64/ARM64 and macOS
  Intel/Apple Silicon, a Windows CLI zip, desktop AppImage/deb/rpm (x86_64 and
  ARM64), Windows MSI/EXE, macOS DMG (Apple Silicon and Intel), and the Android
  APK.
- Trigger: push a `v*` tag, or run it manually:
  `gh workflow run -R HelixCraft/whis "Build Release" --ref main -f tag=v0.7.2`.
- The release is created as a draft and must be published manually.
- No signing certificate is configured, so Windows/macOS warn on first launch.

---

## 9. Documentation

- `README.md`: local models are now "a local model from the built-in catalog";
  lists Whisper, Parakeet, Qwen3-ASR, and Moonshine; states that local models run
  on the CPU and no GPU setup is required.
- `CONTRIBUTING.md`: Vulkan requirements removed.
- `TODO.md` added with two roadmap items:
  - Queue recordings while a transcription is running.
  - Faster and better-scaling local inference.

---

## 10. Conservative audio cleanup

New feature. Audio is cleaned up before it reaches the model, without ever
changing the stored recording.

### Core

`crates/whis-core/src/audio/enhance.rs`

- `EnhanceMode`: `off`, `light` (default), `auto`, `strong`.
- `AudioEnhanceSettings` is part of `UiSettings` and serializes as
  `{ "mode": "light" }`.
- Always-on light chain (`light`, `auto`, `strong`):
  - DC/rumble high-pass at 60 Hz (Q 0.707).
  - Narrow notches for detected 50/60 Hz mains hum and prominent harmonics.
    Detection uses Goertzel amplitudes and a local-prominence test, so a normal
    voice is not filtered.
  - Upward-only level normalisation towards -20 dBFS speech, capped at +18 dB
    and at a -1 dBFS peak ceiling. Loud input is never attenuated, so a good
    microphone cannot be made worse.
- Optional spectral denoise (`auto`, `strong`):
  - Own radix-2 FFT, 512-point window, 8 ms hop, Hann analysis/synthesis,
    smoothed Wiener gains, minimum-statistics noise estimate.
  - Gain floor: -8 dB in auto, -14 dB in strong, so speech is never gated to
    zero.
  - Auto mode triggers only when `3 dB < SNR < 15 dB` and speech is above
    -60 dBFS. A steady clean signal without pauses (held tone, singing) is not
    denoised.
- Safety gate: a chunk is skipped only when its peak is below -70 dBFS and its
  RMS below -75 dBFS. That is digital silence or a muted microphone, far below
  any audible speech.
- `AudioQuality` aggregates peak, RMS, speech, noise, SNR, applied gain, hum,
  clipping, denoise flag, and gated chunks, weighted by duration.
  `is_noisy()` drives the UI recommendation.
- Integration helpers:
  - `enhance_owned` / `enhance` for batch buffers.
  - `enhance_chunk_stream` for progressive chunks; aggregates quality and drops
    only gated chunks.
  - `enhance_audio_stream` / `StreamEnhancer` for the realtime path: stateful
    high-pass plus a slow upward gain only, no denoise, no gating, no dropped
    blocks.

### Transcription API

- `transcribe_samples_enhanced(provider, key_or_path, language, samples, config)`
  returns `(String, AudioQuality)`. The cleanup runs before chunking.
- `transcribe_samples` keeps its old signature and wraps the enhanced version
  with `EnhanceMode::Off`.

### Desktop

- `recording/control.rs` reads `settings.ui.audio_enhance.mode`, wraps the
  progressive chunk stream with `enhance_chunk_stream`, and the realtime audio
  stream with `enhance_audio_stream`.
- `recording/pipeline.rs` publishes quality through `publish_audio_quality` and
  emits `audio-quality-updated` plus `audio-quality-warning` when the recording
  is noisy and the mode is `off` or `light`.
- `commands/last_audio.rs`: re-transcription uses
  `transcribe_samples_enhanced` with the current mode, so changing the mode
  changes later re-transcriptions. The stored WAV remains raw.
- `commands/audio.rs`: `record_audio_test`, `play_audio_test`,
  `transcribe_audio_test`, `get_last_audio_quality`.
  - The test records 6 seconds, VAD off, with the configured microphone.
  - The original is amplified by the same gain as the cleaned version, so the
    comparison is level-matched.
  - When the current mode is `off`, the preview uses `light`.
  - Nothing is written to the stored last recording.
- `toggle_recording` is blocked while a test recording owns the microphone.

### UI

- Settings, "audio cleanup": mode selector plus a test card with play and
  transcribe for original vs cleaned.
- `LastAudioCard.vue`: SNR, hum, gain, denoise, clipping, and skipped silent
  chunks.
- `AudioQualityNotice.vue` (rendered in `App.vue`): recommendation to enable
  auto denoise after a noisy recording.

### Limitations

- Realtime providers get no denoising and no quality metrics; the realtime path
  only applies the stateful high-pass and gain.
- The quality metrics describe the audio as processed, not the theoretical
  maximum; a noisy recording with denoising enabled reports `denoised` and is
  not recommended again.

---

## 11. File map

| Area | Files |
|---|---|
| Model catalog | `crates/whis-core/src/model/{catalog,install,mod}.rs` |
| Local engines | `crates/whis-core/src/provider/{local_cpp,local_parakeet,mod}.rs` |
| History | `crates/whis-core/src/history/{mod,store,types}.rs` |
| Last recording | `crates/whis-core/src/audio/last_recording.rs` |
| Audio cleanup | `crates/whis-core/src/audio/enhance.rs` |
| Batch transcription | `crates/whis-core/src/transcription/audio_input.rs` |
| Progressive preview | `crates/whis-core/src/transcription/transcribe.rs` |
| Desktop commands | `crates/whis-desktop/src/commands/{history,last_audio,audio,models/local}.rs` |
| Desktop pipeline | `crates/whis-desktop/src/recording/{control,pipeline,config}.rs` |
| Desktop UI | `HistoryView.vue`, `LastAudioCard.vue`, `LocalModelsConfig.vue`, `AudioTestCard.vue`, `AudioQualityNotice.vue`, `stores/history.ts`, `composables/useLocalModels.ts` |
| CLI | `crates/whis-cli/src/commands/{model,setup/local,record/mod}.rs`, `service.rs` |
| Docs | `README.md`, `CONTRIBUTING.md`, `TODO.md`, this file |

---

## 12. Compatibility and limitations

- Existing settings are migrated automatically; the old per-engine model paths
  map to catalog ids. Unknown custom paths keep working as `custom_path`.
- Local models are CPU-only. Vulkan does not build on Debian bookworm with the
  system headers, so it is not offered.
- History stores text and metadata only. Audio is not kept per entry.
- Only one last recording is kept. Re-transcribing appends a new history entry
  instead of replacing the original.
- The last-recording card is intentionally on the History tab, not the Home tab.
- Audio cleanup never rewrites the stored recording; it is applied again at
  transcription time. The default mode is `light`.
- Realtime streaming providers only get the stateful high-pass and gain.
- Mobile does not expose the new history/last-audio UI yet; it only compiles
  against the updated core APIs.
