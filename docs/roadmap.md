# MeetingRaft Development Roadmap

Phased development plan. Each phase produces working, testable software on
its own and ends with explicit exit criteria. Before a phase starts, a
detailed implementation plan for it is written to
`docs/superpowers/plans/YYYY-MM-DD-<phase-name>.md` (bite-sized TDD tasks);
this document stays at the milestone level and maps phases to backlog epics
(`docs/backlog.md`).

Ground rules for every phase:

- Architecture boundaries from `AGENTS.md` are non-negotiable (SwiftUI shell
  without business logic, Rust core behind UniFFI, backend outside the shell).
- Language policy travels everywhere: `primary_language` = `ru`,
  `allowed_languages` = `{ru, en, es}` (ADR-003).
- TDD for core logic; every new state machine branch gets a test.
- Conventional Commits with Russian subject.

## Phase 0 — Decisions and tooling bootstrap

Blocking product decisions are not yet recorded; they shape every later
phase, so they are fixed first as ADRs:

- **ADR-004 Audio capture sources.** Microphone only vs microphone + system
  audio (other participants). System audio on macOS requires
  ScreenCaptureKit / audio tap and its own permission flow; a meeting
  companion is barely useful without it, so this must be an explicit
  decision, not an accident of Epic 5.
- **ADR-005 Live STT topology.** On-device (e.g. whisper.cpp / Apple Speech)
  vs cloud streaming gateway, and the concrete provider; must honor the
  ru/en/es policy and Russian-first quality (ADR-003). Decides whether the
  backend is on the critical path for Stage 1.
- **ADR-006 Local persistence.** Concrete store behind the Rust "local
  store facade" (e.g. SQLite via rusqlite) and what is persisted locally:
  caption events, raw audio manifest, glossary.
- **ADR-007 Backend stack and contracts.** Backend language/runtime,
  streaming transport (WebSocket/gRPC), contract schema format and its home
  in `shared/`. May be deferred only if ADR-005 picks on-device STT for v1.

Tooling in the same phase: Xcode project under `apps/macos`, cargo workspace
under `rust/`, lint/format (clippy + rustfmt, SwiftFormat or SwiftLint), CI
that builds both worlds and runs `cargo test`.

**Exit criteria:** ADR-004..006 accepted (ADR-007 accepted or explicitly
deferred); empty app and empty workspace build green in CI.

Maps to: Epic 1 (finishing touches). Status: **done** on
`feat/phase-0-bootstrap` (CI green on PR #1).

## Phase 1 — SwiftUI shell with fake subtitle stream

App skeleton a user can click through: sidebar, toolbar, settings scene,
menu commands and shortcuts, and a live-captions screen rendering a fake
subtitle stream (Swift-local timer for now). Presentation models only — no
business rules in views.

**Exit criteria:** app runs from Xcode; fake captions render with
partial/final visual states; settings scene shows session language selector
(default `ru`) backed by a stub.

Maps to: Epic 2. Status: **done** on `feat/phase-1-swiftui-shell`.

## Phase 2 — Rust core and UniFFI boundary

Domain crate (session, caption event, language policy DTOs), meeting session
state machine, UniFFI facade crate, generated Swift bindings wired into the
Xcode project. The fake subtitle stream moves from Swift into the Rust core,
proving the event path UI ← UniFFI ← core.

**Exit criteria:** state machine transitions covered by `cargo test`;
Swift ↔ Rust integration smoke test passes; captions on screen originate in
Rust.

Maps to: Epics 3, 4. Status: **done** on `feat/phase-2-rust-uniffi`.

## Phase 3 — Audio capture

AVFoundation capture manager (plus system audio path if ADR-004 says so),
permissions flow, input device selection, chunking pipeline feeding the Rust
core, local raw recording manifest per ADR-006.

**Exit criteria:** a recording session produces persisted chunks and a
manifest; permission denial paths handled in UI; chunk cadence matches what
the STT path in ADR-005 expects.

Maps to: Epic 5. Status: **done** on `feat/phase-3-audio-capture`
(mic path + SQLite manifest; system process tap follow-up).

## Phase 4 — Live subtitle pipeline (Stage 1 complete)

Real STT wired per ADR-005: open session with the language policy, stream
chunks, merge partial/final caption events in the subtitle assembler,
persist live caption events, session language override in settings.

**Exit criteria:** live captions on a real meeting in Russian with mixed
English terms; defined latency budget measured and met; caption events
replayable from local store.

Maps to: Epic 6. Status: **done** on `feat/phase-4-live-stt`
(Mock STT + caption_events + UI Start Live; Whisper behind `--features whisper`).

## Phase 5 — Glossary

Glossary domain model and normalization engine in Rust, scopes
(global/workspace/project/meeting; MVP implements global + meeting only),
CSV/TXT import, glossary UI, attaching
the glossary to a live session (bias/normalization), language-tagged terms
with Russian as default scope.

**Exit criteria:** glossary terms demonstrably affect caption output;
normalization covered by unit tests; import round-trips a real CSV.

Maps to: Epic 7. Status: **done** on `feat/phase-5-glossary`
(normalize on live captions, SQLite `glossary_terms`, CSV import, sidebar CRUD;
scopes MVP: global + meeting only).

## Phase 6 — Post-call intelligence (Stage 2)

Refinement trigger on meeting end (same language policy), final transcript
fetch, transcript review screen, speaker entities with correction screen,
versioned refined transcripts, live vs final comparison. Artifact
generation is template-driven: built-in templates (brief, follow-up email,
technical requirements, meeting minutes, action items) plus user-defined
markdown templates; template engine details are fixed in an ADR during this
phase's detailed planning. Live and final transcripts remain separate
domain models (ADR-002).

**Exit criteria:** end-to-end flow: finish meeting → refined transcript →
speaker assignment → artifacts from at least two built-in templates and one
user-defined template; transcript versions comparable in UI.

Maps to: Epics 8, 9. Status: **done (local MVP)** on
`feat/phase-6-postcall-local` (Stop Live → `FinalTranscript` из live finals;
Meetings UI Live | Final | Artifacts; Brief + Follow-up через built-in
templates; `LlmClient` stub). **Done follow-up:** backend stub
(`feat/phase-6-backend-stub`: OpenAPI + FastAPI in-memory jobs + Rust sync +
Settings Test API) и Meetings poll UI / stub e2e
(`feat/meetings-backend-refine-stub`: Artifacts → Submit refine → poll job →
show stub markdown). **Done follow-up:** Artifacts via backend jobs
(`feat/artifacts-via-backend-jobs`: Settings LLM=Backend → Generate Brief/Follow-up
→ `POST /v1/jobs` + poll; no silent fallback to templates). **Done follow-up:**
Speakers skeleton (`feat/speakers-skeleton`: domain + SQLite + UniFFI + Meetings
**Speakers** tab, ручные метки ru/en). **Done follow-up:** Local LLM
(`feat/ollama-openai-compat-llm`: Ollama native + OpenAI-compatible
Brief/Follow-up, без silent fallback). **Done follow-up:** Backend LLM
provider (`feat/backend-llm-provider`: jobs `brief`/`follow_up` → env
OpenAI-compat; app шлёт model + language-aware prompts). **Done follow-up:** Markdown export (`feat/markdown-export-obsidian`: Final +
Brief/Follow-up → `.md` в Settings export folder; overwrite; без frontmatter).
**Done follow-up:** STT model picker (`feat/stt-model-picker`: Settings Live STT
picker `auto|base|small|large-v3-turbo`, HF download в Swift, preference +
resolve в Rust/UniFFI; first-run auto `ggml-base.bin` при пустом `models/`).
**Done follow-up:** Backend provider registry (`feat/backend-provider-registry`:
static JSON + `GET /v1/models` + Settings picker + job routing by `provider_id`;
compat `LLM_*` → `default`).
**Done follow-up:** Versioned Final + Compare (`feat/final-versions-compare`: Stop
Live / re-assemble → next version; Final picker; Compare Live | Final vN;
Brief/Export = latest).
**Remaining:**
Parakeet on-device STT, WhisperX / billing / provider CRUD+discovery, diarization
+ speaker→Final binding (Epic 9), user-defined templates, mail draft export,
Obsidian plugin / export API, remote STT.

## Phase 7 — Hardening and release

UI smoke tests, FFI integration test suite, packaging (signing,
notarization), docs sync check, performance pass against the Phase 4
latency budget.

**Exit criteria:** `AGENTS.md` done-criteria hold across the app; a
notarized build runs on a clean machine.

Maps to: Epic 10 (quality items also run continuously inside each phase).

**Разделена:** непрерывные пункты качества (FFI-suite, перф против бюджета
Phase 4, smoke) уезжают внутрь фаз, которые создают соответствующий риск —
Phase 10 (тяжёлый второй проход) и Phase 12 (HUD и глобальные хоткеи). В
конце остаётся только релизная часть: подпись, нотаризация, чистая машина.
См. Phase 16.

## Продуктовый разворот (Phases 8–16)

Основание — `docs/product-ux-review-2026-08-03.md`. Фазы 0–6 собрали
работающие подсистемы; фазы 8–16 превращают их в продукт. Решения,
зафиксированные 2026-08-03 и действующие на все фазы ниже:

- Final v2 = повторное распознавание сохранённых чанков, затем
  LLM-полировка (не одна лишь LLM-нормализация).
- Язык интерфейса: английский основной, русский вторым.
- Provider CRUD / discovery / billing — **сняты с плана**: для local-first
  ICP ценности не дают, а billing предполагает хостинг, что противоречит
  позиционированию. Статический registry остаётся как есть.
- Parakeet и remote STT — не фаза, а эксперимент за существующим
  `SttEngine`; см. «Отложенные ветки» ниже.

## Phase 8 — System audio capture (ADR-004 wiring)

**Блокирует всё остальное.** `SystemAudioCapture.prepare()`
(`apps/macos/Sources/Audio/SystemAudioCapture.swift:11-15`) жёстко ставит
`isAvailable = false`: process tap не подключён, приложение пишет только
микрофон. Собеседники в Zoom/Teams в запись не попадают. ADR-004 прямо
называет такой продукт «barely useful». Вдобавок `ingest_audio_chunk`
(`rust/crates/ffi/src/lib.rs:1361`) отдаёт в STT только канал Mic — даже
подключённый tap будет сохраняться в manifest, но не распознаваться.

Содержание: `AudioHardwareCreateProcessTap` / `CATapDescription` +
aggregate device, разрешение «System Audio Recording» (macOS 15+) со своим
UI-потоком, выравнивание двух потоков по времени в session engine (ADR-004
отдаёт это Rust), распознавание обоих каналов с сохранением
принадлежности события каналу.

**Exit criteria:** на реальном звонке в записи и в субтитрах есть обе
стороны; каналы остаются раздельными end-to-end; отказ в разрешении
деградирует до mic-only явно, а не молча.

**Follow-up (planned):** стабилизация live-субтитров через
LocalAgreement-2 — движок уже перегоняет растущий буфер раз в секунду, но
не фиксирует стабильный префикс и не обрезает буфер, из-за чего строка
мерцает, а длинный монолог ломает бюджет латентности. План:
`docs/superpowers/plans/2026-08-03-live-local-agreement.md`.

План: `docs/superpowers/plans/2026-08-03-phase-8-system-audio.md`;
решение по live-пути — `docs/adr/ADR-009-live-mix-channel-attribution.md`.
Maps to: Epic 5 (последний незакрытый пункт). Status: **Rust-часть готова
и проверена (T0, T1, T6, T7); Swift-часть написана, ждёт прогона на Mac
(T2–T5, T8)**.

## Phase 9 — Meetings library

Встреча становится опознаваемой и находимой: название, длительность,
полнотекстовый поиск (FTS5) по finals / live-финалам / артефактам,
каскадное удаление (обещание `architecture.md:109`, сейчас не выполненное),
механизм миграций SQLite. Meetings становится стартовым разделом. Отдельно
чинится первый запуск: скачивание STT-модели уезжает из Settings в старт
приложения.

**Exit criteria:** список встреч читается без раскрытия; русский запрос
находит фрагмент со сниппетом; удаление стирает чанки и индекс;
существующая база обновляется без потери данных.

План: `docs/superpowers/plans/2026-08-03-phase-9-meetings-library.md`.
Maps to: Epic 11. Status: **Rust-часть готова и проверена (T2, T4, T5, T6);
Swift-часть написана, ждёт прогона на Mac (T0, T3, T7)**.

## Phase 10 — Final v2: re-ASR и полировка

Сегодня Final — это `filter(Final).map(normalize).join` над live-финалами
(`postcall/src/assemble.rs:11-16`), поэтому вкладка Compare показывает две
одинаковые панели. Фаза даёт настоящий второй проход: чтение PCM-чанков из
`audio_manifest`, повторное распознавание большой моделью без realtime-
ограничений (большое окно, без VAD-нарезки), затем LLM-полировка —
пунктуация, абзацы, слияние фрагментов, глоссарий. `FinalTranscript`
получает сегменты с тайм-кодами; provenance в UI называет реальный
источник. Picker «Post-call STT» (`SettingsView.swift:115`) наконец
получает содержательные варианты.

Два пути распознавания за одним интерфейсом: **локальный** (Whisper
large-v3 по чанкам — путь по умолчанию, local-first) и **backend WhisperX**
как опциональный тяжёлый путь для тех, у кого есть домашний сервер.
WhisperX даёт word-level alignment, который потребуется Phase 11, поэтому
интерфейс сегментов проектируется сразу под него. Здесь же закрывается
пункт Epic 10 «Integration tests for FFI facade»: фаза добавляет самый
тяжёлый FFI-путь и без тестового набора его не удержать.

**Exit criteria:** на реальной русской встрече Final заметно точнее Live и
читается абзацами; Compare показывает осмысленную разницу; проход
запускается фоново, с прогрессом и отменой, и не блокирует приложение.

План: `docs/superpowers/plans/2026-08-03-phase-10-final-v2.md`;
решение — `docs/adr/ADR-011-post-call-re-asr.md`.
Maps to: Epic 8, Epic 10, новый Epic 13. Status: **Rust готов и проверен
(T1–T6, T8, T10); Swift написан, ждёт прогона (T9); backend WhisperX
отложен (T7)**.

## Phase 11 — Speakers: атрибуция и диаризация

Закрывает Epic 9, от которого сейчас есть только skeleton (ручные метки, не
связанные с транскриптом). Опирается на две предпосылки: раздельные каналы
из Phase 8 и сегменты с тайм-кодами из Phase 10.

Дешёвая половина берётся без всякого ML: после Phase 8 канал `mic` — это
пользователь, канал `system` — остальные, поэтому звонки один на один
атрибутируются полностью бесплатно. Настоящая диаризация нужна только для
многосторонних встреч, где несколько голосов живут внутри системного
канала, — и там она навешивается на word-level alignment из Phase 10.

Содержание: `FinalSegment.speaker_id`, привязка меток из Speakers-вкладки к
сегментам, экран коррекции «этот голос — это Пётр», распространение
исправления на весь транскрипт, спикеры в Brief / Follow-up и в экспорте.

**Exit criteria:** в Final видно, кто что сказал; на звонке один на один
атрибуция работает без диаризации; исправление метки в одном месте меняет
весь транскрипт.

План: `docs/superpowers/plans/2026-08-03-phase-11-speakers.md`; решение —
`docs/adr/ADR-012-speaker-attribution-by-channel.md`.
Maps to: Epic 9. Status: **done** — ядро и FFI (T1–T3), экран спикеров и
сегменты Final (T4–T5), имена в артефактах и экспорте (T6); кластеризация
голосов внутри системного канала отложена сознательно.

**Уточнение к формулировке:** «диаризация» здесь была обобщением. После
Phase 8 и 10 канал сегмента известен точно, поэтому основной сценарий
закрывается без ML; кластеризация голосов нужна только для нескольких
участников внутри системного канала и отложена.

## Phase 12 — Присутствие в системе

Menu bar extra с индикатором записи и стартом/стопом; глобальный хоткей
(Carbon `RegisterEventHotKey` — не требует Accessibility); плавающий HUD
субтитров поверх встречи (`NSPanel`, non-activating, `.floating`,
`canJoinAllSpaces` + `fullScreenAuxiliary` — иначе не виден над Zoom в
полноэкранном режиме); автодетект начала звонка по запущенным приложениям
и календарю (EventKit) с ненавязчивым предложением записать. Онбординг
собирает в один поток разрешения (микрофон, системное аудио), модель и
напоминание о согласии на запись.

Здесь же — оставшиеся непрерывные пункты Epic 10: UI smoke-тесты и
перф-проход против бюджета Phase 4 (HUD и постоянно живущий menu bar —
главный источник регрессий по латентности и энергопотреблению).

**Exit criteria:** запись стартует без открытия окна; субтитры видны над
полноэкранным Zoom; старт встречи распознаётся и предлагается; бюджет
латентности из `architecture.md` соблюдён при активном HUD.

Maps to: Epic 2, Epic 5, Epic 10. Status: **planned**.

## Phase 13 — Петля глоссария

Кандидаты в термины добываются из Final (частотные OOV, аббревиатуры,
code-switching ru/en) в Rust; очередь одобрения в UI; одобренный термин
влияет на следующую встречу через bias и нормализацию. Закрывает пункты
Epic 7, помеченные deferred.

**Exit criteria:** после встречи приложение предлагает кандидатов;
одобрение измеримо меняет вывод на следующей встрече.

Maps to: Epic 7. Status: **добыча готова, очередь ждёт чисел** (2026-08-21).

Фаза оказалась вдвое меньше, чем выглядела: **вторая половина петли уже
работала**. Одобренный термин идёт через `refresh_glossary` →
`build_whisper_prompt` → `initial_prompt`, поэтому «одобрение измеримо
меняет вывод» выполняется для любого способа завести термин, а рождение
подсказки из ручной правки закрыто Epic 19 и проверено живьём.

Сделано: добыча кандидатов (`glossary::mine`, три правила формы и
частоты), память об отклонённых (миграция 31), прибор `term-probe`.

Не сделано сознательно: **очередь одобрения и граница UniFFI**. Прибор
считает кандидатов по каждому правилу отдельно, и пока не известно,
какая доля из них мусор, строить под них экран нельзя — это была бы
заглушка за неизмеренной величиной. Спека и план —
`docs/superpowers/specs/2026-08-20-glossary-candidates-design.md`,
`docs/superpowers/plans/2026-08-20-glossary-candidates.md`.

## Phase 14 — Артефакты и линия экспорта

Собирает в одну фазу всё, что превращает результат встречи в рабочий
документ пользователя: пользовательские markdown-шаблоны (промпт +
плейсхолдеры transcript / brief / glossary / participants), оставшиеся
встроенные шаблоны (technical requirements, meeting minutes, action items),
черновик письма в почтовый клиент и Obsidian-плагин.

Obsidian-плагин здесь — не «ещё один экспорт», а фактически вторая
платформа продукта (`product-ux-review-2026-08-03.md` §4): не выгрузка в
папку по кнопке, а живая синхронизация — встреча закончилась, заметка уже
в vault, с ссылками на термины глоссария. Стоит в разы дешевле iOS и
попадает точно в сегмент, который выбирает local-first.

**Exit criteria:** пользователь добавляет свой шаблон без пересборки;
заметка появляется в vault без ручного экспорта; черновик письма
открывается в почтовом клиенте с заполненным телом.

Maps to: Epic 8 (шаблоны, экспорт). Status: **planned**.

## Phase 15 — Чистка UI и локализация

Убрать инженерные артефакты из пользовательского интерфейса: «(stub)»
(`MeetingDetailView.swift:244, 327`), номера ADR в заголовках секций
настроек (`SettingsView.swift:32, 62, 126`), телеметрию `chunks/captions`
(`LiveCaptionsView.swift:26-30`), демо-кнопку из `primaryAction`
(`AppShellView.swift:57`). Compare уезжает в режим разработчика, Speakers
показывается только при наличии диаризации. String Catalog: английский
базовый, русская локализация вторым языком.

**Exit criteria:** в интерфейсе не осталось внутренних терминов; обе
локали полные; скриншоты пригодны для лендинга.

Maps to: новый Epic 12. Status: **инженерные артефакты убраны,
локализация открыта.**

Убрано раньше и по частям: телеметрия `chunks/captions` с живых
субтитров, номера ADR из заголовков настроек, демо-кнопка из
`primaryAction` (уехала в меню Session). Убрано 2026-08-20: последняя
видимая заглушка — `Submit refine (stub)` и панель `Backend refine`.
Она была честной и оттого особенно вредной: backend прогоняет через
настоящую LLM только `brief` и `follow_up` (`backend/app/main.py:95`),
а `refine` отдаёт заглушечный markdown. Вместе с кнопкой ушла машинерия
опроса джоба и три метода протокола, жившие только ради неё.

**Локализации не существует вовсе**, и это вскрылось при разборе:
117 вызовов `String(localized:)` при полном отсутствии каталога —
`.xcstrings` в проекте нет, ресурсов в `project.yml` нет. То есть
строки размечены, а русской локали никогда не было; вдобавок часть
текста захардкожена по-русски прямо в английском интерфейсе. Вынесено в
отдельный подпроект решением пользователя 2026-08-20: работа объёмная,
механическая и полностью непроверяемая на Linux, а смешанная с палитрой
дала бы диff, в котором не видно ни того, ни другого.

Compare в режим разработчика и Speakers по наличию диаризации —
остаются открытыми.

## Phase 16 — Релиз

Остаток старой Phase 7 после того, как непрерывные пункты качества
разошлись по фазам 10 и 12: подпись, нотаризация, проверка синхронности
документации, прогон на чистой машине.

**Exit criteria:** нотаризованная сборка запускается на чистой машине;
done-criteria из `AGENTS.md` выполняются по всему приложению.

Maps to: Epic 10. Status: **planned**.

## Phase 17 — До встречи: календарь, повестка, пункты

Первая фаза, которая работает **до** записи. Продукт сегодня посмертный:
открывается, когда встреча уже идёт. Повестка, заданная заранее, — самый
дешёвый из оставшихся рычагов качества артефактов: она снимает главную
неопределённость LLM, которой сейчас приходится угадывать, что было важно.

Порядок внутри фазы обратный интуитивному — **календарь раньше повестки**.
Момент до встречи занят: человек в календаре, а не в рекордере из
менюбара. Редактор повестки без доставки останется пустым.

Календарь берётся локально через EventKit, из уже настроенных в
Calendar.app аккаунтов: без OAuth, без сервера, без чужого облака —
редкий случай, когда интеграция ничего не стоит local-first.

Таймер входит сюда же и не как отдельная функция: переключение пунктов по
ходу встречи даёт **и** учёт времени, **и** границы тем в транскрипте.

**Exit criteria:** повестка приезжает из события календаря без ручного
ввода; Brief отвечает по пунктам, включая «не обсуждалось»; post-call
показывает факт против плана по времени.

Полный разбор, включая отклонённые варианты:
`analysis-pre-meeting-2026-08-03.md`.

Maps to: новый Epic 14. Status: **analysed**, не запланирована.

## Смена рамки — 2026-08-04

MeetingRaft развивается **как личный инструмент и портфолио**, а не как
продукт на продажу. Доработки идут по собственной потребности.

Что из этого следует для порядка работ:

- Приоритет у того, чем пользуются каждый день: живой контекст встречи
  (Epic 17), правка распознанного (Epic 19), нагрев машины (Epic 18) —
  все три пришли из реального использования, а не из плана.
- **Видимых заглушек в интерфейсе быть не должно.** Для портфолио
  недоделанная функция хуже отсутствующей: «(stub)» рядом с кнопкой и
  перевод, возвращающий исходный текст с меткой, читаются как поломка, а
  не как честная незавершённость. Это поднимает Phase 15 в приоритете.
- Корпоративные ветки (Epic 20: тарифы, backend с выбором моделей,
  командный глоссарий, дообучение) остаются разобранными, но работа по
  ним не ведётся: покупатель не выбран, и без этого набор функций —
  гадание.
- Phase 17 (календарь и повестка) сохраняет ценность: она про ежедневное
  использование, а не про охват.

## Отложенные ветки

Не фазы — ветки, которые открываются по сигналу, а не по плану.

- **Zoom API / облачные транскрипты.** Отклонено, не отложено: данные
  встречи шли бы через облако Zoom, а продукт обещает, что запись не
  покидает машину. Всё, что реально нужно от «интеграции с Zoom» —
  встреча, участники, ссылка — приходит из календаря (Phase 17).
- **Zoom Apps SDK.** Панель внутри клиента Zoom: ревью в маркетплейсе и
  работа ради результата, который есть только в Zoom. Открывается при
  подтверждённом спросе на in-meeting UX.
- **Интеграция с конкретным интерфейсом чата ИИ.** Заменена на импорт
  markdown (покрывает все чаты и заметочники разом) и MCP-сервер над
  локальными данными — там инверсия правильнее: не мы ходим в чаты, а
  ассистент ходит к нам.

- **Parakeet как второй on-device engine.** Живёт за существующим
  `SttEngine`, поэтому не требует фазы. Пересмотрено 2026-08-04 в связи с
  целью «максимум качества и производительности на любом современном
  MacBook».

  **Главный довод — не точность, а архитектура.** Whisper это
  энкодер-декодер на окне в 30 секунд: уточнить последнее слово можно
  только прогнав весь буфер заново. Отсюда LocalAgreement (ADR-010),
  переразбор тридцати секунд ради одной новой и нагрев машины. Parakeet
  (CTC/RNNT/TDT) потоковый по устройству — стоимость растёт с объёмом
  нового звука, а не с длиной окна. На слабом железе это важнее
  процентов WER.

  **Сравнивать надо с `base`, а не с large-v3.** В живом пути стоит
  маленькая модель по необходимости — уложиться в секундный цикл, — и на
  русском она заметно ошибается. Parakeet 0.6B против Whisper `base` —
  сравнение, где преимущество и по качеству, и по цене сразу. Post-call
  остаётся на large-v3: там бюджета латентности нет.

  **Открытый вопрос — русский.** Многоязычные версии заявляют
  европейские языки, надёжных цифр против Whisper нет. Мерить на своих
  записях; иначе меняем известное плохое на неизвестное.

  **Способ интеграции решает судьбу ADR-005.** ONNX Runtime через `ort`
  с CoreML-провайдером **сохраняет** границу: движок остаётся за
  Rust-трейтом. CoreML напрямую из Swift её **инвертирует** — это уже не
  замена движка, а перестройка архитектуры, и решать её надо отдельным
  ADR, а не мимоходом.
- **Альтернативные рантаймы STT (MLX, CoreML/WhisperKit).** Сейчас
  используется whisper.cpp через `whisper-rs` с Metal. MLX — Python-first,
  зрелой Whisper-реализации на `mlx-swift` нет, а Python-рантайм в
  нативном бандле ломает подпись и нотаризацию. WhisperKit (CoreML + ANE)
  технически интереснее, но он Swift-only и инвертирует ADR-005, который
  держит STT за Rust-трейтом ради сменяемости провайдера без правок в
  Swift. **Ключевое:** все три гоняют одни и те же веса Whisper — это
  вопрос скорости и энергии, а не качества распознавания. Открывается,
  если `large-v3-turbo` не уложится в бюджет латентности на младших Маках;
  на M4 Max с `base` запас пока десятикратный.
- **Remote STT.** Противоречит local-first позиционированию и добавляет
  латентность в live-путь. Только как явный opt-in для тех, кто сознательно
  меняет приватность на качество; никогда не default.
- **Provider CRUD / discovery / billing.** Сняты. Статический registry
  покрывает потребность local-first пользователя; billing имеет смысл
  только при собственном хостинге, которого позиционирование не
  предполагает.
- **iOS / iPadOS.** Не рекордер звонков — системного аудио на iOS нет.
  Честная роль: диктофон для очных встреч плюс читалка и поиск по
  библиотеке с Мака, на том же Rust-ядре через UniFFI. Открывается после
  Phase 14.
- **Windows.** Только под подтверждённый спрос ICP. Технически проще macOS
  (WASAPI loopback), но UI-дифференциации не даёт.

## Dependency notes

- Phase 2 blocks 3–6: everything flows through the UniFFI facade.
- ADR-005 (accepted: on-device STT) keeps the backend out of Phases 1–5;
  `backend/` and the `shared/openapi.yaml` contract (ADR-007) start in
  Phase 6.
- Glossary (Phase 5) intentionally precedes post-call (Phase 6): glossary
  bias helps live captions first, refinement reuses the same engine.
