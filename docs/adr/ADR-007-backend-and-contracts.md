# ADR-007: Backend stack and contracts — Python/FastAPI jobs over REST, OpenAPI in `shared/`

## Status
Accepted

## Context
With on-device live STT (ADR-005), the backend serves only Stage 2:
transcript refinement, diarization, brief and follow-up generation, and
artifact storage. Nothing is latency-critical — it is job-based batch
processing. Backend work starts in roadmap Phase 6; this ADR fixes the
stack and contract format now so `shared/` has a defined shape.

## Options considered

1. **Transport: REST + JSON (job-based)** — submit job, poll/receive
   status, fetch artifacts. Matches the async batch nature; trivial from
   Swift/Rust clients.
2. **Transport: gRPC** — codegen on three sides (Swift, Rust, Python) for
   no streaming need; extra toolchain cost without benefit here.
3. **Transport: WebSocket streaming gateway** — needed only if STT moves
   to the cloud; explicitly out of scope after ADR-005.

## Decision
- **Stack:** Python 3.13 + FastAPI (API), Dramatiq + Redis (workers for
  long ML jobs), PostgreSQL (job/meeting metadata), S3-compatible object
  storage (MinIO) for audio and artifacts. Deployed with docker-compose;
  target host — the home server. Dependencies via `uv`, lint via Ruff.
- **ML jobs:** refinement + word alignment via WhisperX (full `large-v3`),
  diarization via pyannote, brief/follow-up generation via an LLM worker
  (local vLLM or cloud API — provider stays swappable inside the worker).
- **Transport:** REST + JSON over HTTPS. Job lifecycle:
  `POST /jobs` → `GET /jobs/{id}` (poll) → `GET /artifacts/{id}`.
- **Contracts:** OpenAPI schema exported from FastAPI and committed to
  `shared/openapi.yaml` — the single source of truth. The Rust sync client
  uses small hand-written DTOs mirroring that schema (codegen optional
  later); language policy fields (`primary_language`, `allowed_languages`)
  are mandatory on every processing job (ADR-003).
- **Auth:** single-user bearer token for v1.

The macOS shell never talks to workers directly; only the Rust sync client
calls the API (architecture rule: backend concerns stay outside the shell).

## Consequences
### Positive
- Python matches the ML tooling (WhisperX, pyannote) and the maintainer's
  primary stack — lowest friction for Stage 2.
- No backend on the critical path until Phase 6; contracts still have a
  fixed home (`shared/openapi.yaml`) from day one.
- Job-based REST keeps clients simple and retry-friendly (local-first
  `jobs` queue from ADR-006 maps 1:1 onto the API).

### Trade-offs
- Polling instead of push: artifact readiness has minutes-scale latency
  tolerance, so acceptable; webhooks/SSE can be added later without
  breaking the contract.
- Self-hosted deployment (MinIO, Postgres, Redis) is on the maintainer;
  no managed-cloud assumptions in v1.
