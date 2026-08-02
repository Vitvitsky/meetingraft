# Phase 6 MVP — Local Post-call Design

**Date:** 2026-08-02  
**Status:** Draft for implementation planning  
**Maps to:** Roadmap Phase 6 (subset), Epics 8 (partial), ADR-002 / ADR-003 / ADR-006  
**Explicitly deferred:** ADR-007 backend (next PR); speakers/diarization (Epic 9); live LLM generation

## Goal

On Stop Live, assemble a **FinalTranscript** from live final captions (glossary-normalized), persist it, show Meetings UI with Live vs Final, and generate two local markdown artifacts (**Brief**, **Follow-up email**) via built-in templates — without a network backend.

## Non-goals (this PR)

- FastAPI / Dramatiq / Redis / OpenAPI sync client (ADR-007) — **next PR**.
- WhisperX re-decode of PCM, pyannote diarization, speaker assignment UI.
- User-defined markdown templates (built-ins only).
- Calling Ollama / LM Studio / Gemma in this PR (interface reserved only).
- Live caption pipeline changes beyond hooking assemble on stop.

## Decisions locked

| Topic | Choice |
|-------|--------|
| Scope | Local-first MVP (option A); backend next PR |
| Final transcript | Stitch live `caption_events` with `phase=final`, glossary-normalized |
| Templates | Brief + Follow-up email (heuristic markdown, no LLM in MVP) |
| Architecture | Rust domain + `meetingraft-postcall` + SQLite + UniFFI; Swift presentation |
| LLM later | Swappable `LlmClient` trait; adapters for Ollama / LM Studio / OpenAI-compatible local Gemma |

## Architecture

```
Stop Live / stopRecording
        │
        ▼
MeetingCore (ffi)
  flush STT → persist captions
  assemble_final(captions + glossary) → final_transcripts
        │
        ▼
meetingraft-postcall
  assemble_final, render_template(brief|follow_up)
        │
        ▼
storage (SQLite): sessions, caption_events, final_transcripts, artifacts

Swift: Meetings list → detail (Live | Final | Artifacts)
```

## Domain

```text
FinalTranscript {
  meeting_id: String
  version: u32              // MVP: always 1 on first assemble; regenerate bumps later optional
  body_markdown: String     // ordered finals joined by blank lines
  created_at_ms: u64
}

ArtifactKind { Brief, FollowUp }

Artifact {
  id: String
  meeting_id: String
  kind: ArtifactKind
  template_id: String       // "builtin.brief" | "builtin.follow_up"
  body_markdown: String
  created_at_ms: u64
}

MeetingSummary {
  id: String
  started_at_ms: u64
  has_final: bool
  artifact_count: u64
}
```

**Assemble rules:**

1. Load caption_events for `meeting_id` ordered by `created_at_ms`.
2. Keep `phase == final` only (partials discarded for final artifact).
3. Run glossary `normalize_caption` on each line (active global + this meeting).
4. Join with `\n\n` into `body_markdown`.
5. Upsert `final_transcripts` version 1 (if already exists, replace body and bump `created_at_ms` — simple overwrite for MVP).

## Templates (heuristic, no LLM)

**`builtin.brief`:** markdown with headings:

- `# Brief`
- `## Summary` — first N chars / first paragraph of final
- `## Key points` — bullet per final paragraph (trimmed)
- `## Next steps` — lines matching simple RU/EN cues (`нужно`, `сделать`, `action`, `TODO`) or placeholder «—»

**`builtin.follow_up`:**

- Subject line comment `<!-- subject: Итоги встречи {date} -->`
- Greeting, short summary, bullet key points, closing ask for corrections
- Language: follow session primary (default `ru` copy)

**Future LLM path (not implemented):**

```text
trait LlmClient {
  fn complete(&self, system: &str, user: &str) -> Result<String, LlmError>;
}
// Adapters (later): OllamaHttp, LmStudioHttp, OpenAiCompatible(base_url for Gemma)
```

Template engine may call LLM when configured; MVP uses pure functions only.

## Persistence (ADR-006)

```sql
CREATE TABLE IF NOT EXISTS final_transcripts (
  meeting_id TEXT NOT NULL,
  version INTEGER NOT NULL,
  body_markdown TEXT NOT NULL,
  created_at_ms INTEGER NOT NULL,
  PRIMARY KEY (meeting_id, version)
);

CREATE TABLE IF NOT EXISTS artifacts (
  id TEXT PRIMARY KEY NOT NULL,
  meeting_id TEXT NOT NULL,
  kind TEXT NOT NULL,           -- brief | follow_up
  template_id TEXT NOT NULL,
  body_markdown TEXT NOT NULL,
  created_at_ms INTEGER NOT NULL
);
```

Meetings list: `SELECT` from existing `sessions` left-join finals/artifacts counts.

## UniFFI (`MeetingCore`)

- `listMeetings() -> [FfiMeetingSummary]`
- `getFinalTranscript(meetingId) -> FfiFinalTranscript?` (empty fields if none)
- `listArtifacts(meetingId) -> [FfiArtifact]`
- `generateArtifact(meetingId, kind) -> FfiArtifact` (or error string + empty)
- `stopRecording` / Stop Live path: after caption flush, call assemble_final internally
- Optional: `assembleFinalNow(meetingId)` for regenerate from UI

No sync / HTTP methods in this PR.

## Swift UI

- Replace Meetings stub: `MeetingsListView` + `MeetingDetailView`.
- Detail segmented control: **Live captions** (read-only list from stored caption_events via new `listCaptions(meetingId)` or reuse existing count + list API) | **Final** | **Artifacts**.
- Artifacts tab: Generate Brief / Generate Follow-up; list; markdown Text + Copy.
- Share `MeetingCore` / data root pattern with Live + Glossary.

If `listCaptions` missing: add storage `list_captions(session_id)` + UniFFI (needed for Live tab in review).

## Testing

| Layer | Cases |
|-------|--------|
| postcall | assemble joins finals only; glossary applied; empty captions → empty/error |
| postcall | brief + follow_up contain expected headings / non-empty for fixture text |
| storage | upsert final + list artifacts |
| ffi | stop_recording creates final; generateArtifact returns markdown |
| Swift | smoke optional list meetings after recording |

## Exit criteria (this MVP PR)

- [ ] Stop Live produces persisted FinalTranscript
- [ ] Meetings UI shows Live vs Final
- [ ] Brief and Follow-up artifacts generate and display
- [ ] Unit tests for assemble + templates
- [ ] Docs: roadmap Phase 6 in progress; backlog Epic 8 partial; note LLM/backends deferred

## Follow-ups (document only)

1. **Backend PR:** ADR-007 FastAPI + OpenAPI + Rust sync + job poll; optional remote refinement replacing local assemble.
2. **Local LLM PR:** `LlmClient` + Ollama/LM Studio/Gemma OpenAI-compatible endpoint; settings for base URL.
3. **Speakers / diarization** (Epic 9).
4. User-defined templates.
