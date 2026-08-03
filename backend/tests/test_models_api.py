import asyncio
import json

import pytest
from fastapi.testclient import TestClient

from app.main import app, lifespan
from app.registry import RegistryError

client = TestClient(app)
AUTH = {"Authorization": "Bearer dev-token"}


def test_lifespan_fails_fast_on_invalid_providers_json(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """Невалидный реестр при старте — lifespan поднимает RegistryError."""
    monkeypatch.setenv("PROVIDERS_JSON", "{not-json")

    async def _run() -> None:
        async with lifespan(app):
            pass

    with pytest.raises(RegistryError, match="Невалидный JSON"):
        asyncio.run(_run())


def test_testclient_fails_on_invalid_registry(monkeypatch: pytest.MonkeyPatch) -> None:
    """TestClient с плохим PROVIDERS_JSON не поднимает приложение."""
    monkeypatch.setenv("PROVIDERS_JSON", "{not-json")
    with pytest.raises(RegistryError):
        with TestClient(app):
            pass


def test_list_models_registry_error_returns_503(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """После старта битый env mid-process → 503 вместо необработанного 500."""
    monkeypatch.setenv("PROVIDERS_JSON", "{not-json")
    response = client.get("/v1/models", headers=AUTH)
    assert response.status_code == 503
    assert "Невалидный JSON" in response.json()["detail"]


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
