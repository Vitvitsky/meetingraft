import json
from typing import Any

import httpx
import pytest
import respx

from app.perplexity import DEFAULT_PRESET, PerplexityError, answer, load_perplexity_api_key

API_URL = "https://api.perplexity.ai/v1/responses"


def _set_perplexity_env(monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.setenv("PERPLEXITY_API_KEY", "test-key")
    monkeypatch.delenv("PERPLEXITY_PRESET", raising=False)
    monkeypatch.delenv("PERPLEXITY_MODEL", raising=False)


def _payload(*, status: str = "completed", text: str = "Ответ [1]") -> dict[str, Any]:
    """Ответ Agent API: search_results с источником-дублем и URL-аннотацией."""
    return {
        "id": "resp_test",
        "object": "response",
        "created_at": 1771891464,
        "status": status,
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
                    },
                    {
                        "id": 2,
                        "title": "Второй",
                        "url": "https://a.test/2",
                        "snippet": "snippet",
                        "date": None,
                        "last_updated": None,
                        "source": "web",
                    },
                ],
            },
            {
                "type": "message",
                "id": "msg_1",
                "role": "assistant",
                "status": "completed",
                "content": [
                    {
                        "type": "output_text",
                        "text": text,
                        "annotations": [
                            {"url": "https://a.test/2", "title": "Второй", "type": "url_citation"},
                            {"url": "https://b.test/3", "title": "Третий", "type": "url_citation"},
                        ],
                    }
                ],
            },
        ],
        "usage": {"input_tokens": 1, "output_tokens": 2, "total_tokens": 3},
    }


def test_load_perplexity_api_key_trims_and_defaults_empty(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    monkeypatch.setenv("PERPLEXITY_API_KEY", "  test-key  ")
    assert load_perplexity_api_key() == "test-key"

    monkeypatch.delenv("PERPLEXITY_API_KEY")
    assert load_perplexity_api_key() == ""


@respx.mock
def test_answer_sends_web_search_preset_and_store_false(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    _set_perplexity_env(monkeypatch)
    route = respx.post(API_URL).mock(return_value=httpx.Response(200, json=_payload()))

    result = answer("Что нового в Perplexity?")

    assert result.text == "Ответ [1]"
    assert result.model == "openai/gpt-5.1"
    assert result.response_id == "resp_test"
    request = route.calls.last.request
    assert request.headers["Authorization"] == "Bearer test-key"
    assert json.loads(request.content) == {
        "input": "Что нового в Perplexity?",
        "preset": DEFAULT_PRESET,
        "store": False,
        "tools": [{"type": "web_search"}],
    }


@respx.mock
def test_answer_extracts_sources_and_dedupes(monkeypatch: pytest.MonkeyPatch) -> None:
    _set_perplexity_env(monkeypatch)
    respx.post(API_URL).mock(return_value=httpx.Response(200, json=_payload()))

    result = answer("запрос")

    assert [(source.url, source.date) for source in result.sources] == [
        ("https://a.test/1", "2025-01-01"),
        ("https://a.test/2", None),
        ("https://b.test/3", None),
    ]
    assert result.sources[0].title == "Первый"
    assert result.sources[2].title == "Третий"


@respx.mock
def test_answer_uses_explicit_model_instead_of_preset(monkeypatch: pytest.MonkeyPatch) -> None:
    _set_perplexity_env(monkeypatch)
    route = respx.post(API_URL).mock(return_value=httpx.Response(200, json=_payload()))

    answer("запрос", preset="high", model="anthropic/claude-sonnet-4-6")

    body = json.loads(route.calls.last.request.content)
    assert body["model"] == "anthropic/claude-sonnet-4-6"
    assert "preset" not in body


@respx.mock
def test_answer_uses_env_preset_and_model(monkeypatch: pytest.MonkeyPatch) -> None:
    _set_perplexity_env(monkeypatch)
    monkeypatch.setenv("PERPLEXITY_PRESET", "fast")
    route = respx.post(API_URL).mock(return_value=httpx.Response(200, json=_payload()))

    answer("запрос")

    assert json.loads(route.calls.last.request.content)["preset"] == "fast"

    monkeypatch.setenv("PERPLEXITY_MODEL", "openai/gpt-5.6-sol")
    answer("запрос")

    body = json.loads(route.calls.last.request.content)
    assert body["model"] == "openai/gpt-5.6-sol"
    assert "preset" not in body


@respx.mock
def test_answer_passes_optional_fields(monkeypatch: pytest.MonkeyPatch) -> None:
    _set_perplexity_env(monkeypatch)
    route = respx.post(API_URL).mock(return_value=httpx.Response(200, json=_payload()))

    answer(
        "запрос",
        instructions="Отвечай кратко",
        language="ru",
        previous_response_id="resp_prev",
    )

    body = json.loads(route.calls.last.request.content)
    assert body["instructions"] == "Отвечай кратко"
    assert body["language_preference"] == "ru"
    assert body["previous_response_id"] == "resp_prev"


def test_answer_rejects_missing_key(monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.delenv("PERPLEXITY_API_KEY", raising=False)

    with pytest.raises(PerplexityError, match="PERPLEXITY_API_KEY"):
        answer("запрос")


def test_answer_rejects_empty_query(monkeypatch: pytest.MonkeyPatch) -> None:
    _set_perplexity_env(monkeypatch)

    with pytest.raises(PerplexityError, match="запрос"):
        answer("   ")


@respx.mock
def test_answer_rejects_non_completed_status(monkeypatch: pytest.MonkeyPatch) -> None:
    _set_perplexity_env(monkeypatch)
    respx.post(API_URL).mock(
        return_value=httpx.Response(200, json=_payload(status="failed") | {"output": []})
    )

    with pytest.raises(PerplexityError, match="статус"):
        answer("запрос")


@respx.mock
def test_answer_rejects_empty_text(monkeypatch: pytest.MonkeyPatch) -> None:
    _set_perplexity_env(monkeypatch)
    respx.post(API_URL).mock(return_value=httpx.Response(200, json=_payload(text="   ")))

    with pytest.raises(PerplexityError, match="пустой"):
        answer("запрос")


@pytest.mark.parametrize("status_code", [400, 401, 500])
@respx.mock
def test_answer_rejects_http_errors(
    monkeypatch: pytest.MonkeyPatch,
    status_code: int,
) -> None:
    _set_perplexity_env(monkeypatch)
    respx.post(API_URL).mock(return_value=httpx.Response(status_code, text="boom"))

    with pytest.raises(PerplexityError, match="Ошибка Perplexity API"):
        answer("запрос")
