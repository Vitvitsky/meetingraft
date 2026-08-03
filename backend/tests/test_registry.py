import json
from pathlib import Path

import pytest

from app.registry import RegistryError, load_registry, provider_settings, public_models


def test_load_providers_json(monkeypatch: pytest.MonkeyPatch) -> None:
    payload = {
        "providers": [
            {
                "id": "home-llm",
                "base_url": "http://host:58001/",
                "api_key": "SECRET",
                "default_model": "m1",
                "models": [
                    {"id": "m1", "display_name": "Model One"},
                    {"id": "m2"},
                ],
            }
        ]
    }
    monkeypatch.setenv("PROVIDERS_JSON", json.dumps(payload))
    monkeypatch.delenv("LLM_PROVIDERS_FILE", raising=False)
    monkeypatch.delenv("LLM_BASE_URL", raising=False)

    registry = load_registry()
    assert registry.source == "json"
    assert len(registry.providers) == 1
    assert registry.providers[0].base_url == "http://host:58001"
    models = public_models(registry)
    assert models == [
        {"provider_id": "home-llm", "model": "m1", "display_name": "Model One"},
        {"provider_id": "home-llm", "model": "m2", "display_name": ""},
    ]
    assert "SECRET" not in json.dumps(models)
    settings = provider_settings(registry, "home-llm")
    assert settings.api_key == "SECRET"
    assert settings.default_model == "m1"


def test_empty_models_uses_default_model(monkeypatch: pytest.MonkeyPatch) -> None:
    payload = {
        "providers": [
            {
                "id": "p1",
                "base_url": "http://x",
                "api_key": "",
                "default_model": "only-default",
                "models": [],
            }
        ]
    }
    monkeypatch.setenv("PROVIDERS_JSON", json.dumps(payload))
    models = public_models(load_registry())
    assert models == [
        {"provider_id": "p1", "model": "only-default", "display_name": ""},
    ]


def test_compat_llm_env(monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.delenv("PROVIDERS_JSON", raising=False)
    monkeypatch.delenv("LLM_PROVIDERS_FILE", raising=False)
    monkeypatch.setenv("LLM_BASE_URL", "http://llm.test/")
    monkeypatch.setenv("LLM_API_KEY", "k")
    monkeypatch.setenv("LLM_MODEL", "Google/gemma")
    registry = load_registry()
    assert registry.source == "env_compat"
    assert registry.providers[0].id == "default"
    assert public_models(registry)[0]["model"] == "Google/gemma"


def test_registry_ignores_llm_env_when_json_present(monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.setenv(
        "PROVIDERS_JSON",
        json.dumps(
            {
                "providers": [
                    {
                        "id": "a",
                        "base_url": "http://a",
                        "api_key": "",
                        "default_model": "ma",
                        "models": [{"id": "ma"}],
                    }
                ]
            }
        ),
    )
    monkeypatch.setenv("LLM_BASE_URL", "http://ignored")
    monkeypatch.setenv("LLM_MODEL", "ignored-model")
    registry = load_registry()
    assert registry.source == "json"
    assert [p.id for p in registry.providers] == ["a"]


def test_duplicate_provider_id_fails(monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.setenv(
        "PROVIDERS_JSON",
        json.dumps(
            {
                "providers": [
                    {
                        "id": "dup",
                        "base_url": "http://a",
                        "api_key": "",
                        "default_model": "",
                        "models": [{"id": "m"}],
                    },
                    {
                        "id": "dup",
                        "base_url": "http://b",
                        "api_key": "",
                        "default_model": "",
                        "models": [{"id": "n"}],
                    },
                ]
            }
        ),
    )
    with pytest.raises(RegistryError):
        load_registry()


def test_duplicate_provider_model_pair_fails(monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.setenv(
        "PROVIDERS_JSON",
        json.dumps(
            {
                "providers": [
                    {
                        "id": "p",
                        "base_url": "http://a",
                        "api_key": "",
                        "default_model": "",
                        "models": [{"id": "m"}, {"id": "m"}],
                    }
                ]
            }
        ),
    )
    with pytest.raises(RegistryError):
        load_registry()


def test_invalid_json_fails(monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.setenv("PROVIDERS_JSON", "{not-json")
    with pytest.raises(RegistryError):
        load_registry()


def test_providers_file(monkeypatch: pytest.MonkeyPatch, tmp_path: Path) -> None:
    path = tmp_path / "providers.json"
    path.write_text(
        json.dumps(
            {
                "providers": [
                    {
                        "id": "file-p",
                        "base_url": "http://f",
                        "api_key": "",
                        "default_model": "fm",
                        "models": [{"id": "fm"}],
                    }
                ]
            }
        ),
        encoding="utf-8",
    )
    monkeypatch.delenv("PROVIDERS_JSON", raising=False)
    monkeypatch.setenv("LLM_PROVIDERS_FILE", str(path))
    registry = load_registry()
    assert registry.source == "file"
    assert registry.providers[0].id == "file-p"


def test_unknown_provider_raises(monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.delenv("PROVIDERS_JSON", raising=False)
    monkeypatch.setenv("LLM_BASE_URL", "http://x")
    monkeypatch.setenv("LLM_MODEL", "m")
    with pytest.raises(RegistryError):
        provider_settings(load_registry(), "nope")
