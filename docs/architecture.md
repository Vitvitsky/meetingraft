# MeetingRaft Architecture

## Overview

MeetingRaft is a native-first macOS application for meeting assistance.
Repository name on GitHub: `meetingraft`.

The product uses a two-stage flow:
1. live subtitles during the meeting;
2. post-call refinement for transcript cleanup, speaker assignment, brief generation, and follow-up drafting.

### Supported speech languages

| Priority | Code | Role |
|----------|------|------|
| 1 (default) | `ru` | Primary recognition and default session language |
| 2 | `en` | Supported |
| 3 | `es` | Supported |

- Session opens with primary language `ru` unless the user overrides.
- Live STT and post-call refinement receive the same language policy (primary + allowed set).
- Glossary terms may be language-tagged; Russian terms are the default scope.
- Mixed-language meetings are in scope; Russian quality is optimized first.

## High-level architecture

```text
macOS App (SwiftUI)
├─ App Shell
├─ Meeting Controls
├─ Live Captions UI
├─ Glossary UI
├─ Review / Brief / Follow-up UI
└─ Swift Platform Adapters
   ├─ AVFoundation audio capture
   ├─ permissions
   ├─ notifications
   └─ exports

Rust Core via UniFFI
├─ domain models
├─ session engine
├─ subtitle assembler
├─ glossary engine
├─ sync client
└─ local store facade

Remote Backend
├─ streaming gateway
├─ meeting API
├─ artifact storage
├─ post-call processing
│  ├─ transcription refinement
│  ├─ diarization
│  ├─ alignment
│  └─ enrichment
└─ generated artifacts
```

## Why this split

SwiftUI and AVFoundation provide native macOS UX and native media access.
Rust provides a fast, structured, reusable core for state transitions, transcript logic, glossary handling, and sync orchestration.
The backend handles long-running and heavy post-call processing.

## Main domains

### Live session
- open meeting session (language policy: primary `ru`, allowed `ru|en|es`)
- capture audio
- stream audio chunks
- receive partial and final subtitle events
- persist live caption events

### Post-call processing
- upload or finalize raw audio artifact
- refine transcript
- assign speakers
- generate brief
- generate follow-up email draft

### Glossary
- meeting glossary
- workspace glossary
- project glossary
- aliases and canonical forms
- acronym and slang normalization

## Operational budgets and policies

### Live caption latency budget

| Metric | Budget |
|--------|--------|
| Partial caption after spoken word | ≤ 2.0 s |
| Final caption after segment end | ≤ 5.0 s |
| Session start with warm model | ≤ 3 s |
| Cold model load (first run) | ≤ 15 s |

Budgets are measured in roadmap Phase 4 on real meetings; regressions block
the phase exit. VAD window tuning (ADR-005) is the primary latency lever.

### Recording privacy and consent

- Local by default: raw audio, captions, and transcripts never leave the
  device (ADR-005); data reaches the backend only through an explicit
  Stage 2 action.
- A visible recording indicator is shown during any capture; on first
  recording the app reminds the user that meeting-recording consent norms
  are their responsibility.
- Storage protection baseline is FileVault; the SQLite build can move to
  SQLCipher without facade changes (ADR-006). Backend tokens live in the
  Keychain.
- Deleting a meeting removes everything: audio chunks, manifest rows,
  caption events, transcript versions, and generated artifacts.

### Network loss behavior

- The live pipeline has zero network dependency (on-device STT): captions
  keep working fully offline.
- Stage 2 is local-first: jobs are queued in the local store (`jobs` table,
  ADR-006) and retried with backoff; connectivity loss never blocks or
  degrades a running meeting session.

## Core principles

- Live and final transcripts are separate entities.
- Realtime captions optimize latency.
- Post-call pipeline optimizes quality.
- Native UX is a product requirement, not a nice-to-have.
- State machine boundaries must be explicit.
- Russian is the default and highest-priority speech language (`ru` > `en` > `es`).
