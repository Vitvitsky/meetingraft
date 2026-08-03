# Backend LLM provider (jobs) — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** При LLM=Backend Brief/Follow-up вызывают OpenAI-compatible провайдер из env backend; фронт передаёт model + language-aware prompts в job payload.

**Architecture:** Rust собирает `brief_prompts`/`follow_up_prompts` и кладёт `{model,system,user}` в `CreateJobRequest.payload`. FastAPI при `LLM_BASE_URL` делает `POST {base}/v1/chat/completions` с optional Bearer; без env — прежний stub. Локальные ollama/openai_compat не трогаем.

**Tech Stack:** FastAPI + httpx, pytest + respx (или httpx mock), Rust UniFFI/mockito, SwiftUI Settings, docker-compose env.

**Spec:** `docs/superpowers/specs/2026-08-03-backend-llm-provider-design.md`

## Global Constraints

- `LLM_BASE_URL` без trailing `/v1`; клиент добавляет `/v1/chat/completions`.
- Пустой `LLM_API_KEY` → без `Authorization`.
- Нет silent fallback на stub markdown при ошибке LLM (`brief`/`follow_up` + LLM configured).
- Промпты только из Rust; Python не дублирует текст промптов.
- Comments Russian; identifiers English; Conventional Commits Russian subject.
- TDD: failing test → implement → green → commit per task.

---

### Task 1: Backend OpenAI-compat client (TDD)

**Files:**
- Create: `backend/app/llm.py`
- Modify: `backend/pyproject.toml` (httpx runtime; respx in dev)
- Test: `backend/tests/test_llm.py`

**Interfaces:**
- Produces:
  ```python
  class LlmSettings:
      base_url: str  # trimmed, no trailing slash
      api_key: str
      default_model: str

  def load_llm_settings() -> LlmSettings: ...  # from LLM_BASE_URL, LLM_API_KEY, LLM_MODEL

  def complete_chat(
      settings: LlmSettings,
      *,
      model: str,
      system: str,
      user: str,
      timeout_s: float = 60.0,
  ) -> str: ...
  ```
  - Raises `LlmError` (message) on HTTP/empty/missing model.

- [ ] **Step 1: Failing tests**

```python
# tests/test_llm.py
import httpx
import pytest
import respx

from app.llm import LlmError, LlmSettings, complete_chat, load_llm_settings


def test_load_llm_settings_trims_slash(monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.setenv("LLM_BASE_URL", "http://example:58001/")
    monkeypatch.setenv("LLM_API_KEY", "LOCAL-API-KEY")
    monkeypatch.setenv("LLM_MODEL", "Google/gemma-4-12b-it")
    s = load_llm_settings()
    assert s.base_url == "http://example:58001"
    assert s.api_key == "LOCAL-API-KEY"
    assert s.default_model == "Google/gemma-4-12b-it"


@respx.mock
def test_complete_chat_sends_bearer_and_messages() -> None:
    route = respx.post("http://llm.test/v1/chat/completions").mock(
        return_value=httpx.Response(
            200,
            json={"choices": [{"message": {"content": "# Brief from LLM"}}]},
        )
    )
    settings = LlmSettings(
        base_url="http://llm.test",
        api_key="LOCAL-API-KEY",
        default_model="fallback",
    )
    out = complete_chat(
        settings, model="Google/gemma-4-12b-it", system="sys", user="usr"
    )
    assert out == "# Brief from LLM"
    assert route.called
    req = route.calls.last.request
    assert req.headers["Authorization"] == "Bearer LOCAL-API-KEY"
    body = req.read()  # or json from call
    # assert model / messages roles in JSON


@respx.mock
def test_complete_chat_omits_auth_when_key_empty() -> None:
    respx.post("http://llm.test/v1/chat/completions").mock(
        return_value=httpx.Response(
            200, json={"choices": [{"message": {"content": "ok"}}]}
        )
    )
    settings = LlmSettings(base_url="http://llm.test", api_key="", default_model="m")
    complete_chat(settings, model="m", system="s", user="u")
    assert "Authorization" not in respx.calls.last.request.headers


@respx.mock
def test_complete_chat_http_error_raises() -> None:
    respx.post("http://llm.test/v1/chat/completions").mock(
        return_value=httpx.Response(500, text="boom")
    )
    settings = LlmSettings(base_url="http://llm.test", api_key="k", default_model="m")
    with pytest.raises(LlmError):
        complete_chat(settings, model="m", system="s", user="u")
```

- [ ] **Step 2: Run — expect FAIL**

```bash
cd backend && uv sync --extra dev && uv run pytest tests/test_llm.py -v
```

- [ ] **Step 3: Implement `app/llm.py` + deps**

Add `httpx` to project dependencies; `respx` to dev.

- [ ] **Step 4: Tests PASS + ruff**

```bash
uv run ruff check app tests && uv run pytest tests/test_llm.py -v
```

- [ ] **Step 5: Commit**

```bash
git add backend
git commit -m "$(cat <<'EOF'
feat: OpenAI-compat клиент в backend

EOF
)"
```

---

### Task 2: Wire `create_job` LLM path (TDD)

**Files:**
- Modify: `backend/app/main.py`
- Modify: `backend/tests/test_api.py` (keep stub without LLM env)
- Create/extend: `backend/tests/test_jobs_llm.py`

**Interfaces:**
- Consumes: `load_llm_settings`, `complete_chat`, `LlmError`
- When `kind in {brief, follow_up}` and `settings.base_url` non-empty:
  - Resolve model = `payload.model` or default; system/user from payload
  - On success: artifact body = completion; status succeeded
  - On `LlmError` / missing fields: status `failed`, error string, `artifact_ids: []`
- Else: existing stub (including refine)

- [ ] **Step 1: Failing tests**

```python
# tests/test_jobs_llm.py — monkeypatch env + respx

@respx.mock
def test_brief_job_uses_llm_when_configured(monkeypatch, ...):
    monkeypatch.setenv("LLM_BASE_URL", "http://llm.test")
    monkeypatch.setenv("LLM_API_KEY", "LOCAL-API-KEY")
    monkeypatch.setenv("LLM_MODEL", "default-model")
    # reload settings or patch load_llm_settings
    respx.post("http://llm.test/v1/chat/completions").mock(
        return_value=httpx.Response(
            200, json={"choices": [{"message": {"content": "# Real brief"}}]}
        )
    )
    created = client.post("/v1/jobs", headers=AUTH, json={
        "meeting_id": "m1",
        "kind": "brief",
        "primary_language": "ru",
        "allowed_languages": ["ru"],
        "payload": {"model": "Google/gemma-4-12b-it", "system": "sys", "user": "usr"},
    })
    assert created.status_code == 201
    job = created.json()
    assert job["status"] == "succeeded"
    art = client.get(f"/v1/artifacts/{job['artifact_ids'][0]}", headers=AUTH).json()
    assert art["body_markdown"] == "# Real brief"
    assert "Stub" not in art["body_markdown"]


@respx.mock
def test_brief_job_llm_error_fails_without_stub(monkeypatch, ...):
    ...
    respx.post(...).mock(return_value=httpx.Response(401, text="nope"))
    job = client.post(...).json()
    assert job["status"] == "failed"
    assert job["artifact_ids"] == []
    assert job["error"]


def test_refine_still_stub_without_llm_call(monkeypatch):
    monkeypatch.delenv("LLM_BASE_URL", raising=False)
    # existing stub path for refine
```

Note: if `EXPECTED_TOKEN` / settings are module-level at import, tests may need to reload `app.main` or read env inside request handlers via `load_llm_settings()` each call.

- [ ] **Step 2: FAIL then implement wiring in `create_job`**

- [ ] **Step 3: Full backend suite green**

```bash
uv run pytest && uv run ruff check app tests
```

- [ ] **Step 4: Commit**

```bash
git commit -m "$(cat <<'EOF'
feat: jobs brief/follow_up через LLM provider

EOF
)"
```

---

### Task 3: UniFFI payload with prompts (TDD)

**Files:**
- Modify: `rust/crates/ffi/src/lib.rs` (`generate_artifact` backend branch; optionally `submit_backend_job` leave payload None for refine)
- Tests in same file

**Interfaces:**
- Consumes: `brief_prompts`, `follow_up_prompts`
- Produces: `CreateJobRequest.payload = Some(json!({ "model": llm_model_id, "system", "user" }))`

- [ ] **Step 1: Update mockito test to match payload**

```rust
#[test]
fn generate_artifact_backend_sends_prompt_payload() {
    let mut server = Server::new();
    let _post = server
        .mock("POST", "/v1/jobs")
        .match_body(Matcher::PartialJson(serde_json::json!({
            "kind": "brief",
            "payload": {
                "model": "Google/gemma-4-12b-it",
                "system": serde_json::Value::String("x".into()), // use Matcher that checks keys exist
            }
        })))
        // Prefer: match_body with custom check or PartialJson for kind + payload.model
        .with_status(201)
        .with_body(r#"{"id":"j1","meeting_id":"m-pay","kind":"brief","status":"succeeded","error":null,"artifact_ids":["a1"]}"#)
        .create();
    // ... seed final, set_llm_config("backend", "Google/gemma-4-12b-it", ""), generate Brief
    // assert success; optionally inspect mock was hit
}
```

 simultaneous: change existing `generate_artifact_backend_*` tests so POST still matches (payload may break PartialJson-less mocks — they still accept any body unless Matcher set).

Implement: before building request in backend branch:

```rust
let model_id = guard.llm_model_id.clone();
let primary = guard.language_policy.primary;
let final_body = final_transcript.body_markdown.clone();
let (system, user) = match domain_kind {
    ArtifactKind::Brief => brief_prompts(&final_body, primary),
    ArtifactKind::FollowUp => follow_up_prompts(&final_body, primary),
};
let payload = Some(serde_json::json!({
    "model": model_id,
    "system": system,
    "user": user,
}));
```

- [ ] **Step 2: `cargo test -p meetingraft-ffi` FAIL then GREEN**

- [ ] **Step 3: Commit**

```bash
git commit -m "$(cat <<'EOF'
feat: backend generateArtifact шлёт prompts в payload

EOF
)"
```

---

### Task 4: Swift — Model id when Backend

**Files:**
- Modify: `ProviderSettingsStore.swift` (`needsModel` or show model for `.backend`)
- Modify: `SettingsView.swift`
- Test: `ProviderSettingsStoreTests.swift`

**Interfaces:**
- `LlmEngine.needsModel: Bool` — true for `.backend`, `.ollama`, `.openaiCompat`
- `needsUrl` stays true only for ollama/openaiCompat
- Settings: show Model field when `needsModel`; URL when `needsUrl`

- [ ] **Step 1: Failing test**

```swift
func testBackendNeedsModelButNotUrl() {
    XCTAssertTrue(LlmEngine.backend.needsModel)
    XCTAssertFalse(LlmEngine.backend.needsUrl)
    XCTAssertTrue(LlmEngine.ollama.needsModel)
}
```

- [ ] **Step 2: Implement + SettingsView bind Model when needsModel**

- [ ] **Step 3: macOS tests for ProviderSettingsStore**

```bash
cd apps/macos && xcodegen generate
xcodebuild ... -only-testing:MeetingRaftTests/ProviderSettingsStoreTests
```

- [ ] **Step 4: Commit**

```bash
git commit -m "$(cat <<'EOF'
feat: Model id в Settings для LLM Backend

EOF
)"
```

---

### Task 5: Docs, compose env, backlog + verify

**Files:**
- `docker-compose.yml` — optional `LLM_*` with comments / empty defaults
- `docs/architecture-and-install.md` §2.5 — LLM provider env + Settings LLM=Backend + model
- `docs/backlog.md` — registry, billing, STT picker, remote STT, full-audio model
- `docs/roadmap.md` — one-liner if needed

- [ ] **Step 1: Docs + compose**

Example compose:

```yaml
environment:
  MEETINGRAFT_API_TOKEN: dev-token
  LLM_BASE_URL: ${LLM_BASE_URL:-}
  LLM_API_KEY: ${LLM_API_KEY:-}
  LLM_MODEL: ${LLM_MODEL:-}
```

- [ ] **Step 2: Full verify**

```bash
cd backend && uv run ruff check app tests && uv run pytest
cd rust && cargo test -p meetingraft-ffi -p meetingraft-postcall -p meetingraft-sync
cd apps/macos && swiftformat Sources Tests --lint && xcodegen generate
xcodebuild -project MeetingRaft.xcodeproj -scheme MeetingRaft \
  -configuration Debug test CODE_SIGNING_ALLOWED=NO
```

- [ ] **Step 3: Commit**

```bash
git commit -m "$(cat <<'EOF'
docs: Backend LLM provider env и backlog

EOF
)"
```

---

## Spec coverage checklist

| Spec item | Task |
|-----------|------|
| Env LLM_BASE_URL / KEY / MODEL | 1–2, 5 |
| complete_chat Bearer optional | 1 |
| jobs brief/follow_up → LLM | 2 |
| failed job no stub on LLM error | 2 |
| refine / no LLM_BASE_URL stub | 2 |
| Rust prompts in payload | 3 |
| Frontend model id for Backend | 4 |
| Docs + backlog product direction | 5 |
| No llmApiKey in app | (explicit non-goal) |
