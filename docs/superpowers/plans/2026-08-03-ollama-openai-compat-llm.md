# Ollama native + OpenAI-compat LLM — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Settings engines `ollama` и `openai_compat` генерируют Brief/Follow-up через локальный HTTP LLM; builtin и backend без регрессии; без silent fallback.

**Architecture:** `OllamaNativeClient` (`/api/chat`) и `OpenAiCompatLlmClient` (`/v1/chat/completions`) реализуют `LlmClient`; `set_llm_config(engine, model, base_url)`; `generate_artifact` ветвится; Swift прокидывает `llmBaseUrl`.

**Tech Stack:** Rust postcall + reqwest + mockito, UniFFI, SwiftUI, XCTest, SwiftFormat, pre-commit.

**Spec:** `docs/superpowers/specs/2026-08-03-ollama-openai-compat-llm-design.md`

## Global Constraints

- No FastAPI Ollama worker; no streaming/tools UI; no silent template fallback.
- Timeout LLM HTTP: **60s**.
- Empty base_url or model → `LlmError::NotConfigured` before request.
- Comments Russian; identifiers English; Conventional Commits Russian subject.
- After UniFFI signature change: `apps/macos/Scripts/generate-ffi.sh` + `xcodegen generate`.
- `cargo test` + swiftformat + xcodebuild test + `pre-commit run --all-files` green.

---

## File map

| File | Role |
|------|------|
| `rust/crates/postcall/Cargo.toml` | reqwest, serde_json; dev mockito |
| `rust/crates/postcall/src/llm.rs` | LlmError expand + NullLlmClient |
| `rust/crates/postcall/src/llm_http.rs` (new) | Ollama + OpenAI-compat clients |
| `rust/crates/postcall/src/prompts.rs` (new) | brief/follow_up system+user |
| `rust/crates/postcall/src/lib.rs` | re-exports |
| `rust/crates/ffi/src/lib.rs` | set_llm_config 3-arg; generate branches |
| `apps/macos/Generated/*` | regenerate |
| `apps/macos/Sources/App/ProviderSettingsStore.swift` | enable ollama + openaiCompat |
| `apps/macos/Sources/Settings/SettingsView.swift` | setLlmConfig baseUrl |
| `apps/macos/Sources/Meetings/MeetingsViewModel.swift` | protocol + applyProviderConfig |
| `apps/macos/Sources/Meetings/MeetingDetailView.swift` | pass llmBaseUrl |
| `apps/macos/Tests/*` | store + spy baseUrl |
| docs | backlog/roadmap/install/providers |

---

### Task 1: postcall HTTP clients + prompts (TDD)

**Files:**
- Modify: `rust/crates/postcall/Cargo.toml`, `llm.rs`, `lib.rs`
- Create: `rust/crates/postcall/src/llm_http.rs`, `prompts.rs`

**Interfaces:**
- Produces:
  ```rust
  pub enum LlmError {
      NotConfigured,
      Http { status: u16, body: String },
      EmptyResponse,
      Transport(String),
  }
  pub struct OllamaNativeClient { /* base_url, model */ }
  pub struct OpenAiCompatLlmClient { /* base_url, model */ }
  impl LlmClient for both
  pub fn brief_prompts(final_md: &str, primary_lang: SpeechLanguage) -> (String, String)
  pub fn follow_up_prompts(final_md: &str, primary_lang: SpeechLanguage) -> (String, String)
  ```

- [ ] **Step 1: Expand `LlmError` + failing HTTP tests**

Update `llm.rs` errors (keep `NotConfigured`; add variants). Update `NullLlmClient` test if Display changes.

In `llm_http.rs` tests (mockito):

```rust
#[test]
fn ollama_native_parses_message_content() {
    let mut server = Server::new();
    let _m = server
        .mock("POST", "/api/chat")
        .with_status(200)
        .with_body(r#"{"message":{"role":"assistant","content":"# Brief from Ollama"},"done":true}"#)
        .create();
    let client = OllamaNativeClient::new(server.url(), "gemma2");
    let out = client.complete("sys", "user").unwrap();
    assert_eq!(out, "# Brief from Ollama");
}

#[test]
fn openai_compat_parses_choices_content() {
    let mut server = Server::new();
    let _m = server
        .mock("POST", "/v1/chat/completions")
        .with_status(200)
        .with_body(r#"{"choices":[{"message":{"role":"assistant","content":"# Brief compat"}}]}"#)
        .create();
    let client = OpenAiCompatLlmClient::new(server.url(), "gemma2");
    assert_eq!(client.complete("sys", "user").unwrap(), "# Brief compat");
}

#[test]
fn http_error_maps_to_llm_http() { /* 500 → Err(Http{status:500,..}) */ }

#[test]
fn empty_content_is_empty_response() { /* 200 with empty content */ }

#[test]
fn empty_base_is_not_configured() {
    let client = OllamaNativeClient::new("", "gemma2");
    assert!(matches!(client.complete("s", "u"), Err(LlmError::NotConfigured)));
}
```

Prompt unit tests: `brief_prompts` system mentions language / markdown; user contains final excerpt.

- [ ] **Step 2: Run — expect FAIL**

```bash
cd rust && cargo test -p meetingraft-postcall -- --nocapture
```

- [ ] **Step 3: Implement clients + prompts**

`OllamaNativeClient::complete`: POST JSON `{model, stream:false, messages:[{role:"system",content},{role:"user",content}]}`; parse `message.content`.

`OpenAiCompatLlmClient::complete`: POST `{model, messages:[...]}`; parse `choices[0].message.content`.

Trim trailing slash on base_url. Timeout 60s via `Client::builder().timeout(...)`.

`prompts.rs`: Russian comments; English identifiers; include `SpeechLanguage` from domain.

- [ ] **Step 4: Tests PASS + clippy**

```bash
cd rust && cargo test -p meetingraft-postcall && cargo clippy -p meetingraft-postcall --all-targets -- -D warnings
```

- [ ] **Step 5: Commit**

```bash
git add rust/crates/postcall
git commit -m "$(cat <<'EOF'
feat: Ollama native и OpenAI-compat LlmClient

EOF
)"
```

---

### Task 2: UniFFI `set_llm_config` + `generate_artifact` branches

**Files:**
- Modify: `rust/crates/ffi/src/lib.rs`
- Regenerate: `apps/macos/Generated/`

**Interfaces:**
- Consumes: `OllamaNativeClient`, `OpenAiCompatLlmClient`, `brief_prompts`, `follow_up_prompts`
- Produces:
  - `set_llm_config(engine_code: String, model_id: String, base_url: String)`
  - `normalize_llm_engine` includes `ollama` | `openai_compat` | `backend`
  - Inner `llm_base_url: String`
  - `generate_artifact` local-LLM path

- [ ] **Step 1: Failing/updating tests**

Update any existing calls to `set_llm_config` with two args → three (`"".into()` or URL).

Add:

```rust
#[test]
fn generate_artifact_ollama_uses_http_body() {
    let mut server = Server::new();
    let _m = server.mock("POST", "/api/chat") /* returns markdown */;
    let root = temp...;
    seed_final_transcript(&root, "m-ollama");
    let core = MeetingCore::with_data_root(...);
    core.set_llm_config("ollama".into(), "gemma2".into(), server.url());
    let result = core.generate_artifact("m-ollama".into(), FfiArtifactKind::Brief);
    assert!(result.error.is_empty(), "{}", result.error);
    assert_eq!(result.artifact.template_id, "ollama.brief");
    assert!(!result.artifact.body_markdown.is_empty());
}

#[test]
fn generate_artifact_ollama_error_does_not_insert() {
    // mock 500; assert error; list_artifacts empty
}

#[test]
fn generate_artifact_openai_compat_template_id() {
    // mock /v1/chat/completions; template_id openai.brief
}
```

- [ ] **Step 2: Implement**

```rust
fn normalize_llm_engine(code: &str) -> &str {
    match code {
        "backend" => "backend",
        "ollama" => "ollama",
        "openai_compat" => "openai_compat",
        _ => "builtin_templates",
    }
}

pub fn set_llm_config(&self, engine_code: String, model_id: String, base_url: String) {
    let mut guard = ...;
    guard.llm_engine = normalize_llm_engine(&engine_code).to_owned();
    guard.llm_model_id = model_id;
    guard.llm_base_url = base_url.trim().trim_end_matches('/').to_owned();
}
```

In `generate_artifact` for `ollama` / `openai_compat`:
1. Clone `llm_base_url`, `llm_model_id`, final body, kind, primary lang
2. Drop guard
3. Build prompts + client; `complete`
4. On error → `FfiGenerateArtifactResult { empty, error }`
5. Re-lock; `make_artifact`; set `template_id`; insert

Do **not** fall back to `render_brief` on LLM error.

- [ ] **Step 3: cargo test ffi + sync + postcall; regenerate FFI**

```bash
cd rust && cargo test -p meetingraft-ffi -- --nocapture
apps/macos/Scripts/generate-ffi.sh
cd apps/macos && xcodegen generate
```

Confirm Generated `setLlmConfig(engineCode:modelId:baseUrl:)`.

- [ ] **Step 4: Commit**

```bash
git add rust/crates/ffi apps/macos/Generated
git commit -m "$(cat <<'EOF'
feat: generateArtifact через Ollama / OpenAI-compat

EOF
)"
```

---

### Task 3: Swift Settings + applyProviderConfig baseUrl

**Files:**
- `ProviderSettingsStore.swift`, `SettingsView.swift`
- `MeetingsViewModel.swift`, `MeetingDetailView.swift`
- `ProviderSettingsStoreTests.swift`, `MeetingsViewModelTests.swift` (spy)

**Interfaces:**
- `LlmEngine.ollama` / `.openaiCompat`: `isAvailable = true`
- Protocol: `setLlmConfig(engineCode:modelId:baseUrl:)`
- `applyProviderConfig(..., llmBaseUrl: String)`
- MeetingDetail + Settings pass `llmBaseUrl`

- [ ] **Step 1: Failing tests**

```swift
func testOllamaAndOpenAiCompatAreAvailable() {
    XCTAssertTrue(LlmEngine.ollama.isAvailable)
    XCTAssertTrue(LlmEngine.openaiCompat.isAvailable)
    XCTAssertTrue(LlmEngine.ollama.needsUrl)
    let store = ProviderSettingsStore()
    store.llmEngine = .ollama
    XCTAssertEqual(store.llmEngine, .ollama)
    store.llmEngine = .openaiCompat
    XCTAssertEqual(store.llmEngine, .openaiCompat)
}

func testApplyProviderConfigPassesLlmBaseUrl() {
    let core = MeetingsCoreSpy()
    let vm = MeetingsViewModel(core: core)
    vm.applyProviderConfig(
        apiBaseUrl: "http://api",
        apiToken: "t",
        llmEngineCode: "ollama",
        llmModelId: "gemma2",
        llmBaseUrl: "http://127.0.0.1:11434"
    )
    XCTAssertEqual(core.lastLlmBaseUrl, "http://127.0.0.1:11434")
    XCTAssertEqual(core.lastLlmEngineCode, "ollama")
}
```

Update spy `setLlmConfig` signature; update any existing Provider tests that asserted ollama unavailable.

- [ ] **Step 2: Implement store + wiring — tests PASS**

Settings caption: оба engine доступны; поле URL/model при `needsUrl`.

```swift
core?.setLlmConfig(
    engineCode: providerStore.llmEngine.rawValue,
    modelId: providerStore.llmModelId,
    baseUrl: providerStore.llmBaseUrl
)
```

MeetingDetail `applyProviderConfig` adds `llmBaseUrl: providerStore.llmBaseUrl`.

- [ ] **Step 3: swiftformat + xcodebuild focused tests**

```bash
cd apps/macos && swiftformat Sources Tests --lint
xcodegen generate
xcodebuild ... -only-testing:MeetingRaftTests/ProviderSettingsStoreTests
xcodebuild ... -only-testing:MeetingRaftTests/MeetingsViewModelTests
```

- [ ] **Step 4: Commit**

```bash
git add apps/macos/Sources apps/macos/Tests
git commit -m "$(cat <<'EOF'
feat: Settings Ollama и OpenAI-compat + llmBaseUrl

EOF
)"
```

---

### Task 4: Docs + full verify

**Files:** `docs/backlog.md`, `docs/roadmap.md`, `docs/architecture-and-install.md`, providers design spec

- [ ] **Step 1: Docs** — Real LLM partial ollama+openai_compat; Remaining; install smoke; providers table enable both

- [ ] **Step 2: Full verify**

```bash
cd rust && cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test
cd apps/macos && swiftformat Sources Tests --lint && xcodegen generate
xcodebuild -project MeetingRaft.xcodeproj -scheme MeetingRaft \
  -configuration Debug test CODE_SIGNING_ALLOWED=NO
pre-commit run --all-files
```

Optional manual: Ollama with model → Settings Ollama → Generate Brief; switch openai_compat на том же `:11434`.

- [ ] **Step 3: Commit**

```bash
git add docs
git commit -m "$(cat <<'EOF'
docs: Ollama / OpenAI-compat LLM в backlog/roadmap/install

EOF
)"
```

---

## Spec coverage checklist

| Spec item | Task |
|-----------|------|
| OllamaNativeClient + OpenAiCompatLlmClient | 1 |
| Prompts | 1 |
| set_llm_config 3-arg + normalize | 2 |
| generate_artifact branches + template_id | 2 |
| No silent fallback | 2 |
| Swift available + baseUrl wire | 3 |
| Docs | 4 |
