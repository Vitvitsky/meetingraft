# Phase 6 / Epic 9 — Speakers skeleton (manual labels)

**Date:** 2026-08-03  
**Status:** Approved for implementation  
**Maps to:** Epic 9 (partial), ADR-002 (no live speaker attribution), ADR-006 (`speakers` table)  
**Depends on:** Meetings detail UI (Live | Final | Artifacts)

## Goal

Meeting-scoped speaker entities with CRUD and a **Speakers** tab in Meetings
detail — manual labels only. No diarization, no assignment onto Final
segments.

## Non-goals

- pyannote / WhisperX / auto speaker detection
- Binding speakers to Final paragraphs or caption events
- Versioned refined transcript / live vs final compare UI
- Global speakers across meetings
- Voice enrollment

## Approach

**Meeting-scoped speakers + Speakers tab** (chosen). Assignment to Final
segments deferred until Final is segmented.

## Domain

```text
Speaker {
  id: String
  meeting_id: String
  display_name: String
  sort_index: i64   // or u32; order in list
}
```

Default display name on Add: `Спикер {n}` when session primary is `ru`,
else `Speaker {n}` (`n` = count+1 for that meeting).

## Persistence (ADR-006)

In `AudioManifestStore` bootstrap (`CREATE TABLE IF NOT EXISTS`):

```sql
CREATE TABLE IF NOT EXISTS speakers (
  id TEXT PRIMARY KEY NOT NULL,
  meeting_id TEXT NOT NULL,
  display_name TEXT NOT NULL,
  sort_index INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_speakers_meeting
  ON speakers(meeting_id, sort_index);
```

API:

- `list_speakers(meeting_id) -> Vec<Speaker>` ordered by `sort_index`, then `id`
- `upsert_speaker(speaker) -> Result`
- `delete_speaker(id) -> Result`

Cascade on meeting delete: YAGNI unless an existing delete-meeting path
exists; orphans acceptable for MVP.

## UniFFI (`MeetingCore`)

| Method | Behavior |
|--------|----------|
| `list_speakers(meeting_id) -> Vec<FfiSpeaker>` | From store |
| `upsert_speaker(meeting_id, id, display_name, sort_index) -> String` | Empty id → new UUID; empty error = ok |
| `delete_speaker(id) -> String` | Empty = ok |

`FfiSpeaker`: `id`, `meetingId`, `displayName`, `sortIndex`.

## Swift UI

- `MeetingDetailSection.speakers` — title «Speakers»
- Banner: «Ручные метки · diarization — скоро»
- List of speakers; Add; Rename (inline or sheet); Delete
- `MeetingsViewModel`: load/upsert/delete; extend `MeetingsCoreProviding`
- Views: presentation only; no store access

## Architecture boundaries

- No Cocoa types in Rust contracts
- Live captions remain without speaker attribution (ADR-002 / AGENTS.md)
- Backend jobs unchanged

## Testing

- Storage CRUD + meeting isolation
- ViewModel spy: reload / upsert / delete / error
- Optional thin FFI test via temp data root

## Docs

- This spec
- Plan: `docs/superpowers/plans/2026-08-03-speakers-skeleton.md`
- `docs/backlog.md` Epic 9 — entities + correction screen marked partial/done for skeleton
- `docs/roadmap.md` — speakers skeleton done; diarization remaining
- `docs/architecture-and-install.md` — Speakers tab

## Done criteria

- [ ] Add / Rename / Delete speaker for a meeting; survives app reopen
- [ ] Live captions unchanged (no speaker labels)
- [ ] `cargo test` + `xcodebuild test` + swiftformat clean
- [ ] No pyannote / Final assignment
