"""MeetingRaft backend stub — in-memory jobs (ADR-007 slice A)."""

from __future__ import annotations

import os
import uuid
from datetime import UTC, datetime
from typing import Any

from fastapi import Depends, FastAPI, HTTPException, status
from fastapi.security import HTTPAuthorizationCredentials, HTTPBearer
from pydantic import BaseModel, Field

from app.llm import LlmError, complete_chat, load_llm_settings

app = FastAPI(title="MeetingRaft Backend", version="0.1.0")
security = HTTPBearer(auto_error=False)

EXPECTED_TOKEN = os.environ.get("MEETINGRAFT_API_TOKEN", "dev-token")

_jobs: dict[str, dict[str, Any]] = {}
_artifacts: dict[str, dict[str, Any]] = {}


class CreateJobRequest(BaseModel):
    meeting_id: str
    kind: str = Field(pattern="^(refine|translate|brief|follow_up)$")
    primary_language: str = Field(pattern="^(ru|en|es)$")
    allowed_languages: list[str] = Field(min_length=1)
    payload: dict[str, Any] | None = None


class HealthResponse(BaseModel):
    status: str


def require_bearer(
    credentials: HTTPAuthorizationCredentials | None = Depends(security),
) -> None:
    if credentials is None or credentials.scheme.lower() != "bearer":
        raise HTTPException(status_code=status.HTTP_401_UNAUTHORIZED, detail="missing bearer")
    if credentials.credentials != EXPECTED_TOKEN:
        raise HTTPException(status_code=status.HTTP_401_UNAUTHORIZED, detail="invalid token")


@app.get("/health", response_model=HealthResponse)
def health() -> HealthResponse:
    return HealthResponse(status="ok")


@app.post("/v1/jobs", status_code=status.HTTP_201_CREATED, dependencies=[Depends(require_bearer)])
def create_job(body: CreateJobRequest) -> dict[str, Any]:
    for lang in body.allowed_languages:
        if lang not in {"ru", "en", "es"}:
            raise HTTPException(status_code=422, detail=f"unsupported language: {lang}")

    job_id = str(uuid.uuid4())
    created_at = datetime.now(UTC).isoformat()
    settings = load_llm_settings()

    if body.kind in {"brief", "follow_up"} and settings.base_url:
        payload = body.payload or {}
        model = payload.get("model", "")
        system = payload.get("system")
        user = payload.get("user")
        try:
            if not isinstance(model, str):
                raise LlmError("Модель LLM должна быть строкой")
            if not isinstance(system, str) or not system.strip():
                raise LlmError("Не указан system prompt")
            if not isinstance(user, str) or not user.strip():
                raise LlmError("Не указан user prompt")
            body_md = complete_chat(
                settings,
                model=model,
                system=system,
                user=user,
            )
        except LlmError as error:
            job = {
                "id": job_id,
                "meeting_id": body.meeting_id,
                "kind": body.kind,
                "status": "failed",
                "error": str(error),
                "artifact_ids": [],
            }
            _jobs[job_id] = job
            return job
    else:
        body_md = (
            f"# Stub {body.kind}\n\n"
            f"meeting=`{body.meeting_id}` primary=`{body.primary_language}`\n\n"
            "_In-memory ADR-007 slice A — no ML yet._\n"
        )

    artifact_id = str(uuid.uuid4())
    _artifacts[artifact_id] = {
        "id": artifact_id,
        "kind": body.kind,
        "body_markdown": body_md,
        "created_at": created_at,
    }
    job = {
        "id": job_id,
        "meeting_id": body.meeting_id,
        "kind": body.kind,
        "status": "succeeded",
        "error": None,
        "artifact_ids": [artifact_id],
    }
    _jobs[job_id] = job
    return job


@app.get("/v1/jobs/{job_id}", dependencies=[Depends(require_bearer)])
def get_job(job_id: str) -> dict[str, Any]:
    job = _jobs.get(job_id)
    if job is None:
        raise HTTPException(status_code=404, detail="job not found")
    return job


@app.get("/v1/artifacts/{artifact_id}", dependencies=[Depends(require_bearer)])
def get_artifact(artifact_id: str) -> dict[str, Any]:
    artifact = _artifacts.get(artifact_id)
    if artifact is None:
        raise HTTPException(status_code=404, detail="artifact not found")
    return artifact
