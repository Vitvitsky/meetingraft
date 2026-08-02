# Phase 6 backend stub — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL tracking via checkboxes.

**Goal:** ADR-007 slice A — OpenAPI, FastAPI stub jobs, docker api, Rust sync, Settings Test API.

**Architecture:** Python FastAPI in-memory; Rust `meetingraft-sync` + UniFFI; Swift Settings only.

**Tech Stack:** FastAPI, uv, docker-compose, reqwest, UniFFI, SwiftUI, pytest, ruff.

**Spec:** `docs/superpowers/specs/2026-08-02-phase-6-backend-stub-design.md`

## Global Constraints

- Bearer auth; language policy on every job (ADR-003).
- No Postgres/Redis/Dramatiq/MinIO/WhisperX in this PR.
- Backend concerns stay out of SwiftUI (only call UniFFI).

---

### Task 1: OpenAPI + FastAPI + docker

- [ ] `shared/openapi.yaml`
- [ ] `backend/` FastAPI app + pytest
- [ ] `docker-compose.yml` + Dockerfile
- [ ] CI backend job

### Task 2: Rust sync crate

- [ ] `meetingraft-sync` DTOs + SyncClient
- [ ] Unit tests (serde + mockito or similar)
- [ ] Wire workspace

### Task 3: UniFFI + Settings

- [ ] MeetingCore API config + test/submit/get
- [ ] regenerate FFI
- [ ] Settings: URL, token, Test API
- [ ] cargo test + xcodebuild test
- [ ] Update roadmap/backlog note

---
