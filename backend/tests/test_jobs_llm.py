import json
from typing import Any

import httpx
import pytest
import respx
from fastapi.testclient import TestClient

from app.main import app

client = TestClient(app)
AUTH = {"Authorization": "Bearer dev-token"}
CHAT_URL = "http://llm.test/v1/chat/completions"


def create_job(kind: str, payload: dict[str, Any] | None = None) -> dict[str, Any]:
    response = client.post(
        "/v1/jobs",
        headers=AUTH,
        json={
            "meeting_id": "m1",
            "kind": kind,
            "primary_language": "ru",
            "allowed_languages": ["ru"],
            "payload": payload,
        },
    )
    assert response.status_code == 201
    return response.json()


@pytest.mark.parametrize(
    ("kind", "payload", "expected_model", "completion"),
    [
        (
            "brief",
            {"model": "Google/gemma-4-12b-it", "system": "brief sys", "user": "brief usr"},
            "Google/gemma-4-12b-it",
            "# Real brief",
        ),
        (
            "follow_up",
            {"system": "follow-up sys", "user": "follow-up usr"},
            "default-model",
            "# Real follow-up",
        ),
    ],
)
@respx.mock
def test_generation_job_uses_llm_when_configured(
    monkeypatch: pytest.MonkeyPatch,
    kind: str,
    payload: dict[str, Any],
    expected_model: str,
    completion: str,
) -> None:
    monkeypatch.setenv("LLM_BASE_URL", "http://llm.test")
    monkeypatch.setenv("LLM_API_KEY", "LOCAL-API-KEY")
    monkeypatch.setenv("LLM_MODEL", "default-model")
    route = respx.post(CHAT_URL).mock(
        return_value=httpx.Response(
            200,
            json={"choices": [{"message": {"content": completion}}]},
        )
    )

    job = create_job(kind, payload)

    assert job["status"] == "succeeded"
    artifact = client.get(f"/v1/artifacts/{job['artifact_ids'][0]}", headers=AUTH)
    assert artifact.status_code == 200
    assert artifact.json()["body_markdown"] == completion
    request_body = json.loads(route.calls.last.request.content)
    assert request_body == {
        "model": expected_model,
        "messages": [
            {"role": "system", "content": payload["system"]},
            {"role": "user", "content": payload["user"]},
        ],
    }


@respx.mock
def test_llm_error_fails_job_without_stub_artifact(monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.setenv("LLM_BASE_URL", "http://llm.test")
    respx.post(CHAT_URL).mock(return_value=httpx.Response(401, text="nope"))

    job = create_job("brief", {"model": "model", "system": "sys", "user": "usr"})

    assert job["status"] == "failed"
    assert job["artifact_ids"] == []
    assert job["error"]


@pytest.mark.parametrize(
    "payload",
    [
        {"model": "model", "user": "usr"},
        {"model": "model", "system": "sys"},
        {"model": "model", "system": " ", "user": "usr"},
        {"model": "model", "system": "sys", "user": ""},
    ],
)
@respx.mock
def test_missing_prompt_fails_job(
    monkeypatch: pytest.MonkeyPatch,
    payload: dict[str, Any],
) -> None:
    monkeypatch.setenv("LLM_BASE_URL", "http://llm.test")
    respx.post(CHAT_URL).mock(
        return_value=httpx.Response(
            200,
            json={"choices": [{"message": {"content": "must not become an artifact"}}]},
        )
    )

    job = create_job("brief", payload)

    assert job["status"] == "failed"
    assert job["artifact_ids"] == []
    assert job["error"]


@respx.mock
def test_refine_remains_stub_with_llm_configured(monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.setenv("LLM_BASE_URL", "http://llm.test")

    job = create_job("refine")

    assert job["status"] == "succeeded"
    artifact = client.get(f"/v1/artifacts/{job['artifact_ids'][0]}", headers=AUTH)
    assert artifact.status_code == 200
    assert "Stub refine" in artifact.json()["body_markdown"]
