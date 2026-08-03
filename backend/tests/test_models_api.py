import json

import pytest
from fastapi.testclient import TestClient

from app.main import app

client = TestClient(app)
AUTH = {"Authorization": "Bearer dev-token"}


def test_list_models_from_registry(monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.setenv(
        "PROVIDERS_JSON",
        json.dumps(
            {
                "providers": [
                    {
                        "id": "home-llm",
                        "base_url": "http://h",
                        "api_key": "SECRET",
                        "default_model": "m1",
                        "models": [{"id": "m1", "display_name": "One"}],
                    }
                ]
            }
        ),
    )
    response = client.get("/v1/models", headers=AUTH)
    assert response.status_code == 200
    body = response.json()
    assert body == {
        "models": [
            {"provider_id": "home-llm", "model": "m1", "display_name": "One"},
        ]
    }
    assert "SECRET" not in response.text


def test_list_models_empty_without_llm(monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.delenv("PROVIDERS_JSON", raising=False)
    monkeypatch.delenv("LLM_PROVIDERS_FILE", raising=False)
    monkeypatch.delenv("LLM_BASE_URL", raising=False)
    response = client.get("/v1/models", headers=AUTH)
    assert response.status_code == 200
    assert response.json() == {"models": []}


def test_list_models_requires_auth(monkeypatch: pytest.MonkeyPatch) -> None:
    response = client.get("/v1/models")
    assert response.status_code == 401
