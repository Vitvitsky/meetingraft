# Versioned Final transcript + Compare — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Каждый assemble создаёт новую version Final; Meetings показывают picker версий и секцию Compare (Live | Final vN); Brief/Export всегда latest.

**Architecture:** Storage умеет list/get_by_version/next; `assemble_final` принимает явный `version`; FFI пишет `max+1`; Swift Final picker + новая секция Compare side-by-side без line-diff.

**Tech Stack:** Rust `meetingraft-storage` / `postcall` / UniFFI; SwiftUI Meetings; XCTest.

**Spec:** `docs/superpowers/specs/2026-08-03-final-transcript-versions-compare-design.md`

## Global Constraints

- Brief / Follow-up / Export / `has_final` → всегда **latest** (`ORDER BY version DESC LIMIT 1`).
- Обычный assemble **не** overwrite: всегда `next_version`.
- Compare: без line-diff; Live column = join caption finals `\n\n` (raw store).
- Comments Russian; identifiers English; Conventional Commits Russian subject.
- TDD per task; after UniFFI changes: `apps/macos/Scripts/generate-ffi.sh`.

## File map

| File | Role |
|------|------|
| `rust/crates/storage/src/audio_manifest.rs` | `next_final_version`, `list_final_transcripts`, `get_final_transcript_version` |
| `rust/crates/postcall/src/assemble.rs` | `assemble_final(..., version: u32)` |
| `rust/crates/ffi/src/lib.rs` | next version on assemble; list/get_version UniFFI |
| `apps/macos/Generated/*` | Regenerated bindings |
| `apps/macos/Sources/Meetings/MeetingsViewModel.swift` | `finalVersions`, `selectedFinalVersion`, reload |
| `apps/macos/Sources/Meetings/MeetingDetailView.swift` | Final picker + Compare section |
| `apps/macos/Tests/MeetingsViewModelTests.swift` | Fake core + selection tests |
| `docs/backlog.md`, `docs/roadmap.md`, `docs/architecture-and-install.md` | Docs |

---

### Task 1: Storage — next / list / get_by_version (TDD)

**Files:**
- Modify: `rust/crates/storage/src/audio_manifest.rs`
- Tests in same file `#[cfg(test)]`

**Interfaces:**
```rust
impl AudioManifestStore {
    pub fn next_final_version(&self, meeting_id: &str) -> Result<u32, AudioManifestError>;
    // MAX(version)+1 or 1 if none

    pub fn list_final_transcripts(
        &self,
        meeting_id: &str,
    ) -> Result<Vec<FinalTranscript>, AudioManifestError>;
    // ORDER BY version DESC

    pub fn get_final_transcript_version(
        &self,
        meeting_id: &str,
        version: u32,
    ) -> Result<Option<FinalTranscript>, AudioManifestError>;

    // get_final_transcript — unchanged (latest)
}
```

- [ ] **Step 1: Failing tests**

```rust
#[test]
fn next_list_and_get_final_versions() {
    let root = tmp_root();
    {
        let mut store = AudioManifestStore::open(&root).unwrap();
        assert_eq!(store.next_final_version("m1").unwrap(), 1);
        store
            .upsert_final_transcript(&FinalTranscript {
                meeting_id: "m1".into(),
                version: 1,
                body_markdown: "one".into(),
                created_at_ms: 10,
            })
            .unwrap();
        store
            .upsert_final_transcript(&FinalTranscript {
                meeting_id: "m1".into(),
                version: 2,
                body_markdown: "two".into(),
                created_at_ms: 20,
            })
            .unwrap();
        assert_eq!(store.next_final_version("m1").unwrap(), 3);
        let list = store.list_final_transcripts("m1").unwrap();
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].version, 2);
        assert_eq!(list[0].body_markdown, "two");
        assert_eq!(list[1].version, 1);
        assert_eq!(
            store.get_final_transcript("m1").unwrap().unwrap().version,
            2
        );
        assert_eq!(
            store
                .get_final_transcript_version("m1", 1)
                .unwrap()
                .unwrap()
                .body_markdown,
            "one"
        );
        assert_eq!(store.get_final_transcript_version("m1", 9).unwrap(), None);
    }
    let _ = fs::remove_dir_all(&root);
}
```

- [ ] **Step 2: Run — expect FAIL**

```bash
cd rust && cargo test -p meetingraft-storage next_list_and_get_final_versions -- --nocapture
```

- [ ] **Step 3: Implement** SQL helpers; keep existing `upsert_final_transcript_overwrites_same_version` test.

- [ ] **Step 4: Run — expect PASS**

```bash
cd rust && cargo test -p meetingraft-storage -- --nocapture
```

- [ ] **Step 5: Commit**

```bash
git add rust/crates/storage/src/audio_manifest.rs
git commit -m "$(cat <<'EOF'
feat: list/get версий Final transcript в storage

EOF
)"
```

---

### Task 2: assemble_final takes version (TDD)

**Files:**
- Modify: `rust/crates/postcall/src/assemble.rs`
- Update all call sites that pass version (ffi Task 3; fix compile in this task for postcall tests only)

**Interfaces:**
```rust
pub fn assemble_final(
    meeting_id: &str,
    captions: &[CaptionEvent],
    normalize: impl Fn(&str) -> String,
    now_ms: u64,
    version: u32,
) -> FinalTranscript;
```

- [ ] **Step 1: Update failing test** — change existing assert `version == 1` to pass `version: 7` and assert `7`; update empty-finals call.

```rust
let transcript = assemble_final("m1", &captions, |text| ..., 100, 7);
assert_eq!(transcript.version, 7);
```

- [ ] **Step 2: Implement** — use parameter instead of hardcoded `1`.

- [ ] **Step 3:**

```bash
cd rust && cargo test -p meetingraft-postcall -- --nocapture
```

- [ ] **Step 4: Commit**

```bash
git add rust/crates/postcall/src/assemble.rs
git commit -m "$(cat <<'EOF'
feat: assemble_final принимает явный version

EOF
)"
```

Note: `meetingraft-ffi` may not compile until Task 3 — if workspace `cargo test` breaks, Task 3 immediately follows; prefer `cargo test -p meetingraft-postcall` for this task gate.

---

### Task 3: UniFFI — next version assemble + list/get_version

**Files:**
- Modify: `rust/crates/ffi/src/lib.rs`
- Regenerate: `apps/macos/Generated/` via `apps/macos/Scripts/generate-ffi.sh`

**Interfaces:**
```rust
fn assemble_and_store_final(...) {
    let version = store.next_final_version(meeting_id)?;
    let transcript = assemble_final(..., now_ms(), version);
    store.upsert_final_transcript(&transcript)?;
}

pub fn list_final_transcripts(&self, meeting_id: String) -> Vec<FfiFinalTranscript>;
pub fn get_final_transcript_version(
    &self,
    meeting_id: String,
    version: u32,
) -> FfiFinalTranscript; // empty record if missing
```

- [ ] **Step 1: Failing FFI test**

```rust
#[test]
fn assemble_final_now_increments_version() {
    // use existing temp MeetingCore harness
    // seed captions finals for meeting
    assert!(core.assemble_final_now(meeting_id.clone()).is_empty());
    let v1 = core.get_final_transcript(meeting_id.clone());
    assert_eq!(v1.version, 1);
    assert!(core.assemble_final_now(meeting_id.clone()).is_empty());
    let latest = core.get_final_transcript(meeting_id.clone());
    assert_eq!(latest.version, 2);
    let list = core.list_final_transcripts(meeting_id.clone());
    assert_eq!(list.len(), 2);
    assert_eq!(list[0].version, 2);
    let old = core.get_final_transcript_version(meeting_id.clone(), 1);
    assert_eq!(old.version, 1);
    assert_eq!(old.body_markdown, v1.body_markdown);
}
```

Reuse caption seeding pattern from existing `assemble` / meetings tests in `lib.rs`.

- [ ] **Step 2: Run — expect FAIL**

```bash
cd rust && cargo test -p meetingraft-ffi assemble_final_now_increments_version -- --nocapture
```

- [ ] **Step 3: Implement** — fix `assemble_and_store_final` call to pass version; add UniFFI methods; empty FFI when missing version.

- [ ] **Step 4:**

```bash
cd rust && cargo test -p meetingraft-ffi -- --nocapture
apps/macos/Scripts/generate-ffi.sh
```

- [ ] **Step 5: Commit**

```bash
git add rust/crates/ffi/src/lib.rs apps/macos/Generated/
git commit -m "$(cat <<'EOF'
feat: UniFFI list Final versions и next version на assemble

EOF
)"
```

---

### Task 4: Swift Final picker + Compare section

**Files:**
- Modify: `apps/macos/Sources/Meetings/MeetingsViewModel.swift`
- Modify: `apps/macos/Sources/Meetings/MeetingDetailView.swift`
- Modify: `apps/macos/Tests/MeetingsViewModelTests.swift` (fake core)

**Interfaces:**
```swift
protocol MeetingsCoreProviding {
    // existing...
    func listFinalTranscripts(meetingId: String) -> [FfiFinalTranscript]
    func getFinalTranscriptVersion(meetingId: String, version: UInt32) -> FfiFinalTranscript
}

// MeetingsViewModel
private(set) var finalVersions: [FfiFinalTranscript] = []
var selectedFinalVersion: UInt32?  // nil → treat as latest
var selectedFinalBody: String { ... } // from versions or finalTranscript

func liveFinalsText(from captions: [FfiCaptionEvent]) -> String {
    captions.filter { $0.phase == .final }.map(\.text).joined(separator: "\n\n")
}
```

- [ ] **Step 1: Extend protocol + fake + reload**

On `reload(meetingId:)`:
- `finalTranscript = getFinalTranscript` (latest)
- `finalVersions = listFinalTranscripts`
- `selectedFinalVersion = finalTranscript?.version` (latest)

- [ ] **Step 2: Final tab UI** — picker over `finalVersions` labels `v\(version)`; show body of selected via `getFinalTranscriptVersion` or cached list item; if empty versions → existing empty state.

- [ ] **Step 3: Compare section**

```swift
case compare // in MeetingDetailSection, title "Compare"
```

`HSplitView`:
- Left: ScrollView Text(liveFinalsText) + banner «Live finals»
- Right: version picker + Final body
- Empty overlays when no captions finals / no Final

- [ ] **Step 4: Tests**

```swift
func testReloadLoadsFinalVersionsDescending() {
    // fake returns two versions; assert viewModel.finalVersions[0].version == 2
    // selectedFinalVersion == 2
}
```

- [ ] **Step 5: Build/test**

```bash
cd apps/macos && xcodegen generate
xcodebuild -project MeetingRaft.xcodeproj -scheme MeetingRaft -configuration Debug test CODE_SIGNING_ALLOWED=NO
# or at least swiftformat + MeetingsViewModelTests
```

- [ ] **Step 6: Commit**

```bash
git add apps/macos/Sources apps/macos/Tests
git commit -m "$(cat <<'EOF'
feat: Final version picker и секция Compare Live vs Final

EOF
)"
```

---

### Task 5: Docs

**Files:**
- Modify: `docs/backlog.md` Epic 9
- Modify: `docs/roadmap.md` Remaining
- Modify: `docs/architecture-and-install.md` (short note)

- [ ] **Step 1:** Mark versioned Final + Compare done; diarization / speaker binding still deferred.

- [ ] **Step 2: Commit**

```bash
git add docs/backlog.md docs/roadmap.md docs/architecture-and-install.md
git commit -m "$(cat <<'EOF'
docs: версии Final transcript и Compare в backlog

EOF
)"
```

---

### Task 6: Final verification

- [ ] **Step 1:**

```bash
cd rust && cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test --workspace
cd backend && uv run pytest -q   # no expected changes; smoke
cd apps/macos && swiftformat Sources Tests --lint
xcodebuild -project MeetingRaft.xcodeproj -scheme MeetingRaft -configuration Debug test CODE_SIGNING_ALLOWED=NO
```

- [ ] **Step 2:** Confirm success criteria from spec.

---

## Spec coverage

| Spec item | Task |
|-----------|------|
| next version on assemble | 2, 3 |
| list / get_by_version / latest | 1, 3 |
| Final picker | 4 |
| Compare HSplit | 4 |
| Brief/Export = latest | 3, 4 (no change to generate path) |
| Docs | 5 |

## Self-review

- No TBD; `assemble_final` signature change called out before FFI.
- Soft overwrite test for same version kept (test upsert only).
- Diarization / line-diff / backend refine explicitly out of scope.
