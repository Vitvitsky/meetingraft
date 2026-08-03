# MeetingRaft — архитектура (схемы) и установка

Документ для онбординга: как устроен стек сейчас и как поднять прототип
локально. Детали решений — в [architecture.md](architecture.md) и ADR в
`docs/adr/`.

**Статус прототипа (2026-08):** Phase 0–6 local MVP + ADR-007 **slice A**
(FastAPI jobs; optional OpenAI-compat LLM via `LLM_*`) + **Speakers skeleton** +
локальные LLM (Ollama / OpenAI-compatible). WhisperX / diarization /
provider registry — ещё не в рантайме.

---

## 1. Отрисованная архитектура

### 1.1 Слои (runtime)

```mermaid
flowchart TB
  subgraph macos["macOS shell — apps/macos"]
    UI["SwiftUI\nLive Captions · Meetings · Glossary · Settings"]
    AV["AVFoundation\nmic (+ system tap)"]
    Host["HostTranslationBridge\nApple path stub"]
  end

  subgraph rust["Rust core — UniFFI"]
    FFI["meetingraft-ffi\nMeetingCore"]
    STT["meetingraft-stt\nWhisper / Mock"]
    SES["meetingraft-session"]
    GLO["meetingraft-glossary"]
    PC["meetingraft-postcall\nFinal + Brief/Follow-up templates"]
    TR["meetingraft-translate\nstub / http / host queue"]
    SYNC["meetingraft-sync\nHTTP client"]
    STORE["meetingraft-storage\nSQLite"]
  end

  subgraph data["Локальные данные"]
    AS["~/Library/Application Support/meetingraft/\nmodels/ · SQLite · chunks"]
  end

  subgraph be["Backend — docker compose :8080"]
    API["FastAPI stub\n/health · /v1/jobs · /v1/artifacts"]
    MEM["In-memory store\n(slice A, без Postgres)"]
  end

  UI --> FFI
  AV --> FFI
  Host --> FFI
  FFI --> STT & SES & GLO & PC & TR & SYNC & STORE
  STORE --> AS
  STT --> AS
  SYNC -->|"Bearer + OpenAPI"| API
  API --> MEM
```

### 1.2 Live-сессия (Stage 1)

```mermaid
sequenceDiagram
  participant User
  participant Swift as SwiftUI / AVFoundation
  participant Core as MeetingCore (Rust)
  participant STT as SttEngine
  participant DB as SQLite

  User->>Swift: Start Live
  Swift->>Core: setSessionLanguage + startRecording
  Core->>DB: begin_session
  loop chunks ~100 ms
    Swift->>Core: ingestAudioChunk(mic)
    Core->>STT: push PCM
    STT-->>Core: partial/final CaptionEvent
    Core->>DB: append_caption
    Swift->>Core: drainLiveCaptions
    Core-->>Swift: events → UI
  end
  User->>Swift: Stop Live
  Swift->>Core: stopRecording
  Core->>STT: flush
  Core->>DB: assemble FinalTranscript
```

### 1.3 Post-call и backend (Stage 2 сейчас)

```mermaid
flowchart LR
  subgraph local["На устройстве"]
    Final["FinalTranscript\nиз live finals + glossary"]
    Art["Brief / Follow-up\nbuiltin templates или local LLM"]
    Final --> Art
  end

  LLM["Ollama / OpenAI-compatible\n:11434"]
  Art -->|"POST /api/chat или\n/v1/chat/completions"| LLM

  subgraph optional["Опционально — stub API"]
    Jobs["POST /v1/jobs"]
    Artifacts["GET /v1/artifacts/{id}"]
    Jobs --> Artifacts
  end

  Settings["Settings → Backend API\nTest API / health"] --> Jobs
  Meetings["Meetings → Live | Final | Speakers | Artifacts\nSubmit refine (stub)"] --> Jobs
  Final --> Meetings
  Speakers["Speakers tab\nручные метки (SQLite)"] --> Meetings
```

**Provenance (Meetings UI):** Live ≠ вход для Brief; Brief/Follow-up ← **Final**.

### 1.4 Карта репозитория

```text
meetingraft/
├─ apps/macos/          SwiftUI + Generated UniFFI + Scripts/
├─ rust/crates/         domain, session, stt, glossary, postcall,
│                       translate, sync, storage, ffi
├─ backend/             FastAPI stub (uv + pytest)
├─ shared/openapi.yaml  контракт ADR-007
├─ docker-compose.yml   api:8080
└─ docs/                ADR, roadmap, этот файл
```

---

## 2. Процедура установки (потенциальный / текущий прототип)

### 2.1 Требования

| Компонент | Минимум |
|-----------|---------|
| OS | macOS 15+ (Apple Silicon предпочтительно для Whisper Metal) |
| Xcode | 16+ с Command Line Tools |
| Rust | stable (`rustup`) |
| Прочее | [XcodeGen](https://github.com/yonaskolb/XcodeGen), опционально [SwiftFormat](https://github.com/nicklockwood/SwiftFormat) |
| Backend (опц.) | Docker Desktop **или** Python 3.13 + [uv](https://github.com/astral-sh/uv) |
| Whisper (опц.) | `curl` / `hf` CLI для ggml с Hugging Face |

### 2.2 Клонирование

```bash
git clone https://github.com/Vitvitsky/meetingraft.git
cd meetingraft
git checkout main   # или feature-ветка
```

### 2.3 Rust + UniFFI + macOS app

```bash
# 1) FFI dylib + Swift bindings + Xcode project
apps/macos/Scripts/generate-ffi.sh
# по умолчанию включает --features whisper; для CI без Metal:
# MEETINGRAFT_FFI_FEATURES= apps/macos/Scripts/generate-ffi.sh

# 2) Открыть в Xcode
open apps/macos/MeetingRaft.xcodeproj
# ⌘R — Run

# или CLI
cd apps/macos
xcodebuild -project MeetingRaft.xcodeproj -scheme MeetingRaft \
  -configuration Debug build CODE_SIGNING_ALLOWED=NO
```

Данные приложения:
`~/Library/Application Support/meetingraft/`

### 2.4 Модель Whisper (live STT)

Без файла модели используется **Mock** STT.

```bash
# dev (~base)
apps/macos/Scripts/download-stt-model.sh

# prod-ориентир
MODEL=large-v3-turbo apps/macos/Scripts/download-stt-model.sh

# затем пересобрать FFI с whisper (скрипт по умолчанию уже так делает)
apps/macos/Scripts/generate-ffi.sh
```

Файлы: `…/meetingraft/models/ggml-*.bin` (Hugging Face `ggerganov/whisper.cpp`).

<a id="backend-setup"></a>

### 2.5 Backend stub — настройка (опционально)

Сейчас это **ADR-007 slice A**: FastAPI in-memory jobs без Postgres / Redis /
WhisperX. `refine` (и `brief`/`follow_up` без `LLM_BASE_URL`) — stub markdown.
С `LLM_*` jobs `brief`/`follow_up` вызывают OpenAI-compatible провайдер.
Контракт: [`shared/openapi.yaml`](../shared/openapi.yaml).

#### Шаг 1. Поднять API

Из корня репозитория:

```bash
# Вариант A — Docker (рекомендуется)
# без LLM — только stub jobs:
docker compose up --build

# с реальным LLM (пример):
export LLM_BASE_URL=http://93.189.243.223:58001   # без /v1
export LLM_API_KEY=LOCAL-API-KEY
export LLM_MODEL=Google/gemma-4-12b-it
docker compose up --build
# слушает http://127.0.0.1:8080
# MEETINGRAFT_API_TOKEN=dev-token (см. docker-compose.yml)
```

```bash
# Вариант B — локально через uv
cd backend
uv sync --extra dev
MEETINGRAFT_API_TOKEN=dev-token \
LLM_BASE_URL=http://93.189.243.223:58001 \
LLM_API_KEY=LOCAL-API-KEY \
LLM_MODEL=Google/gemma-4-12b-it \
  uv run uvicorn app.main:app --host 127.0.0.1 --port 8080
```

Проверка без приложения:

```bash
curl -s http://127.0.0.1:8080/health
# {"status":"ok"}

curl -s -o /dev/null -w "%{http_code}\n" \
  -H "Authorization: Bearer dev-token" \
  -H "Content-Type: application/json" \
  -d '{"meeting_id":"m1","kind":"brief","primary_language":"ru","allowed_languages":["ru"]}' \
  http://127.0.0.1:8080/v1/jobs
# 201
```

| Параметр | Значение по умолчанию | Где задаётся |
|----------|------------------------|--------------|
| URL | `http://127.0.0.1:8080` | порт compose / uvicorn |
| Bearer token | `dev-token` | `MEETINGRAFT_API_TOKEN` |
| Auth | HTTP Bearer | на все `/v1/*`; `/health` без токена |
| `LLM_BASE_URL` | пусто (= stub brief/follow_up) | env backend; **без** `/v1` |
| `LLM_API_KEY` | пусто (= без Authorization к LLM) | env backend |
| `LLM_MODEL` | пусто (fallback, если model с app пуст) | env backend |

Смена токена MeetingRaft API: тот же `MEETINGRAFT_API_TOKEN` в окружении API
**и** в Settings приложения (иначе 401). Секреты LLM провайдера — **только**
в env backend, не во фронте.

#### Шаг 2. Прописать в приложении

**Settings → Backend API (ADR-007)** (дефолты уже совпадают со stub):

| Поле | Значение для local stub |
|------|-------------------------|
| API base URL | `http://127.0.0.1:8080` |
| Bearer token | `dev-token` |

Нажать **Test API** → должно стать **OK** (`GET /health` через UniFFI sync).

Замечания:

- Значения живут в `ProviderSettingsStore` (сессия UI); после перезапуска app
  снова подставятся дефолты `8080` / `dev-token`, пока нет Keychain persistence.
- Перед Generate / Submit refine shell передаёт URL+token в Rust через
  `setApiConfig` (`applyProviderConfig` на экране встречи / onChange в Settings).

#### Шаг 3. Что включать в Providers

Backend URL нужен для двух сценариев (не путать с локальным Ollama §2.6):

1. **LLM = Backend** (**Settings → Providers → LLM**)
   - Engine: **Backend LLM**
   - **Model id** (например `Google/gemma-4-12b-it`) — уходит в job `payload.model`
   - Language-aware `system`/`user` собирает Rust (`brief_prompts` /
     `follow_up_prompts`) и кладёт в `payload`; backend только
     `POST {LLM_BASE_URL}/v1/chat/completions`
   - Meetings: **Generate Brief** / **Generate Follow-up** → jobs → артефакт
     `backend.brief` / `backend.follow_up`
   - Без `LLM_BASE_URL` на сервере — stub markdown; с LLM и ошибкой провайдера —
     job `failed`, **без** silent stub и без fallback на templates

2. **Artifacts → Submit refine (stub)**
   Тот же API, `kind: refine` (пока всегда stub). Нужен Backend API + Final.

`builtin_templates` / `ollama` / `openai_compat` **не** ходят в этот FastAPI
для Brief/Follow-up (локальные пути — §2.6).

#### Типичные ошибки

| Симптом | Что проверить |
|---------|----------------|
| Test API = Fail | `docker compose` / uvicorn запущен; URL без trailing slash; порт 8080 |
| 401 / invalid token | токен в Settings == `MEETINGRAFT_API_TOKEN` на сервере |
| Generate Brief → stub `# Stub brief` | задан ли `LLM_BASE_URL` на backend |
| Generate Brief ошибка / failed job | `LLM_API_KEY`, model id, доступность провайдера; Final есть |
| 401 от LLM | ключ в `LLM_API_KEY`, не в Settings Bearer MeetingRaft |
| Путаница с Ollama | Ollama — §2.6 (`:11434`); jobs API — `:8080` |

### 2.6 Локальный LLM: Ollama / OpenAI-compatible (опционально)

```bash
brew install ollama
ollama serve
# в другом terminal
ollama pull gemma2
```

В **Settings → Providers → LLM**:

- **Ollama:** Base URL `http://127.0.0.1:11434`, Model id `gemma2`;
  приложение вызывает `POST /api/chat`.
- **OpenAI-compatible:** тот же Base URL и Model id для Ollama либо адрес
  другого совместимого сервера; приложение вызывает
  `POST /v1/chat/completions`.

Затем открыть завершённую встречу с Final transcript и нажать
**Generate Brief** или **Generate Follow-up**. Ошибка локального LLM
показывается явно: fallback на builtin templates не выполняется.

### 2.7 Проверка тестами

```bash
# Rust
cd rust && cargo test && cargo fmt --check && cargo clippy --all-targets -- -D warnings

# Backend
cd backend && uv sync --extra dev && uv run ruff check app tests && uv run pytest

# macOS
cd apps/macos && xcodegen generate
xcodebuild -project MeetingRaft.xcodeproj -scheme MeetingRaft \
  -configuration Debug test CODE_SIGNING_ALLOWED=NO
```

CI: `.github/workflows/ci.yml` (rust + macos + backend).

Локально перед push: `pre-commit install` (один раз) и/или
`pre-commit run --all-files` — см. `.pre-commit-config.yaml` и `AGENTS.md`.

### 2.8 Минимальный smoke после установки

1. Запустить app → **Start Captions** (demo) — появляются строки.
2. Сменить Language → English → снова demo — английский скрипт.
3. (Опц.) Whisper model + **Start Live** — captions / Mock.
4. Stop Live → **Meetings** → Final / **Speakers** (Add, rename, delete) / Generate Brief.
5. (Опц.) Backend по §2.5: `docker compose up` → Settings **Test API** = OK.
6. (Опц.) Settings **LLM = Backend** → **Meetings** → Final → **Generate Brief** → markdown из stub job (`kind: brief`).
7. (Опц.) Запустить Ollama с моделью → Settings **LLM = Ollama**, URL
   `http://127.0.0.1:11434`, model id → **Generate Brief**; затем выбрать
   **OpenAI-compatible** с тем же URL и повторить.
8. (Опц.) **Meetings** → **Artifacts** → **Submit refine (stub)** → refine markdown из backend (§2.5 шаг 3).

### 2.9 Потенциальная «продакшен»-инсталляция (ещё не автоматизирована)

Целевое направление (не реализовано end-to-end):

1. Подписанный / notarized `.app` (Phase 7).
2. First-run download ggml в Application Support.
3. Backend на домашнем сервере: полный ADR-007 (Postgres, Redis, Dramatiq, MinIO, WhisperX).
4. Token в Keychain; `apiBaseUrl` на HTTPS.
5. Опционально: NLLB translate worker и production LLM worker для Brief.

Пока используйте §2.3–2.8 как единственную поддерживаемую процедуру.

---

## 3. Связанные документы

| Документ | Содержание |
|----------|------------|
| [architecture.md](architecture.md) | Принципы, latency, privacy |
| [roadmap.md](roadmap.md) | Фазы 0–7 |
| [backlog.md](backlog.md) | Эпики |
| [adr/](adr/) | ADR-001…008 |
| [../AGENTS.md](../AGENTS.md) | Команды агентов / границы слоёв |
| [../README.md](../README.md) | Короткий обзор репо |
| [../shared/openapi.yaml](../shared/openapi.yaml) | Контракт backend jobs (ADR-007) |
| §2.5 выше | Пошаговая настройка backend stub в Settings |
