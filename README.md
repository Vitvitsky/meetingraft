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
- Backend: processing API + workers + storage
- UX: macOS-native patterns following Apple Human Interface Guidelines

## Repository map

- `apps/macos` — native SwiftUI shell
- `rust/crates` — Rust core, session engine, glossary engine, sync client
- `backend` — remote processing services
- `docs` — architecture, ADRs, backlog
- `shared` — contracts and cross-layer schemas

## Initial milestones

1. SwiftUI shell with fake subtitle stream
2. Rust core + UniFFI boundary
3. Audio capture and live subtitle pipeline
4. Glossary management
5. Post-call refinement and generated outputs

Phased plan with exit criteria: [`docs/roadmap.md`](docs/roadmap.md).
