# TODO

Planned feature work. Each item records the goal, the intended behaviour, and
the technical context needed to pick it up later.

## Queue recordings while a transcription is running

Status: planned

### Problem

Recording and transcribing share one state machine (`Idle` -> `Recording` ->
`Transcribing`). While a transcription runs, the app is `Transcribing`, so the
record button and the hotkey are blocked and the user has to wait. With large
local models this can take a while, which interrupts the workflow.

### Goal

The user can start the next recording immediately after the previous one, even
while the previous audio is still being transcribed. Recordings are processed
one after another in a queue, so nothing is lost and the order is preserved.

### Intended behaviour

- The record button is only disabled while audio is actually being captured,
  not while a transcription runs.
- Each new recording is appended to a queue and transcribed when the worker is
  free. The queue length is visible in the UI (for example "1 waiting").
- History entries keep the order of the recordings, regardless of which
  transcription finishes first.
- A failing job does not stop the queue; the error is reported per job and the
  stored recording of that job stays available for re-transcription.
- The idle model unload must not unload a model while jobs are queued.
- The stored "last recording" (single WAV slot) keeps pointing at the most
  recent recording, not at the job that is currently being processed.

### Technical notes

- Today `AppState::transcription_rx` holds a single oneshot receiver and
  `stop_and_transcribe` runs the whole pipeline inline; both need to become a
  queue plus one long-lived worker.
- Recording state and transcription state have to be split (or extended with a
  `queued` counter) so `Idle` is not required to start a recording.
- The local engines are a single global model cache behind a `Mutex`; a queue
  serialises access anyway, so no parallel inference is required.
- Hotkey/toggle handling and the Tauri commands (`toggle_recording`,
  `stop_and_transcribe`) must agree on the new states.
- Keep the CLI/mobile behaviour unchanged unless they opt in.

### Acceptance criteria

- Two recordings can be made back to back without waiting for the first
  transcript.
- Both transcripts appear in history, in recording order.
- A failure in the first job does not prevent the second from being processed.

## Faster and better-scaling local inference

Status: planned

### Goal

Use the available CPU (and optionally GPU) better so local models transcribe
faster and larger models become practical.

### Ideas

- Make the CPU thread count configurable and default it to the available
  physical cores instead of an engine default. Verify what `transcribe-cpp`
  exposes (`RunOptions`/session options) before wiring it up.
- Check whether ONNX Runtime (Parakeet) uses sensible intra/inter-op thread
  defaults, and expose them in settings if not.
- Re-evaluate GPU backends. Vulkan currently does not build on Debian bookworm
  because the system Vulkan headers are older than what ggml expects
  (`layer_setting_info`); CUDA/Metal exist as cargo features but change
  packaging and runtime requirements.
- Measure before changing: keep a small benchmark (for example `jfk60.wav`
  through `whis --file`) and record load time, real-time factor, and peak RSS
  for each model.
- Consider model-side options: quantization choice (Q4 vs Q8), and whether
  keeping the model loaded between jobs (already supported) is the better
  default than reloading.

### Acceptance criteria

- Benchmarks show a measurable speedup on the reference machine without
  changing transcript quality.
- New settings are optional and documented; defaults must not regress current
  users.
