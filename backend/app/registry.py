"""Загрузка JSON-реестра LLM-провайдеров из окружения."""

from __future__ import annotations

import json
import os
from dataclasses import dataclass
from pathlib import Path
from typing import Any

from app.llm import LlmSettings


class RegistryError(RuntimeError):
    """Ошибка загрузки или разбора реестра провайдеров."""


@dataclass(frozen=True, slots=True)
class ProviderModel:
    id: str
    display_name: str


@dataclass(frozen=True, slots=True)
class Provider:
    id: str
    base_url: str
    api_key: str
    default_model: str
    models: tuple[ProviderModel, ...]


@dataclass(frozen=True, slots=True)
class Registry:
    providers: tuple[Provider, ...]
    source: str


def load_registry() -> Registry:
    """Загрузить реестр провайдеров из окружения процесса."""
    providers_json = os.environ.get("PROVIDERS_JSON", "").strip()
    if providers_json:
        return _parse_providers_json(providers_json, source="json")

    providers_file = os.environ.get("LLM_PROVIDERS_FILE", "").strip()
    if providers_file:
        path = Path(providers_file)
        if path.is_file():
            return _parse_providers_json(path.read_text(encoding="utf-8"), source="file")

    base_url = os.environ.get("LLM_BASE_URL", "").strip()
    if base_url:
        return _load_compat_registry(base_url)

    return Registry(providers=(), source="empty")


def public_models(registry: Registry) -> list[dict[str, str]]:
    """Публичный каталог моделей без секретов."""
    result: list[dict[str, str]] = []
    for provider in registry.providers:
        if not provider.models and provider.default_model:
            result.append(
                {
                    "provider_id": provider.id,
                    "model": provider.default_model,
                    "display_name": "",
                }
            )
            continue
        for model in provider.models:
            result.append(
                {
                    "provider_id": provider.id,
                    "model": model.id,
                    "display_name": model.display_name,
                }
            )
    return result


def provider_settings(registry: Registry, provider_id: str) -> LlmSettings:
    """Настройки LLM для указанного провайдера."""
    for provider in registry.providers:
        if provider.id == provider_id:
            if not provider.base_url:
                raise RegistryError(f"У провайдера «{provider_id}» не задан base_url")
            return LlmSettings(
                base_url=provider.base_url,
                api_key=provider.api_key,
                default_model=provider.default_model,
            )
    raise RegistryError(f"Провайдер не найден: {provider_id}")


def _load_compat_registry(base_url: str) -> Registry:
    default_model = os.environ.get("LLM_MODEL", "").strip()
    models: tuple[ProviderModel, ...] = ()
    if default_model:
        models = (ProviderModel(id=default_model, display_name=""),)
    provider = Provider(
        id="default",
        base_url=base_url.rstrip("/"),
        api_key=os.environ.get("LLM_API_KEY", ""),
        default_model=default_model,
        models=models,
    )
    return Registry(providers=(provider,), source="env_compat")


def _parse_providers_json(raw: str, *, source: str) -> Registry:
    try:
        data: Any = json.loads(raw)
    except json.JSONDecodeError as error:
        raise RegistryError("Невалидный JSON реестра провайдеров") from error

    if not isinstance(data, dict):
        raise RegistryError("Реестр провайдеров должен быть JSON-объектом")

    providers_raw = data.get("providers")
    if not isinstance(providers_raw, list):
        raise RegistryError("Поле providers должно быть массивом")

    providers: list[Provider] = []
    seen_ids: set[str] = set()

    for entry in providers_raw:
        provider = _parse_provider(entry)
        if provider.id in seen_ids:
            raise RegistryError(f"Дубликат provider.id: {provider.id}")
        seen_ids.add(provider.id)
        providers.append(provider)

    return Registry(providers=tuple(providers), source=source)


def _parse_provider(entry: Any) -> Provider:
    if not isinstance(entry, dict):
        raise RegistryError("Элемент providers должен быть объектом")

    provider_id = _require_str(entry, "id")
    base_url = _require_str(entry, "base_url").rstrip("/")
    api_key = _optional_str(entry, "api_key")
    default_model = _optional_str(entry, "default_model")
    models_raw = entry.get("models", [])
    if not isinstance(models_raw, list):
        raise RegistryError("Поле models должно быть массивом")

    models: list[ProviderModel] = []
    seen_model_ids: set[str] = set()
    for model_entry in models_raw:
        model = _parse_model(model_entry)
        if model.id in seen_model_ids:
            raise RegistryError(
                f"Дубликат model.id «{model.id}» у провайдера «{provider_id}»"
            )
        seen_model_ids.add(model.id)
        models.append(model)

    return Provider(
        id=provider_id,
        base_url=base_url,
        api_key=api_key,
        default_model=default_model,
        models=tuple(models),
    )


def _parse_model(entry: Any) -> ProviderModel:
    if not isinstance(entry, dict):
        raise RegistryError("Элемент models должен быть объектом")
    model_id = _require_str(entry, "id")
    display_name = _optional_str(entry, "display_name")
    return ProviderModel(id=model_id, display_name=display_name)


def _require_str(entry: dict[str, Any], key: str) -> str:
    value = entry.get(key)
    if not isinstance(value, str) or not value.strip():
        raise RegistryError(f"Обязательное поле «{key}» должно быть непустой строкой")
    return value.strip()


def _optional_str(entry: dict[str, Any], key: str) -> str:
    value = entry.get(key, "")
    if value is None:
        return ""
    if not isinstance(value, str):
        raise RegistryError(f"Поле «{key}» должно быть строкой")
    return value
