import json
from typing import Any

import httpx
import pytest
import respx
from fastapi.testclient import TestClient

from app.main import app

client = TestClient(app)
AUTH = {"Authorization": "Bearer dev-token"}
API_URL = "https://api.perplexity.ai/v1/responses"


def _payload() -> dict[str, Any]:
    return {
        "id": "resp_test",
        "object": "response",
        "created_at": 1771891464,
        "status": "completed",
        "model": "openai/gpt-5.1",
        "output": [
            {
                "type": "search_results",
                "queries": ["запрос"],
                "results": [
                    {
                        "id": 1,
                        "title": "Первый",
                        "url": "https://a.test/1",
                        "snippet": "snippet",
                        "date": "2025-01-01",
                        "last_updated": "2025-02-01",
                        "source": "web",
                    }
                ],
            },
            {
                "type": "message",
                "id": "msg_1",
                "role": "assistant",
                "status": "completed",
                "content": [
                    {"type": "output_text", "text": "Ответ со ссылкой [1]", "annotations": []}
                ],
            },
        ],
        "usage": {"input_tokens": 1, "output_tokens": 2, "total_tokens": 3},
    }


def _answer(body: dict[str, Any]) -> httpx.Response:
    return client.post("/v1/answers", headers=AUTH, json=body)


def test_answer_requires_bearer() -> None:
    response = client.post("/v1/answers", json={"query": "запрос"})

    assert response.status_code == 401


def test_answer_returns_503_when_key_missing(monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.delenv("PERPLEXITY_API_KEY", raising=False)

    response = _answer({"query": "запрос"})

    assert response.status_code == 503
    assert "PERPLEXITY_API_KEY" in response.json()["detail"]


@pytest.mark.parametrize("body", [{"query": ""}, {"query": "   "}, {}])
def test_answer_rejects_empty_query(body: dict[str, Any]) -> None:
    response = _answer(body)

    assert response.status_code == 422


def test_answer_rejects_unknown_language(monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.setenv("PERPLEXITY_API_KEY", "test-key")

    response = _answer({"query": "запрос", "language": "de"})

    assert response.status_code == 422


@respx.mock
def test_answer_returns_grounded_answer_with_sources(monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.setenv("PERPLEXITY_API_KEY", "test-key")
    monkeypatch.delenv("PERPLEXITY_PRESET", raising=False)
    monkeypatch.delenv("PERPLEXITY_MODEL", raising=False)
    route = respx.post(API_URL).mock(return_value=httpx.Response(200, json=_payload()))

    response = _answer({"query": "Что нового?", "language": "ru", "preset": "medium"})

    assert response.status_code == 200
    assert response.json() == {
        "answer_markdown": "Ответ со ссылкой [1]",
        "sources": [
            {"title": "Первый", "url": "https://a.test/1", "date": "2025-01-01"},
        ],
        "model": "openai/gpt-5.1",
        "response_id": "resp_test",
    }
    body = json.loads(route.calls.last.request.content)
    assert body["input"] == "Что нового?"
    assert body["preset"] == "medium"
    assert body["language_preference"] == "ru"
    assert body["store"] is False
    assert body["tools"] == [{"type": "web_search"}]


@respx.mock
def test_answer_returns_502_when_provider_fails(monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.setenv("PERPLEXITY_API_KEY", "test-key")
    respx.post(API_URL).mock(return_value=httpx.Response(401, text="invalid key"))

    response = _answer({"query": "запрос"})

    assert response.status_code == 502
    assert response.json()["detail"]


def test_answer_never_echoes_the_key(monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.setenv("PERPLEXITY_API_KEY", "SECRET-KEY")

    response = _answer({"query": "запрос"})

    assert "SECRET-KEY" not in response.text
