# Phase 6 follow-up — Backend provider registry + GET /v1/models

**Date:** 2026-08-03
**Status:** Approved for implementation (approach 1)
**Maps to:** Epic 8 (Backend provider platform), ADR-007 jobs, Settings LLM=Backend
**Depends on:** `feat/backend-llm-provider` (single `LLM_*` OpenAI-compat proxy)

## Goal

Несколько OpenAI-compatible LLM-провайдеров настраиваются **только на
backend** (статический JSON-реестр). App при LLM=Backend получает каталог
через `GET /v1/models` и выбирает пару `(provider_id, model)`. Jobs
`brief`/`follow_up` роутятся на `base_url` + key выбранного провайдера.

Ключи и URL провайдеров во фронте не хранятся и не отдаются в API models.

## Decisions (brainstorming)

| Тема | Выбор |
|------|--------|
| Где реестр | Только сервер: `PROVIDERS_JSON` / `LLM_PROVIDERS_FILE`; без UI «добавить» |
| Каталог моделей | Статический список в конфиге (без live discovery) |
| Routing | Явные `provider_id` + `model` в job payload |
| Compat | Нет реестра → один провайдер `id=default` из `LLM_*` |

## Non-goals

- CRUD API / UI «добавить провайдера» в macOS
- Billing / лимиты (учёт и enforcement)
- Live discovery `{base}/v1/models` у upstream
- WhisperX / remote STT / Parakeet
- Keychain; streaming; silent stub fallback при ошибке LLM
- Удаление local engines `ollama` / `openai_compat`

## Approach

**1 (approved)** — JSON registry + OpenAPI `GET /v1/models` + Settings picker +
payload `provider_id`/`model`.

Отклонено: только YAML-файл без env (2); «тонкий» список поверх одного
`LLM_*` без multi-provider (3).

## Architecture

```text
Backend startup:
  PROVIDERS_JSON | LLM_PROVIDERS_FILE  → Registry
  else LLM_BASE_URL set                → Registry{ default from LLM_* }
  else                                 → empty (stub brief/follow_up)

Settings LLM=Backend:
  list_backend_llm_models() → GET /v1/models
  → picker stores llmProviderId + llmModelId

Generate Brief|Follow-up:
  payload: { provider_id, model, system, user }
  → resolve(provider_id) → complete_chat(base_url, api_key, model)
```

## Config schema

Env (один из источников реестра):

| Env | Meaning |
|-----|---------|
| `PROVIDERS_JSON` | Inline JSON реестра |
| `LLM_PROVIDERS_FILE` | Путь к JSON-файлу (если `PROVIDERS_JSON` пуст) |
| `LLM_BASE_URL` / `LLM_API_KEY` / `LLM_MODEL` | Compat → провайдер `default`, только если реестр отсутствует |

Пример реестра:

```json
{
  "providers": [
    {
      "id": "home-llm",
      "base_url": "http://93.189.243.223:58001",
      "api_key": "LOCAL-API-KEY",
      "default_model": "Google/gemma-4-12b-it",
      "models": [
        { "id": "Google/gemma-4-12b-it", "display_name": "Gemma 4 12B" },
        { "id": "Qwen/Qwen3-32B", "display_name": "Qwen3 32B" }
      ]
    }
  ]
}
```

Правила загрузки (fail-fast при старте backend):

- Невалидный JSON / схема → процесс не стартует
- Дубликат `provider.id` или пары `(provider_id, model.id)` → не стартует
- При наличии валидного реестра `LLM_*` **игнорируются**
- Пустые `PROVIDERS_JSON` / отсутствующий файл → считать «реестра нет» (compat)
- У провайдера пустой `models`, но непустой `default_model` → в каталог одна запись с этим id
- Compat: каталог `default` содержит только непустой `LLM_MODEL`

## API

### `GET /v1/models` (bearer)

```json
{
  "models": [
    {
      "provider_id": "home-llm",
      "model": "Google/gemma-4-12b-it",
      "display_name": "Gemma 4 12B"
    }
  ]
}
```

- `200` + пустой `models`, если LLM не сконфигурирован
- Без `base_url` / `api_key` в ответе
- `display_name` опционален → UI fallback на `model`

### Job payload (`brief` | `follow_up`)

```json
{
  "provider_id": "home-llm",
  "model": "Google/gemma-4-12b-it",
  "system": "...",
  "user": "..."
}
```

| Условие | Результат |
|---------|-----------|
| Реестр/compat активен, неизвестный `provider_id` | job `failed` |
| `model` пуст | `default_model` провайдера; оба пусты → `failed` |
| Реестр активен, нет `provider_id` | `failed` |
| Только compat `default`, legacy payload без `provider_id` | принять как `default` (один релиз мягкой миграции) |
| Нет реестра и нет `LLM_BASE_URL` | stub markdown (как сейчас) |
| HTTP/пустой ответ LLM | job `failed`, не stub |
| `refine` | без изменений |

OpenAPI: path + schemas. `meetingraft-sync`: `list_models()`.

## App / UniFFI

- Settings Backend: picker вместо free-text Model id; Refresh; disabled Generate при пустом каталоге
- `ProviderSettingsStore`: `llmProviderId` + `llmModelId`
- Local Ollama / OpenAI-compat: TextField model + URL без изменений
- `SyncClient::list_models` → DTO `{ provider_id, model, display_name }`
- `MeetingCore.list_backend_llm_models()`; `set_llm_config(..., provider_id, ...)`
- Backend `generate_artifact` кладёт оба поля в payload
- Banner: `backend · {provider_id}/{model}`

## Errors (summary)

| Ситуация | Поведение |
|----------|-----------|
| Bad registry | fail-fast startup |
| Models list empty / unreachable refresh | UI message; session cache списка при network fail |
| Selection исчезла после refresh | сбросить; Generate disabled |
| Test API / refine / local LLM | без регрессии |

## Testing

- Backend pytest: registry load, compat `default`, fail-fast duplicates, models API без секретов, job routing, unknown provider failed
- Sync/FFI: `list_models`; generate payload includes `provider_id` + `model`
- Swift: store fields; Backend path uses picker selection

## Docs

- `shared/openapi.yaml`
- `docs/architecture-and-install.md` §2.5 — registry + compat
- `docs/backlog.md` — registry **partial** (static JSON + models API); CRUD/billing/discovery deferred
- `docs/roadmap.md` Remaining — отметить partial registry

## Success criteria

- [ ] Multi-provider JSON → модели в Settings picker
- [ ] Brief/Follow-up бьют в `base_url` выбранного `provider_id`
- [ ] Без реестра `LLM_*` работает как `default`
- [ ] Ключи не в `/v1/models` и не в app
- [ ] Без LLM-конфига stub/refine без регрессии

## Out of scope follow-ups

CRUD providers API/UI, billing, live discovery, WhisperX, diarization.
