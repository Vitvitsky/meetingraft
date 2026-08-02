# MeetingRaft Development Roadmap

Phased development plan. Each phase produces working, testable software on
its own and ends with explicit exit criteria. Before a phase starts, a
detailed implementation plan for it is written to
`docs/superpowers/plans/YYYY-MM-DD-<phase-name>.md` (bite-sized TDD tasks);
this document stays at the milestone level and maps phases to backlog epics
(`docs/backlog.md`).

Ground rules for every phase:

- Architecture boundaries from `AGENTS.md` are non-negotiable (SwiftUI shell
  without business logic, Rust core behind UniFFI, backend outside the shell).
- Language policy travels everywhere: `primary_language` = `ru`,
  `allowed_languages` = `{ru, en, es}` (ADR-003).
- TDD for core logic; every new state machine branch gets a test.
- Conventional Commits with Russian subject.

## Phase 0 — Decisions and tooling bootstrap

Blocking product decisions are not yet recorded; they shape every later
phase, so they are fixed first as ADRs:

- **ADR-004 Audio capture sources.** Microphone only vs microphone + system
  audio (other participants). System audio on macOS requires
  ScreenCaptureKit / audio tap and its own permission flow; a meeting
  companion is barely useful without it, so this must be an explicit
  decision, not an accident of Epic 5.
- **ADR-005 Live STT topology.** On-device (e.g. whisper.cpp / Apple Speech)
  vs cloud streaming gateway, and the concrete provider; must honor the
  ru/en/es policy and Russian-first quality (ADR-003). Decides whether the
  backend is on the critical path for Stage 1.
- **ADR-006 Local persistence.** Concrete store behind the Rust "local
  store facade" (e.g. SQLite via rusqlite) and what is persisted locally:
  caption events, raw audio manifest, glossary.
- **ADR-007 Backend stack and contracts.** Backend language/runtime,
  streaming transport (WebSocket/gRPC), contract schema format and its home
  in `shared/`. May be deferred only if ADR-005 picks on-device STT for v1.

Tooling in the same phase: Xcode project under `apps/macos`, cargo workspace
under `rust/`, lint/format (clippy + rustfmt, SwiftFormat or SwiftLint), CI
that builds both worlds and runs `cargo test`.

**Exit criteria:** ADR-004..006 accepted (ADR-007 accepted or explicitly
deferred); empty app and empty workspace build green in CI.

Maps to: Epic 1 (finishing touches).

## Phase 1 — SwiftUI shell with fake subtitle stream

App skeleton a user can click through: sidebar, toolbar, settings scene,
menu commands and shortcuts, and a live-captions screen rendering a fake
subtitle stream (Swift-local timer for now). Presentation models only — no
business rules in views.

**Exit criteria:** app runs from Xcode; fake captions render with
partial/final visual states; settings scene shows session language selector
(default `ru`) backed by a stub.

Maps to: Epic 2.

## Phase 2 — Rust core and UniFFI boundary

Domain crate (session, caption event, language policy DTOs), meeting session
state machine, UniFFI facade crate, generated Swift bindings wired into the
Xcode project. The fake subtitle stream moves from Swift into the Rust core,
proving the event path UI ← UniFFI ← core.

**Exit criteria:** state machine transitions covered by `cargo test`;
Swift ↔ Rust integration smoke test passes; captions on screen originate in
Rust.

Maps to: Epics 3, 4.

## Phase 3 — Audio capture

AVFoundation capture manager (plus system audio path if ADR-004 says so),
permissions flow, input device selection, chunking pipeline feeding the Rust
core, local raw recording manifest per ADR-006.

**Exit criteria:** a recording session produces persisted chunks and a
manifest; permission denial paths handled in UI; chunk cadence matches what
the STT path in ADR-005 expects.

Maps to: Epic 5.

## Phase 4 — Live subtitle pipeline (Stage 1 complete)

Real STT wired per ADR-005: open session with the language policy, stream
chunks, merge partial/final caption events in the subtitle assembler,
persist live caption events, session language override in settings.

**Exit criteria:** live captions on a real meeting in Russian with mixed
English terms; defined latency budget measured and met; caption events
replayable from local store.

Maps to: Epic 6.

## Phase 5 — Glossary

Glossary domain model and normalization engine in Rust, scopes
(global/workspace/project/meeting), CSV/TXT import, glossary UI, attaching
the glossary to a live session (bias/normalization), language-tagged terms
with Russian as default scope.

**Exit criteria:** glossary terms demonstrably affect caption output;
normalization covered by unit tests; import round-trips a real CSV.

Maps to: Epic 7.

## Phase 6 — Post-call intelligence (Stage 2)

Refinement trigger on meeting end (same language policy), final transcript
fetch, transcript review screen, brief draft, follow-up email draft, speaker
entities with correction screen, versioned refined transcripts, live vs
final comparison. Live and final transcripts remain separate domain models
(ADR-002).

**Exit criteria:** end-to-end flow: finish meeting → refined transcript →
speaker assignment → brief + follow-up drafts; transcript versions
comparable in UI.

Maps to: Epics 8, 9.

## Phase 7 — Hardening and release

UI smoke tests, FFI integration test suite, packaging (signing,
notarization), docs sync check, performance pass against the Phase 4
latency budget.

**Exit criteria:** `AGENTS.md` done-criteria hold across the app; a
notarized build runs on a clean machine.

Maps to: Epic 10 (quality items also run continuously inside each phase).

## Dependency notes

- Phase 2 blocks 3–6: everything flows through the UniFFI facade.
- ADR-005 (accepted: on-device STT) keeps the backend out of Phases 1–5;
  `backend/` and the `shared/openapi.yaml` contract (ADR-007) start in
  Phase 6.
- Glossary (Phase 5) intentionally precedes post-call (Phase 6): glossary
  bias helps live captions first, refinement reuses the same engine.
