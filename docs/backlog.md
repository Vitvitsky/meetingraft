# MeetingRaft Backlog

## Epic 1 — Repository Bootstrap
- Create repo skeleton
- Add AGENTS.md
- Add architecture.md
- Add ADR template and first ADRs
- Add contribution and coding conventions

## Epic 2 — Native macOS Shell
- [x] Create SwiftUI app shell
- [x] Add sidebar and toolbar
- [x] Add settings scene
- [x] Add menu commands and keyboard shortcuts
- [x] Add fake subtitle stream screen

## Epic 3 — Rust Core
- [x] Create domain crate
- [x] Create session engine crate
- [x] Create glossary engine crate
- [x] Create sync client crate — `meetingraft-sync` (ADR-007 slice A)
- [x] Create UniFFI facade crate

## Epic 4 — Swift ↔ Rust Boundary
- [x] Define UniFFI contracts
- [x] Wire generated Swift bindings into Xcode
- [x] Expose simple DTO-based interfaces
- [x] Add integration smoke test

## Epic 5 — Audio Capture
- [x] Add AVFoundation capture manager
- Add device selection
- [x] Add permissions flow
- [x] Add chunking pipeline
- [x] Add local raw recording manifest
- [x] System audio process tap (ADR-004) — Core Audio process tap +
  приватное aggregate-устройство; каналы раздельны на диске, live-путь
  идёт через микс с атрибуцией (ADR-009)
- Непрерывный ресемплер в `PCMDownmixer` — сейчас `converter.reset()`
  после каждого чанка сбрасывает состояние ресемплера, из-за чего на
  границах 100 мс кусков остаются мелкие разрывы. Сделано ради хвоста
  через `endOfStream` (без него терялось ~15% кадров на 48→16 kHz).
  Правильное решение — держать конвертер живым и не сигналить
  `endOfStream` на каждом чанке. Важно для Phase 10, где точность и есть
  цель.

## Epic 6 — Live Subtitle Flow
- [x] Open session with STT pipeline (Mock; Whisper when model + feature)
- Pass language policy: primary `ru`, allowed `{ru, en, es}`
- [x] Stream chunks → SttEngine
- [x] Render partial captions
- [x] Merge final captions
- [x] Save local caption events
- Settings: session language override (default Russian) — stub exists
- [x] STT model picker в Settings (Whisper ggml: auto / base / small / large-v3-turbo; HF download; first-run `ggml-base.bin`)
- Whisper Metal + model download script (opt-in `--features whisper`)

## Epic 7 — Glossary
- [x] Create glossary domain model
- [x] Add glossary UI
- [x] Add import from CSV/TXT
- Add scope: global/workspace/project/meeting — **partial:** global + meeting in MVP
- [x] Attach glossary to live session
- Glossary candidates from transcript corrections (review feedback loop)
- Post-call mining of candidates (acronyms, code-switching terms) with
  approval queue

## Epic 8 — Post-call Intelligence
- [x] Trigger refinement after meeting end — **local MVP:** assemble on Stop Live
  (backend refinement / ADR-007 HTTP deferred)
- [x] Fetch final transcript — SQLite `final_transcripts`
- [x] Show transcript review screen — Meetings detail: Live | Final | Artifacts
- Artifact template system: built-in templates — **partial:** Brief + Follow-up
  (technical requirements, meeting minutes, action items deferred)
- User-defined markdown templates (prompt + placeholders: transcript,
  brief, glossary, participants) — **deferred**
- Template picker, regeneration and versioning — **partial:** generate Brief /
  Follow-up in UI; versioning deferred
- Export artifacts — **partial:** copy to clipboard + **.md file export**
  (Final + Brief/Follow-up → Settings export folder, flat Obsidian-friendly
  names; `feat/markdown-export-obsidian`); mail draft **deferred**
- Obsidian plugin / export HTTP API — **deferred:** pull meetings from app
  or backend без ручного folder export (spec
  `docs/superpowers/specs/2026-08-03-markdown-export-obsidian-design.md`)
- Real LLM generation — **partial:**
  - Local: Ollama native + OpenAI-compatible из app
  - Backend jobs: Settings LLM=Backend + Model id → payload prompts →
    OpenAI-compat провайдер из env (`LLM_BASE_URL` / `LLM_API_KEY` /
    `LLM_MODEL`)
  - Streaming/tools **deferred**
- Backend provider platform — **partial:** static JSON registry
  (`PROVIDERS_JSON` / `LLM_PROVIDERS_FILE`), `GET /v1/models`, Settings picker
  `(provider_id, model)`, job routing по `provider_id`; compat `LLM_*` →
  `default` (`feat/backend-provider-registry`)
  - CRUD API / UI «добавить провайдера», billing, live upstream discovery —
    **deferred**
- Parakeet on-device STT (второй engine рядом с Whisper) — **deferred**
- Remote STT API (latency risk для live; не default) — **deferred**
- Более жирная модель для глубокого анализа полного аудио / refined
  transcript — **deferred**
- [x] Backend HTTP (ADR-007) — **slice A:** OpenAPI + FastAPI stub jobs +
  `meetingraft-sync` + Settings Test API (`feat/phase-6-backend-stub`)
- [x] Meetings UI: Submit refine (stub) → poll → show artifact
  (`feat/meetings-backend-refine-stub`)
- [x] Backend LLM provider for brief/follow_up jobs (`feat/backend-llm-provider`)
- Create sync client crate — **done** (`meetingraft-sync`)

## Epic 9 — Speaker Assignment
- [x] Add speaker entities — **skeleton:** `domain::Speaker`, SQLite `speakers`,
  UniFFI list/upsert/delete (`feat/speakers-skeleton`)
- [x] Add speaker correction screen — **partial (skeleton):** Meetings detail
  **Speakers** tab: ручные метки (add/rename/delete), banner «diarization — скоро»;
  без diarization и без привязки к Final transcript
- [x] Add versioned refined transcript — Stop Live / re-assemble → next version
  (`max+1`); Final tab picker; Brief/Follow-up/Export = latest
  (`feat/final-versions-compare`)
- [x] Compare live vs final transcript — Meetings **Compare** tab: side-by-side
  Live finals | Final vN (`feat/final-versions-compare`)
- [x] Привязка спикеров к сегментам Final — атрибуция по каналам
  (ADR-012): спикер на канал, массовое назначение, точечная правка с
  флагом, очистка ссылок при удалении
- [x] Спикеры по умолчанию при пересборе; детерминированный id, имя
  человека переживает повторный проход
- [x] Сводка по участникам: доля речи, число реплик, время
- Экран спикеров и сегменты в Final UI — **в работе** (Phase 11, T4–T5)
- Спикеры в артефактах и экспорте — **в работе** (Phase 11, T6)
- Кластеризация голосов внутри системного канала — **deferred**,
  открывается при спросе на многосторонние встречи (ADR-012).
  **Сигнал получен 2026-08-04:** очная запись двух человек в один
  микрофон ноутбука. Это ровно тот случай, который атрибуция по каналам
  не берёт и взять не может — канал один. Сценарий «диктофон на столе»
  не был учтён в ADR-012: там подразумевался звонок
- Распознавание людей между встречами — **не делаем**: биометрия голоса
  противоречит local-first без явной просьбы пользователя

## Epic 13 — Post-call re-ASR (Phase 10)
- [x] Чтение сохранённых чанков сессии по каналам
- [x] Контракт пакетного распознавания + Whisper-реализация
- [x] Сегменты Final с тайм-кодами и каналом в хранилище
- [x] Слияние дорожек в хронологию без эвристики
- [x] LLM-полировка со сверкой ответа по номерам строк
- [x] Фоновый джоб: прогресс, отмена, приоритет записи
- [x] Пословная диффа Live против Final
- [x] UI пересбора: кнопка, прогресс, отмена, provenance
- Backend WhisperX вторым путём — **deferred** (T7)
- Provenance в `final_transcripts`, чтобы переживал перезапуск
- Сегменты с тайм-кодами и диффа в UI — вместе с D2 редизайна

## Epic 11 — Meetings library
- [x] Название встречи (задаётся при старте записи, переименование в UI)
- [x] Время окончания и длительность
- [x] Полнотекстовый поиск FTS5 по finals / live-финалам / артефактам
- [x] Каскадное удаление встречи (строки, индекс, PCM-чанки)
- [x] Meetings — стартовый раздел приложения
- [x] Скачивание STT-модели при первом запуске вне Settings
- Фильтры по времени (Today / This week / Older) — ТЗ редизайна §4.2
- Участники встречи — после diarization (Epic 9)

## Epic 15 — Синхронный перевод (ADR-008)

Обвязка построена, **самого перевода нет**: `HostTranslationBridge`
возвращает исходный текст с меткой `[en·apple]`. Это заглушка из ADR-008,
а не сбой — но в бэклоге она до сих пор не значилась, и снаружи
выглядела как сломанная функция.

- [x] Контракт `TranslateEngine`, политика, разрешение `auto` (ADR-008)
- [x] Очередь host-запросов через UniFFI без типов Cocoa в Rust
- [x] Вторая колонка субтитров и переключатели в Settings
- [x] Отказ ядра виден в UI — раньше ошибка «target совпадает с языком
  сессии» проглатывалась молча, и переключатель оставался включённым
- Apple Translation вместо заглушки — **основная работа**. Ограничение
  платформы: `TranslationSession` (macOS 15+) выдаётся модификатором
  `.translationTask` в SwiftUI, headless-API нет; значит мост придётся
  держать за невидимым вью, а не в отдельном объекте
- Скачивание языковых пар: первый перевод может потребовать загрузки и
  времени — нужно состояние, иначе это снова «не работает»
- `HttpTranslateEngine` на `POST /v1/translate` — после ADR-007
- Локальная LLM как третий путь — скелет есть, реализации нет

## Epic 14 — До встречи: повестка, календарь, таймер

Разбор: `analysis-pre-meeting-2026-08-03.md`. Все пункты сдвигают продукт
в момент **до** записи — сегодня он целиком посмертный. Порядок обратный
интуитивному: календарь раньше редактора повестки, потому что редактор
повестки без доставки никто не откроет.

- Запланированная встреча в домене: существует до записи, получает
  запись позже, не мусорит в истории, если записи не было
- Календарь на чтение через EventKit: ближайшие события, название,
  время, участники, ссылка на конференцию; opt-in, фильтр шума
- Повестка встречи: пункты и вопросы; импорт markdown (покрывает чаты
  ИИ, Notion, трекеры разом), импорт из описания события
- Пункты по ходу встречи: переключение из оверлея, отметки времени —
  дают и таймер, и границы тем в транскрипте
- Факт против плана в post-call: сколько времени ушло на каждый пункт
- Артефакты по повестке: Brief отвечает по пунктам, включая
  «не обсуждалось» — сегодня отрицательный результат недостижим
- Многоразовые форматы встреч (1:1, ретро, созвон с клиентом)
- Создание события из приложения (`.ics` / Calendar.app) — **с
  оговоркой:** участников EventKit программно добавить не даёт
- MCP-сервер над локальными данными: ассистент оформляет продуманный
  план сразу повесткой и ищет по прошлым встречам
- Имена участников из события вместо «Собеседник» — уточняет Epic 9
- Термины повестки в глоссарий встречи — уточняет Epic 7
- Zoom API и облачные транскрипты — **не делаем**: данные пошли бы через
  облако Zoom, что прямо противоречит local-first (та же логика, что у
  биометрии в ADR-012). Всё нужное от Zoom даёт календарь
- Автозапись всех встреч из календаря — **не делаем**: тихая запись без
  явного действия человека разрушает доверие быстрее любого бага
- Автоматическая сегментация транскрипта по темам — **deferred**: ручное
  переключение пунктов даёт те же границы точнее и бесплатно
- Zoom Apps SDK (панель внутри клиента) — **deferred** до спроса на
  in-meeting UX

## Epic 10 — Quality
- Unit tests for state machine
- Integration tests for FFI facade
- UI smoke tests
- Docs sync rules
