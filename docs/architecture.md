# BriefLane Architecture

## Overview

BriefLane is a native-first macOS application for meeting assistance.

The product uses a two-stage flow:
1. live subtitles during the meeting;
2. post-call refinement for transcript cleanup, speaker assignment, brief generation, and follow-up drafting.

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
- open meeting session
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

## Core principles

- Live and final transcripts are separate entities.
- Realtime captions optimize latency.
- Post-call pipeline optimizes quality.
- Native UX is a product requirement, not a nice-to-have.
- State machine boundaries must be explicit.
