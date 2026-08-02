# Phase 6 follow-up — Backend stub (ADR-007 slice A)

**Date:** 2026-08-02
**Status:** Approved for implementation
**Maps to:** ADR-007, Phase 6 follow-up, Epic 8 (HTTP path)

## Goal

Рабочий e2e stub: OpenAPI + FastAPI in-memory jobs + docker `api:8080` + Rust
`meetingraft-sync` + Settings `apiBaseUrl` / token / Test API — без Postgres,
Redis, Dramatiq, MinIO, WhisperX.

## Non-goals

- Real refinement / diarization / NLLB / LLM workers
- Switching Brief/Follow-up generation to backend in Meetings UI
- Persisting API token to Keychain (in-memory / UserDefaults later OK for MVP token field)

## Decisions

| Topic | Choice |
|-------|--------|
| Scope | Slice A (skeleton) |
| Storage | In-memory in API process |
| Job completion | Immediate `succeeded` + dummy artifact |
| Auth | Bearer `MEETINGRAFT_API_TOKEN` |
| Rust HTTP | `reqwest` blocking |
| OpenAPI | Hand-written `shared/openapi.yaml` |

## API

- `GET /health` → `{ "status": "ok" }`
- `POST /v1/jobs` → create job, return job (status may already be `succeeded`)
- `GET /v1/jobs/{id}`
- `GET /v1/artifacts/{id}`

Language policy required on every job: `primary_language`, `allowed_languages`.

## Layout

```text
backend/app/main.py
backend/pyproject.toml
backend/Dockerfile
backend/tests/
docker-compose.yml
shared/openapi.yaml
rust/crates/sync/
```

## Client

`SyncClient` in Rust; UniFFI on `MeetingCore`:
`setApiConfig`, `testApiConnection`, `submitBackendJob`, `getBackendJob`, `getBackendArtifact`.

Settings wires `ProviderSettingsStore.apiBaseUrl` + token → core; Test API button.

## Done criteria

- `docker compose up` serves `/health`
- Settings Test API succeeds against local compose
- `cargo test -p meetingraft-sync` green without live server (DTO + mock/httpx-level)
- CI backend job runs ruff + pytest
