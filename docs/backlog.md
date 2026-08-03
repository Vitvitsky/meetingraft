# MeetingRaft Backlog

## Epic 1 — Repository Bootstrap
- Create repo skeleton
- Add AGENTS.md
- Add architecture.md
- Add ADR template and first ADRs
- Add contribution and coding conventions

## Epic 2 — Native macOS Shell
- [x] Create SwiftUI app shell
- [x] Add sidebar and toolbar
- [x] Add settings scene
- [x] Add menu commands and keyboard shortcuts
- [x] Add fake subtitle stream screen

## Epic 3 — Rust Core
- [x] Create domain crate
- [x] Create session engine crate
- [x] Create glossary engine crate
- [x] Create sync client crate — `meetingraft-sync` (ADR-007 slice A)
- [x] Create UniFFI facade crate

## Epic 4 — Swift ↔ Rust Boundary
- [x] Define UniFFI contracts
- [x] Wire generated Swift bindings into Xcode
- [x] Expose simple DTO-based interfaces
- [x] Add integration smoke test

## Epic 5 — Audio Capture
- [x] Add AVFoundation capture manager
- Add device selection
- [x] Add permissions flow
- [x] Add chunking pipeline
- [x] Add local raw recording manifest
- [x] System audio process tap (ADR-004) — Core Audio process tap +
  приватное aggregate-устройство; каналы раздельны на диске, live-путь
  идёт через микс с атрибуцией (ADR-009)

## Epic 6 — Live Subtitle Flow
- [x] Open session with STT pipeline (Mock; Whisper when model + feature)
- Pass language policy: primary `ru`, allowed `{ru, en, es}`
- [x] Stream chunks → SttEngine
- [x] Render partial captions
- [x] Merge final captions
- [x] Save local caption events
- Settings: session language override (default Russian) — stub exists
- [x] STT model picker в Settings (Whisper ggml: auto / base / small / large-v3-turbo; HF download; first-run `ggml-base.bin`)
- Whisper Metal + model download script (opt-in `--features whisper`)

## Epic 7 — Glossary
- [x] Create glossary domain model
- [x] Add glossary UI
- [x] Add import from CSV/TXT
- Add scope: global/workspace/project/meeting — **partial:** global + meeting in MVP
- [x] Attach glossary to live session
- Glossary candidates from transcript corrections (review feedback loop)
- Post-call mining of candidates (acronyms, code-switching terms) with
  approval queue

## Epic 8 — Post-call Intelligence
- [x] Trigger refinement after meeting end — **local MVP:** assemble on Stop Live
  (backend refinement / ADR-007 HTTP deferred)
- [x] Fetch final transcript — SQLite `final_transcripts`
- [x] Show transcript review screen — Meetings detail: Live | Final | Artifacts
- Artifact template system: built-in templates — **partial:** Brief + Follow-up
  (technical requirements, meeting minutes, action items deferred)
- User-defined markdown templates (prompt + placeholders: transcript,
  brief, glossary, participants) — **deferred**
- Template picker, regeneration and versioning — **partial:** generate Brief /
  Follow-up in UI; versioning deferred
- Export artifacts — **partial:** copy to clipboard + **.md file export**
  (Final + Brief/Follow-up → Settings export folder, flat Obsidian-friendly
  names; `feat/markdown-export-obsidian`); mail draft **deferred**
- Obsidian plugin / export HTTP API — **deferred:** pull meetings from app
  or backend без ручного folder export (spec
  `docs/superpowers/specs/2026-08-03-markdown-export-obsidian-design.md`)
- Real LLM generation — **partial:**
  - Local: Ollama native + OpenAI-compatible из app
  - Backend jobs: Settings LLM=Backend + Model id → payload prompts →
    OpenAI-compat провайдер из env (`LLM_BASE_URL` / `LLM_API_KEY` /
    `LLM_MODEL`)
  - Streaming/tools **deferred**
- Backend provider platform — **partial:** static JSON registry
  (`PROVIDERS_JSON` / `LLM_PROVIDERS_FILE`), `GET /v1/models`, Settings picker
  `(provider_id, model)`, job routing по `provider_id`; compat `LLM_*` →
  `default` (`feat/backend-provider-registry`)
  - CRUD API / UI «добавить провайдера», billing, live upstream discovery —
    **deferred**
- Parakeet on-device STT (второй engine рядом с Whisper) — **deferred**
- Remote STT API (latency risk для live; не default) — **deferred**
- Более жирная модель для глубокого анализа полного аудио / refined
  transcript — **deferred**
- [x] Backend HTTP (ADR-007) — **slice A:** OpenAPI + FastAPI stub jobs +
  `meetingraft-sync` + Settings Test API (`feat/phase-6-backend-stub`)
- [x] Meetings UI: Submit refine (stub) → poll → show artifact
  (`feat/meetings-backend-refine-stub`)
- [x] Backend LLM provider for brief/follow_up jobs (`feat/backend-llm-provider`)
- Create sync client crate — **done** (`meetingraft-sync`)

## Epic 9 — Speaker Assignment
- [x] Add speaker entities — **skeleton:** `domain::Speaker`, SQLite `speakers`,
  UniFFI list/upsert/delete (`feat/speakers-skeleton`)
- [x] Add speaker correction screen — **partial (skeleton):** Meetings detail
  **Speakers** tab: ручные метки (add/rename/delete), banner «diarization — скоро»;
  без diarization и без привязки к Final transcript
- [x] Add versioned refined transcript — Stop Live / re-assemble → next version
  (`max+1`); Final tab picker; Brief/Follow-up/Export = latest
  (`feat/final-versions-compare`)
- [x] Compare live vs final transcript — Meetings **Compare** tab: side-by-side
  Live finals | Final vN (`feat/final-versions-compare`)
- Diarization / speaker binding to Final segments — **deferred**

## Epic 10 — Quality
- Unit tests for state machine
- Integration tests for FFI facade
- UI smoke tests
- Docs sync rules
