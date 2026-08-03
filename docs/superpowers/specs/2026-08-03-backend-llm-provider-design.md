# Phase 6 follow-up — Backend LLM provider (jobs)

**Date:** 2026-08-03
**Status:** Approved for implementation (approach A + A1)
**Maps to:** Epic 8 (Real LLM), ADR-007 jobs, Settings LLM=Backend
**Depends on:** backend stub jobs; `generate_artifact` backend path; `brief_prompts` / `follow_up_prompts`

## Goal

При **LLM = Backend** Brief/Follow-up идут через FastAPI jobs, которые вызывают
**один** OpenAI-compatible провайдер, настроенный **только на backend**
(`LLM_BASE_URL`, `LLM_API_KEY`, default `LLM_MODEL`).

Во фронте пользователь выбирает **model id**; URL и API key провайдера во
фронте не хранятся.

Language-aware промпты остаются в Rust (`postcall::prompts`) и передаются в
job `payload` как готовые `system` + `user` — без дублирования текста в Python.

## Non-goals (этот слайс)

- Реестр нескольких провайдеров / UI «добавить провайдера»
- Тарификация / billing
- `GET /v1/models` с remote discovery
- Remote STT / STT model picker UI (кроме уже существующего local Whisper)
- Тяжёлая модель на весь сырой аудиоролик (post-call audio analysis)
- Keychain; streaming; silent fallback на stub markdown при ошибке LLM
- Удаление локальных engines `ollama` / `openai_compat` из app
- `llmApiKey` во фронте (отклонено в пользу backend env)

## Product direction (backlog, не этот PR)

Зафиксировано для `docs/backlog.md`:

- Backend: реестр провайдеров (API URL, keys, лимиты), тарификация
- App: выбор модели LLM из списка backend; выбор модели STT (local ggml)
- Опционально remote STT API (latency risk — не default для live)
- Более жирная модель для глубокого анализа полного аудио / refined transcript

## Approach

**A** — один env-провайдер на процесс backend.
**A1** — промпты строит Rust; backend = completion proxy.

Отклонено: полный registry (B); model только из env без фронта (C); порт
промптов в Python (A2); общий `shared/` prompt pack (A3) в этом слайсе.

## Architecture

```text
Settings: llmEngine=backend, llmModelId=<user choice>
  → applyProviderConfig / setLlmConfig / setApiConfig
MeetingDetail Generate Brief|Follow-up
  → MeetingCore.generate_artifact
       engine backend:
         brief_prompts|follow_up_prompts(final, primary_lang)
         CreateJobRequest {
           kind: brief|follow_up,
           primary_language, allowed_languages,
           payload: { model, system, user }
         }
         wait_for_job_artifact → insert Artifact (template_id backend.*)

Backend POST /v1/jobs
  if kind in {brief, follow_up} and LLM_BASE_URL set:
    POST {LLM_BASE_URL}/v1/chat/completions
      Authorization: Bearer {LLM_API_KEY}  (если key непустой)
      model: payload.model || LLM_MODEL
      messages: [system, user]
    → artifact body = assistant content
  else:
    existing in-memory stub (refine / unconfigured LLM)
```

Local `ollama` / `openai_compat` paths unchanged (прямые HTTP из app).

## Backend config

| Env | Meaning | Example |
|-----|---------|---------|
| `LLM_BASE_URL` | Base **без** `/v1` | `http://93.189.243.223:58001` |
| `LLM_API_KEY` | Bearer для провайдера; пусто = без заголовка | `LOCAL-API-KEY` |
| `LLM_MODEL` | Default model, если `payload.model` пуст | `Google/gemma-4-12b-it` |
| `MEETINGRAFT_API_TOKEN` | Auth к MeetingRaft jobs API (как сейчас) | `dev-token` |

`docker-compose.yml` / local `uv` должны позволять пробросить эти переменные.
Документация: `docs/architecture-and-install.md` §2.5.

## Job payload contract

Для `kind: brief | follow_up` при LLM-пути:

```json
{
  "model": "Google/gemma-4-12b-it",
  "system": "<from brief_prompts|follow_up_prompts>",
  "user": "<from brief_prompts|follow_up_prompts>"
}
```

Правила:

- Пустой / отсутствующий `model` → `LLM_MODEL`; если и он пуст → job `failed`
- Пустые `system` или `user` → job `failed` (не stub)
- HTTP/empty LLM response → job `failed`, `error` заполнен; **не** подменять stub markdown
- `refine` и отсутствие `LLM_BASE_URL` → прежнее stub-поведение (без регрессии Test API / refine UI)

## UniFFI / Swift

- При `engine == "backend"` в `generate_artifact`: собрать prompts, положить
  `payload` в `CreateJobRequest` (сейчас `payload: None` — изменить).
- `llmModelId` из Settings уже есть; при Backend показывать поле Model
  (не требовать `needsUrl` для Backend).
- Backend API section по-прежнему только `apiBaseUrl` + `apiToken` (MeetingRaft),
  не LLM provider secrets.

## Testing

- Backend pytest: mock OpenAI-compat HTTP; assert Bearer; assert messages;
  assert failed job on LLM error (no stub body).
- FFI / sync: backend generate path sends payload with system/user/model;
  existing stub tests still pass when LLM env unset.
- Docs smoke: compose with `LLM_*` → Settings LLM=Backend + model → Generate Brief.

## Success criteria

- [ ] С `LLM_*` на backend Generate Brief/Follow-up возвращает текст модели, не stub `# Stub brief`
- [ ] Model id с фронта попадает в request `model`
- [ ] Промпты содержат language code / transcript (те же helpers, что local LLM)
- [ ] Без `LLM_BASE_URL` stub jobs и refine UI без регрессии
- [ ] Backlog обновлён пунктами registry / billing / STT picker / remote STT / full-audio model

## Out of scope follow-ups

См. Non-goals и Product direction выше.
