# ADR-008: Live translation backends — pluggable engines with platform switch

## Status
Accepted

## Context
Sync translation of live captions is a separate product function from STT
(ADR-003 amendment): captions always reflect recognition language; a parallel
stream may target another allowed language.

We want:
- backend reuse (NLLB / job API from ADR-007) for multiplatform clients;
- Apple Translation on macOS/iOS for low-latency on-device when available;
- optional local LLM later;
- one Rust contract shared across shells (UniFFI today, other bindings later).

Cocoa / Translation framework types must not enter Rust (architecture rule).

## Options considered

1. **Apple Translation only** — best latency on Apple, not reusable elsewhere.
2. **Backend NLLB only** — portable and aligns with ADR-007; higher live latency.
3. **Local GGUF LLM only** — offline/privacy; heavier and weaker pure-MT quality.
4. **Pluggable `TranslateEngine` + policy switch** — Auto picks best available;
   explicit modes for tests and user preference.

## Decision
Introduce crate `meetingraft-translate` with:

- `TranslationBackendKind`: `off` | `auto` | `stub` | `apple` | `backend` | `local_llm`
- `TranslationPolicy`: enabled flag, target language, backend kind, optional
  HTTP base URL for backend
- trait `TranslateEngine` for synchronous string translation (source → target)

**Effective resolution for `auto`:**
1. if Apple host bridge is registered → `apple`;
2. else if backend base URL is set → `backend`;
3. else → `stub` (CI / first-run).

**Apple path:** Rust does not call Translation APIs. It enqueues
`HostTranslationRequest` DTOs; the Swift shell drains them, runs
`TranslationSession` (or a stub), and completes via UniFFI. No AVFoundation /
Translation types cross the FFI boundary.

**Backend path:** `HttpTranslateEngine` calls a future
`POST {base}/v1/translate` (OpenAPI in `shared/` when ADR-007 work lands).
Skeleton returns a marked placeholder until the endpoint exists.

**Local LLM path:** skeleton for Ollama/GGUF; same UniFFI surface.

Live captions remain Whisper on-device (ADR-005). Translation never replaces
the caption stream.

## Consequences
### Positive
- Platform advantages without locking the domain to Apple
- Same policy/engine contract for macOS MVP and future clients
- Backend NLLB stays the portable default for non-Apple and shared jobs

### Trade-offs
- Apple latency path needs a host poll/complete loop (extra Swift glue)
- Auto heuristics may surprise users — Settings expose an explicit picker
- Real HTTP/NLLB and Apple TranslationSession wiring land in follow-up PRs
