# Phase 2 — Rust Core and UniFFI Boundary Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Домен + session state machine в Rust; UniFFI facade; Swift показывает captions, приходящие из Rust (UI ← UniFFI ← core).

**Architecture:** `meetingraft-domain` — чистые DTO/enums. `meetingraft-session` — state machine и fake caption producer (без UniFFI). `meetingraft-ffi` — cdylib + UniFFI proc-macros, тонкая обёртка. Swift `RustCaptionStream` реализует `CaptionStreaming` через poll `drain_caption_events`. Views без бизнес-логики.

**Tech Stack:** Rust edition 2024, UniFFI ≥0.29 (proc-macros), cargo workspace, XcodeGen, Swift 6 / macOS 15.

## Global Constraints

- Language policy: primary `ru`, allowed `{ru,en,es}` (ADR-003).
- Live vs final captions — разные фазы (`partial`/`final`) (ADR-002).
- UniFFI — единственная граница Swift↔Rust (AGENTS.md).
- Comments Russian, identifiers English; Conventional Commits RU subject.
- Bundle id `com.vitvitsky.meetingraft`; deployment 15.0.
- Generated Swift under `apps/macos/Generated/` — **коммитим** сгенерированный `.swift` + headers/modulemap для CI без сети; dylib собирается в CI/локально скриптом.

## File map

| Path | Role |
|------|------|
| `rust/crates/domain/src/language.rs` | `SpeechLanguage`, `LanguagePolicy` |
| `rust/crates/domain/src/caption.rs` | `CaptionPhase`, `CaptionEvent` |
| `rust/crates/domain/src/session.rs` | `SessionState` |
| `rust/crates/session/` | State machine + `FakeCaptionProducer` |
| `rust/crates/ffi/` | UniFFI cdylib facade |
| `rust/crates/uniffi-bindgen/` | CLI helper bin |
| `apps/macos/Scripts/generate-ffi.sh` | build dylib + generate Swift |
| `apps/macos/Generated/` | checked-in bindings |
| `apps/macos/Sources/LiveCaptions/RustCaptionStream.swift` | CaptionStreaming via FFI |
| `apps/macos/Sources/LiveCaptions/LiveCaptionsViewModel.swift` | default stream → Rust |

---

### Task 1: Domain DTOs + tests

**Files:**
- Create: `rust/crates/domain/src/language.rs`
- Create: `rust/crates/domain/src/caption.rs`
- Create: `rust/crates/domain/src/session.rs`
- Modify: `rust/crates/domain/src/lib.rs`

**Interfaces:**
- Produces:
  - `SpeechLanguage { Ru, En, Es }` — `Default` = `Ru`; `fn code(&self) -> &'static str`
  - `LanguagePolicy { primary: SpeechLanguage, allowed: Vec<SpeechLanguage> }` — `fn default_v1() -> Self` primary Ru, allowed [Ru,En,Es]; `fn is_allowed(&self, lang) -> bool`
  - `CaptionPhase { Partial, Final }`
  - `CaptionEvent { id: String, text: String, phase: CaptionPhase }`
  - `SessionState { Idle, Live, Ended }`

- [ ] **Step 1: Падающий тест**

В `lib.rs` tests добавить:

```rust
#[test]
fn default_language_policy_is_russian_first() {
    let p = LanguagePolicy::default_v1();
    assert_eq!(p.primary, SpeechLanguage::Ru);
    assert_eq!(p.allowed, vec![SpeechLanguage::Ru, SpeechLanguage::En, SpeechLanguage::Es]);
}
```

Run: `cd rust && cargo test -p meetingraft-domain`
Expected: FAIL compile.

- [ ] **Step 2: Реализация modules + `pub use`**

- [ ] **Step 3: `cargo test -p meetingraft-domain` PASS; fmt/clippy**

- [ ] **Step 4: Commit** `feat: доменные DTO языка и caption/session`

---

### Task 2: Session engine crate + state machine tests

**Files:**
- Create: `rust/crates/session/Cargo.toml` (dep `meetingraft-domain`)
- Create: `rust/crates/session/src/lib.rs`
- Create: `rust/crates/session/src/engine.rs`
- Create: `rust/crates/session/src/fake_captions.rs`
- Modify: `rust/Cargo.toml` members

**Interfaces:**
- Produces:
  - `MeetingSession` — `new()`, `start(LanguagePolicy) -> Result<(), SessionError>`, `stop()`, `state() -> SessionState`, `push_tick(now_ms: u64) -> Vec<CaptionEvent>` (advances fake producer when Live)
  - Illegal transitions return `SessionError::InvalidTransition`
  - `FakeCaptionProducer` — same script as Swift Phase 1 (Russian-first lines)

- [ ] **Step 1: Тесты**

```rust
#[test]
fn start_moves_idle_to_live() {
    let mut s = MeetingSession::new();
    assert_eq!(s.state(), SessionState::Idle);
    s.start(LanguagePolicy::default_v1()).unwrap();
    assert_eq!(s.state(), SessionState::Live);
}

#[test]
fn stop_from_live_ends() {
    let mut s = MeetingSession::new();
    s.start(LanguagePolicy::default_v1()).unwrap();
    s.stop().unwrap();
    assert_eq!(s.state(), SessionState::Ended);
}

#[test]
fn cannot_start_twice() {
    let mut s = MeetingSession::new();
    s.start(LanguagePolicy::default_v1()).unwrap();
    assert!(matches!(s.start(LanguagePolicy::default_v1()), Err(SessionError::InvalidTransition)));
}

#[test]
fn tick_emits_partial_then_final() {
    let mut s = MeetingSession::new();
    s.start(LanguagePolicy::default_v1()).unwrap();
    let first = s.push_tick(0);
    assert_eq!(first.len(), 1);
    assert_eq!(first[0].phase, CaptionPhase::Partial);
    let second = s.push_tick(800);
    assert_eq!(second[0].phase, CaptionPhase::Final);
}
```

- [ ] **Step 2–4: implement, green, commit** `feat: session engine и fake captions в Rust`

---

### Task 3: UniFFI facade crate

**Files:**
- Create: `rust/crates/ffi/Cargo.toml` — `crate-type = ["cdylib", "lib"]`, deps `uniffi`, `meetingraft-session`, `meetingraft-domain`
- Create: `rust/crates/ffi/src/lib.rs` — `uniffi::setup_scaffolding!()` + exported types/object
- Create: `rust/crates/ffi/uniffi.toml` — `[bindings.swift] cdylib_name = "meetingraft_ffi"`
- Create: `rust/crates/uniffi-bindgen/Cargo.toml` + `src/main.rs` calling `uniffi::uniffi_bindgen_main()`
- Modify workspace members

**Interfaces (exported):**
- Mirror domain enums/records via `#[derive(uniffi::Enum/Record)]` **or** map in ffi layer — prefer re-export wrappers in ffi to keep domain UniFFI-free:
  - `FfiSpeechLanguage`, `FfiLanguagePolicy`, `FfiCaptionPhase`, `FfiCaptionEvent`
  - `MeetingCore` object:
    - `new() -> Arc<MeetingCore>`
    - `start_demo() ` — starts session with `LanguagePolicy::default_v1()`
    - `stop()`
    - `drain_events() -> Vec<FfiCaptionEvent>` — calls `push_tick` with monotonic ms and returns events
    - `state() -> String` ("idle"|"live"|"ended")

Use `Mutex<MeetingSession>` + `Instant` start inside `MeetingCore`.

- [ ] **Step 1: Rust unit test via `meetingraft-ffi` lib target** calling `MeetingCore` without Swift

- [ ] **Step 2: `cargo build -p meetingraft-ffi` PASS**

- [ ] **Step 3: Commit** `feat: UniFFI facade meetingraft-ffi`

---

### Task 4: Generate Swift bindings + Xcode link

**Files:**
- Create: `apps/macos/Scripts/generate-ffi.sh`
- Create: `apps/macos/Generated/` (output)
- Modify: `apps/macos/project.yml` — sources Generated, library search paths, OTHER_LDFLAGS `-lmeetingraft_ffi`, rpath to `rust/target/debug`
- Modify: `.gitignore` — ignore `*.dylib` copy in app if any; **do not** ignore Generated sources
- Modify: CI — build ffi before xcodebuild

**Script outline:**

```bash
#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/../../.." && pwd)"
cd "$ROOT/rust"
cargo build -p meetingraft-ffi
LIB="$ROOT/rust/target/debug/libmeetingraft_ffi.dylib"
OUT="$ROOT/apps/macos/Generated"
mkdir -p "$OUT"
cargo run -p uniffi-bindgen -- generate --library "$LIB" --language swift --out-dir "$OUT"
```

XcodeGen settings:
```yaml
LIBRARY_SEARCH_PATHS: $(SRCROOT)/../../rust/target/debug
OTHER_LDFLAGS: -lmeetingraft_ffi
LD_RUNPATH_SEARCH_PATHS: $(inherited) $(SRCROOT)/../../rust/target/debug
SWIFT_INCLUDE_PATHS / HEADER_SEARCH_PATHS as needed for modulemap
```

Include `Generated` sources; ensure bridging of `meetingraft_ffiFFI` modulemap per UniFFI Swift output.

- [ ] **Step 1: Run script, commit Generated**

- [ ] **Step 2: xcodebuild build SUCCEEDED**

- [ ] **Step 3: Commit** `feat: Swift bindings UniFFI и линковка dylib`

---

### Task 5: Wire UI to Rust captions

**Files:**
- Create: `apps/macos/Sources/LiveCaptions/RustCaptionStream.swift`
- Modify: `LiveCaptionsViewModel` default to `RustCaptionStream()`
- Modify/keep: `FakeCaptionStream` for unit tests only
- Create: `apps/macos/Tests/RustCaptionStreamSmokeTests.swift` — start/drain gets non-empty Russian text (skip if dylib missing? — require dylib in CI)

`RustCaptionStream`:
```swift
final class RustCaptionStream: CaptionStreaming, @unchecked Sendable {
  private let core = MeetingCore()
  private var task: Task<Void, Never>?
  func start(onEvent: ...) {
    core.startDemo()
    task = Task { @MainActor in
      while !Task.isCancelled {
        let events = core.drainEvents()
        for e in events {
          onEvent(CaptionLine(id: UUID(uuidString: e.id) ?? UUID(), text: e.text, phase: e.phase == .partial ? .partial : .final))
        }
        try? await Task.sleep(nanoseconds: 50_000_000)
      }
    }
  }
  func stop() { task?.cancel(); core.stop() }
}
```

Map UUID: Rust should emit UUID strings via `uuid` crate.

- [ ] **Step 1: Smoke test PASS**

- [ ] **Step 2: Manual — captions in app from Rust**

- [ ] **Step 3: Commit** `feat: live captions из Rust через UniFFI`

---

### Task 6: CI + docs

- CI rust job already tests workspace; ensure `meetingraft-ffi` builds
- CI macos: run `Scripts/generate-ffi.sh` (or `cargo build -p meetingraft-ffi`) before xcodegen/build/test
- Update `AGENTS.md` Setup UniFFI commands
- Mark Phase 2 done in roadmap/backlog
- PR

---

## Exit criteria

- [ ] Session transitions covered by `cargo test`
- [ ] Swift↔Rust smoke test passes
- [ ] Captions on screen originate in Rust (`RustCaptionStream`)

## Spec coverage

| Requirement | Task |
|-------------|------|
| Domain DTOs + language policy | 1 |
| Session state machine | 2 |
| UniFFI facade | 3 |
| Swift bindings wired | 4 |
| Captions from Rust in UI | 5 |
| CI | 6 |
