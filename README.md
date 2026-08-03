# meetingraft

GitHub repository for **MeetingRaft** — a native-first macOS meeting companion for live subtitles, post-call refinement, brief generation, and follow-up email drafting.

| | |
|---|---|
| Repository | [`meetingraft`](https://github.com/Vitvitsky/meetingraft) |
| Product name | MeetingRaft |

## Product idea

- Stage 1: live streaming subtitles without speaker identification.
- Stage 2: post-call diarization, speaker assignment, brief filling, and follow-up generation.
- Glossary support for slang, abbreviations, internal terms, and product names.
- Speech languages: **Russian (primary)**, English, Spanish — default session language is Russian.

## Suggested stack

- macOS app: SwiftUI + AVFoundation
- Native core: Rust via UniFFI
- Backend: processing API + workers + storage (stub FastAPI today, ADR-007)
- UX: macOS-native patterns following Apple Human Interface Guidelines

## Quick start

Схемы архитектуры и полная процедура установки:

→ **[`docs/architecture-and-install.md`](docs/architecture-and-install.md)**

Настройка backend stub (Docker / uv, Settings, Test API, LLM=Backend, refine):

→ **[`docs/architecture-and-install.md` §2.5](docs/architecture-and-install.md#backend-setup)**

Кратко:

```bash
apps/macos/Scripts/generate-ffi.sh
open apps/macos/MeetingRaft.xcodeproj   # ⌘R

# опционально Whisper
apps/macos/Scripts/download-stt-model.sh

# опционально backend stub → затем Settings → Backend API (см. §2.5)
docker compose up --build   # :8080, token dev-token
```

Команды тестов и границы слоёв: [`AGENTS.md`](AGENTS.md).

## Repository map

- `apps/macos` — native SwiftUI shell
- `rust/crates` — Rust core, session, STT, glossary, postcall, translate, sync, UniFFI
- `backend` — FastAPI job API stub
- `shared/openapi.yaml` — ADR-007 contract
- `docs` — architecture, ADRs, roadmap, install guide

## Milestones

Phases 0–6 local MVP are on `main` (see [`docs/roadmap.md`](docs/roadmap.md)).
Next: full ADR-007 workers (WhisperX / LLM), speakers (Epic 9), Phase 7 hardening.
