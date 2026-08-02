# Artifacts via backend jobs — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** При LLM engine=`backend` Generate Brief/Follow-up идут через jobs API, poll в Rust, stub markdown сохраняется как local Artifact; `builtin_templates` без регрессии.

**Architecture:** `setLlmConfig` на `MeetingCore`; `generate_artifact` ветвится на builtin templates vs `SyncClient` create/poll/get; Swift включает `LlmEngine.backend` и прокидывает config вместе с API URL. Ollama остаётся disabled.

**Tech Stack:** Rust (`meetingraft-sync`, `meetingraft-ffi`), UniFFI, mockito, SwiftUI `ProviderSettingsStore`, XCTest, SwiftFormat.

**Spec:** `docs/superpowers/specs/2026-08-02-artifacts-via-backend-jobs-design.md`

## Global Constraints

- No real LLM / Ollama HTTP in this PR; stub jobs only.
- No silent fallback to templates on backend error.
- `payload: None` on CreateJobRequest.
- Poll: max **20** attempts, **250 ms** sleep (injectable in tests via helper params).
- Do not change Submit refine (stub) panel behavior.
- Comments Russian; identifiers English; Conventional Commits Russian subject.
- After UniFFI changes: run `apps/macos/Scripts/generate-ffi.sh` then `xcodegen generate` in `apps/macos`.
- `cargo test` + `swiftformat Sources Tests --lint` + macOS tests must stay green.

---

## File map

| File | Role |
|------|------|
| `rust/crates/sync/src/job_poll.rs` (new) | `wait_for_job_artifact` helper |
| `rust/crates/sync/src/lib.rs` | re-export helper |
| `rust/crates/sync/src/client.rs` | unchanged API; tests may stay |
| `rust/crates/ffi/src/lib.rs` | `set_llm_config`, fields, `generate_artifact` branch |
| `rust/crates/ffi/Cargo.toml` | `dev-dependencies.mockito` |
| `apps/macos/Generated/*` | regenerate FFI |
| `apps/macos/Sources/App/ProviderSettingsStore.swift` | `backend` available; `needsUrl` |
| `apps/macos/Sources/Settings/SettingsView.swift` | `setLlmConfig` + copy |
| `apps/macos/Tests/ProviderSettingsStoreTests.swift` | backend available |
| `docs/backlog.md`, `docs/roadmap.md`, `docs/architecture-and-install.md` | status notes |

---

### Task 1: Sync job poll helper (TDD)

**Files:**
- Create: `rust/crates/sync/src/job_poll.rs`
- Modify: `rust/crates/sync/src/lib.rs`
- Test: same module `#[cfg(test)]` with mockito

**Interfaces:**
- Produces:
  ```rust
  pub fn wait_for_job_artifact(
      client: &SyncClient,
      request: &CreateJobRequest,
      max_attempts: u32,
      poll_delay: Duration,
  ) -> Result<ArtifactDto, SyncError>
  ```
  Semantics: `create_job` → if failed/error → Err; if succeeded with artifact_ids → `get_artifact(first)`; else poll `get_job` up to `max_attempts` with `poll_delay` between attempts; timeout / no artifacts → `SyncError::Http` or dedicated message via existing `SyncError` (prefer `SyncError::Http(408, "Backend job timeout".into())` or extend only if needed — prefer reuse: map timeout to `SyncError::Http(408, …)` without schema change).

- [ ] **Step 1: Write failing tests in `job_poll.rs`**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::dto::{CreateJobRequest, JobKind};
    use mockito::Server;
    use std::time::Duration;

    fn request(meeting: &str) -> CreateJobRequest {
        CreateJobRequest {
            meeting_id: meeting.into(),
            kind: JobKind::Brief,
            primary_language: "ru".into(),
            allowed_languages: vec!["ru".into()],
            payload: None,
        }
    }

    #[test]
    fn immediate_success_fetches_artifact() {
        let mut server = Server::new();
        let _post = server
            .mock("POST", "/v1/jobs")
            .with_status(201)
            .with_body(r#"{"id":"j1","meeting_id":"m1","kind":"brief","status":"succeeded","error":null,"artifact_ids":["a1"]}"#)
            .create();
        let _art = server
            .mock("GET", "/v1/artifacts/a1")
            .with_status(200)
            .with_body(r#"{"id":"a1","kind":"brief","body_markdown":"# Stub brief","created_at":"2026-08-02T00:00:00Z"}"#)
            .create();
        let client = SyncClient::new(server.url(), "dev-token");
        let art = wait_for_job_artifact(&client, &request("m1"), 20, Duration::ZERO).unwrap();
        assert_eq!(art.body_markdown, "# Stub brief");
        assert_eq!(art.id, "a1");
    }

    #[test]
    fn failed_job_does_not_fetch_artifact() {
        let mut server = Server::new();
        let _post = server
            .mock("POST", "/v1/jobs")
            .with_status(201)
            .with_body(r#"{"id":"j1","meeting_id":"m1","kind":"brief","status":"failed","error":null,"artifact_ids":[]}"#)
            .create();
        let client = SyncClient::new(server.url(), "dev-token");
        let err = wait_for_job_artifact(&client, &request("m1"), 2, Duration::ZERO).unwrap_err();
        assert!(err.to_string().contains("failed") || err.to_string().contains("Backend"));
    }

    #[test]
    fn timeout_while_queued() {
        let mut server = Server::new();
        let _post = server
            .mock("POST", "/v1/jobs")
            .with_status(201)
            .with_body(r#"{"id":"j1","meeting_id":"m1","kind":"brief","status":"queued","error":null,"artifact_ids":[]}"#)
            .create();
        let _get = server
            .mock("GET", "/v1/jobs/j1")
            .with_status(200)
            .with_body(r#"{"id":"j1","meeting_id":"m1","kind":"brief","status":"queued","error":null,"artifact_ids":[]}"#)
            .expect_at_least(1)
            .create();
        let client = SyncClient::new(server.url(), "dev-token");
        let err = wait_for_job_artifact(&client, &request("m1"), 2, Duration::ZERO).unwrap_err();
        assert!(err.to_string().to_lowercase().contains("timeout"));
    }
}
```

- [ ] **Step 2: Run tests — expect FAIL / missing module**

```bash
cd rust && cargo test -p meetingraft-sync wait_for_job -- --nocapture
```

Expected: compile error `wait_for_job_artifact` not found.

- [ ] **Step 3: Implement `job_poll.rs` + wire `mod job_poll` / `pub use`**

Logic sketch:

```rust
use crate::client::SyncClient;
use crate::dto::{ArtifactDto, CreateJobRequest, JobStatus};
use crate::error::SyncError;
use std::thread;
use std::time::Duration;

pub fn wait_for_job_artifact(
    client: &SyncClient,
    request: &CreateJobRequest,
    max_attempts: u32,
    poll_delay: Duration,
) -> Result<ArtifactDto, SyncError> {
    let mut job = client.create_job(request)?;
    if let Some(err) = job.error.as_ref().filter(|e| !e.is_empty()) {
        return Err(SyncError::Http(500, err.clone()));
    }
    if job.status == JobStatus::Failed {
        return Err(SyncError::Http(
            500,
            job.error.unwrap_or_else(|| "Backend job failed".into()),
        ));
    }

    let mut attempts = 0u32;
    while job.status != JobStatus::Succeeded {
        if attempts >= max_attempts {
            return Err(SyncError::Http(408, "Backend job timeout".into()));
        }
        if !poll_delay.is_zero() {
            thread::sleep(poll_delay);
        }
        job = client.get_job(&job.id)?;
        if let Some(err) = job.error.as_ref().filter(|e| !e.is_empty()) {
            return Err(SyncError::Http(500, err.clone()));
        }
        if job.status == JobStatus::Failed {
            return Err(SyncError::Http(
                500,
                job.error.unwrap_or_else(|| "Backend job failed".into()),
            ));
        }
        attempts += 1;
    }

    let artifact_id = job
        .artifact_ids
        .first()
        .ok_or_else(|| SyncError::Http(500, "Backend job has no artifacts".into()))?;
    client.get_artifact(artifact_id)
}
```

Check `JobDto.error` type (`Option<String>`) in `dto.rs` and adjust.

- [ ] **Step 4: Run tests — PASS**

```bash
cd rust && cargo test -p meetingraft-sync -- --nocapture
```

- [ ] **Step 5: Commit**

```bash
git add rust/crates/sync
git commit -m "$(cat <<'EOF'
feat: wait_for_job_artifact poll helper в sync

EOF
)"
```

---

### Task 2: UniFFI `setLlmConfig` + `generate_artifact` backend branch

**Files:**
- Modify: `rust/crates/ffi/src/lib.rs`
- Modify: `rust/crates/ffi/Cargo.toml` (dev-dep `mockito = "1"`)
- Regenerate: `apps/macos/Generated/` via script
- Test: new tests in `ffi` `mod tests`

**Interfaces:**
- Consumes: `wait_for_job_artifact`, `CreateJobRequest`, `JobKind`
- Produces:
  - `MeetingCore.set_llm_config(engine_code: String, model_id: String)`
  - `MeetingCore.llm_engine() -> String` (optional getter for tests — add if useful)
  - Inner fields: `llm_engine: String` default `"builtin_templates"`, `llm_model_id: String` default `""`
  - `generate_artifact`: when normalized engine == `"backend"`, call helper with `max_attempts=20`, `poll_delay=250ms`; template_id override `backend.brief` / `backend.follow_up`

Normalization:

```rust
fn normalize_llm_engine(code: &str) -> &str {
    match code {
        "backend" => "backend",
        _ => "builtin_templates",
    }
}
```

- [ ] **Step 1: Failing ffi tests**

Add helpers to seed a final transcript quickly — reuse pattern from existing postcall test (`stop_recording` after captions) **or** insert via store if exposed. Prefer minimal: copy the existing meeting setup from `stop_recording_assembles…` but shorter — if too heavy, open store and `insert_final_transcript` if API exists.

Simpler path for backend test:

```rust
#[test]
fn generate_artifact_backend_uses_job_stub() {
    let mut server = Server::new();
    let _post = server.mock("POST", "/v1/jobs") /* succeeded brief + artifact a1 */;
    let _art = server.mock("GET", "/v1/artifacts/a1") /* body "# Stub brief" */;

    let root = temp_dir(...);
    let core = MeetingCore::with_data_root(...);
    // seed final: start_recording + stop OR insert_final if available
    seed_final_transcript(&core, "m-backend");
    core.set_api_config(server.url(), "dev-token".into());
    core.set_llm_config("backend".into(), "unused".into());

    let result = core.generate_artifact("m-backend".into(), FfiArtifactKind::Brief);
    assert!(result.error.is_empty(), "{}", result.error);
    assert!(result.artifact.body_markdown.contains("Stub brief"));
    assert_eq!(result.artifact.template_id, "backend.brief");
    let _ = fs::remove_dir_all(root);
}

#[test]
fn generate_artifact_backend_surfaces_job_error() {
    // POST returns failed; assert error non-empty; list_artifacts empty
}
```

Implement `seed_final_transcript` using the same flow as existing ffi postcall test (start/stop) **or** `AudioManifestStore` insert if `pub` — check `insert_final_transcript` / `put_final` in storage and call via test-only path through core if needed.

If seeding is painful, extract a private test helper that writes Final via `write_store` inside the test module using the same store open as production.

- [ ] **Step 2: Run — expect FAIL**

```bash
cd rust && cargo test -p meetingraft-ffi generate_artifact_backend -- --nocapture
```

- [ ] **Step 3: Implement**

In `MeetingCoreInner` add fields; in `new`/`with_data_root` init defaults.

```rust
pub fn set_llm_config(&self, engine_code: String, model_id: String) {
    let mut guard = self.inner.lock().expect("meeting core poisoned");
    guard.llm_engine = normalize_llm_engine(&engine_code).to_owned();
    guard.llm_model_id = model_id;
}
```

In `generate_artifact`, after loading final:

```rust
let engine = normalize_llm_engine(&guard.llm_engine).to_owned();
let body = if engine == "backend" {
    let kind = match domain_kind {
        ArtifactKind::Brief => JobKind::Brief,
        ArtifactKind::FollowUp => JobKind::FollowUp,
    };
    let request = CreateJobRequest {
        meeting_id: meeting_id.clone(),
        kind,
        primary_language: guard.language_policy.primary.code().to_owned(),
        allowed_languages: guard.language_policy.allowed.iter().map(|l| l.code().to_owned()).collect(),
        payload: None,
    };
    let client = guard.sync_client.clone();
    // Drop lock during HTTP? Prefer clone client + release lock before wait_for_job_artifact to avoid holding mutex across sleep.
    drop(guard);
    match wait_for_job_artifact(&client, &request, 20, Duration::from_millis(250)) {
        Ok(dto) => dto.body_markdown,
        Err(e) => return FfiGenerateArtifactResult { artifact: empty_artifact(), error: e.to_string() },
    }
} else {
    // existing render_brief / render_follow_up using guard — keep lock
    ...
};

// re-lock for insert
let mut guard = self.inner.lock()...;
let mut artifact = make_artifact(...);
if engine == "backend" {
    artifact.template_id = match domain_kind {
        ArtifactKind::Brief => "backend.brief".into(),
        ArtifactKind::FollowUp => "backend.follow_up".into(),
    };
}
artifact.id = Uuid::new_v4().to_string();
// insert as today
```

**Important:** structure code so builtin path still holds one lock; backend path releases lock during HTTP. Avoid deadlock.

Import `wait_for_job_artifact` from `sync`.

- [ ] **Step 4: Tests PASS + existing ffi postcall still PASS**

```bash
cd rust && cargo test -p meetingraft-ffi -- --nocapture
cd rust && cargo test -p meetingraft-sync -- --nocapture
```

- [ ] **Step 5: Regenerate FFI**

```bash
# from repo root
apps/macos/Scripts/generate-ffi.sh
cd apps/macos && xcodegen generate
```

Verify Generated Swift has `setLlmConfig(engineCode:modelId:)`.

- [ ] **Step 6: Commit**

```bash
git add rust/crates/ffi apps/macos/Generated apps/macos/project.yml
# include any xcodegen-tracked files that change; do not commit xcodeproj if gitignored
git commit -m "$(cat <<'EOF'
feat: generateArtifact через backend jobs + setLlmConfig

EOF
)"
```

---

### Task 3: Swift Settings — enable Backend LLM

**Files:**
- Modify: `apps/macos/Sources/App/ProviderSettingsStore.swift`
- Modify: `apps/macos/Sources/Settings/SettingsView.swift`
- Modify: `apps/macos/Tests/ProviderSettingsStoreTests.swift`
- Optionally: `MeetingRaftApp` / Meetings appear — call `setLlmConfig` when core ready

**Interfaces:**
- `LlmEngine.backend.isAvailable == true`
- `LlmEngine.backend.needsUrl == false` (uses apiBaseUrl)
- `applyApiConfig()` also calls `core?.setLlmConfig(engineCode: providerStore.llmEngine.rawValue, modelId: providerStore.llmModelId)`
- onChange of `llmEngine` / `llmModelId` → apply config
- Settings copy: replace «скоро» block for backend; keep ollama caption as скоро for disabled engines

- [ ] **Step 1: Update failing/updated tests**

```swift
func testBackendLlmIsAvailableAndSelectable() {
    let store = ProviderSettingsStore()
    XCTAssertTrue(LlmEngine.backend.isAvailable)
    XCTAssertFalse(LlmEngine.ollama.isAvailable)
    store.llmEngine = .backend
    XCTAssertEqual(store.llmEngine, .backend)
    XCTAssertFalse(LlmEngine.backend.needsUrl)
    XCTAssertEqual(
        store.artifactsPipelineCaption,
        "Генерация из Final · LLM: backend"
    )
}
```

Update any test that asserted `backend` unavailable / forced revert.

- [ ] **Step 2: Run — expect FAIL**

```bash
cd apps/macos && xcodegen generate
xcodebuild -project MeetingRaft.xcodeproj -scheme MeetingRaft -configuration Debug \
  test CODE_SIGNING_ALLOWED=NO -only-testing:MeetingRaftTests/ProviderSettingsStoreTests
```

- [ ] **Step 3: Implement store + Settings**

`ProviderSettingsStore.swift`:

```swift
var isAvailable: Bool {
    switch self {
    case .builtinTemplates, .backend: true
    case .ollama, .openaiCompat: false
    }
}

var needsUrl: Bool {
    switch self {
    case .builtinTemplates, .backend: false
    case .ollama, .openaiCompat: true
    }
}
```

`SettingsView.applyApiConfig`:

```swift
core?.setApiConfig(baseUrl: providerStore.apiBaseUrl, token: providerStore.apiToken)
core?.setLlmConfig(
    engineCode: providerStore.llmEngine.rawValue,
    modelId: providerStore.llmModelId
)
```

Update LLM section caption:

```swift
Text("Builtin templates локально; Backend — jobs brief/follow_up (stub). Ollama — скоро.")
```

Show model id field only if useful; for backend can hide model field or leave disabled. Prefer: show Model id only when `needsUrl` (ollama path); for backend hide both URL and model.

Wire `.onChange(of: providerStore.llmEngine)` and `llmModelId` to `applyApiConfig()`.

- [ ] **Step 4: Tests + swiftformat**

```bash
cd apps/macos && swiftformat Sources Tests --lint
xcodebuild ... -only-testing:MeetingRaftTests/ProviderSettingsStoreTests
# also MeetingsViewModelTests to ensure no break
```

- [ ] **Step 5: Commit**

```bash
git add apps/macos/Sources apps/macos/Tests
git commit -m "$(cat <<'EOF'
feat: Settings LLM Backend → setLlmConfig

EOF
)"
```

---

### Task 4: Docs + full verify

**Files:**
- `docs/backlog.md`
- `docs/roadmap.md`
- `docs/architecture-and-install.md`
- Optionally one-line in providers design status

- [ ] **Step 1: Docs**

Backlog: change Real LLM line to note **partial:** backend stub jobs for Brief/Follow-up; Ollama still deferred.

Roadmap Phase 6 Remaining: note Artifacts-via-backend done on this branch.

architecture-and-install: Settings LLM=Backend → Generate Brief uses `/v1/jobs` kind `brief`.

- [ ] **Step 2: Full verify**

```bash
cd rust && cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test
cd apps/macos && swiftformat Sources Tests --lint
xcodegen generate
xcodebuild -project MeetingRaft.xcodeproj -scheme MeetingRaft -configuration Debug test CODE_SIGNING_ALLOWED=NO
```

Optional docker smoke: Settings Backend + Test API + Generate Brief.

- [ ] **Step 3: Commit**

```bash
git add docs
git commit -m "$(cat <<'EOF'
docs: Artifacts via backend jobs в backlog/roadmap/install

EOF
)"
```

---

## Spec coverage checklist

| Spec item | Task |
|-----------|------|
| `wait`/poll 20×250ms | 1 |
| `setLlmConfig` + normalize | 2 |
| `generate_artifact` backend branch + template_id | 2 |
| No silent fallback | 2 |
| `backend` available, ollama disabled | 3 |
| `needsUrl` false for backend | 3 |
| Wire Settings → core | 3 |
| Docs | 4 |
| Refine panel untouched | (no task edits MeetingDetailView refine) |
