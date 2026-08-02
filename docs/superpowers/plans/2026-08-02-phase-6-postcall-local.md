# Phase 6 MVP — Local Post-call Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Stop Live → FinalTranscript из live finals → Meetings UI (Live | Final | Artifacts) → Brief + Follow-up markdown локально, без backend.

**Architecture:** Domain types + `meetingraft-postcall` (assemble + heuristic templates + stub `LlmClient` trait). SQLite `final_transcripts` / `artifacts` + `list_captions`. `MeetingCore.stop_recording` собирает final; UniFFI для list/generate. Swift Meetings screens. Backend/LLM HTTP — out of scope.

**Tech Stack:** Rust workspace, rusqlite, UniFFI 0.32, SwiftUI, glossary normalize reuse.

**Spec:** `docs/superpowers/specs/2026-08-02-phase-6-postcall-local-design.md`

## Global Constraints

- Comments RU; Conventional Commits with Russian subject.
- ADR-002: live captions ≠ final transcript (separate tables/models).
- ADR-003: primary `ru` copy in templates by default.
- No HTTP / FastAPI / sync client in this PR (ADR-007 next).
- No real LLM calls; only `LlmClient` trait stub for Ollama/LM Studio/Gemma later.
- SwiftUI: presentation only; business logic in Rust.
- Unique tmp roots in storage tests; after UniFFI changes run `MEETINGRAFT_FFI_FEATURES= apps/macos/Scripts/generate-ffi.sh`.
- Share one `MeetingCore` / Application Support root with Live + Glossary (existing AppShell pattern).

## File map

| Path | Role |
|------|------|
| `rust/crates/domain/src/postcall.rs` | `FinalTranscript`, `Artifact`, `ArtifactKind`, `MeetingSummary` |
| `rust/crates/postcall/` | assemble, templates, `LlmClient` stub |
| `rust/crates/storage/src/audio_manifest.rs` | schema + CRUD finals/artifacts/list_captions/list_sessions |
| `rust/crates/ffi/src/lib.rs` | UniFFI + assemble on stop |
| `apps/macos/Sources/Meetings/` | list + detail UI |

---

### Task 1: Domain post-call types

**Files:**
- Create: `rust/crates/domain/src/postcall.rs`
- Modify: `rust/crates/domain/src/lib.rs`

**Interfaces:**
- Produces:
  - `FinalTranscript { meeting_id, version: u32, body_markdown, created_at_ms }`
  - `ArtifactKind::{Brief, FollowUp}`
  - `Artifact { id, meeting_id, kind, template_id, body_markdown, created_at_ms }`
  - `MeetingSummary { id, started_at_ms, has_final, artifact_count }`

- [ ] **Step 1: Failing test**

```rust
#[test]
fn artifact_kind_brief_distinct_from_follow_up() {
    assert_ne!(ArtifactKind::Brief, ArtifactKind::FollowUp);
}
```

- [ ] **Step 2: Implement types + `pub use`**

- [ ] **Step 3:** `cargo test -p meetingraft-domain` → commit

```bash
git commit -m "feat: domain FinalTranscript и Artifact"
```

---

### Task 2: `meetingraft-postcall` — assemble + templates + LlmClient stub

**Files:**
- Create: `rust/crates/postcall/Cargo.toml`, `src/lib.rs`, `src/assemble.rs`, `src/templates.rs`, `src/llm.rs`
- Modify: `rust/Cargo.toml` (member)

**Interfaces:**
- Consumes: `CaptionEvent`, `CaptionPhase`, `GlossaryEngine` (or `Fn(&str)->String` normalize), domain postcall types
- Produces:
  - `fn assemble_final(meeting_id, captions: &[CaptionEvent], normalize: impl Fn(&str)->String, now_ms) -> FinalTranscript`
  - `fn render_brief(final_body: &str, primary_lang: SpeechLanguage) -> String`
  - `fn render_follow_up(final_body: &str, primary_lang: SpeechLanguage, date_label: &str) -> String`
  - `fn make_artifact(meeting_id, kind, body, now_ms) -> Artifact` (sets template_id)
  - `pub trait LlmClient: Send { fn complete(&self, system: &str, user: &str) -> Result<String, LlmError>; }`
  - `pub struct NullLlmClient;` — always `Err(NotConfigured)` (documents Ollama/LM Studio/Gemma later)

- [ ] **Step 1: Scaffold crate** (`meetingraft-postcall`, lib `postcall`, deps: domain, thiserror)

- [ ] **Step 2: Assemble tests**

```rust
#[test]
fn assemble_keeps_finals_only_and_normalizes() {
    let caps = vec![
        CaptionEvent { id: "1".into(), text: "частичный".into(), phase: CaptionPhase::Partial },
        CaptionEvent { id: "2".into(), text: "привет униффи".into(), phase: CaptionPhase::Final },
        CaptionEvent { id: "3".into(), text: "вторая".into(), phase: CaptionPhase::Final },
    ];
    let ft = assemble_final("m1", &caps, |t| t.replace("униффи", "UniFFI"), 100);
    assert_eq!(ft.body_markdown, "привет UniFFI\n\nвторая");
    assert_eq!(ft.version, 1);
}

#[test]
fn assemble_empty_finals_yields_empty_body() {
    let ft = assemble_final("m1", &[], |_| unreachable!(), 1);
    assert!(ft.body_markdown.is_empty());
}
```

- [ ] **Step 3: Template tests**

```rust
#[test]
fn brief_has_required_headings() {
    let md = render_brief("Первый абзац.\n\nНужно сделать X.", SpeechLanguage::Ru);
    assert!(md.contains("# Brief"));
    assert!(md.contains("## Summary"));
    assert!(md.contains("## Key points"));
    assert!(md.contains("## Next steps"));
    assert!(md.contains("Нужно сделать X") || md.contains("- "));
}

#[test]
fn follow_up_has_subject_comment_and_ru_greeting() {
    let md = render_follow_up("Итог один.", SpeechLanguage::Ru, "2026-08-02");
    assert!(md.contains("<!-- subject:"));
    assert!(md.contains("Итоги встречи") || md.contains("Здравствуйте"));
}
```

Heuristic details (implement exactly):
- Summary = first paragraph truncated to 280 chars.
- Key points = one `- ` bullet per non-empty paragraph.
- Next steps = paragraphs containing case-insensitive substrings: `нужно`, `сделать`, `todo`, `action`; else `- —`.
- Follow-up RU: `Здравствуйте,` + summary + bullets + `Пожалуйста, проверьте и дополните, если что-то упущено.`

- [ ] **Step 4:** `cargo test -p meetingraft-postcall` → commit

```bash
git commit -m "feat: postcall assemble и builtin-шаблоны Brief/Follow-up"
```

---

### Task 3: Storage — captions list, finals, artifacts, meetings

**Files:**
- Modify: `rust/crates/storage/src/audio_manifest.rs`
- Modify: `rust/crates/storage/src/lib.rs` if re-exports needed
- Dep: storage may depend on domain only (already); CaptionEvent already in domain

**Interfaces on `AudioManifestStore`:**
- `fn list_captions(&self, session_id: &str) -> Result<Vec<CaptionEvent>, …>` — order by `created_at_ms`, map phase
- `fn upsert_final_transcript(&mut self, t: &FinalTranscript) -> Result<(), …>`
- `fn get_final_transcript(&self, meeting_id: &str) -> Result<Option<FinalTranscript>, …>` — latest version (MVP version=1)
- `fn insert_artifact(&mut self, a: &Artifact) -> Result<(), …>`
- `fn list_artifacts(&self, meeting_id: &str) -> Result<Vec<Artifact>, …>`
- `fn list_meeting_summaries(&self) -> Result<Vec<MeetingSummary>, …>` — from `sessions` LEFT JOIN finals/artifacts

Schema additions in `open()` `execute_batch` (as in spec).

- [ ] **Step 1: Tests** with `tmp_root()` — captions list; upsert final; insert+list artifacts; meeting summary flags

- [ ] **Step 2: Implement**

Note: `list_captions` must work when store reopened on same root (no active recording). Prefer always using `conn` queries by `session_id`.

- [ ] **Step 3:** `cargo test -p meetingraft-storage` → commit

```bash
git commit -m "feat: SQLite final_transcripts, artifacts и list_captions"
```

---

### Task 4: UniFFI — stop assembles final + meetings/artifacts API

**Files:**
- Modify: `rust/crates/ffi/Cargo.toml` — dep `meetingraft-postcall`
- Modify: `rust/crates/ffi/src/lib.rs`
- Regenerate: `apps/macos/Generated/*` via `generate-ffi.sh`

**Interfaces:**
```rust
FfiMeetingSummary { id, started_at_ms, has_final, artifact_count }
FfiFinalTranscript { meeting_id, version, body_markdown, created_at_ms } // empty meeting_id = none
FfiArtifactKind { Brief, FollowUp }
FfiArtifact { id, meeting_id, kind, template_id, body_markdown, created_at_ms }
FfiCaptionEvent // already exists

list_meetings() -> Vec<FfiMeetingSummary>
list_captions(meeting_id) -> Vec<FfiCaptionEvent>
get_final_transcript(meeting_id) -> FfiFinalTranscript
list_artifacts(meeting_id) -> Vec<FfiArtifact>
generate_artifact(meeting_id, kind) -> FfiArtifact  // empty id + use lastError pattern OR return String error via separate out — prefer: returns artifact; on failure return Artifact with empty id and put reason in a String-returning twin OR `generate_artifact(...) -> String` error + `take` — **use:** `generate_artifact -> String` (empty ok) and artifact via list; **simpler:** return `FfiArtifact` and on error return placeholder with `body_markdown` = "" and `id` = "" while logging — **plan choice:** `generate_artifact(...) -> FfiGenerateArtifactResult { artifact: FfiArtifact, error: String }`
assemble_final_now(meeting_id) -> String  // empty = ok
```

**`stop_recording` change:**
1. Existing flush captions to store/queue.
2. Before clearing `recording_session_id` / store: if `Some(sid)`, open path:
   - `list_captions(&sid)`
   - reload glossary active for sid → normalize
   - `assemble_final` → `upsert_final_transcript`
3. Then end session / clear as today.

`generate_artifact`: load final (else error); render template; `insert_artifact`; return result.

Open store when idle: same pattern as `manifest_chunk_count` (reopen from `data_root`).

- [ ] **Step 1: Failing ffi test**

```rust
#[test]
fn stop_recording_writes_final_transcript() {
    // start_recording, ingest loud+silence (mock finals with униффи), upsert glossary optional
    core.stop_recording();
    let ft = core.get_final_transcript("…session…".into());
    assert!(!ft.body_markdown.is_empty());
    let art = core.generate_artifact(sid, FfiArtifactKind::Brief);
    assert!(art.error.is_empty());
    assert!(art.artifact.body_markdown.contains("# Brief"));
}
```

- [ ] **Step 2: Implement + regenerate bindings**

- [ ] **Step 3:** `cargo test` + clippy → commit

```bash
git commit -m "feat: UniFFI post-call final и artifacts"
```

---

### Task 5: Swift Meetings UI

**Files:**
- Create: `apps/macos/Sources/Meetings/MeetingsListView.swift`
- Create: `apps/macos/Sources/Meetings/MeetingDetailView.swift`
- Create: `apps/macos/Sources/Meetings/MeetingsViewModel.swift`
- Modify: `apps/macos/Sources/Shell/AppShellView.swift` — replace stub; pass shared `MeetingCore`
- Optional: `apps/macos/Tests/MeetingsViewModelTests.swift` with spy core protocol

**UI:**
- List: meeting id (short) + date from `started_at_ms` + badges final/artifacts
- Detail: `Picker` Live | Final | Artifacts
  - Live: `List` of `listCaptions` (italic partial / normal final)
  - Final: scrollable markdown/`Text`
  - Artifacts: buttons Generate Brief / Follow-up; list; selected body + Copy (`NSPasteboard`)
- On appear / after navigation: `reload()`

- [ ] **Step 1: Wire navigation from `AppDestination.meetings`**

- [ ] **Step 2: ViewModel over shared core**

- [ ] **Step 3: swiftformat + xcodebuild test** → commit

```bash
git commit -m "feat: Meetings UI Live/Final/Artifacts"
```

---

### Task 6: Docs + PR

**Files:**
- Modify: `docs/roadmap.md` — Phase 6 status in progress/done on branch
- Modify: `docs/backlog.md` — Epic 8 partial checkmarks; note backend/LLM deferred

- [ ] **Step 1: Docs update**

- [ ] **Step 2: Full verify** (`cargo fmt/test/clippy`, swiftformat, xcodebuild)

- [ ] **Step 3: Push `feat/phase-6-postcall-local` + draft PR**

```bash
git commit -m "docs: Phase 6 local post-call roadmap и backlog"
```

## Exit criteria

- [ ] Stop Live persists FinalTranscript
- [ ] Meetings shows Live vs Final
- [ ] Brief + Follow-up generate in UI
- [ ] Assemble + template unit tests green
- [ ] Docs updated; backend/LLM noted as follow-up

## Spec coverage

| Spec item | Task |
|-----------|------|
| Domain types | 1 |
| assemble + templates + LlmClient stub | 2 |
| SQLite + list_captions | 3 |
| UniFFI + stop hook | 4 |
| Meetings UI | 5 |
| Roadmap/backlog | 6 |
| Backend / real LLM | deferred |
