# Phase 5 — Glossary MVP Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Glossary terms normalize live captions (and bias Whisper prompt); SQLite + CSV import + sidebar CRUD via UniFFI.

**Architecture:** Crate `meetingraft-glossary` (normalize, CSV, prompt). Table `glossary_terms` in `meetingraft-storage`. `MeetingCore` loads active glossary on `start_recording`, normalizes every STT event before persist/drain, sets Whisper `initial_prompt` when backend is Whisper. Swift: `AppDestination.glossary` + CRUD UI.

**Tech Stack:** Rust workspace, rusqlite, UniFFI 0.32, SwiftUI, existing Mock STT for CI proofs.

**Spec:** `docs/superpowers/specs/2026-08-02-phase-5-glossary-design.md`

## Global Constraints

- Comments RU; Conventional Commits with Russian subject.
- Language policy: primary `ru`, allowed `{ru,en,es}` (ADR-003).
- SwiftUI: no glossary business rules — only UniFFI DTOs.
- Scopes in MVP: `global` + `meeting` only.
- Normalize always (Mock + Whisper); `initial_prompt` only when Whisper.
- Unique tmp roots in storage tests (atomic seq) — do not regress SQLITE_BUSY fix.
- After UniFFI API changes: `MEETINGRAFT_FFI_FEATURES= apps/macos/Scripts/generate-ffi.sh` (or with whisper locally).

## File map

| Path | Role |
|------|------|
| `rust/crates/domain/src/glossary.rs` | `GlossaryTerm`, `GlossaryScope` |
| `rust/crates/glossary/` | `GlossaryEngine`, CSV parse, normalize, prompt |
| `rust/crates/storage/src/audio_manifest.rs` | `glossary_terms` schema + CRUD |
| `rust/crates/stt/` | optional `set_initial_prompt` on trait / Whisper |
| `rust/crates/ffi/src/lib.rs` | UniFFI glossary + wire normalize on ingest |
| `apps/macos/Sources/Glossary/` | View + ViewModel |
| `apps/macos/Sources/App/AppDestination.swift` | `.glossary` |

---

### Task 1: Domain types `GlossaryTerm` / `GlossaryScope`

**Files:**
- Create: `rust/crates/domain/src/glossary.rs`
- Modify: `rust/crates/domain/src/lib.rs`

**Interfaces:**
- Produces: `GlossaryScope::{Global, Meeting { meeting_id: String }}`, `GlossaryTerm { id, surface, canonical, language: SpeechLanguage, scope: GlossaryScope }`

- [ ] **Step 1: Add failing unit test in domain**

```rust
#[test]
fn glossary_term_holds_meeting_scope() {
    let t = GlossaryTerm {
        id: "1".into(),
        surface: "униффи".into(),
        canonical: "UniFFI".into(),
        language: SpeechLanguage::Ru,
        scope: GlossaryScope::Meeting {
            meeting_id: "s1".into(),
        },
    };
    assert!(matches!(t.scope, GlossaryScope::Meeting { .. }));
}
```

- [ ] **Step 2: Implement types and `pub use` from `lib.rs`**

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GlossaryScope {
    Global,
    Meeting { meeting_id: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GlossaryTerm {
    pub id: String,
    pub surface: String,
    pub canonical: String,
    pub language: SpeechLanguage,
    pub scope: GlossaryScope,
}
```

- [ ] **Step 3: `cargo test -p meetingraft-domain` → pass → commit**

```bash
git add rust/crates/domain
git commit -m "feat: domain GlossaryTerm и GlossaryScope"
```

---

### Task 2: `meetingraft-glossary` — normalize + CSV + prompt

**Files:**
- Create: `rust/crates/glossary/Cargo.toml`, `src/lib.rs`, `src/engine.rs`, `src/csv_import.rs`, `src/normalize.rs`
- Modify: `rust/Cargo.toml` (workspace member)

**Interfaces:**
- Consumes: `GlossaryTerm`, `SpeechLanguage`
- Produces:
  - `GlossaryEngine::from_terms(Vec<GlossaryTerm>)`
  - `fn normalize_caption(&self, text: &str) -> String`
  - `fn build_whisper_prompt(&self, max_chars: usize) -> String`
  - `fn parse_csv(csv: &str) -> Result<(Vec<GlossaryTerm>, u32 /*skipped*/), String>`
  - `fn active_terms(all: &[GlossaryTerm], session_id: Option<&str>) -> Vec<GlossaryTerm>` — global + matching meeting

- [ ] **Step 1: Scaffold crate** (`name = "meetingraft-glossary"`, lib name `glossary`, deps: `meetingraft-domain`, `uuid`)

- [ ] **Step 2: Failing normalize tests**

```rust
#[test]
fn normalizes_uniffi_surface() {
    let eng = GlossaryEngine::from_terms(vec![term_global("униффи", "UniFFI")]);
    assert_eq!(eng.normalize_caption("посмотри униффи завтра"), "посмотри UniFFI завтра");
}

#[test]
fn longer_surface_wins() {
    let eng = GlossaryEngine::from_terms(vec![
        term_global("униф", "SHORT"),
        term_global("униффи", "UniFFI"),
    ]);
    assert_eq!(eng.normalize_caption("униффи"), "UniFFI");
}

#[test]
fn empty_glossary_is_identity() {
    let eng = GlossaryEngine::from_terms(vec![]);
    assert_eq!(eng.normalize_caption("привет"), "привет");
}
```

Normalize algorithm (MVP):
1. Sort surfaces by length descending.
2. Case-insensitive search for whole phrase boundaries (ascii/cyrillic letter boundaries; spaces/punctuation as separators).
3. Replace non-overlapping leftmost matches with canonical (canonical casing preserved).

- [ ] **Step 3: Implement until tests pass**

- [ ] **Step 4: CSV parse test**

Fixture:
```csv
surface,canonical,language,scope
униффи,UniFFI,ru,global
foo,Foo,en,global
bad,,,global
```
Expect: 2 terms imported logic in parse (skipped ≥ 1 for bad row). Generate ids with `Uuid::new_v4()`.

- [ ] **Step 5: `build_whisper_prompt` test** — unique canonicals, ru before en, truncated to `max_chars`

- [ ] **Step 6: `cargo test -p meetingraft-glossary` → commit**

```bash
git commit -m "feat: glossary engine normalize, CSV и whisper prompt"
```

---

### Task 3: SQLite `glossary_terms`

**Files:**
- Modify: `rust/crates/storage/src/audio_manifest.rs` (schema in `open` + methods)
- Optionally keep methods on `AudioManifestStore` (same DB file) — YAGNI, no separate store type

**Interfaces:**
- Produces on `AudioManifestStore`:
  - `fn upsert_glossary_term(&mut self, term: &GlossaryTerm, updated_at_ms: u64) -> Result<(), AudioManifestError>`
  - `fn delete_glossary_term(&mut self, id: &str) -> Result<(), AudioManifestError>`
  - `fn list_glossary_terms(&self) -> Result<Vec<GlossaryTerm>, AudioManifestError>`
  - `fn replace_glossary_from_import(&mut self, terms: &[GlossaryTerm], updated_at_ms: u64) -> Result<(), AudioManifestError>` — insert/upsert each in one transaction (do **not** wipe unrelated terms unless product chooses merge-by-upsert; **MVP = upsert each imported row by unique key / id**)

Schema (in existing `execute_batch`):
```sql
CREATE TABLE IF NOT EXISTS glossary_terms (
  id TEXT PRIMARY KEY NOT NULL,
  surface TEXT NOT NULL,
  canonical TEXT NOT NULL,
  language TEXT NOT NULL,
  scope TEXT NOT NULL,
  meeting_id TEXT,
  updated_at_ms INTEGER NOT NULL
);
CREATE UNIQUE INDEX IF NOT EXISTS idx_glossary_unique
  ON glossary_terms(surface, language, scope, ifnull(meeting_id, ''));
```

- [ ] **Step 1: Failing test `glossary_upsert_list_delete`** with `tmp_root()` (seq + thread id)

- [ ] **Step 2: Implement schema + methods**

Map `SpeechLanguage` ↔ `"ru"|"en"|"es"`; scope ↔ `"global"|"meeting"`.

- [ ] **Step 3: `cargo test -p meetingraft-storage` → commit**

```bash
git commit -m "feat: SQLite glossary_terms CRUD"
```

---

### Task 4: Wire STT prompt + normalize into `MeetingCore`

**Files:**
- Modify: `rust/crates/stt/src/engine.rs` — add default method:
  ```rust
  fn set_initial_prompt(&mut self, _prompt: &str) {}
  ```
- Modify: `rust/crates/stt/src/whisper.rs` — store prompt string; in `transcribe` call `params.set_initial_prompt(...)` when non-empty
- Modify: `rust/crates/stt/src/mock.rs` — no-op (default)
- Modify: `rust/crates/stt/src/window.rs` — `LiveCaptionPipeline::set_initial_prompt`, `with_glossary` not required if ffi owns `GlossaryEngine`
- Modify: `rust/crates/ffi/Cargo.toml` — dep `meetingraft-glossary`
- Modify: `rust/crates/ffi/src/lib.rs`

**Interfaces:**
- `MeetingCoreInner` gains `glossary: GlossaryEngine` (reload from DB on mutations and on `start_recording`)
- On `start_recording(session_id)`:
  1. Open store as today
  2. `list_glossary_terms` → `active_terms(..., Some(&session_id))` → `GlossaryEngine::from_terms`
  3. `pipeline.set_initial_prompt(&engine.build_whisper_prompt(800))` when backend Whisper (check `pipeline.backend()`)
- On ingest STT events / flush: for each event `event.text = glossary.normalize_caption(&event.text)` then persist + queue
- UniFFI DTOs:
  ```rust
  FfiGlossaryScope { Global, Meeting }
  FfiGlossaryTerm { id, surface, canonical, language: String, scope: FfiGlossaryScope, meeting_id: String }
  FfiGlossaryImportResult { imported: u32, skipped: u32, error: String }
  ```
- Methods: `list_glossary_terms`, `upsert_glossary_term`, `delete_glossary_term`, `import_glossary_csv`

- [ ] **Step 1: Failing ffi test**

```rust
#[test]
fn glossary_normalizes_live_mock_captions() {
    let root = /* unique tmp */;
    let core = MeetingCore::with_data_root(...);
    assert!(core.upsert_glossary_term(FfiGlossaryTerm {
        id: "t1".into(),
        surface: "речь".into(), // Mock final text contains «фрагмент речи» — use surface from mock final: "речи" or full phrase
        canonical: "РЕЧЬ".into(),
        language: "ru".into(),
        scope: FfiGlossaryScope::Global,
        meeting_id: String::new(),
    }).is_empty());
    // Prefer: change MockSttEngine final text to include a stable token "униффи" for this test,
    // OR upsert surface that appears in "[final ru] фрагмент речи" → e.g. surface "фрагмент речи" → "ФРАГМЕНТ"
    assert!(core.start_recording("g1".into()).is_empty());
    // push loud + silence like existing loud_mic_produces_live_captions
    let finals = core.drain_live_captions();
    assert!(finals.iter().any(|e| e.text.contains("ФРАГМЕНТ") || e.text.contains("РЕЧЬ") || e.text.contains("UniFFI")));
}
```

Concrete approach for stable CI: update `MockSttEngine` final string to `"[final {lang}] фрагмент речи униффи"` and assert normalize to `UniFFI`.

- [ ] **Step 2: Implement wiring + UniFFI methods**

- [ ] **Step 3: Regenerate Swift bindings**

```bash
MEETINGRAFT_FFI_FEATURES= apps/macos/Scripts/generate-ffi.sh
```

- [ ] **Step 4: `cargo test` + `cargo clippy --all-targets -- -D warnings` → commit**

```bash
git commit -m "feat: UniFFI glossary и normalize live captions"
```

---

### Task 5: Swift Glossary UI

**Files:**
- Modify: `apps/macos/Sources/App/AppDestination.swift` — add `case glossary`
- Modify: `apps/macos/Sources/Shell/AppShellView.swift` — host `GlossaryView`
- Create: `apps/macos/Sources/Glossary/GlossaryView.swift`
- Create: `apps/macos/Sources/Glossary/GlossaryViewModel.swift`
- Modify: `apps/macos/Sources/Settings/SettingsView.swift` — one line: Glossary в sidebar

**ViewModel duties (presentation only):**
- Hold `MeetingCore` with Application Support data root (same as `AudioCaptureCoordinator`)
- `reload()`, `upsert`, `delete`, `importCsv(String)`
- Meeting scope: if no live session id available, only allow Global in picker (disable Meeting)

- [ ] **Step 1: Destination + sidebar title `"Glossary"` / image `book`**

- [ ] **Step 2: List + toolbar Add + context Delete; sheet fields surface/canonical/language/scope**

- [ ] **Step 3: `.fileImporter` for `.commaSeparatedText` / `.plainText` → UTF-8 → `importGlossaryCsv`**

- [ ] **Step 4: `swiftformat Sources Tests --lint` + `xcodebuild … build` → commit**

```bash
git commit -m "feat: Glossary sidebar CRUD и CSV import"
```

---

### Task 6: Docs + PR

**Files:**
- Modify: `docs/roadmap.md` — Phase 4 done; Phase 5 in progress/done on branch
- Modify: `docs/backlog.md` — Epic 7 checkmarks for domain, UI, import, attach to session (scopes partial)
- Commit plan already in repo; ensure this plan file is on the feature branch

- [ ] **Step 1: Update roadmap/backlog**

- [ ] **Step 2: Full verify**

```bash
cd rust && cargo fmt --check && cargo test && cargo clippy --all-targets -- -D warnings
cd apps/macos && swiftformat Sources Tests --lint
# xcodebuild build+test as in CI
```

- [ ] **Step 3: Push `feat/phase-5-glossary` and open PR**

```bash
git commit -m "docs: Phase 5 glossary roadmap и backlog"
```

## Exit criteria

- [ ] `cargo test` proves normalize changes Mock live caption text
- [ ] Normalize + CSV unit tests green
- [ ] CSV fixture imports and lists via storage/ffi
- [ ] Glossary sidebar CRUD against UniFFI
- [ ] Docs updated

## Spec coverage check

| Spec item | Task |
|-----------|------|
| Domain model | 1 |
| Normalize + CSV + prompt | 2 |
| SQLite glossary_terms | 3 |
| Live wire + UniFFI | 4 |
| Sidebar CRUD | 5 |
| Roadmap/backlog | 6 |
| LoRA / voices out of scope | documented in spec only |
