"""Веб-ответы через Perplexity Agent API (официальный SDK ``perplexityai``)."""

from __future__ import annotations

import os
from dataclasses import dataclass
from typing import Any

import perplexity
from perplexity import Perplexity

#: Пресет по умолчанию: повседневный поиск с цитатами (docs/agent-api/presets).
DEFAULT_PRESET = "low"
DEFAULT_TIMEOUT_S = 120.0


class PerplexityError(RuntimeError):
    """Ошибка конфигурации или вызова Perplexity Agent API."""


@dataclass(frozen=True, slots=True)
class WebSource:
    """Источник, на который опирается веб-ответ."""

    title: str
    url: str
    date: str | None = None


@dataclass(frozen=True, slots=True)
class WebAnswer:
    """Веб-ответ вместе со ссылками на источники."""

    text: str
    sources: tuple[WebSource, ...]
    model: str
    response_id: str


def load_perplexity_api_key() -> str:
    """Ключ Perplexity из окружения процесса. Значение нигде не печатается."""
    return os.environ.get("PERPLEXITY_API_KEY", "").strip()


def answer(
    query: str,
    *,
    preset: str = "",
    model: str = "",
    instructions: str = "",
    language: str = "",
    previous_response_id: str = "",
    timeout_s: float = DEFAULT_TIMEOUT_S,
) -> WebAnswer:
    """Получить веб-ответ с источниками через ``POST /v1/agent``.

    ``store=False``: ответ не сохраняется на стороне Perplexity для
    последующего retrieve — наружу уходит только сам запрос. Ключ не
    логируется. Пустой или неуспешный ответ — видимая ошибка, а не заглушка.
    """
    prompt = query.strip()
    if not prompt:
        raise PerplexityError("Не указан запрос к Perplexity")

    api_key = load_perplexity_api_key()
    if not api_key:
        raise PerplexityError("Не задан PERPLEXITY_API_KEY")

    resolved_model = model.strip() or os.environ.get("PERPLEXITY_MODEL", "").strip()
    resolved_preset = ""
    if not resolved_model:
        resolved_preset = (
            preset.strip() or os.environ.get("PERPLEXITY_PRESET", "").strip() or DEFAULT_PRESET
        )

    request: dict[str, Any] = {
        "input": prompt,
        "tools": [{"type": "web_search"}],
        "store": False,
    }
    if resolved_model:
        request["model"] = resolved_model
    else:
        request["preset"] = resolved_preset
    if instructions.strip():
        request["instructions"] = instructions.strip()
    if language.strip():
        request["language_preference"] = language.strip()
    if previous_response_id.strip():
        request["previous_response_id"] = previous_response_id.strip()

    client = Perplexity(api_key=api_key, timeout=timeout_s)
    try:
        response = client.responses.create(**request)
    except perplexity.APIError as error:
        raise PerplexityError(f"Ошибка Perplexity API: {error}") from error
    except perplexity.PerplexityError as error:
        raise PerplexityError(f"Ошибка клиента Perplexity: {error}") from error

    if response.status != "completed":
        raise PerplexityError(f"Perplexity вернул статус «{response.status}»")

    text = response.output_text.strip()
    if not text:
        raise PerplexityError("Perplexity вернул пустой ответ")

    return WebAnswer(
        text=text,
        sources=_extract_sources(response),
        model=response.model,
        response_id=response.id,
    )


def _extract_sources(response: Any) -> tuple[WebSource, ...]:
    """Источники из ``output``: элементы search_results и URL-аннотации текста."""
    sources: list[WebSource] = []
    seen: set[str] = set()

    def add(url: str | None, title: str | None, date: str | None) -> None:
        if not url or url in seen:
            return
        seen.add(url)
        sources.append(WebSource(title=title or url, url=url, date=date))

    for item in response.output:
        if item.type == "search_results":
            for result in item.results:
                add(result.url, result.title, result.date)
        elif item.type == "message":
            for content in item.content:
                for annotation in content.annotations or ():
                    add(annotation.url, annotation.title, None)

    return tuple(sources)
