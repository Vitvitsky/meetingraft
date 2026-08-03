# Phase 6 follow-up — Live STT model picker + HF download

**Date:** 2026-08-03
**Status:** Approved for implementation (approach A)
**Maps to:** Epic 6 (Live Subtitle), ADR-005 on-device STT, Settings Providers
**Depends on:** `resolve_whisper_model`, `models_dir`, download script parity, UniFFI MeetingCore

## Goal

В Settings выбрать on-device Whisper ggml-модель, скачать с Hugging Face
(`ggerganov/whisper.cpp`), при пустом `models/` автоматически скачать
**`ggml-base.bin`**. Выбранная модель используется Live STT (если собран
`--features whisper`).

## Decisions (approved)

| Topic | Choice |
|-------|--------|
| Scope | Picker + Download from HF; first-run auto `base` |
| Default model | Whisper **base** (`ggml-base.bin`) |
| Parakeet | **Backlog** (второй on-device engine) |
| Layer | Download in **Swift**; preference + resolve in **Rust** via UniFFI |
| Catalog | Known ids: `base`, `small`, `large-v3-turbo` (+ `auto`) |

## Non-goals

- Parakeet / Apple Speech picker
- Remote STT / WhisperX
- Bundling ggml inside `.app`
- Cancel/resume multi-download queue UI (beyond progress % + error)
- Changing Mock fallback semantics when model missing / whisper feature off

## Architecture

```text
Settings Live STT
  selectedSttModelId: auto | base | small | large-v3-turbo
  onAppear:
    list local ggml via UniFFI
    if none → WhisperModelDownloader.download(base)
  Picker + Download button
  → setPreferredWhisperModel(id) on MeetingCore

WhisperModelDownloader (Swift)
  URL: https://huggingface.co/ggerganov/whisper.cpp/resolve/main/{file}
  Destination: modelsDirectory()/ggml-*.bin
  Progress → UI

Rust stt::resolve_whisper_model(data_root, preferred: Option<&str>)
  preferred None/"auto" → existing priority list
  preferred "base" → models/ggml-base.bin if present else None (then Mock)
```

File map:

| id | file |
|----|------|
| `base` | `ggml-base.bin` |
| `small` | `ggml-small.bin` |
| `large-v3-turbo` | `ggml-large-v3-turbo.bin` |

## UniFFI

```text
set_preferred_whisper_model(model_id: String)  // "auto"|"base"|"small"|"large-v3-turbo"
preferred_whisper_model() -> String
list_local_whisper_models() -> Vec<String>     // filenames present under models/
```

`whisper_model_path()` continues to return the **resolved** absolute path
(empty if none). After preference change, next `start_recording` loads that
model (no hot-swap mid-session required in v1).

Persist preference in `MeetingCoreInner` for process lifetime; Swift also
keeps `ProviderSettingsStore.selectedSttModelId` and re-applies on Settings
appear / before Start Live.

## Swift download

- `URLSession` download to `{modelsDir}/{file}.partial` then rename.
- Skip if file already exists (idempotent).
- First-run: only when `list_local_whisper_models` empty (or dir missing).
- Show progress 0…1; surface HTTP errors in Settings caption.
- Do not shell out to `download-stt-model.sh` (script remains for CLI).

## Testing

- Rust: resolve respects preferred id; auto keeps priority; missing preferred → None.
- Swift: downloader unit test with local HTTP mock or file fixture (prefer
  injecting URLSession / protocol); Settings store default id.
- Regenerate FFI after UniFFI changes.

## Docs / backlog

- STT picker + HF download + first-run base — done (this slice).
- **Parakeet on-device engine** — deferred.
- Install §2.4: mention Settings download / first-run.

## Success criteria

- [ ] Empty models → Settings appear downloads `ggml-base.bin`
- [ ] User can pick small/turbo and Download; Live uses preferred file when whisper feature on
- [ ] `auto` preserves previous resolve priority among installed files
- [ ] Parakeet listed as deferred in backlog
- [ ] No silent crash if download fails (Mock + error caption)
