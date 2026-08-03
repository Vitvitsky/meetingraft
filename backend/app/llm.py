"""Клиент OpenAI-compatible провайдера для backend jobs."""

from __future__ import annotations

import os
from dataclasses import dataclass
from typing import Any

import httpx


class LlmError(RuntimeError):
    """Ошибка конфигурации или вызова LLM-провайдера."""


@dataclass(frozen=True, slots=True)
class LlmSettings:
    base_url: str
    api_key: str
    default_model: str


def load_llm_settings() -> LlmSettings:
    """Загрузить настройки LLM из окружения процесса."""
    return LlmSettings(
        base_url=os.environ.get("LLM_BASE_URL", "").rstrip("/"),
        api_key=os.environ.get("LLM_API_KEY", ""),
        default_model=os.environ.get("LLM_MODEL", ""),
    )


def complete_chat(
    settings: LlmSettings,
    *,
    model: str,
    system: str,
    user: str,
    timeout_s: float = 60.0,
) -> str:
    """Выполнить один синхронный OpenAI-compatible chat completion."""
    resolved_model = model.strip() or settings.default_model.strip()
    if not resolved_model:
        raise LlmError("Не указана модель LLM")

    headers = {}
    if settings.api_key:
        headers["Authorization"] = f"Bearer {settings.api_key}"

    try:
        response = httpx.post(
            f"{settings.base_url.rstrip('/')}/v1/chat/completions",
            headers=headers,
            json={
                "model": resolved_model,
                "messages": [
                    {"role": "system", "content": system},
                    {"role": "user", "content": user},
                ],
            },
            timeout=timeout_s,
        )
        response.raise_for_status()
    except httpx.HTTPError as error:
        raise LlmError(f"Ошибка HTTP LLM: {error}") from error

    content = _extract_content(response)
    if not content.strip():
        raise LlmError("LLM вернул пустой ответ")
    return content


def _extract_content(response: httpx.Response) -> str:
    try:
        payload: Any = response.json()
        content = payload["choices"][0]["message"]["content"]
    except (IndexError, KeyError, TypeError, ValueError) as error:
        raise LlmError("LLM вернул некорректный ответ") from error
    if not isinstance(content, str):
        raise LlmError("LLM вернул некорректный ответ")
    return content
