# Phase 5 — Glossary MVP Design

**Date:** 2026-08-02
**Status:** Draft for implementation planning
**Maps to:** Roadmap Phase 5, Epic 7 (subset), ADR-003 / ADR-005 / ADR-006

## Goal

Glossary terms demonstrably affect live caption text (normalize always; Whisper `initial_prompt` when available), normalization is unit-tested, CSV import round-trips, and macOS has a Glossary screen with CRUD + import.

## Non-goals (this phase)

- Workspace / project scopes (enum may exist; UI and persistence only **global** + **meeting**).
- Glossary candidate mining / approval queue from transcript corrections.
- Voice enrollment, speaker diarization, speaker memory (Phase 6 / Epic 9; ADR-007 pyannote).
- Offline Whisper LoRA/QLoRA training (future ADR; data flywheel may record corrections later, not in this MVP).
- System audio tap completion (ADR-004 follow-up).
- Silero VAD / beam-search STT quality work (separate from glossary).

## Decisions locked

| Topic | Choice |
|-------|--------|
| Scope of PR | MVP under Phase 5 exit criteria |
| Caption influence | **Both:** post-STT normalize (Mock + Whisper) + Whisper `initial_prompt` when engine is Whisper |
| UI | Sidebar **Glossary**: list + add/edit + delete + CSV import |
| Architecture | New crate `meetingraft-glossary`; SQLite in `meetingraft-storage`; UniFFI via `MeetingCore` |
| Live STT speakers | Unchanged — no speaker attribution in live |

## Architecture

```
SwiftUI GlossaryView / ViewModel
        │ UniFFI DTOs only
        ▼
MeetingCore (ffi)
        ├─ CRUD / importGlossaryCsv
        └─ start_recording → load active glossary → LiveCaptionPipeline
                │
                ├─ SttEngine (Mock | Whisper)
                │      └─ Whisper: set_initial_prompt(glossary.build_whisper_prompt())
                └─ on each CaptionEvent: glossary.normalize_caption(text) → persist + drain
        ▲
meetingraft-glossary (normalize, CSV parse, prompt builder)
        ▲
meetingraft-storage (glossary_terms table)
```

Boundaries (AGENTS.md):

- No networking or glossary rules in SwiftUI.
- No Cocoa/AVFoundation in Rust contracts.
- Prefer small DTOs and explicit enums across UniFFI.

## Domain model

```text
GlossaryTerm {
  id: String          // uuid
  surface: String     // what ASR / users write wrongly (match key)
  canonical: String   // replacement / prompt token
  language: SpeechLanguage  // ru | en | es (default ru)
  scope: GlossaryScope      // Global | Meeting { meeting_id: String }
}
```

**Matching (normalize):**

- Case-insensitive whole-token / phrase match on `surface` (Unicode-aware simple split + multi-word surfaces).
- Longer `surface` wins when overlapping.
- Replace matched spans with `canonical` (preserve surrounding punctuation where trivial).
- Apply to both Partial and Final events after STT, before SQLite caption persist and before queue for Swift.

**Active set for a live session `session_id`:**

- All `scope = Global` terms, plus
- All `scope = Meeting { meeting_id }` where `meeting_id == session_id`.

If recording has not started, UI CRUD for meeting-scoped terms uses an explicit meeting id only when a live session is active; otherwise new terms default to **Global** (meeting-scoped create disabled or labeled “available when Live”).

## Persistence (ADR-006)

Table `glossary_terms` in existing `meetingraft.sqlite3`:

| Column | Type | Notes |
|--------|------|--------|
| id | TEXT PK | |
| surface | TEXT NOT NULL | |
| canonical | TEXT NOT NULL | |
| language | TEXT NOT NULL | `ru` / `en` / `es` |
| scope | TEXT NOT NULL | `global` / `meeting` |
| meeting_id | TEXT NULL | required iff scope=meeting |
| updated_at_ms | INTEGER NOT NULL | |

Unique index: `(surface, language, scope, COALESCE(meeting_id, ''))` to avoid duplicates.

Storage API (used by glossary engine / ffi): `list_terms`, `upsert_term`, `delete_term`, `replace_from_import` (transaction).

## CSV import

- Columns (header required): `surface,canonical,language,scope`
  Optional: `meeting_id` (required when `scope=meeting`).
- `language` default `ru` if empty; `scope` default `global`.
- Round-trip: export format = same columns (export can be Phase 5.1; **import** is required for exit criteria; tests use fixture string in Rust).
- Invalid rows skipped with count; UniFFI returns `{ imported: u32, skipped: u32, error: String }` (empty error = ok).

## Whisper prompt

- `build_whisper_prompt(max_chars: ~800)`: space-separated unique `canonical` values, ru-first then en/es, truncated.
- Set once when starting recording if backend is Whisper; Mock ignores prompt but still normalizes.

## UniFFI surface (additive on `MeetingCore`)

- `listGlossaryTerms() -> [FfiGlossaryTerm]`
- `upsertGlossaryTerm(term: FfiGlossaryTerm) -> String` (empty = ok)
- `deleteGlossaryTerm(id: String) -> String`
- `importGlossaryCsv(csvUtf8: String) -> FfiGlossaryImportResult`
- Existing live path unchanged except normalize + optional prompt wiring inside Rust.

## Swift UI

- New `AppDestination.glossary`.
- `GlossaryView` + `GlossaryViewModel`: load list, sheet for add/edit, delete, fileImporter for CSV → read UTF-8 → `importGlossaryCsv`.
- Settings: one-line hint “Glossary in sidebar” (optional); no second CRUD.

## Testing

| Layer | Cases |
|-------|--------|
| glossary crate | normalize: `униффи`→`UniFFI`; no false positive on substrings of unrelated words; longer surface wins; empty glossary identity |
| glossary crate | CSV import fixture round-trip count |
| storage | upsert/list/delete glossary_terms isolation (unique tmp root) |
| ffi | start_recording + mock loud speech → drained caption text contains canonical after term upsert |
| Swift | smoke optional: list after upsert via UniFFI (if existing smoke pattern) |

## Future hooks (document only)

- **Corrections corpus:** later store `(hypothesis, correction)` for fine-tune / glossary candidates — not in this MVP.
- **LoRA/QLoRA:** train offline in Python; merge → ggml; swap model file — not whisper-rs runtime adapters.
- **Voice memory:** Phase 6+ speakers + diarization; enrollment ADR later.

## Exit criteria checklist

- [x] Terms change caption output via normalize (proven in `cargo test` on Mock path).
- [x] Normalization unit tests green.
- [x] CSV import of a real fixture succeeds and lists terms.
- [x] Glossary sidebar CRUD works against UniFFI.
- [x] Docs: roadmap Phase 5 status; backlog Epic 7 partial checkmarks.
