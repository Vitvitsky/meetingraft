import json

import httpx
import pytest
import respx

from app.llm import LlmError, LlmSettings, complete_chat, load_llm_settings


def test_load_llm_settings_reads_environment_and_trims_slashes(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    monkeypatch.setenv("LLM_BASE_URL", "http://example:58001///")
    monkeypatch.setenv("LLM_API_KEY", "LOCAL-API-KEY")
    monkeypatch.setenv("LLM_MODEL", "Google/gemma-4-12b-it")

    settings = load_llm_settings()

    assert settings == LlmSettings(
        base_url="http://example:58001",
        api_key="LOCAL-API-KEY",
        default_model="Google/gemma-4-12b-it",
    )


@respx.mock
def test_complete_chat_sends_bearer_model_and_messages() -> None:
    route = respx.post("http://llm.test/v1/chat/completions").mock(
        return_value=httpx.Response(
            200,
            json={"choices": [{"message": {"role": "assistant", "content": "# Brief"}}]},
        )
    )
    settings = LlmSettings(
        base_url="http://llm.test/",
        api_key="LOCAL-API-KEY",
        default_model="fallback",
    )

    result = complete_chat(
        settings,
        model="Google/gemma-4-12b-it",
        system="system prompt",
        user="user prompt",
    )

    assert result == "# Brief"
    request = route.calls.last.request
    assert request.headers["Authorization"] == "Bearer LOCAL-API-KEY"
    assert set(request.extensions["timeout"].values()) == {60.0}
    assert json.loads(request.content) == {
        "model": "Google/gemma-4-12b-it",
        "messages": [
            {"role": "system", "content": "system prompt"},
            {"role": "user", "content": "user prompt"},
        ],
    }


@respx.mock
def test_complete_chat_omits_authorization_when_api_key_empty() -> None:
    respx.post("http://llm.test/v1/chat/completions").mock(
        return_value=httpx.Response(
            200,
            json={"choices": [{"message": {"role": "assistant", "content": "ok"}}]},
        )
    )
    settings = LlmSettings(base_url="http://llm.test", api_key="", default_model="model")

    complete_chat(settings, model="", system="system", user="user")

    request = respx.calls.last.request
    assert "Authorization" not in request.headers
    assert json.loads(request.content)["model"] == "model"


@pytest.mark.parametrize(
    ("response", "message"),
    [
        (httpx.Response(500, text="boom"), "HTTP"),
        (
            httpx.Response(
                200,
                json={"choices": [{"message": {"role": "assistant", "content": ""}}]},
            ),
            "пуст",
        ),
    ],
)
@respx.mock
def test_complete_chat_rejects_http_and_empty_responses(
    response: httpx.Response,
    message: str,
) -> None:
    respx.post("http://llm.test/v1/chat/completions").mock(return_value=response)
    settings = LlmSettings(base_url="http://llm.test", api_key="key", default_model="model")

    with pytest.raises(LlmError, match=message):
        complete_chat(settings, model="model", system="system", user="user")


def test_complete_chat_rejects_missing_model() -> None:
    settings = LlmSettings(base_url="http://llm.test", api_key="", default_model="")

    with pytest.raises(LlmError, match="модел"):
        complete_chat(settings, model="", system="system", user="user")
