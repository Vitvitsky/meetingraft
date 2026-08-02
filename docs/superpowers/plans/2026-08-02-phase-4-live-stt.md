# Phase 4 — Live Subtitle Pipeline (STT) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Live captions из on-device STT: audio chunks → `SttEngine` → partial/final events → SQLite + UI (ADR-005). Fake demo остаётся отдельно.

**Architecture:** Новый крейт `meetingraft-stt`: trait `SttEngine`, `WhisperSttEngine` (whisper-rs + Metal), `MockSttEngine` для CI. `LiveCaptionPipeline` копит PCM окно, VAD/тишина → finalize. `MeetingCore.start_live` = recording + STT; `drain_events` отдаёт STT captions. Storage: таблица `caption_events`.

**Tech Stack:** whisper-rs 0.16 (Metal), rusqlite, UniFFI, Swift poll как сейчас.

## Global Constraints

- Language policy ru primary, {ru,en,es} (ADR-003).
- Audio stays local (ADR-005).
- Live vs final artifacts separate (ADR-002); здесь только live events.
- Chunks уже 100 ms @ 16 kHz i16 (Phase 3).
- Comments RU, Conventional Commits RU.

## File map

| Path | Role |
|------|------|
| `rust/crates/stt/` | `SttEngine`, Whisper, Mock, window assembler |
| `rust/crates/storage/src/caption_events.rs` | persist/replay live captions |
| `rust/crates/ffi` | `start_live`, model path, drain STT events |
| Swift LiveCaptions | Start Live Session → recording + Rust captions |

## Tasks

### Task 1: `SttEngine` + Mock + window tests
- Trait: `push_pcm(&[i16], sample_rate) -> Vec<CaptionEvent>`, `flush() -> Vec<CaptionEvent>`, `set_language_policy`
- Mock: energy VAD, emits Russian partial/final for tests
- Commit

### Task 2: caption_events in SQLite
- Schema + `append_caption` / `list_captions(session_id)`
- Commit

### Task 3: Wire pipeline into MeetingCore
- `start_live(session_id, model_path optional)`
- ingest_audio_chunk also feeds STT; events persisted + queued for drain
- Default engine: Whisper if model file exists, else Mock (log warning)
- Regenerate UniFFI
- Commit

### Task 4: WhisperSttEngine
- whisper-rs with `metal` feature
- Model path: `~/Library/Application Support/meetingraft/models/ggml-*.bin`
- Script `apps/macos/Scripts/download-stt-model.sh` downloads `ggml-base` (dev) / doc for turbo
- Commit

### Task 5: Swift UI live session
- Button Start Live / Stop Live
- Captions from `drainEvents` while live (reuse RustCaptionStream pattern or LiveSessionStream)
- Settings: show model path status
- Commit + CI + PR

## Exit criteria (this PR may partial)
- [x] STT trait + tests green in CI (Mock)
- [x] Captions persist and drain from Rust during live
- [ ] Whisper loads when model present (manual verify; `--features whisper` + download script)
- [x] UI shows live captions from Rust path
