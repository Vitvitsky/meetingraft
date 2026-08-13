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
  (правила, невидимые на Linux, — ниже отдельным разделом)
- Pre-commit (локально, зеркало быстрых CI-линтов): `brew install pre-commit`
  (или `pipx install pre-commit`), затем из корня репо `pre-commit install`;
  разовый прогон `pre-commit run --all-files`. Хуки: `cargo fmt --check`,
  `swiftformat Sources Tests --lint`, `ruff` для `backend/`. Clippy / полный
  `cargo test` / `xcodebuild` — только в CI.
- CI: автозапуск GitHub Actions отключён (`workflow_dispatch` only в
  `.github/workflows/ci.yml`). Проверки — локально: команды выше +
  `pre-commit run --all-files`
- UniFFI + Xcode project (из корня репо): `apps/macos/Scripts/generate-ffi.sh`
  (dylib → `rust/target/debug`, биндинги → `apps/macos/Generated/`, затем
  `xcodegen generate` в `apps/macos/`)
- Backend: `cd backend && uv sync --extra dev && uv run pytest`;
  docker: `docker compose up --build` (API `:8080`, token `dev-token`);
  настройка в app: `docs/architecture-and-install.md` §2.5 (`#backend-setup`)
- Docs: architecture и ADR — в `docs/`; схемы + install —
  `docs/architecture-and-install.md`
- OpenAPI: `shared/openapi.yaml`

## swiftformat rules that Linux cannot see

Swift собирается только на Mac, поэтому `swiftformat --lint` — шаг 5
`scripts/verify-mac.sh` — единственное место, где эти правила
обнаруживаются. Каждое стоит целого прогона, если ловить их по очереди,
поэтому список ведётся: пойманное сюда дописывается.

Правил в `apps/macos/.swiftformat` не перечислено вовсе — там один
`--swiftversion 6.0`, — значит действует то, что включено в установленной
версии. Предполагать по памяти, что рулится, а что нет, здесь не выходит;
только этот список и прогон.

- **Разделители в числах.** По умолчанию `--decimalgrouping 3,6`: до пяти
  цифр — **без** `_` (`50000`, `16000`), от шести — группами по три
  (`1_150_000`). Написанное на глаз `1_150` и `50_000` роняет шаг целиком.
  Сверяется по уже лежащим файлам:
  `grep -rnoP '(?<![_0-9.])[0-9]{5}(?![_0-9.])' apps/macos` показывает
  пятизначные без разделителей — так и надо.
- **`redundantAsync`.** `func … async` без единого `await` в теле —
  ошибка. Легко получается в тестах, где `await` убрался вместе с
  последней асинхронной строкой.
- **`preferKeyPath`.** `filter { $0.foo }` и `map { $0.foo }` обязаны быть
  `filter(\.foo)`. Только тривиальные: замыкания, которые сравнивают
  (`first { $0.id == other }`) или отрицают (`contains { !$0.ok }`), под
  правило не попадают.
- **`wrapFunctionBodies`.** Тело функции в одну строку
  (`func f() -> Float { 0.45 }`) — ошибка, включая тестовые заглушки.

## Stack & conventions

- Stack: SwiftUI + AVFoundation (macOS shell), Rust + UniFFI (domain core), backend workers/API (post-call)
- Comments/docstrings: Russian; identifiers: English
- Prefer explicit types and small DTOs across UniFFI; keep state machines testable
- SQL (when present): keywords UPPERCASE, identifiers lowercase
- Commits: Conventional Commits, **English** subject and body (`feat:`, `fix:`,
  `docs:`, …). До 2026-08-04 они были русскими; старую историю не переписывать.
  Комментарии в коде и `docs/` при этом остаются русскими — язык кода и язык
  истории здесь разные намеренно

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
