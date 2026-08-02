# BriefLane Backlog

## Epic 1 — Repository Bootstrap
- Create repo skeleton
- Add AGENTS.md
- Add architecture.md
- Add ADR template and first ADRs
- Add contribution and coding conventions

## Epic 2 — Native macOS Shell
- Create SwiftUI app shell
- Add sidebar and toolbar
- Add settings scene
- Add menu commands and keyboard shortcuts
- Add fake subtitle stream screen

## Epic 3 — Rust Core
- Create domain crate
- Create session engine crate
- Create glossary engine crate
- Create sync client crate
- Create UniFFI facade crate

## Epic 4 — Swift ↔ Rust Boundary
- Define UniFFI contracts
- Wire generated Swift bindings into Xcode
- Expose simple DTO-based interfaces
- Add integration smoke test

## Epic 5 — Audio Capture
- Add AVFoundation capture manager
- Add device selection
- Add permissions flow
- Add chunking pipeline
- Add local raw recording manifest

## Epic 6 — Live Subtitle Flow
- Open session with backend
- Stream chunks
- Render partial captions
- Merge final captions
- Save local caption events

## Epic 7 — Glossary
- Create glossary domain model
- Add glossary UI
- Add import from CSV/TXT
- Add scope: global/workspace/project/meeting
- Attach glossary to live session

## Epic 8 — Post-call Intelligence
- Trigger refinement after meeting end
- Fetch final transcript
- Show transcript review screen
- Generate brief draft
- Generate follow-up draft

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
