# Live STT model picker + HF download — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Settings picker for Whisper ggml models, HF download, first-run auto `base`, preference wired through UniFFI into `resolve_whisper_model`.

**Architecture:** Rust owns preference + resolve; Swift owns HF download + Settings UI. Parakeet deferred.

**Tech Stack:** Rust stt/ffi + UniFFI regen, SwiftUI, URLSession, XCTest, mockito not needed for Rust path tests.

**Spec:** `docs/superpowers/specs/2026-08-03-stt-model-picker-design.md`

## Global Constraints

- Known ids: `auto`, `base`, `small`, `large-v3-turbo` → files `ggml-base.bin` / `ggml-small.bin` / `ggml-large-v3-turbo.bin`.
- First-run download only when no local `ggml-*.bin`.
- Download via Swift URLSession to Application Support models dir (not shell script).
- Comments Russian; identifiers English; Conventional Commits Russian subject.
- TDD per task; regenerate FFI after UniFFI signature changes (`apps/macos/Scripts/generate-ffi.sh`).

---

### Task 1: Rust preferred model resolve (TDD)

**Files:**
- Modify: `rust/crates/stt/src/model_path.rs`
- Modify: `rust/crates/stt/src/lib.rs` (re-exports if needed)
- Modify: `rust/crates/stt/src/window.rs` — pass preferred into resolve
- Tests in `model_path.rs`

**Interfaces:**
```rust
pub fn whisper_filename_for_id(model_id: &str) -> Option<&'static str>;
// "base" → Some("ggml-base.bin"), "auto"/unknown → None for explicit file

pub fn resolve_whisper_model(
    data_root: impl AsRef<Path>,
    preferred: Option<&str>,
) -> Option<PathBuf>;
// preferred None or "auto" → existing priority
// preferred "base"|"small"|"large-v3-turbo" → that file if present, else None
// (do not silently fall back to another size when user picked explicit id)
```

- [ ] **Step 1: Failing tests**

```rust
#[test]
fn preferred_base_selects_base_even_if_turbo_present() {
    let root = tempfile();
    let models = models_dir(&root);
    std::fs::create_dir_all(&models).unwrap();
    std::fs::write(models.join("ggml-large-v3-turbo.bin"), b"t").unwrap();
    std::fs::write(models.join("ggml-base.bin"), b"b").unwrap();
    let path = resolve_whisper_model(&root, Some("base")).unwrap();
    assert!(path.ends_with("ggml-base.bin"));
}

#[test]
fn preferred_missing_returns_none() {
    let root = tempfile();
    std::fs::create_dir_all(models_dir(&root)).unwrap();
    assert!(resolve_whisper_model(&root, Some("small")).is_none());
}

#[test]
fn auto_prefers_turbo_over_base() {
    let root = tempfile();
    let models = models_dir(&root);
    std::fs::create_dir_all(&models).unwrap();
    std::fs::write(models.join("ggml-base.bin"), b"b").unwrap();
    std::fs::write(models.join("ggml-large-v3-turbo.bin"), b"t").unwrap();
    let path = resolve_whisper_model(&root, Some("auto")).unwrap();
    assert!(path.ends_with("ggml-large-v3-turbo.bin"));
}
```

Update all existing `resolve_whisper_model(&root)` call sites to
`resolve_whisper_model(&root, None)` or `Some("auto")`.

- [ ] **Step 2: RED → implement → GREEN**

```bash
cd rust && cargo test -p meetingraft-stt
```

- [ ] **Step 3: Commit**

```bash
git commit -m "$(cat <<'EOF'
feat: preferred Whisper model id в resolve_whisper_model

EOF
)"
```

---

### Task 2: UniFFI preference + list local models (TDD)

**Files:**
- Modify: `rust/crates/ffi/src/lib.rs`
- Regenerate: `apps/macos/Generated/` via `generate-ffi.sh`
- Tests in ffi crate

**Interfaces:**
```rust
// MeetingCoreInner
preferred_whisper_model: String, // default "auto"

pub fn set_preferred_whisper_model(&self, model_id: String);
pub fn preferred_whisper_model(&self) -> String;
pub fn list_local_whisper_models(&self) -> Vec<String>; // sorted filenames
```

`whisper_model_path` / `try_whisper` use `resolve_whisper_model(data_root, preferred)`.

Normalize unknown ids to `"auto"`. Accept `base`|`small`|`large-v3-turbo`|`auto`.

- [ ] **Step 1: Tests**

```rust
#[test]
fn set_preferred_whisper_model_affects_whisper_model_path() {
    // seed models dir under with_data_root temp with base+turbo files
    core.set_preferred_whisper_model("base".into());
    assert!(core.whisper_model_path().ends_with("ggml-base.bin"));
    core.set_preferred_whisper_model("auto".into());
    assert!(core.whisper_model_path().ends_with("ggml-large-v3-turbo.bin"));
}

#[test]
fn list_local_whisper_models_lists_ggml_bins() { ... }
```

- [ ] **Step 2: Implement + `cargo test -p meetingraft-ffi`**

- [ ] **Step 3: `apps/macos/Scripts/generate-ffi.sh` + xcodegen**

- [ ] **Step 4: Commit**

```bash
git commit -m "$(cat <<'EOF'
feat: UniFFI preferred Whisper model и list local ggml

EOF
)"
```

---

### Task 3: Swift HF downloader (TDD)

**Files:**
- Create: `apps/macos/Sources/App/WhisperModelCatalog.swift` (id ↔ filename ↔ HF URL)
- Create: `apps/macos/Sources/App/WhisperModelDownloader.swift`
- Create: `apps/macos/Tests/WhisperModelDownloaderTests.swift`

**Interfaces:**
```swift
enum WhisperModelId: String, CaseIterable, Identifiable {
    case auto, base, small, largeV3Turbo = "large-v3-turbo"
    var filename: String? // nil for auto
    var downloadURL: URL? // HF resolve URL
}

protocol WhisperDownloading: Sendable {
    func download(id: WhisperModelId, modelsDirectory: URL, progress: @escaping @MainActor (Double) -> Void) async throws -> URL
}

struct WhisperModelDownloader: WhisperDownloading { ... }
```

- Download to `.partial` then `replaceItem` / move.
- If destination exists → return existing URL (no re-download).
- Tests: write fake server file via local file URL scheme **or** inject a test double that writes bytes; assert skip-if-exists and partial→final rename with a `FileManager` temp dir (prefer testable `download(from:to:)` helper).

Minimal testable core without live HF:

```swift
static func installDownloadedFile(tempPartial: URL, destination: URL) throws
static func destinationURL(modelsDirectory: URL, id: WhisperModelId) -> URL?
```

- [ ] **Step 1–4:** TDD helpers + URLSession download implementation + commit

```bash
git commit -m "$(cat <<'EOF'
feat: WhisperModelDownloader с Hugging Face

EOF
)"
```

---

### Task 4: Settings UI + first-run + wire preference

**Files:**
- Modify: `ProviderSettingsStore.swift` — `selectedSttModelId`
- Modify: `SettingsView.swift` — picker, progress, Download, first-run
- Modify: `LiveCaptionsViewModel` / start path — `setPreferredWhisperModel` before record if needed
- Tests: `ProviderSettingsStoreTests`

**Behavior:**
- onAppear: refresh `listLocalWhisperModels` / paths; if empty → start download `base` (set progress).
- Picker changes → `core.setPreferredWhisperModel`.
- Download button for non-auto ids.
- Caption errors on failure; keep Mock messaging if still no model.

- [ ] **Step 1:** test default selected id `auto` or `base` (spec: first-run downloads base; store default `auto` is fine if first-run fills disk).
- [ ] **Step 2:** UI wiring
- [ ] **Step 3:** macOS tests + swiftformat
- [ ] **Step 4: Commit**

```bash
git commit -m "$(cat <<'EOF'
feat: Settings STT model picker и first-run base download

EOF
)"
```

---

### Task 5: Docs + verify

**Files:** `docs/backlog.md`, `docs/roadmap.md`, `docs/architecture-and-install.md` §2.4

- Mark STT picker done; Parakeet deferred.
- Note Settings first-run `ggml-base` download.
- Verify: `cargo test -p meetingraft-stt -p meetingraft-ffi`, swiftformat, xcodegen, xcodebuild test.

```bash
git commit -m "$(cat <<'EOF'
docs: STT model picker и Parakeet в backlog

EOF
)"
```

---

## Spec coverage checklist

| Spec item | Task |
|-----------|------|
| preferred resolve / no silent wrong size | 1 |
| UniFFI set/list/path | 2 |
| HF download + idempotent | 3 |
| Picker + first-run base | 4 |
| Docs + Parakeet deferred | 5 |
