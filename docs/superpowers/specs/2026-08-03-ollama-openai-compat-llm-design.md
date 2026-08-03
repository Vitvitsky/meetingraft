# Phase 6 follow-up — Local LLM: Ollama native + OpenAI-compat

**Date:** 2026-08-03
**Status:** Approved for implementation
**Maps to:** Epic 8 (Real LLM), Settings Providers, `LlmClient` trait
**Depends on:** `setLlmConfig` / `generate_artifact` builtin+backend paths; `applyProviderConfig`

## Goal

Два **локальных** LLM engine в Settings для Generate Brief/Follow-up:

1. **Ollama native** — `POST {base}/api/chat`
2. **OpenAI-compat** — `POST {base}/v1/chat/completions`

Чтобы сравнивать производительность и качество при смене моделей.
`builtin_templates` и `backend` (ADR-007 jobs) без регрессии.

## Non-goals

- FastAPI worker, вызывающий Ollama
- Streaming / tools / multi-turn chat UI
- Авто-выбор «лучшего» engine; метрики latency в БД
- Keychain; silent fallback на templates при ошибке LLM
- Off-MainActor рефактор blocking HTTP (follow-up)

## Approach

Два отдельных HTTP-клиента, общий trait `LlmClient::complete(system, user)`.
Один shared `llmBaseUrl` + `llmModelId` в Settings; engine switch = A/B.

Отклонено: только OpenAI-compat; только native; LLM только через backend jobs.

## Architecture

```text
Settings: llmEngine = ollama | openai_compat
  → setLlmConfig(engine, modelId, baseUrl)
MeetingDetail Generate Brief/Follow-up
  → applyProviderConfig (incl. llmBaseUrl)
  → MeetingCore.generate_artifact
       builtin → templates
       backend → jobs poll
       ollama → OllamaNativeClient (/api/chat)
       openai_compat → OpenAiCompatLlmClient (/v1/chat/completions)
  → insert local Artifact (template_id ollama.* | openai.*)
```

## postcall crate

### `LlmError`

```text
NotConfigured
Http { status, body }
EmptyResponse
Transport(String)
```

### Clients

| Type | Endpoint | Non-stream request | Parse |
|------|----------|--------------------|-------|
| `OllamaNativeClient` | `{base}/api/chat` | `model`, `stream: false`, `messages[{role,content}]` | `message.content` |
| `OpenAiCompatLlmClient` | `{base}/v1/chat/completions` | `model`, `messages` | `choices[0].message.content` |

- `reqwest` blocking; timeout **60s**
- Empty `base_url` or `model` → `NotConfigured` before HTTP
- System + user messages: system first, then user (оба API)

### Prompts

Shared helpers (e.g. `brief_prompts(final_md, primary_lang)`, `follow_up_prompts(...)`):

- **system:** meeting assistant; answer in session primary language; markdown
- **user Brief:** Final body + ask for summary / key points / next steps
- **user Follow-up:** Final body + draft follow-up email

Do not paste builtin heuristic template text into prompts.

### Save

| Engine | Brief `template_id` | Follow-up |
|--------|---------------------|-----------|
| ollama | `ollama.brief` | `ollama.follow_up` |
| openai_compat | `openai.brief` | `openai.follow_up` |

## UniFFI

### `set_llm_config(engine_code, model_id, base_url)`

Breaking vs current 2-arg API → regenerate FFI.

`normalize_llm_engine`:

| code | stored |
|------|--------|
| `ollama` | `ollama` |
| `openai_compat` | `openai_compat` |
| `backend` | `backend` |
| else | `builtin_templates` |

Inner fields: `llm_engine`, `llm_model_id`, `llm_base_url` (default `""`).

### `generate_artifact`

1. Load Final; missing → error
2. Match engine:
   - `backend` → existing jobs path
   - `ollama` / `openai_compat` → build client, `complete`, on `Err` return error (no insert, no template fallback)
   - else → existing `render_brief` / `render_follow_up`
3. Insert artifact with engine-specific `template_id`

Release mutex during HTTP (clone config, drop guard) — same as backend job path.

## Swift

- `LlmEngine.ollama` and `.openaiCompat`: `isAvailable = true`; `needsUrl = true`
- Settings: show URL + model when `needsUrl`; update caption (оба не «скоро»)
- `MeetingsCoreProviding.setLlmConfig(engineCode:modelId:baseUrl:)`
- `applyProviderConfig` passes `providerStore.llmBaseUrl`
- Meetings Generate unchanged aside from config wiring

## Testing

**postcall (mockito):**
- Ollama success parse; OpenAI-compat success parse
- HTTP error → `Http`
- Empty content → `EmptyResponse`
- Empty base/model → `NotConfigured`

**ffi:**
- ollama mock → `template_id == ollama.brief`, body non-empty
- LLM error → no artifact inserted
- builtin + backend regression (existing tests)

**Swift:**
- both engines available/selectable
- `applyProviderConfig` records `baseUrl` on spy

## Docs

- This spec
- Plan: `docs/superpowers/plans/2026-08-03-ollama-openai-compat-llm.md`
- `docs/backlog.md` — Real LLM partial: ollama + openai_compat
- `docs/roadmap.md` — Remaining update
- `docs/architecture-and-install.md` — Settings LLM engines
- Providers design: enable ollama + openai_compat

## Done criteria

- [ ] Settings → Ollama or OpenAI-compat → Generate Brief against local server
- [ ] Builtin + backend paths unchanged in behavior/tests
- [ ] No silent fallback to templates on LLM error
- [ ] `cargo test` + `xcodebuild test` + swiftformat + pre-commit green
