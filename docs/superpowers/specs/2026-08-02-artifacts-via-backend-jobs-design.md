# Phase 6 follow-up — Artifacts via backend jobs (LLM engine `backend`)

**Date:** 2026-08-02  
**Status:** Approved for implementation  
**Maps to:** ADR-007, Phase 6 follow-up, Epic 8 (artifacts path)  
**Depends on:** backend stub + Meetings refine poll UI; `LlmClient` trait remains for future Ollama

## Goal

При выборе LLM provider **`backend`** кнопки Generate Brief / Follow-up
создают jobs `brief` / `follow_up` через существующий sync client, poll’ят
результат и сохраняют stub markdown как **local** `Artifact` в том же списке
Artifacts. Режим **`builtin_templates`** без изменений (локальные heuristic
templates). Ollama / openai_compat остаются disabled «скоро».

## Non-goals

- Реальный LLM в FastAPI или локальный Ollama HTTP (`LlmClient` adapters)
- Авто-fallback на templates при ошибке backend
- Отправка текста Final в `payload` job (stub игнорирует; `payload: None`)
- Изменение панели Submit refine (stub)
- Keychain / новые OpenAPI endpoints

## Approach

Выбран **backend jobs** (не локальный Ollama): Settings `LlmEngine.backend`
available; генерация идёт через ADR-007 jobs API.

Отклонённые: Ollama-only в `postcall`; общий OpenAI-compat клиент в этом PR.

## Architecture

```text
Settings: llmEngine=backend + setApiConfig + setLlmConfig
MeetingDetailView Generate Brief/Follow-up
  → MeetingCore.generate_artifact(kind)
       engine builtin → render_brief / render_follow_up → SQLite
       engine backend → SyncClient create_job(brief|follow_up)
                      → poll get_job (20×250ms) → get_artifact
                      → insert Artifact (template_id backend.*)
  → MeetingsViewModel refreshes listArtifacts
```

Networking остаётся в Rust. Swift не дублирует poll-цикл refine для Brief.

## UniFFI / core

### `setLlmConfig(engine_code: String, model_id: String)`

| `engine_code` | Behavior |
|---------------|----------|
| `builtin_templates` | Local templates |
| `backend` | Jobs path |
| other / empty | Treat as `builtin_templates` (safe default) |

`model_id` хранится для будущей provenance; stub не использует.

Хранить в `MeetingCoreInner`: `llm_engine: String`, `llm_model_id: String`
(default `builtin_templates`, `""`).

Settings / app: при sync API config также вызывать `setLlmConfig` из
`ProviderSettingsStore` (`llmEngine.rawValue`, `llmModelId`).

### `generate_artifact` branching

1. Load Final; missing → error `"final transcript not found"`.
2. If `llm_engine == "backend"`:
   - Build `CreateJobRequest` with `kind` Brief→`brief`, FollowUp→`follow_up`,
     language policy from session, `payload: None`.
   - `create_job`; on sync/`error` field / `failed` → return error, no insert.
   - If not immediately `succeeded`, poll `get_job` up to **20** times,
     **250 ms** sleep (same policy as Meetings refine UI).
   - Require non-empty `artifact_ids`; `get_artifact`; on error → return.
   - `make_artifact` / insert with `template_id` =
     `backend.brief` | `backend.follow_up`, body = artifact markdown.
3. Else (builtin): existing `render_brief` / `render_follow_up` + insert.

## Swift

- `LlmEngine.backend.isAvailable = true`; `ollama` / `openaiCompat` stay false.
- Settings: URL field for LLM only when `needsUrl && isAvailable`; for
  `backend`, `needsUrl` becomes **false** (uses `apiBaseUrl` already shown).
- Update `ProviderSettingsStoreTests`: backend available; selecting backend sticks.
- `artifactsPipelineCaption` for backend already correct.
- Meetings Generate API unchanged; optional: ensure `setLlmConfig` before generate
  if Settings not visited (wire in app appear / Meetings appear).

## Errors

- No silent fallback to templates when backend fails.
- Surface message via existing `FfiGenerateArtifactResult.error` → Meetings alert.
- User may switch Settings to builtin and retry.

## Testing

**Rust (`meetingraft-ffi` or postcall/sync):**
- Builtin path unchanged behavior (unit already covers templates).
- Backend path with mock SyncClient / httpmock: succeeded job → stored artifact
  with `backend.brief`; failed/timeout → error, no insert.

**Swift:**
- `LlmEngine.backend.isAvailable == true`; didSet does not revert backend.
- Existing Meetings generate spy tests still pass (spy ignores engine).

## Docs

- This spec
- Plan: `docs/superpowers/plans/2026-08-02-artifacts-via-backend-jobs.md`
- `docs/backlog.md` — LLM partial via backend stub jobs; Ollama still deferred
- `docs/architecture-and-install.md` — Generate Brief when engine=backend
- `docs/superpowers/specs/2026-08-02-providers-and-provenance-design.md` note
  or roadmap one-liner: backend LLM engine available

## Done criteria

- [ ] Settings → LLM = Backend; Generate Brief/Follow-up against docker shows
      stub markdown in Artifacts list (`template_id` backend.*)
- [ ] Builtin path regression-free
- [ ] `cargo test` + `xcodebuild test` + swiftformat lint clean
- [ ] Ollama still disabled «скоро»
