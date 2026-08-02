# ADR-006: Local persistence — SQLite via rusqlite behind the store facade

## Status
Accepted

## Context
The Rust core exposes "local persistence abstractions", but no concrete
store or data inventory is fixed. Local data must survive crashes
mid-meeting, support versioned refined transcripts (ADR-002), glossary
scopes, and replay of live caption events — i.e. real queries, not just
key-value blobs. Everything is local-first (ADR-005 keeps audio on device).

## Options considered

1. **SQLite via `rusqlite`** — battle-tested single-file DB, WAL mode for
   crash safety during live capture, SQL for versioning/queries, zero
   server. Encryption upgrade path via SQLCipher without API changes.
2. **Pure-Rust KV stores (`redb`, `sled`)** — no SQL: transcript
   versioning, glossary scope queries, and migrations become hand-rolled
   application code.
3. **Core Data / SwiftData** — puts persistence in the Swift layer,
   violating the "domain logic and local state orchestration in Rust" rule.
4. **Plain JSON/files** — no transactions; a crash mid-meeting can corrupt
   caption history.

## Decision
**SQLite via `rusqlite`** (bundled build, WAL mode) in a dedicated storage
crate implementing the store facade trait from the domain crate — domain
stays storage-agnostic. Versioned SQL migrations embedded in the crate.

Database location: `~/Library/Application Support/meetingraft/`.
Raw audio is **not** stored in the DB: chunked audio files live on disk
(CAF/FLAC), referenced by an `audio_manifest` table (ADR-004 manifest).

Data inventory (initial schema scope):
- `meetings` / `sessions` (language policy snapshot per session);
- `caption_events` — live partial/final events, replayable (ADR-002);
- `transcripts` + `transcript_versions` — refined artifacts (ADR-002);
- `speakers` and speaker assignments;
- `glossary_terms` with scope (global/workspace/project/meeting) and
  language tags;
- `artifacts` — brief and follow-up drafts;
- `audio_manifest` — chunk files, channels (mic/system), timestamps;
- `jobs` — queued Stage 2 work and sync state (local-first queue).

Baseline protection relies on FileVault; switching the SQLite build to
SQLCipher later changes nothing above the facade.

## Consequences
### Positive
- Crash-safe live capture (WAL) and transactional caption writes.
- SQL covers transcript versioning, glossary scoping, and job queue
  without bespoke index code.
- Single-file DB simplifies "delete meeting completely" and backups.

### Trade-offs
- Schema migrations become a maintained artifact from day one.
- `rusqlite` bundles a C dependency into the Rust build (accepted — same
  situation as whisper.cpp in ADR-005).
