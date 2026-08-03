# Backend provider registry + GET /v1/models — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Статический JSON-реестр LLM-провайдеров на backend, `GET /v1/models`, Settings picker `(provider_id, model)`, jobs `brief`/`follow_up` роутятся по `provider_id`; compat `LLM_*` → `default`.

**Architecture:** Backend `registry.py` загружает `PROVIDERS_JSON` / `LLM_PROVIDERS_FILE` или synthetic `default` из `LLM_*`. `complete_chat` остаётся прокси; `main` резолвит провайдера из payload. Rust sync + UniFFI отдают каталог; Swift Settings при Backend — picker + Refresh.

**Tech Stack:** FastAPI, pydantic, httpx/respx, pytest; Rust `meetingraft-sync` + UniFFI; SwiftUI Settings.

**Spec:** `docs/superpowers/specs/2026-08-03-backend-provider-registry-design.md`

## Global Constraints

- Ключи/`base_url` провайдеров никогда не в `/v1/models` и не в app.
- При валидном реестре `LLM_*` игнорируются.
- Нет silent stub fallback при ошибке LLM, если LLM сконфигурирован.
- Legacy payload без `provider_id` принимается **только** в compat-режиме (`default` из `LLM_*`).
- Comments Russian; identifiers English; Conventional Commits Russian subject.
- TDD: failing test → implement → green → commit per task.
- После смены UniFFI: `apps/macos/Scripts/generate-ffi.sh`.

## File map

| File | Role |
|------|------|
| `backend/app/registry.py` | Load/validate registry; public model list; resolve provider → `LlmSettings` |
| `backend/app/llm.py` | Unchanged `complete_chat` / `LlmSettings` (reuse) |
| `backend/app/main.py` | `GET /v1/models`; job routing by `provider_id` |
| `backend/tests/test_registry.py` | Load / fail-fast / compat |
| `backend/tests/test_models_api.py` | Models endpoint |
| `backend/tests/test_jobs_llm.py` | Update for `provider_id` + multi-provider |
| `shared/openapi.yaml` | Path + schemas |
| `rust/crates/sync/src/dto.rs` | `LlmModelRefDto`, `ListModelsResponse` |
| `rust/crates/sync/src/client.rs` | `list_models()` |
| `rust/crates/ffi/src/lib.rs` | `FfiLlmModelRef`, `list_backend_llm_models`, `set_llm_config` + payload |
| `apps/macos/Generated/*` | Regenerated |
| `apps/macos/Sources/App/ProviderSettingsStore.swift` | `llmProviderId`, catalog cache, banner |
| `apps/macos/Sources/Settings/SettingsView.swift` | Backend picker + Refresh |
| `apps/macos/Sources/Meetings/*` | Pass `providerId` into `setLlmConfig` |
| `docs/architecture-and-install.md`, `backlog.md`, `roadmap.md` | Docs |
| `docker-compose.yml` | Optional comment / example env for registry |

---

### Task 1: Backend registry load (TDD)

**Files:**
- Create: `backend/app/registry.py`
- Test: `backend/tests/test_registry.py`
- Keep: `backend/app/llm.py` (`LlmSettings`, `complete_chat`)

**Interfaces:**
- Produces:
  ```python
  @dataclass(frozen=True, slots=True)
  class ProviderModel:
      id: str
      display_name: str  # "" if absent → UI uses model id

  @dataclass(frozen=True, slots=True)
  class Provider:
      id: str
      base_url: str  # rstrip /
      api_key: str
      default_model: str
      models: tuple[ProviderModel, ...]

  @dataclass(frozen=True, slots=True)
  class Registry:
      providers: tuple[Provider, ...]  # ordered
      source: str  # "json" | "file" | "env_compat" | "empty"

  class RegistryError(RuntimeError): ...

  def load_registry() -> Registry: ...
  # PROVIDERS_JSON if non-empty → parse
  # else LLM_PROVIDERS_FILE if non-empty → read file
  # else if LLM_BASE_URL → Provider(id="default", ..., models from LLM_MODEL)
  # else empty

  def public_models(registry: Registry) -> list[dict[str, str]]:
      # [{provider_id, model, display_name}, ...] — no secrets

  def provider_settings(registry: Registry, provider_id: str) -> LlmSettings:
      # Lookup; raise RegistryError if missing / empty base_url
  ```

- [ ] **Step 1: Failing tests**

```python
# backend/tests/test_registry.py
import json
from pathlib import Path

import pytest

from app.registry import RegistryError, load_registry, public_models, provider_settings


def test_load_providers_json(monkeypatch: pytest.MonkeyPatch) -> None:
    payload = {
        "providers": [
            {
                "id": "home-llm",
                "base_url": "http://host:58001/",
                "api_key": "SECRET",
                "default_model": "m1",
                "models": [
                    {"id": "m1", "display_name": "Model One"},
                    {"id": "m2"},
                ],
            }
        ]
    }
    monkeypatch.setenv("PROVIDERS_JSON", json.dumps(payload))
    monkeypatch.delenv("LLM_PROVIDERS_FILE", raising=False)
    monkeypatch.delenv("LLM_BASE_URL", raising=False)

    registry = load_registry()
    assert registry.source == "json"
    assert len(registry.providers) == 1
    assert registry.providers[0].base_url == "http://host:58001"
    models = public_models(registry)
    assert models == [
        {"provider_id": "home-llm", "model": "m1", "display_name": "Model One"},
        {"provider_id": "home-llm", "model": "m2", "display_name": ""},
    ]
    assert "SECRET" not in json.dumps(models)
    settings = provider_settings(registry, "home-llm")
    assert settings.api_key == "SECRET"
    assert settings.default_model == "m1"


def test_empty_models_uses_default_model(monkeypatch: pytest.MonkeyPatch) -> None:
    payload = {
        "providers": [
            {
                "id": "p1",
                "base_url": "http://x",
                "api_key": "",
                "default_model": "only-default",
                "models": [],
            }
        ]
    }
    monkeypatch.setenv("PROVIDERS_JSON", json.dumps(payload))
    models = public_models(load_registry())
    assert models == [
        {"provider_id": "p1", "model": "only-default", "display_name": ""},
    ]


def test_compat_llm_env(monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.delenv("PROVIDERS_JSON", raising=False)
    monkeypatch.delenv("LLM_PROVIDERS_FILE", raising=False)
    monkeypatch.setenv("LLM_BASE_URL", "http://llm.test/")
    monkeypatch.setenv("LLM_API_KEY", "k")
    monkeypatch.setenv("LLM_MODEL", "Google/gemma")
    registry = load_registry()
    assert registry.source == "env_compat"
    assert registry.providers[0].id == "default"
    assert public_models(registry)[0]["model"] == "Google/gemma"


def test_registry_ignores_llm_env_when_json_present(monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.setenv(
        "PROVIDERS_JSON",
        json.dumps(
            {
                "providers": [
                    {
                        "id": "a",
                        "base_url": "http://a",
                        "api_key": "",
                        "default_model": "ma",
                        "models": [{"id": "ma"}],
                    }
                ]
            }
        ),
    )
    monkeypatch.setenv("LLM_BASE_URL", "http://ignored")
    monkeypatch.setenv("LLM_MODEL", "ignored-model")
    registry = load_registry()
    assert registry.source == "json"
    assert [p.id for p in registry.providers] == ["a"]


def test_duplicate_provider_id_fails(monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.setenv(
        "PROVIDERS_JSON",
        json.dumps(
            {
                "providers": [
                    {
                        "id": "dup",
                        "base_url": "http://a",
                        "api_key": "",
                        "default_model": "",
                        "models": [{"id": "m"}],
                    },
                    {
                        "id": "dup",
                        "base_url": "http://b",
                        "api_key": "",
                        "default_model": "",
                        "models": [{"id": "n"}],
                    },
                ]
            }
        ),
    )
    with pytest.raises(RegistryError):
        load_registry()


def test_duplicate_provider_model_pair_fails(monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.setenv(
        "PROVIDERS_JSON",
        json.dumps(
            {
                "providers": [
                    {
                        "id": "p",
                        "base_url": "http://a",
                        "api_key": "",
                        "default_model": "",
                        "models": [{"id": "m"}, {"id": "m"}],
                    }
                ]
            }
        ),
    )
    with pytest.raises(RegistryError):
        load_registry()


def test_invalid_json_fails(monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.setenv("PROVIDERS_JSON", "{not-json")
    with pytest.raises(RegistryError):
        load_registry()


def test_providers_file(monkeypatch: pytest.MonkeyPatch, tmp_path: Path) -> None:
    path = tmp_path / "providers.json"
    path.write_text(
        json.dumps(
            {
                "providers": [
                    {
                        "id": "file-p",
                        "base_url": "http://f",
                        "api_key": "",
                        "default_model": "fm",
                        "models": [{"id": "fm"}],
                    }
                ]
            }
        ),
        encoding="utf-8",
    )
    monkeypatch.delenv("PROVIDERS_JSON", raising=False)
    monkeypatch.setenv("LLM_PROVIDERS_FILE", str(path))
    registry = load_registry()
    assert registry.source == "file"
    assert registry.providers[0].id == "file-p"


def test_unknown_provider_raises(monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.delenv("PROVIDERS_JSON", raising=False)
    monkeypatch.setenv("LLM_BASE_URL", "http://x")
    monkeypatch.setenv("LLM_MODEL", "m")
    with pytest.raises(RegistryError):
        provider_settings(load_registry(), "nope")
```

- [ ] **Step 2: Run — expect FAIL**

```bash
cd backend && uv run pytest tests/test_registry.py -v
```

Expected: import / missing module errors.

- [ ] **Step 3: Implement `registry.py`**

Minimal: parse JSON schema as in spec; trim `base_url`; empty `PROVIDERS_JSON`/missing file → fall through; empty `models` + non-empty `default_model` → one catalog entry; `provider_settings` → `LlmSettings(base_url=..., api_key=..., default_model=...)`.

- [ ] **Step 4: Run — expect PASS**

```bash
cd backend && uv run pytest tests/test_registry.py -v
```

- [ ] **Step 5: Commit**

```bash
git add backend/app/registry.py backend/tests/test_registry.py
git commit -m "$(cat <<'EOF'
feat: загрузка JSON-реестра LLM-провайдеров на backend

EOF
)"
```

---

### Task 2: `GET /v1/models` + job routing by `provider_id`

**Files:**
- Modify: `backend/app/main.py`
- Create: `backend/tests/test_models_api.py`
- Modify: `backend/tests/test_jobs_llm.py`
- Modify: `backend/tests/test_api.py` if health/jobs stub assumptions break

**Interfaces:**
- Consumes: `load_registry`, `public_models`, `provider_settings`, `RegistryError`; `complete_chat`, `LlmError`
- Produces:
  - `GET /v1/models` → `{"models": [...]}` (bearer)
  - Jobs: if `kind in {brief,follow_up}` and registry has any provider with non-empty `base_url`:
    - resolve `provider_id` (compat legacy: missing → `"default"` only when `source == "env_compat"`)
    - else missing/unknown → failed job
    - `complete_chat(settings, model=..., system=..., user=...)`
  - else stub as today

- [ ] **Step 1: Failing tests**

```python
# backend/tests/test_models_api.py
import json

import pytest
from fastapi.testclient import TestClient

from app.main import app

client = TestClient(app)
AUTH = {"Authorization": "Bearer dev-token"}


def test_list_models_from_registry(monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.setenv(
        "PROVIDERS_JSON",
        json.dumps(
            {
                "providers": [
                    {
                        "id": "home-llm",
                        "base_url": "http://h",
                        "api_key": "SECRET",
                        "default_model": "m1",
                        "models": [{"id": "m1", "display_name": "One"}],
                    }
                ]
            }
        ),
    )
    response = client.get("/v1/models", headers=AUTH)
    assert response.status_code == 200
    body = response.json()
    assert body == {
        "models": [
            {"provider_id": "home-llm", "model": "m1", "display_name": "One"},
        ]
    }
    assert "SECRET" not in response.text


def test_list_models_empty_without_llm(monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.delenv("PROVIDERS_JSON", raising=False)
    monkeypatch.delenv("LLM_PROVIDERS_FILE", raising=False)
    monkeypatch.delenv("LLM_BASE_URL", raising=False)
    response = client.get("/v1/models", headers=AUTH)
    assert response.status_code == 200
    assert response.json() == {"models": []}


def test_list_models_requires_auth(monkeypatch: pytest.MonkeyPatch) -> None:
    response = client.get("/v1/models")
    assert response.status_code == 401
```

Update `test_jobs_llm.py`:

```python
# Add provider_id to successful payloads when using LLM_*;
# add test with PROVIDERS_JSON two providers — assert correct base URL called;
# add test unknown provider_id → failed;
# add test registry mode missing provider_id → failed;
# keep legacy without provider_id succeeding only with LLM_* compat.
```

Concrete new test:

```python
@respx.mock
def test_job_routes_by_provider_id(monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.setenv(
        "PROVIDERS_JSON",
        json.dumps(
            {
                "providers": [
                    {
                        "id": "a",
                        "base_url": "http://a.test",
                        "api_key": "ka",
                        "default_model": "ma",
                        "models": [{"id": "ma"}],
                    },
                    {
                        "id": "b",
                        "base_url": "http://b.test",
                        "api_key": "kb",
                        "default_model": "mb",
                        "models": [{"id": "mb"}],
                    },
                ]
            }
        ),
    )
    route_a = respx.post("http://a.test/v1/chat/completions").mock(
        return_value=httpx.Response(
            200, json={"choices": [{"message": {"content": "# from A"}}]}
        )
    )
    route_b = respx.post("http://b.test/v1/chat/completions").mock(
        return_value=httpx.Response(
            200, json={"choices": [{"message": {"content": "# from B"}}]}
        )
    )
    job = create_job(
        "brief",
        {
            "provider_id": "b",
            "model": "mb",
            "system": "s",
            "user": "u",
        },
    )
    assert job["status"] == "succeeded"
    assert route_b.called
    assert not route_a.called
```

- [ ] **Step 2: Run — expect FAIL**

```bash
cd backend && uv run pytest tests/test_models_api.py tests/test_jobs_llm.py -v
```

- [ ] **Step 3: Wire `main.py`**

Replace `load_llm_settings()` gate with `load_registry()`; add endpoint:

```python
@app.get("/v1/models", dependencies=[Depends(require_bearer)])
def list_models() -> dict[str, Any]:
    return {"models": public_models(load_registry())}
```

Job branch sketch:

```python
registry = load_registry()
llm_ready = any(p.base_url for p in registry.providers)
if body.kind in {"brief", "follow_up"} and llm_ready:
    payload = body.payload or {}
    raw_provider = payload.get("provider_id")
    if raw_provider is None or raw_provider == "":
        if registry.source == "env_compat":
            provider_id = "default"
        else:
            raise LlmError("Не указан provider_id")  # caught → failed job
    else:
        provider_id = str(raw_provider)
    settings = provider_settings(registry, provider_id)  # RegistryError → failed
    ...
```

Map `RegistryError` to failed job like `LlmError`.

- [ ] **Step 4: Fix existing jobs tests** — add `provider_id: "default"` where needed for clarity; ensure legacy-without-id still passes under `LLM_*` only.

- [ ] **Step 5: Run — expect PASS**

```bash
cd backend && uv run pytest -v
```

- [ ] **Step 6: Commit**

```bash
git add backend/app/main.py backend/tests/test_models_api.py backend/tests/test_jobs_llm.py
git commit -m "$(cat <<'EOF'
feat: GET /v1/models и routing jobs по provider_id

EOF
)"
```

---

### Task 3: OpenAPI contract

**Files:**
- Modify: `shared/openapi.yaml`

- [ ] **Step 1: Add path + schemas**

```yaml
  /v1/models:
    get:
      summary: List configured LLM models
      operationId: listModels
      responses:
        "200":
          description: Model catalog (no provider secrets)
          content:
            application/json:
              schema:
                $ref: "#/components/schemas/ListModelsResponse"
        "401":
          description: Unauthorized
```

Schemas:

```yaml
    LlmModelRef:
      type: object
      required: [provider_id, model]
      properties:
        provider_id:
          type: string
        model:
          type: string
        display_name:
          type: string
    ListModelsResponse:
      type: object
      required: [models]
      properties:
        models:
          type: array
          items:
            $ref: "#/components/schemas/LlmModelRef"
```

In description of `CreateJobRequest.payload` note: for `brief`/`follow_up` preferred keys `provider_id`, `model`, `system`, `user`.

- [ ] **Step 2: Commit**

```bash
git add shared/openapi.yaml
git commit -m "$(cat <<'EOF'
docs: OpenAPI GET /v1/models для LLM registry

EOF
)"
```

---

### Task 4: Rust sync `list_models` (TDD)

**Files:**
- Modify: `rust/crates/sync/src/dto.rs`
- Modify: `rust/crates/sync/src/client.rs`
- Modify: `rust/crates/sync/src/lib.rs` (re-exports)
- Tests in `client.rs` `#[cfg(test)]` (mockito pattern as existing)

**Interfaces:**
```rust
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LlmModelRefDto {
    pub provider_id: String,
    pub model: String,
    #[serde(default)]
    pub display_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ListModelsResponse {
    pub models: Vec<LlmModelRefDto>,
}

impl SyncClient {
    pub fn list_models(&self) -> Result<Vec<LlmModelRefDto>, SyncError> { ... }
}
```

- [ ] **Step 1: Failing test**

```rust
#[test]
fn list_models_parses_catalog() {
    let mut server = mockito::Server::new();
    let _m = server
        .mock("GET", "/v1/models")
        .match_header("Authorization", "Bearer t")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{"models":[{"provider_id":"home-llm","model":"m1","display_name":"One"}]}"#)
        .create();
    let client = SyncClient::new(server.url(), "t");
    let models = client.list_models().unwrap();
    assert_eq!(models.len(), 1);
    assert_eq!(models[0].provider_id, "home-llm");
    assert_eq!(models[0].model, "m1");
    assert_eq!(models[0].display_name, "One");
}
```

- [ ] **Step 2: Run — expect FAIL**

```bash
cd rust && cargo test -p meetingraft-sync list_models -- --nocapture
```

- [ ] **Step 3: Implement DTO + `list_models`**

```rust
pub fn list_models(&self) -> Result<Vec<LlmModelRefDto>, SyncError> {
    self.ensure_configured()?;
    let response = self
        .http()?
        .get(format!("{}/v1/models", self.base_url))
        .header("Authorization", format!("Bearer {}", self.token))
        .send()?;
    let parsed: ListModelsResponse = Self::parse_json(response)?;
    Ok(parsed.models)
}
```

- [ ] **Step 4: Run — expect PASS**

```bash
cd rust && cargo test -p meetingraft-sync list_models -- --nocapture
```

- [ ] **Step 5: Commit**

```bash
git add rust/crates/sync/src/dto.rs rust/crates/sync/src/client.rs rust/crates/sync/src/lib.rs
git commit -m "$(cat <<'EOF'
feat: SyncClient.list_models для каталога LLM

EOF
)"
```

---

### Task 5: UniFFI — provider_id + list models + payload

**Files:**
- Modify: `rust/crates/ffi/src/lib.rs`
- Regenerate: `apps/macos/Generated/` via `apps/macos/Scripts/generate-ffi.sh`
- Update call sites in Swift in Task 6 (compile may break until then — regenerate after Rust green)

**Interfaces:**
```rust
#[derive(Debug, Clone, uniffi::Record)]
pub struct FfiLlmModelRef {
    pub provider_id: String,
    pub model: String,
    pub display_name: String,
}

// MeetingCore:
pub fn list_backend_llm_models(&self) -> Vec<FfiLlmModelRef>;
// empty vec on sync error / not configured (or surface error string — prefer empty + log via return; keep simple: empty on Err)

pub fn set_llm_config(
    &self,
    engine_code: String,
    model_id: String,
    base_url: String,
    provider_id: String, // NEW last arg; "" for local engines
);

// Inner: llm_provider_id: String
// generate_artifact backend payload:
// { "provider_id": guard.llm_provider_id, "model": guard.llm_model_id, "system", "user" }
```

- [ ] **Step 1: Failing FFI test** — extend `generate_artifact_backend_uses_job_artifact` expected POST body to include `"provider_id":"default"` (or chosen id); add test that `list_backend_llm_models` maps mockito `/v1/models`.

```rust
#[test]
fn list_backend_llm_models_maps_sync() {
    let mut server = mockito::Server::new();
    let _m = server
        .mock("GET", "/v1/models")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{"models":[{"provider_id":"p","model":"m","display_name":"D"}]}"#)
        .create();
    let core = MeetingCore::new_for_tests(/* existing helper or temp data_root */);
    core.set_api_config(server.url(), "dev-token".into());
    let models = core.list_backend_llm_models();
    assert_eq!(models.len(), 1);
    assert_eq!(models[0].provider_id, "p");
}
```

Use the same test harness / temp dir pattern as other `MeetingCore` tests in `lib.rs`.

- [ ] **Step 2: Run — expect FAIL**

```bash
cd rust && cargo test -p meetingraft-ffi list_backend_llm_models -- --nocapture
```

- [ ] **Step 3: Implement** — field `llm_provider_id`, update `set_llm_config`, payload, `list_backend_llm_models`, fix all Rust `set_llm_config(` call sites (add `String::new()` or `"default"`).

- [ ] **Step 4: Run**

```bash
cd rust && cargo test -p meetingraft-ffi -- --nocapture
```

- [ ] **Step 5: Regenerate FFI**

```bash
apps/macos/Scripts/generate-ffi.sh
```

- [ ] **Step 6: Commit**

```bash
git add rust/crates/ffi/src/lib.rs apps/macos/Generated/
git commit -m "$(cat <<'EOF'
feat: UniFFI list_backend_llm_models и provider_id в LLM config

EOF
)"
```

---

### Task 6: Swift Settings picker + wiring

**Files:**
- Modify: `apps/macos/Sources/App/ProviderSettingsStore.swift`
- Modify: `apps/macos/Sources/Settings/SettingsView.swift`
- Modify: `apps/macos/Sources/Meetings/MeetingsViewModel.swift` (+ protocol)
- Modify: `apps/macos/Sources/Meetings/MeetingDetailView.swift`
- Modify: `apps/macos/Tests/MeetingsViewModelTests.swift` (fake core `setLlmConfig` signature)
- Modify: `apps/macos/Tests/ProviderSettingsStoreTests.swift` if exists / add coverage for provider id defaults
- Optionally small helper for selection key `provider_id\0model`

**Interfaces:**
```swift
// ProviderSettingsStore
var llmProviderId: String = "default"
var backendLlmModels: [FfiLlmModelRef] = []
var backendLlmModelsMessage: String = ""

// Selection identity for Picker
struct BackendLlmSelection: Hashable, Identifiable {
    var id: String { "\(providerId)|\(model)" }
    let providerId: String
    let model: String
    let displayName: String
}

// LlmEngine.needsModel: backend → false for free-text; show picker instead
// needsBackendModelPicker: true for .backend
```

- [ ] **Step 1: Update store + tests**

```swift
// ProviderSettingsStoreTests — llmProviderId default "default";
// artifactsPipelineCaption for backend includes provider/model when set
```

- [ ] **Step 2: Settings UI**

When `llmEngine == .backend`:
- Picker over `backendLlmModels` (label: `displayName.isEmpty ? "\(providerId) · \(model)" : displayName`)
- Button «Обновить» → `core.listBackendLlmModels()`
- Empty list → caption «Нет моделей — настройте PROVIDERS_JSON / LLM_* на backend»
- Hide TextField Model id for backend; keep TextField for ollama/openai_compat

`applyApiConfig` / `setLlmConfig`:

```swift
core?.setLlmConfig(
    engineCode: providerStore.llmEngine.rawValue,
    modelId: providerStore.llmModelId,
    baseUrl: providerStore.llmBaseUrl,
    providerId: providerStore.llmProviderId
)
```

On picker change: set both `llmProviderId` and `llmModelId`.

- [ ] **Step 3: MeetingsViewModel protocol + MeetingDetailView** — pass `llmProviderId`.

- [ ] **Step 4: Build/tests**

```bash
cd apps/macos && xcodegen generate
xcodebuild -project MeetingRaft.xcodeproj -scheme MeetingRaft -configuration Debug test CODE_SIGNING_ALLOWED=NO
```

(Or at least `swiftformat Sources Tests --lint` + targeted tests if full xcodebuild heavy.)

- [ ] **Step 5: Commit**

```bash
git add apps/macos/Sources apps/macos/Tests
git commit -m "$(cat <<'EOF'
feat: Settings picker моделей backend LLM по provider_id

EOF
)"
```

---

### Task 7: Docs + compose hint

**Files:**
- Modify: `docs/architecture-and-install.md` §2.5
- Modify: `docs/backlog.md` — registry **partial**
- Modify: `docs/roadmap.md` Remaining — partial registry
- Modify: `docker-compose.yml` — comment example `PROVIDERS_JSON` / keep `LLM_*` documented

- [ ] **Step 1: Update install §2.5** — document registry JSON example, env table (`PROVIDERS_JSON`, `LLM_PROVIDERS_FILE`), compat `LLM_*` → `default`, Settings flow: Test API → LLM=Backend → Refresh models → Generate.

- [ ] **Step 2: Backlog / roadmap** as in spec success docs.

- [ ] **Step 3: Commit**

```bash
git add docs/architecture-and-install.md docs/backlog.md docs/roadmap.md docker-compose.yml
git commit -m "$(cat <<'EOF'
docs: registry LLM-провайдеров и GET /v1/models

EOF
)"
```

---

### Task 8: Final verification

- [ ] **Step 1: Backend**

```bash
cd backend && uv run pytest -v
```

- [ ] **Step 2: Rust**

```bash
cd rust && cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test
```

- [ ] **Step 3: macOS** (if machine allows)

```bash
cd apps/macos && swiftformat Sources Tests --lint
xcodebuild -project MeetingRaft.xcodeproj -scheme MeetingRaft -configuration Debug test CODE_SIGNING_ALLOWED=NO
```

- [ ] **Step 4: Manual smoke (optional)** — `PROVIDERS_JSON` with two providers → Settings picker shows both → Brief hits second base_url (check backend logs / respx-style).

- [ ] **Step 5: Confirm success criteria from spec all checked.

---

## Spec coverage checklist

| Spec requirement | Task |
|------------------|------|
| `PROVIDERS_JSON` / file registry | 1 |
| Compat `LLM_*` → `default` | 1, 2 |
| Fail-fast duplicates / bad JSON | 1 |
| Empty models → `default_model` | 1 |
| `GET /v1/models` no secrets | 2, 3 |
| Job routing `provider_id` | 2 |
| Legacy payload only env_compat | 2 |
| OpenAPI | 3 |
| Sync `list_models` | 4 |
| UniFFI + payload both fields | 5 |
| Settings picker + Refresh | 6 |
| Docs / backlog partial | 7 |

## Self-review notes

- No TBD placeholders; signatures aligned across tasks (`provider_id` last on `set_llm_config`).
- Billing/CRUD/discovery intentionally absent.
- Existing `test_jobs_llm` must be updated in Task 2 so CI stays green.
