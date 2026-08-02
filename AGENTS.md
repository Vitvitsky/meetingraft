# AGENTS.md

## Purpose

This repository contains MeetingRaft, a native-first macOS meeting companion with live subtitles first and post-call intelligence second.

## Product constraints

- Live subtitles and final transcript are different artifacts.
- Realtime mode does not require speaker attribution.
- Post-call mode may assign or recognize speakers.
- Glossary support is a first-class feature.
- Native UX on macOS is required.
- Speech recognition languages: Russian (primary), English, Spanish.
- Default session language is Russian; EN/ES are supported for mixed and multilingual meetings.
- Language hints (session primary + allowed set) travel with live and post-call pipelines.

## Architecture rules

- SwiftUI views must not contain networking or business rules.
- AVFoundation stays in the Swift platform layer.
- Rust contains shared domain logic, session engine, transcript assembly, glossary normalization, sync logic, and local state orchestration.
- UniFFI is the only preferred boundary between Swift and Rust.
- Backend concerns stay outside the macOS shell.
- Live transcript and final refined transcript use separate domain models.

## Module boundaries

### Swift layer
- app lifecycle
- navigation
- window scenes
- menu bar and commands
- permissions
- audio capture adapters
- presentation models

### Rust layer
- domain entities
- meeting session state machine
- subtitle aggregation
- glossary engine
- sync client
- local persistence abstractions
- DTOs exposed through UniFFI facade

### Backend layer
- processing jobs
- storage
- diarization
- transcript refinement
- generated brief and follow-up artifacts

## Guardrails for coding agents

- Do not bypass UniFFI with ad hoc FFI glue unless explicitly approved.
- Do not put Cocoa or AVFoundation types into Rust-facing contracts.
- Prefer small DTOs and explicit enums across boundaries.
- Keep state transitions explicit and testable.
- Add tests for any new state machine branch.
- Prefer repository interfaces over direct API calls in views or view models.
- Keep docs updated when changing domain boundaries or contracts.

## Agent roles

- Swift Shell Agent
- Swift Audio Agent
- Swift UI Agent
- Rust Core Agent
- Rust Sync Agent
- Rust UniFFI Agent
- Backend Agent
- QA Agent
- Docs/ADR Agent

## Done criteria

A feature is not done until:
- architecture boundaries are respected;
- tests cover core logic;
- docs changed if contracts changed;
- no UI layer contains direct business logic;
- glossary and transcript version impact is considered where relevant.

## Setup

- Rust core: `cd rust && cargo test` (workspace; крейты в `rust/crates/`)
- Lint Rust: `cd rust && cargo fmt --check && cargo clippy --all-targets -- -D warnings`
- macOS shell: `cd apps/macos && xcodegen generate`, затем открыть
  `MeetingRaft.xcodeproj` в Xcode или
  `xcodebuild -project MeetingRaft.xcodeproj -scheme MeetingRaft -configuration Debug build CODE_SIGNING_ALLOWED=NO`
  (`.xcodeproj` генерируется, в git не трекается — источник `project.yml`)
- macOS tests: `xcodebuild -project MeetingRaft.xcodeproj -scheme MeetingRaft -configuration Debug test CODE_SIGNING_ALLOWED=NO`
- Lint Swift: `cd apps/macos && swiftformat Sources Tests --lint`
- CI: `.github/workflows/ci.yml` — fmt, clippy, cargo test, xcodebuild build+test
- UniFFI + Xcode project (из корня репо): `apps/macos/Scripts/generate-ffi.sh`
  (dylib → `rust/target/debug`, биндинги → `apps/macos/Generated/`, затем
  `xcodegen generate` в `apps/macos/`)
- Backend: появится в Phase 6 (ADR-007); контракт — `shared/openapi.yaml`
- Docs: architecture и ADR — в `docs/`

## Stack & conventions

- Stack: SwiftUI + AVFoundation (macOS shell), Rust + UniFFI (domain core), backend workers/API (post-call)
- Comments/docstrings: Russian; identifiers: English
- Prefer explicit types and small DTOs across UniFFI; keep state machines testable
- SQL (when present): keywords UPPERCASE, identifiers lowercase
- Commits: Conventional Commits with Russian subject (`feat:`, `fix:`, `docs:`, …)

## Skills & tooling

- graphify (CLI): `uv tool install graphifyy` — codebase knowledge graph
- Slash skills (agent side): `/agents-init`, graphify skill when answering structure questions
- Prefer UniFFI-facing contracts and ADRs in `docs/adr/` over ad hoc cross-layer glue

## hindsight (agent memory)

Long-term memory service (local Docker, `http://localhost:8888`, MCP at `/mcp`).
Bank id = repo name: `meetingraft`. Skip this section silently if the
service is not running or MCP tools are absent.

- **Recall at task start**: call `recall` with the task topic — past
  decisions, gotchas, and data quirks live there.
- **Retain on the way out**: after a significant decision, verified finding,
  or data gotcha, call `retain` with a short self-contained fact.
- Do not retain what the repo already records (code, git history, READMEs)
  or session-local noise.
- Recalled facts about code are historical context, not ground truth: verify
  against the current code. On mismatch the code wins — retire the stale
  fact via `invalidate_memory`.
- Connect in Claude Code:
  `claude mcp add --transport http --scope user hindsight http://localhost:8888/mcp`.

## graphify (knowledge graph)

Plain CLI (`uv tool install graphifyy`), works with any agent.

- When `graphify-out/graph.json` exists, answer codebase questions with
  `graphify query "<question>"` first; fall back to grep for exact strings.
- After modifying code, run `graphify update .` (AST-only, no API cost).
- Build initially with `graphify .` if the graph does not exist yet.
