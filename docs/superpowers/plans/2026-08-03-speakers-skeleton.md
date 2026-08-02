# Speakers skeleton — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Meeting-scoped Speakers CRUD + вкладка Speakers в Meetings detail (ручные метки, без diarization).

**Architecture:** Domain `Speaker` → SQLite в `AudioManifestStore` → UniFFI `MeetingCore` → `MeetingsViewModel` + segmented Speakers tab. Live captions без speaker attribution.

**Tech Stack:** Rust domain/storage/ffi, UniFFI, SwiftUI, XCTest, SwiftFormat, pre-commit (уже в ветке).

**Spec:** `docs/superpowers/specs/2026-08-03-speakers-skeleton-design.md`

## Global Constraints

- No pyannote / WhisperX / assignment to Final segments.
- No Cocoa types in Rust contracts; UniFFI-only Swift↔Rust.
- Live captions unchanged (no speaker labels) — ADR-002 / AGENTS.md.
- Comments Russian; identifiers English; Conventional Commits Russian subject.
- After UniFFI API changes: `apps/macos/Scripts/generate-ffi.sh` then `cd apps/macos && xcodegen generate`.
- `cargo test` + `swiftformat Sources Tests --lint` + `xcodebuild test` + `pre-commit run --all-files` green.
- Default Add name: `Спикер {n}` if session primary `ru`, else `Speaker {n}` (`n = list.count + 1`).

---

## File map

| File | Role |
|------|------|
| `rust/crates/domain/src/speaker.rs` (new) | `Speaker` struct |
| `rust/crates/domain/src/lib.rs` | `mod speaker` + re-export |
| `rust/crates/storage/src/audio_manifest.rs` | table + CRUD |
| `rust/crates/ffi/src/lib.rs` | `FfiSpeaker` + list/upsert/delete |
| `apps/macos/Generated/*` | regenerate |
| `apps/macos/Sources/Meetings/MeetingsViewModel.swift` | protocol + state |
| `apps/macos/Sources/Meetings/MeetingDetailView.swift` | Speakers tab |
| `apps/macos/Tests/MeetingsViewModelTests.swift` | spy tests |
| `docs/backlog.md`, `docs/roadmap.md`, `docs/architecture-and-install.md` | status |

---

### Task 1: Domain + SQLite CRUD (TDD)

**Files:**
- Create: `rust/crates/domain/src/speaker.rs`
- Modify: `rust/crates/domain/src/lib.rs`
- Modify: `rust/crates/storage/src/audio_manifest.rs` (bootstrap SQL + methods)
- Modify: `rust/crates/storage/src/lib.rs` if needed for re-exports (domain already dep)

**Interfaces:**
- Produces:
  ```rust
  pub struct Speaker {
      pub id: String,
      pub meeting_id: String,
      pub display_name: String,
      pub sort_index: i64,
  }
  ```
  - `AudioManifestStore::list_speakers(&self, meeting_id: &str) -> Result<Vec<Speaker>, AudioManifestError>`
  - `AudioManifestStore::upsert_speaker(&mut self, speaker: &Speaker) -> Result<(), AudioManifestError>`
  - `AudioManifestStore::delete_speaker(&mut self, id: &str) -> Result<(), AudioManifestError>`

- [ ] **Step 1: Domain type + failing storage test**

`speaker.rs`:

```rust
//! Спикеры встречи (ручные метки; diarization — позже).

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Speaker {
    pub id: String,
    pub meeting_id: String,
    pub display_name: String,
    pub sort_index: i64,
}
```

In `lib.rs`: `mod speaker;` + `pub use speaker::Speaker;`

In storage tests (same style as glossary tests — temp dir + `AudioManifestStore::open`):

```rust
#[test]
fn speakers_crud_and_meeting_isolation() {
    let root = temp_dir(...);
    let mut store = AudioManifestStore::open(&root).unwrap();
    let a = Speaker {
        id: "s1".into(),
        meeting_id: "m1".into(),
        display_name: "Алиса".into(),
        sort_index: 0,
    };
    let b = Speaker {
        id: "s2".into(),
        meeting_id: "m2".into(),
        display_name: "Bob".into(),
        sort_index: 0,
    };
    store.upsert_speaker(&a).unwrap();
    store.upsert_speaker(&b).unwrap();
    assert_eq!(store.list_speakers("m1").unwrap(), vec![a.clone()]);
    let mut renamed = a.clone();
    renamed.display_name = "Алиса К.".into();
    store.upsert_speaker(&renamed).unwrap();
    assert_eq!(store.list_speakers("m1").unwrap()[0].display_name, "Алиса К.");
    store.delete_speaker("s1").unwrap();
    assert!(store.list_speakers("m1").unwrap().is_empty());
    assert_eq!(store.list_speakers("m2").unwrap().len(), 1);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn speakers_ordered_by_sort_index() {
    // insert sort_index 2 then 0 then 1; list returns 0,1,2
}
```

- [ ] **Step 2: Run — expect FAIL**

```bash
cd rust && cargo test -p meetingraft-storage speakers_ -- --nocapture
```

Expected: missing methods / table.

- [ ] **Step 3: Implement schema + CRUD**

Append to `execute_batch` bootstrap:

```sql
CREATE TABLE IF NOT EXISTS speakers (
    id TEXT PRIMARY KEY NOT NULL,
    meeting_id TEXT NOT NULL,
    display_name TEXT NOT NULL,
    sort_index INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_speakers_meeting
    ON speakers(meeting_id, sort_index);
```

Implement `list_speakers` / `upsert_speaker` / `delete_speaker` mirroring glossary patterns (`INSERT ... ON CONFLICT(id) DO UPDATE` for upsert).

Import `Speaker` from `meetingraft_domain` in storage (already depends on domain).

- [ ] **Step 4: Tests PASS**

```bash
cd rust && cargo test -p meetingraft-domain -- --nocapture
cd rust && cargo test -p meetingraft-storage speakers_ -- --nocapture
cd rust && cargo fmt && cargo clippy -p meetingraft-storage -p meetingraft-domain --all-targets -- -D warnings
```

- [ ] **Step 5: Commit**

```bash
git add rust/crates/domain rust/crates/storage
git commit -m "$(cat <<'EOF'
feat: Speaker domain + SQLite speakers CRUD

EOF
)"
```

---

### Task 2: UniFFI facade + regenerate

**Files:**
- Modify: `rust/crates/ffi/src/lib.rs`
- Regenerate: `apps/macos/Generated/`
- Optional test in `ffi` `mod tests`

**Interfaces:**
- Consumes: storage speakers API
- Produces:
  ```rust
  #[derive(uniffi::Record)]
  pub struct FfiSpeaker {
      pub id: String,
      pub meeting_id: String,
      pub display_name: String,
      pub sort_index: i64,
  }
  ```
  - `MeetingCore::list_speakers(meeting_id: String) -> Vec<FfiSpeaker>`
  - `MeetingCore::upsert_speaker(meeting_id, id, display_name, sort_index) -> String`
    - if `id` empty → `Uuid::new_v4()`; if `sort_index < 0` treat as `list.len() as i64` **or** require caller to pass index (prefer: if `id` empty, set `sort_index` to `list_speakers.len() as i64` when caller passes `-1` OR always use provided sort_index from Swift)
    - Spec: Swift computes `n`; FFI trusts `sort_index` as given. Empty id → new UUID.
  - `MeetingCore::delete_speaker(id: String) -> String`

Use same `read_store` / `write_store` helpers as artifacts/glossary.

- [ ] **Step 1: Failing FFI test (optional but preferred)**

```rust
#[test]
fn speakers_round_trip_via_core() {
    let root = temp...;
    let core = MeetingCore::with_data_root(...);
    assert!(core.upsert_speaker("m1".into(), "".into(), "Спикер 1".into(), 0).is_empty());
    let list = core.list_speakers("m1".into());
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].display_name, "Спикер 1");
    assert!(!list[0].id.is_empty());
    assert!(core.delete_speaker(list[0].id.clone()).is_empty());
    assert!(core.list_speakers("m1".into()).is_empty());
    let _ = fs::remove_dir_all(root);
}
```

- [ ] **Step 2: Implement + cargo test -p meetingraft-ffi**

- [ ] **Step 3: Regenerate FFI**

```bash
apps/macos/Scripts/generate-ffi.sh
cd apps/macos && xcodegen generate
```

Confirm Generated has `FfiSpeaker`, `listSpeakers`, `upsertSpeaker`, `deleteSpeaker`.

- [ ] **Step 4: Commit**

```bash
git add rust/crates/ffi apps/macos/Generated
git commit -m "$(cat <<'EOF'
feat: UniFFI list/upsert/delete speakers

EOF
)"
```

---

### Task 3: Swift ViewModel + Speakers UI

**Files:**
- Modify: `apps/macos/Sources/Meetings/MeetingsViewModel.swift`
- Modify: `apps/macos/Sources/Meetings/MeetingDetailView.swift`
- Modify: `apps/macos/Tests/MeetingsViewModelTests.swift`

**Interfaces:**
- Extend `MeetingsCoreProviding`:
  ```swift
  func listSpeakers(meetingId: String) -> [FfiSpeaker]
  func upsertSpeaker(meetingId: String, id: String, displayName: String, sortIndex: Int64) -> String
  func deleteSpeaker(id: String) -> String
  ```
- ViewModel:
  - `speakers: [FfiSpeaker]`
  - `reload(meetingId:)` also loads speakers
  - `addSpeaker(meetingId:primaryLanguage:)` — builds default name from `primaryLanguage` (`ru` → `Спикер \(n)`, else `Speaker \(n)`), `sortIndex = Int64(speakers.count)`, empty id
  - `renameSpeaker(meetingId:id:displayName:)`
  - `removeSpeaker(id:meetingId:)` — delete then reload speakers
  - Pass `primaryLanguage` from `SessionLanguageStore` via view (Environment) into `addSpeaker`

- [ ] **Step 1: Failing ViewModel tests**

```swift
func testReloadPublishesSpeakers() {
    let speaker = FfiSpeaker(id: "s1", meetingId: "m1", displayName: "Алиса", sortIndex: 0)
    let core = MeetingsCoreSpy(speakers: [speaker])
    let vm = MeetingsViewModel(core: core)
    vm.reload(meetingId: "m1")
    XCTAssertEqual(vm.speakers, [speaker])
}

func testAddSpeakerUsesRussianDefaultName() {
    let core = MeetingsCoreSpy()
    let vm = MeetingsViewModel(core: core)
    vm.reload(meetingId: "m1")
    vm.addSpeaker(meetingId: "m1", primaryLanguage: "ru")
    XCTAssertEqual(core.lastUpsertDisplayName, "Спикер 1")
    XCTAssertEqual(core.lastUpsertSortIndex, 0)
}

func testRemoveSpeakerSurfacesCoreError() {
    let core = MeetingsCoreSpy()
    core.deleteSpeakerError = "boom"
    let vm = MeetingsViewModel(core: core)
    vm.removeSpeaker(id: "s1", meetingId: "m1")
    XCTAssertEqual(vm.errorMessage, "boom")
}
```

Extend spy with speakers storage / call recording.

- [ ] **Step 2: Implement ViewModel — tests PASS**

- [ ] **Step 3: UI Speakers tab**

In `MeetingDetailSection` add `.speakers` / title `"Speakers"`.

```swift
case .speakers:
    speakersPanel
```

Panel sketch:

```swift
private var speakersPanel: some View {
    VStack(spacing: 0) {
        provenanceBanner("Ручные метки · diarization — скоро")
        HStack {
            Button("Add", systemImage: "person.badge.plus") {
                viewModel.addSpeaker(
                    meetingId: meeting.id,
                    primaryLanguage: languageStore.primary.rawValue // SpeechLanguage: ru/en/es
                )
            }
            Spacer()
        }
        .padding()
        List {
            ForEach(viewModel.speakers, id: \.id) { speaker in
                // TextField bound via local @State or rename onSubmit
                // Delete button
            }
        }
        .overlay {
            if viewModel.speakers.isEmpty {
                ContentUnavailableView("Спикеров нет", systemImage: "person.3")
            }
        }
    }
}
```

Inject `@Environment(SessionLanguageStore.self)` — поле `primary: SpeechLanguage` (`.ru` / `.en` / `.es`).

Rename: simplest — `TextField` with `onSubmit` calling `renameSpeaker`. Delete: button → `removeSpeaker`.

Do **not** change Live captions rendering.

- [ ] **Step 4: swiftformat + focused tests**

```bash
cd apps/macos && swiftformat Sources Tests --lint
xcodegen generate
xcodebuild ... -only-testing:MeetingRaftTests/MeetingsViewModelTests
```

- [ ] **Step 5: Commit**

```bash
git add apps/macos/Sources/Meetings apps/macos/Tests/MeetingsViewModelTests.swift
git commit -m "$(cat <<'EOF'
feat: вкладка Speakers в Meetings detail

EOF
)"
```

---

### Task 4: Docs + full verify

**Files:**
- `docs/backlog.md` — Epic 9: speaker entities + correction screen → done/partial skeleton note
- `docs/roadmap.md` — Remaining: speakers skeleton done; diarization still remaining
- `docs/architecture-and-install.md` — Speakers tab in Meetings

Pre-commit already on branch — verify still green; do not re-add config.

- [ ] **Step 1: Docs updates**

- [ ] **Step 2: Full verify**

```bash
cd rust && cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test
cd apps/macos && swiftformat Sources Tests --lint && xcodegen generate
xcodebuild -project MeetingRaft.xcodeproj -scheme MeetingRaft \
  -configuration Debug test CODE_SIGNING_ALLOWED=NO
pre-commit run --all-files
```

- [ ] **Step 3: Commit**

```bash
git add docs
git commit -m "$(cat <<'EOF'
docs: Speakers skeleton в backlog/roadmap/install

EOF
)"
```

---

## Spec coverage checklist

| Spec item | Task |
|-----------|------|
| Domain Speaker | 1 |
| SQLite speakers + CRUD | 1 |
| UniFFI list/upsert/delete | 2 |
| Speakers tab + banner | 3 |
| Default name ru/en | 3 |
| ViewModel tests | 3 |
| Docs | 4 |
| pre-commit present | done on branch (`e8a1827`); verify in 4 |
| No diarization / Final assignment | all (non-goals) |
