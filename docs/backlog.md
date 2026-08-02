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
- Create glossary engine crate
- Create sync client crate
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
- System audio process tap (ADR-004) — follow-up wiring

## Epic 6 — Live Subtitle Flow
- [x] Open session with STT pipeline (Mock; Whisper when model + feature)
- Pass language policy: primary `ru`, allowed `{ru, en, es}`
- [x] Stream chunks → SttEngine
- [x] Render partial captions
- [x] Merge final captions
- [x] Save local caption events
- Settings: session language override (default Russian) — stub exists; STT model status in Settings
- Whisper Metal + model download script (opt-in `--features whisper`)

## Epic 7 — Glossary
- Create glossary domain model
- Add glossary UI
- Add import from CSV/TXT
- Add scope: global/workspace/project/meeting
- Attach glossary to live session
- Glossary candidates from transcript corrections (review feedback loop)
- Post-call mining of candidates (acronyms, code-switching terms) with
  approval queue

## Epic 8 — Post-call Intelligence
- Trigger refinement after meeting end (same language policy as live)
- Fetch final transcript
- Show transcript review screen
- Artifact template system: built-in templates (brief, follow-up email,
  technical requirements, meeting minutes, action items)
- User-defined markdown templates (prompt + placeholders: transcript,
  brief, glossary, participants)
- Template picker, regeneration and versioning of generated artifacts
- Export artifacts (copy, .md file, mail draft)

## Epic 9 — Speaker Assignment
- Add speaker entities
- Add speaker correction screen
- Add versioned refined transcript
- Compare live vs final transcript

## Epic 10 — Quality
- Unit tests for state machine
- Integration tests for FFI facade
- UI smoke tests
- Docs sync rules
