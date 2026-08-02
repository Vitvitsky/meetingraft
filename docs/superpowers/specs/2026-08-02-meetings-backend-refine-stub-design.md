# Phase 6 follow-up — Meetings ↔ backend refine stub UI

**Date:** 2026-08-02
**Status:** Approved for implementation
**Maps to:** ADR-007, Phase 6 follow-up (after slice A API stub), Epic 8
**Depends on:** `2026-08-02-phase-6-backend-stub-design.md` (OpenAPI + sync + Settings Test API)

## Goal

Закрыть e2e UX-петлю: Meetings → Artifacts → submit stub `refine` job → poll →
показать dummy markdown рядом с local Brief/Follow-up, используя уже
существующие UniFFI методы без новых API/Rust контрактов.

## Non-goals

- Persist `job_id` / история jobs после ухода с detail
- Замена local Brief/Follow-up на backend generation
- Реальные workers (WhisperX, diarization, LLM)
- Изменения OpenAPI / `meetingraft-sync` DTO (кроме случая бага)
- Keychain для API token

## Approach

**Session-only UI** (выбран): состояние backend job живёт в `MeetingsViewModel`
на время экрана detail; local `FfiArtifact` list не смешивается с stub
артефактом.

Отклонённые: persist job id в SQLite/UserDefaults; вливание stub body в
`listArtifacts`.

## Architecture

```text
MeetingDetailView (Artifacts)
  → MeetingsViewModel.submitBackendRefine(meetingId)
      → MeetingsCoreProviding.submitBackendJob(kind: "refine")
      → poll getBackendJob (MainActor Task)
      → getBackendArtifact(first artifact_id)
  → панель «Backend refine (stub)» (status + markdown + Copy)
```

Границы AGENTS.md: networking остаётся в Rust sync client; Swift — presentation
+ orchestration poll; views без бизнес-правил.

### Protocol

Расширить `MeetingsCoreProviding` (для spy/тестов):

- `submitBackendJob(meetingId:kindCode:) -> FfiBackendJob`
- `getBackendJob(jobId:) -> FfiBackendJob`
- `getBackendArtifact(artifactId:) -> FfiBackendArtifact`

`MeetingCore` уже реализует эти методы; добавить в protocol + extension.

### ViewModel state

| Field | Role |
|-------|------|
| `backendJobStatus` | `idle` / `submitting` / `polling` / `succeeded` / `failed` (Swift enum или string codes) |
| `backendJobId` | last job id (empty if none) |
| `backendArtifactMarkdown` | body for display |
| `backendRefineTask` | cancellable `Task` (private) |

`reload(meetingId:)` **не** сбрасывает backend session state (чтобы poll не
терялся при incidental reload); уход с detail / новый meeting selection —
cancel Task и reset backend fields.

## UI

Вкладка Artifacts:

1. Provenance banner как сейчас + caption у stub-блока:
   «Stub job `refine` · не заменяет local Brief».
2. Кнопки: Generate Brief / Follow-up + **Submit refine (stub)**.
3. Submit disabled если: нет Final, или status ∈ {submitting, polling}.
4. Под `HSplitView` local artifacts — секция Backend refine:
   статус, job id (caption), ScrollView markdown, Copy.
5. Ошибки — существующий alert через `errorMessage`.

Пустой `apiBaseUrl` (если доступен через store/core): сообщение в духе
«настройте Backend URL в Settings» без вызова submit. Практично: полагаться
на ошибку FFI от sync client, если URL пустой уже даёт понятный error —
допустимо без дублирования ProviderSettingsStore в ViewModel.

## Poll policy

1. `submitBackendJob(meetingId, "refine")`.
2. Если `error` непустой → `failed` + `errorMessage`.
3. Если `status == "succeeded"` и `artifact_ids` непустой → сразу
   `getBackendArtifact`.
4. Иначе до **20** попыток `getBackendJob` с паузой **250 ms**.
5. На `failed` / timeout / succeeded без artifacts → `errorMessage`.
6. Успех fetch: `backendArtifactMarkdown`, status `succeeded`.

HTTP блокирующий в Rust; poll только в Swift Task на MainActor.

## Testing

`MeetingsViewModelTests` + spy:

1. Happy path: immediate succeeded + artifact → markdown, local artifacts
   unchanged.
2. Submit FFI error → `errorMessage`, no artifact fetch.
3. Missing Final → no core submit (guard in ViewModel).
4. Poll never succeeds → timeout → `errorMessage`.

## Docs touchpoints

- Этот spec
- Plan: `docs/superpowers/plans/2026-08-02-meetings-backend-refine-stub.md`
- `docs/backlog.md` — отметить e2e Meetings↔stub UI
- `docs/architecture-and-install.md` — кнопка + poll
- `docs/roadmap.md` — Phase 6 follow-up: Meetings poll UI

## Done criteria

- [ ] Submit refine против `docker compose` + Settings URL показывает stub markdown
- [ ] Local Brief/Follow-up без регрессий
- [ ] Unit-тесты ViewModel зелёные
- [ ] `swiftformat Sources Tests --lint` чистый
- [ ] Docs обновлены
