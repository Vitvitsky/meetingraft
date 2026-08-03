# Phase 6 / Epic 9 — Versioned Final transcript + Live vs Final Compare

**Date:** 2026-08-03
**Status:** Approved for implementation (approach 1)
**Maps to:** Epic 9 (versioned refined transcript, compare live vs final), ADR-002
**Depends on:** local Final assemble (Stop Live), Meetings detail Live | Final | Artifacts | Speakers

## Goal

Каждый Stop Live / `assemble_final_now` создаёт **новую** версию Final
(`version = max+1`), не перезаписывая историю. В Meetings: picker версий на
вкладке Final и секция **Compare** — side-by-side Live finals | Final vN.
Brief / Follow-up / Export всегда используют **latest**.

## Decisions

| Тема | Выбор |
|------|--------|
| Scope | Локальные версии + Compare; без backend refine / speakers / line-diff |
| Compare UI | Новая секция, два столбца, picker версии |
| Brief/Export | Всегда latest (`max version`) |

## Non-goals

- Backend refine → новая «refined» версия
- Line-diff / highlighted Myers diff
- Active-flag версии (отдельно от latest)
- Speaker binding к сегментам Final
- Diarization / WhisperX
- Изменение контракта export имён файлов под version (latest only)

## Approach

**1 (approved)** — next version on assemble; `list` / `get_by_version`; Final
picker + Compare tab.

Отклонено: отдельная snapshots-таблица (2); diff-engine Compare (3).

## Domain & persistence

Таблица уже есть:

```sql
PRIMARY KEY (meeting_id, version)
```

Правила:

- Assemble → `version = next_final_version(meeting_id)` где next = `max+1` или `1`
- Не overwrite при обычном assemble (явный upsert с version остаётся для тестов)
- `get_final_transcript(meeting_id)` → latest (`ORDER BY version DESC LIMIT 1`)
- `list_final_transcripts(meeting_id)` → все версии, **DESC** (новые сверху)
- `get_final_transcript_version(meeting_id, version)` → одна или empty

`assemble_final` получает явный `version: u32` (вычисляется снаружи).

Существующие БД с единственной `v1` валидны без миграции схемы.

## Architecture

```text
Stop Live / assemble_final_now
  → list captions → next_version → assemble_final(..., version) → upsert

Brief / Follow-up / Export / has_final
  → get_final_transcript (latest)

Final tab
  → list_final_transcripts + picker → show selected body
  → default selection = latest

Compare tab
  → left: Live finals text (captions phase=Final joined \n\n, raw store)
  → right: Final vN (picker, shared or same selected version as Final tab)
```

## UniFFI

| Method | Behavior |
|--------|----------|
| `list_final_transcripts(meeting_id)` | `Vec<FfiFinalTranscript>` DESC |
| `get_final_transcript(meeting_id)` | latest (unchanged semantics) |
| `get_final_transcript_version(meeting_id, version)` | one version or empty record |
| `assemble_final_now` | writes **next** version |

`FfiFinalTranscript` unchanged: `meetingId`, `version`, `bodyMarkdown`, `createdAtMs`.

## Swift UI

- `MeetingDetailSection.compare` — title «Compare»
- Final: provenance banner + version picker (`v{N} · formatted date`) + body
- Compare: `HSplitView` Live | Final; picker on Final side; empty states
- VM: `finalVersions`, `selectedFinalVersion`, load on meeting select; Brief/Export
  keep using `finalTranscript` = latest (reload after assemble)

Live column text: join `listCaptions` where final phase with `\n\n` (не
пере-normalize glossary — честный live vs assembled Final).

## Errors / edge cases

| Ситуация | Поведение |
|----------|-----------|
| Два assemble подряд | v1, v2 сохранены |
| Пустые finals | новая версия с пустым body |
| Нет Final | Final/Compare empty states |
| Неизвестный version | empty FFI record |
| Picker на старой версии | просмотр only; Brief/Export = latest |

## Testing

- Storage: next version, list DESC, get by version, latest unchanged
- FFI/assemble: second assemble → version 2; list length 2
- Swift: selection + Compare section wiring (VM / store tests)

## Docs

- `docs/backlog.md` Epic 9 — versioned Final + Compare done; diarization deferred
- `docs/roadmap.md` Remaining — compare/versioned done; diarization remains
- `docs/architecture-and-install.md` — short note on Final versions / Compare

## Success criteria

- [ ] Stop Live / re-assemble создаёт новую version
- [ ] Final picker показывает историю; Brief/Export = latest
- [ ] Compare: Live \| Final vN side-by-side
- [ ] Нет diarization / backend refine versioning / line-diff

## Out of scope follow-ups

Diarization, speaker→segments, backend refined versions, line-diff, per-version export filenames.
