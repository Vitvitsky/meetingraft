# Phase 6 follow-up — Markdown export for Obsidian

**Date:** 2026-08-03
**Status:** Approved for implementation (approach A)
**Maps to:** Epic 8 (Export artifacts), Meetings UX
**Depends on:** Final transcript + Artifacts in SQLite / UniFFI; Meetings detail UI

## Goal

Экспорт meeting в **отдельные `.md` файлы** (Final, Brief, Follow-up) в папку
Obsidian vault / Documents — из macOS app, без backend.

## Decisions (approved)

| Topic | Choice |
|-------|--------|
| Files | Up to **3 files** per meeting: `final`, `brief`, `follow-up` |
| Missing | No Final → fail export; missing Brief/Follow-up → **skip** |
| Path | Settings default folder + **Choose folder…** on export |
| Names | `{yyyy-MM-dd}-{shortId}-{kind}.md` (flat in folder) |
| Conflict | **Overwrite** |
| Body | Raw `body_markdown` (no frontmatter / wiki-links in v1) |
| Layer | **Swift** writes files; content via existing UniFFI getters |

## Non-goals

- HTTP API for Obsidian plugin (→ backlog)
- Mail draft
- Export Live captions
- Transcript versioning / unique `-2` suffixes
- YAML frontmatter, templates, wiki-links
- Rust writing to filesystem
- Keychain (folder path: UserDefaults / in-memory store like other Settings)

## Architecture

```text
Settings: exportFolderPath (default ~/Documents/MeetingRaft)
MeetingDetail: Export to Markdown
  → resolve folder (Settings or NSOpenPanel)
  → MeetingsViewModel.exportMarkdown(meetingId, folder, startedAtMs, …)
       getFinalTranscript / listArtifacts via MeetingsCoreProviding
       MarkdownExport.write(files…) via FileManager
  → status: N files written + path
```

## Naming

- `yyyy-MM-dd` from `startedAtMs` in local calendar (ISO-like date).
- `shortId`: first 8 characters of `meeting_id`, filesystem-safe
  (`[^A-Za-z0-9_-]` → `_`); if shorter, use full id sanitized.
- `kind`: `final` | `brief` | `follow-up`
- Example: `2026-08-03-a1b2c3d4-brief.md`

For Brief/Follow-up: if several artifacts of same kind exist, export the
**latest by `createdAtMs`** (one file per kind).

## Export folder

- Default path string: `~/Documents/MeetingRaft` (expand tilde at write time).
- Settings section **Export**: TextField path + optional «Choose…» (NSOpenPanel).
- On Export: use Settings path; secondary control «Choose folder…» updates
  path for this write (and persists to Settings).
- Create directory if missing (`withIntermediateDirectories: true`).
- App currently non-sandboxed in local Debug builds — plain paths OK; if
  sandbox appears later, add security-scoped bookmarks (follow-up).

## UI

- Meeting detail toolbar / Artifacts or header: **Export to Markdown**.
- Disabled or error when no Final.
- Success caption: e.g. `Exported 2 files → ~/Documents/MeetingRaft`.
- Keep existing **Copy** buttons.

## Testing

- Unit-test pure naming + file write with temp directory (no UniFFI).
- ViewModel test with spy core: writes expected files; skips missing kinds;
  fails without Final.
- Docs: backlog export + Obsidian plugin API deferred; install one-liner.

## Success criteria

- [ ] Export creates expected `.md` files under chosen folder
- [ ] Overwrite on re-export
- [ ] Missing Brief/Follow-up skipped; no Final → clear error
- [ ] Settings stores export folder path
- [ ] Backlog notes Obsidian plugin / export API as deferred

## Backlog (explicit)

- MeetingRaft HTTP/local API + **Obsidian community plugin** to pull meetings
- Frontmatter / wikilinks / templates
- Mail draft export
