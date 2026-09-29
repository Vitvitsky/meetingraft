# AGENTS.md — backend

Python-заглушка по ADR-007 (slice A). Это **не** целевая архитектура:
хранилища, очереди, диаризации и уточнения расшифровки здесь нет. Не
описывай целевое устройство как реализованное.

## Устройство

- `app/main.py` — FastAPI-приложение, все маршруты, словари `_jobs` /
  `_artifacts` в памяти.
- `app/llm.py` — синхронный клиент, совместимый с OpenAI.
- `app/perplexity.py` — веб-ответы через Perplexity Agent API (SDK
  `perplexityai`). Наружу уходит только запрос; `store=False`.
- `app/registry.py` — загрузчик реестра провайдеров.
- `tests/` — семь плоских файлов (`test_api`, `test_models_api`,
  `test_jobs_llm`, `test_llm`, `test_registry`, `test_perplexity`,
  `test_answers_api`). Нет `conftest.py` и
  своих маркеров; фикстуры — `monkeypatch`, модульный `TestClient(app)`,
  `respx` для HTTP к LLM.

Маршруты: `GET /health` (без авторизации), `GET /v1/models`,
`POST /v1/jobs` (201), `GET /v1/jobs/{id}`, `GET /v1/artifacts/{id}`,
`POST /v1/answers`.
Bearer-авторизация. `lifespan` зовёт `load_registry()` и падает сразу на
битом JSON или дубле id.

Реестр, порядок: `PROVIDERS_JSON` > `LLM_PROVIDERS_FILE` (если файл есть)
> совместимый `LLM_BASE_URL` (синтетический id `default`, источник
`env_compat`) > `empty`. `public_models()` срезает секреты.

## Ловушки

- **Токен читается один раз при импорте**: `EXPECTED_TOKEN =
  os.environ.get("MEETINGRAFT_API_TOKEN", "dev-token")` (`app/main.py:40`).
  Смена переменной на ходу не действует — в отличие от реестра, который
  перечитывается на каждый запрос через `get_registry()` (намеренно, для
  тестов). Проверка на старте и чтение на запросе могут разойтись; для
  заглушки это принято.
- `queued`/`running` из `JobStatus` (`shared/openapi.yaml`) **никогда не
  выдаются** — только `succeeded`/`failed`. HTTP-запрос блокируется на
  всё время вызова LLM.
- Ошибка LLM **не** откатывается на заглушку: джоба становится `failed` с
  `artifact_ids=[]`. Заглушечный markdown появляется, только если
  провайдера с `base_url` нет вовсе (правило продукта: молчаливый откат
  хуже видимой ошибки).
- `provider_id` обязателен, если источник реестра не `env_compat`; в
  режиме реестра payload без него даёт failed-джобу.
- `base_url` LLM **не** включает `/v1` — клиент дописывает
  `/v1/chat/completions` сам.
- `shared/openapi.yaml` ведётся **руками** (ADR-007 говорит «экспорт из
  FastAPI», но файл лежит в репозитории), поэтому расходится; в нём уже
  есть нереализованные `queued`/`running`.
- `backend/docker-compose.yml` нет; единственный compose — в корне репо.
- `_jobs`/`_artifacts` — словари уровня модуля: не потокобезопасны, теряются
  при перезапуске, не разделяются между воркерами.
- Dockerfile ставит через `pip install .` (**не** uv) и копирует только
  `pyproject.toml` + `app/` — ни тестов, ни dev-зависимостей, ни
  `uv.lock` в образе.
- `/v1/answers` не заводит джобу и артефакт: ответ отдаётся прямо в теле,
  состояние не хранится. Ключа нет — **503** (не настроено), ошибка
  провайдера — **502**; заглушки вместо ответа нет. Запрос из одних
  пробелов — 422 от модели запроса, до проверки ключа.
- SDK `perplexityai` ходит на `/v1/responses` (алиас `/v1/agent`), а не
  на `/v1/agent` напрямую — это тот же Agent API. Мокать в тестах надо
  `https://api.perplexity.ai/v1/responses`. Ретраи (2) и бэкофф — внутри
  SDK, `respx` их видит как повторные вызовы.
- `PERPLEXITY_API_KEY` SDK читает сам из окружения; в `app/perplexity.py`
  ключ передаётся в клиент явно и **никогда** не печатается.

## Конвенции

- `requires-python >=3.13`; uv: `uv sync --extra dev`, `uv run pytest`,
  `uv run ruff check app tests`.
- Ruff: line-length 100, target py313, `select = ["E","F","I","UP"]`.
- pytest: `testpaths = ["tests"]`.
- Порт 8080, токен по умолчанию `dev-token`.
- Переменные: `PROVIDERS_JSON`, `LLM_PROVIDERS_FILE`, `LLM_BASE_URL`,
  `LLM_API_KEY`, `LLM_MODEL`, `MEETINGRAFT_API_TOKEN`,
  `PERPLEXITY_API_KEY`, `PERPLEXITY_PRESET`, `PERPLEXITY_MODEL`.
- Pre-commit-хук: `ruff-backend`.
