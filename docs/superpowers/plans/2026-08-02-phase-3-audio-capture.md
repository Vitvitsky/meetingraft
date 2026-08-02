# Phase 3 — Audio Capture Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Два PCM-потока (mic + system) → chunking → Rust ingest + disk chunks + SQLite `audio_manifest`; UI permissions/device; denial paths.

**Architecture:** AVFoundation/Core Audio только в Swift (`Audio/` adapters). Rust: `AudioChannel`, `ingest_audio_chunk` на UniFFI facade, `meetingraft-storage` (rusqlite WAL) пишет manifest. Streams remain separate (ADR-004). Chunk cadence **100 ms** @ 16 kHz mono PCM i16 (под VAD/STT Phase 4).

**Tech Stack:** AVAudioEngine, Core Audio process tap (macOS 15), UniFFI, rusqlite bundled, XcodeGen.

## Global Constraints

- macOS 15+, bundle `com.vitvitsky.meetingraft`.
- Mic + system streams separate end-to-end (ADR-004).
- Audio files on disk; DB only manifest (ADR-006). Path: `~/Library/Application Support/meetingraft/`.
- SwiftUI без business rules; comments RU, ids EN.
- Fake captions demo остаётся; recording — отдельный control path.

## File map

| Path | Role |
|------|------|
| `rust/crates/domain/src/audio.rs` | `AudioChannel`, chunk meta types |
| `rust/crates/storage/` | SQLite migrations + `AudioManifestStore` |
| `rust/crates/ffi/src/lib.rs` | `start_recording`, `ingest_audio_chunk`, `stop_recording` |
| `apps/macos/Sources/Audio/AudioPermissions.swift` | mic + system audio auth |
| `apps/macos/Sources/Audio/MicrophoneCapture.swift` | AVAudioEngine tap |
| `apps/macos/Sources/Audio/SystemAudioCapture.swift` | process tap wrapper |
| `apps/macos/Sources/Audio/AudioChunkPipeline.swift` | resample/pack 100ms i16 |
| `apps/macos/Sources/Audio/AudioCaptureCoordinator.swift` | start/stop both → FFI |
| `apps/macos/Sources/Audio/AudioDeviceInfo.swift` | input device list |
| UI: LiveCaptions / Settings recording controls | Start/Stop recording, denial alerts |
| `Info.plist` via project.yml | `NSMicrophoneUsageDescription`; system audio usage string |

---

### Task 1: Domain audio types + storage crate

**Produces:**
- `AudioChannel { Mic, System }`
- `AudioManifestStore::open(path)`, `begin_session(id)`, `append_chunk(...)`, `list_chunks(session_id)`
- Schema: `sessions(id TEXT PK, started_at_ms INTEGER)`, `audio_manifest(id INTEGER PK, session_id, channel, seq, path, sample_rate, frame_count, timestamp_ms)`

- [ ] TDD store append/list
- [ ] Commit `feat: SQLite audio_manifest store`

### Task 2: UniFFI recording API

**Produces on `MeetingCore`:**
- `start_recording(session_id: String) -> String` (returns session id / error string empty on ok — or throw)
- `ingest_audio_chunk(channel: FfiAudioChannel, pcm: Vec<u8>, sample_rate: u32, timestamp_ms: u64)`
- `stop_recording()`
- `manifest_chunk_count(session_id) -> u64` for tests

Chunk file: `{app_support}/sessions/{id}/{mic|system}/{seq:06}.pcm`

- [ ] Rust test: ingest 2 chunks → count 2, files exist
- [ ] Regenerate Swift bindings
- [ ] Commit `feat: UniFFI ingest audio chunks и recording session`

### Task 3: Swift mic capture + permissions + pipeline

- `AudioPermissions.requestMicrophone() async -> Bool`
- `MicrophoneCapture` → Float32 buffers → `AudioChunkPipeline` → 100ms Int16 LE bytes
- Device picker: `AVCaptureDevice.DiscoverySession` / `AVAudioApplication` input devices
- Info.plist mic usage (RU/EN copy)

- [ ] Unit test pipeline packing (pure Swift)
- [ ] Commit `feat: AVAudioEngine mic capture и chunk pipeline`

### Task 4: System audio process tap

- Request system audio recording permission (macOS 15 API)
- `SystemAudioCapture` using `AudioHardwareCreateProcessTap` / aggregate device pattern
- If tap unavailable in CI/simulator: coordinator records mic-only; system denial surfaced in UI
- Commit `feat: Core Audio system playback tap`

### Task 5: Coordinator + UI + denial paths

- `AudioCaptureCoordinator` start/stop; feeds FFI
- Live Captions toolbar: Record / Stop; alert on mic denied
- Settings: input device picker stub bound to coordinator
- Smoke test: coordinator with mock sink OR Rust manifest count after synthetic ingest (keep Swift UI test light)
- Commit `feat: recording UI и permission denial paths`

### Task 6: CI + docs + PR

- CI: unchanged rust tests cover storage; macos builds (no real mic in CI)
- Mark Epic 5 / Phase 3 in backlog/roadmap
- PR

## Exit criteria

- [ ] Recording produces chunk files + manifest rows
- [ ] Mic denial handled in UI
- [ ] Chunk cadence 100 ms @ 16 kHz documented for Phase 4 STT
- [ ] Mic and system channels remain separate in manifest
