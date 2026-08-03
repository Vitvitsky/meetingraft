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


def _set_legacy_llm_env(monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.delenv("PROVIDERS_JSON", raising=False)
    monkeypatch.delenv("LLM_PROVIDERS_FILE", raising=False)
    monkeypatch.setenv("LLM_BASE_URL", "http://llm.test")
    monkeypatch.setenv("LLM_API_KEY", "LOCAL-API-KEY")
    monkeypatch.setenv("LLM_MODEL", "default-model")


@pytest.mark.parametrize(
    ("kind", "payload", "expected_model", "completion"),
    [
        (
            "brief",
            {
                "provider_id": "default",
                "model": "Google/gemma-4-12b-it",
                "system": "brief sys",
                "user": "brief usr",
            },
            "Google/gemma-4-12b-it",
            "# Real brief",
        ),
        (
            "follow_up",
            {
                "provider_id": "default",
                "system": "follow-up sys",
                "user": "follow-up usr",
            },
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
    _set_legacy_llm_env(monkeypatch)
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
def test_legacy_job_without_provider_id_uses_default(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """Legacy payload без provider_id допустим только в env_compat."""
    _set_legacy_llm_env(monkeypatch)
    route = respx.post(CHAT_URL).mock(
        return_value=httpx.Response(
            200,
            json={"choices": [{"message": {"content": "# legacy ok"}}]},
        )
    )

    job = create_job(
        "brief",
        {"model": "m", "system": "sys", "user": "usr"},
    )

    assert job["status"] == "succeeded"
    assert route.called


@respx.mock
def test_llm_error_fails_job_without_stub_artifact(monkeypatch: pytest.MonkeyPatch) -> None:
    _set_legacy_llm_env(monkeypatch)
    respx.post(CHAT_URL).mock(return_value=httpx.Response(401, text="nope"))

    job = create_job(
        "brief",
        {"provider_id": "default", "model": "model", "system": "sys", "user": "usr"},
    )

    assert job["status"] == "failed"
    assert job["artifact_ids"] == []
    assert job["error"]


@pytest.mark.parametrize(
    "payload",
    [
        {"provider_id": "default", "model": "model", "user": "usr"},
        {"provider_id": "default", "model": "model", "system": "sys"},
        {"provider_id": "default", "model": "model", "system": " ", "user": "usr"},
        {"provider_id": "default", "model": "model", "system": "sys", "user": ""},
    ],
)
@respx.mock
def test_missing_prompt_fails_job(
    monkeypatch: pytest.MonkeyPatch,
    payload: dict[str, Any],
) -> None:
    _set_legacy_llm_env(monkeypatch)
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
    _set_legacy_llm_env(monkeypatch)

    job = create_job("refine")

    assert job["status"] == "succeeded"
    artifact = client.get(f"/v1/artifacts/{job['artifact_ids'][0]}", headers=AUTH)
    assert artifact.status_code == 200
    assert "Stub refine" in artifact.json()["body_markdown"]


@respx.mock
def test_job_routes_by_provider_id(monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.setenv(
        "PROVIDERS_JSON",
        json.dumps(
            {
                "providers": [
                    {
                        "id": "a",
                        "base_url": "http://a.test",
                        "api_key": "ka",
                        "default_model": "ma",
                        "models": [{"id": "ma"}],
                    },
                    {
                        "id": "b",
                        "base_url": "http://b.test",
                        "api_key": "kb",
                        "default_model": "mb",
                        "models": [{"id": "mb"}],
                    },
                ]
            }
        ),
    )
    route_a = respx.post("http://a.test/v1/chat/completions").mock(
        return_value=httpx.Response(
            200, json={"choices": [{"message": {"content": "# from A"}}]}
        )
    )
    route_b = respx.post("http://b.test/v1/chat/completions").mock(
        return_value=httpx.Response(
            200, json={"choices": [{"message": {"content": "# from B"}}]}
        )
    )
    job = create_job(
        "brief",
        {
            "provider_id": "b",
            "model": "mb",
            "system": "s",
            "user": "u",
        },
    )
    assert job["status"] == "succeeded"
    assert route_b.called
    assert not route_a.called


@respx.mock
def test_unknown_provider_id_fails_job(monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.setenv(
        "PROVIDERS_JSON",
        json.dumps(
            {
                "providers": [
                    {
                        "id": "a",
                        "base_url": "http://a.test",
                        "api_key": "ka",
                        "default_model": "ma",
                        "models": [{"id": "ma"}],
                    }
                ]
            }
        ),
    )
    respx.post("http://a.test/v1/chat/completions").mock(
        return_value=httpx.Response(
            200, json={"choices": [{"message": {"content": "must not"}}]}
        )
    )

    job = create_job(
        "brief",
        {
            "provider_id": "missing",
            "model": "ma",
            "system": "s",
            "user": "u",
        },
    )

    assert job["status"] == "failed"
    assert job["artifact_ids"] == []
    assert job["error"]


@respx.mock
def test_registry_mode_missing_provider_id_fails(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    monkeypatch.setenv(
        "PROVIDERS_JSON",
        json.dumps(
            {
                "providers": [
                    {
                        "id": "a",
                        "base_url": "http://a.test",
                        "api_key": "ka",
                        "default_model": "ma",
                        "models": [{"id": "ma"}],
                    }
                ]
            }
        ),
    )
    respx.post("http://a.test/v1/chat/completions").mock(
        return_value=httpx.Response(
            200, json={"choices": [{"message": {"content": "must not"}}]}
        )
    )

    job = create_job(
        "brief",
        {"model": "ma", "system": "s", "user": "u"},
    )

    assert job["status"] == "failed"
    assert job["artifact_ids"] == []
    assert job["error"]
