# AGENTS.md

**`CLAUDE.md` — симлинк на этот файл.** Правила живут в одном месте и
одинаковы для любого агента; правится только `AGENTS.md`.

До 2026-08-21 файлов было два, и **`CLAUDE.md` лежал в `.gitignore`** —
то есть на Мак не доезжал вовсе. В нём при этом жила вся практика:
устройство крейтов, граница UniFFI, правило сверки перед мерджем, разбор
способов проверить пустоту и не заметить. Сам он утверждал обратное —
будто в игноре `AGENTS.md`, — и по этой ошибке список правил swiftformat
завели именно здесь. Правило оказалось верным, обоснование — нет.

## Purpose

This repository contains MeetingRaft, a native-first macOS meeting companion with live subtitles first and post-call intelligence second.

## Product constraints

- Live subtitles and final transcript are different artifacts.
- Realtime mode does not require speaker attribution.
- Post-call mode may assign or recognize speakers.
- Glossary support is a first-class feature.
- Native UX on macOS is required.
- Speech recognition languages: Russian (primary), English, Spanish.
- Default session language is Russian; EN/ES are supported for mixed and multilingual meetings.
- Language hints (session primary + allowed set) travel with live and post-call pipelines.

### Свойства продукта, которые не обсуждаются

- **Всё считается локально.** Аудио и транскрипты не уходят с машины.
- **Никаких виртуальных драйверов и ручной перенастройки звука.**
- **Заглушек в интерфейсе быть не должно.** Если функция не работает — она
  не показывается, а не показывается сломанной.
- **Молчаливый отказ хуже видимой ошибки.** Человек, который провёл
  встречу в уверенности, что она пишется, — худший исход из возможных.

MeetingRaft развивается **как личный инструмент и портфолио**, а не как
продукт на продажу (решение 2026-08-04). Доработки идут по собственной
потребности; приоритет у того, чем пользуются каждый день.

## Architecture rules

- SwiftUI views must not contain networking or business rules.
- AVFoundation stays in the Swift platform layer.
- Rust contains shared domain logic, session engine, transcript assembly, glossary normalization, sync logic, and local state orchestration.
- UniFFI is the only preferred boundary between Swift and Rust.
- Backend concerns stay outside the macOS shell.
- Live transcript and final refined transcript use separate domain models.

## Module boundaries

### Swift layer
- app lifecycle
- navigation
- window scenes
- menu bar and commands
- permissions
- audio capture adapters
- presentation models

### Rust layer
- domain entities
- meeting session state machine
- subtitle aggregation
- glossary engine
- sync client
- local persistence abstractions
- DTOs exposed through UniFFI facade

### Backend layer
- processing jobs
- storage
- diarization
- transcript refinement
- generated brief and follow-up artifacts

## Устройство кода

Три слоя, порядок жёсткий: **SwiftUI shell → UniFFI → Rust core →
(опционально) backend**.

### Rust workspace (`rust/crates/`)

`domain` — лист графа: типы, без зависимостей. От него зависят все, кроме
`sync` (тот замкнут на свои DTO). `ffi` — единственный крейт, который
зависит от всех остальных; между собой крейты не связаны, кроме
`postcall` → `glossary`.

| Крейт | Пакет | За что отвечает |
|---|---|---|
| `domain` | `meetingraft-domain` | сущности: caption, session, glossary, speaker, language, postcall |
| `session` | `meetingraft-session` | машина состояний сессии, `ChannelMixer` (ADR-009) |
| `stt` | `meetingraft-stt` | live-окна, local agreement (ADR-010), отсев галлюцинаций, noise gate, batch-проход для post-call |
| `glossary` | `meetingraft-glossary` | нормализация терминов, CSV-импорт, добыча кандидатов |
| `postcall` | `meetingraft-postcall` | сборка финала, диффы, правки сегментов, спикеры, LLM-промпты и артефакты |
| `storage` | `meetingraft-storage` | SQLite: миграции, манифест аудио, правки, журнал диагностики |
| `sync` | `meetingraft-sync` | HTTP-клиент backend, опрос джобов (ADR-007) |
| `translate` | `meetingraft-translate` | движки перевода: host (Apple), HTTP, локальная LLM, stub (ADR-008) |
| `diarize` | `meetingraft-diarize` | разделение голосов внутри дорожки: sherpa-onnx за фичей `model`, иначе заглушка |
| `ffi` | `meetingraft-ffi` | фасад `MeetingCore` + все `Ffi*`-DTO |

Отдельно — приборы, в приложение не входящие: `echo-probe` (детектор эха),
`gate-probe` (гейт речи и число запусков модели), `diarize-probe`
(разделение голосов), `dup-probe` (удвоение реплик), `term-probe`
(кандидаты в глоссарий). Плюс `uniffi-bindgen`.

**Каждый прибор начинает с заведомо положительного и заведомо
отрицательного случая** и до настоящих данных не доходит, если те не
разошлись. Второе основание для отказа завёл Epic 25: **записи без общего
времени каналов `echo-probe` не судит вовсе**, а `diarize-probe` считает,
но предупреждает вслух. Диапазон поиска эха — 250 мс, сдвиг старта у таких
записей — секунды, и величина неизвестна. Полученное на них число 0.09
однажды прочли как «эха нет».

Приборы вне Rust: `scripts/ax-probe.swift` (дерево Accessibility Zoom —
виден ли активный говорящий) и `scripts/check-localization.py` (полнота
каталога переводов; единственный, который гоняется на Linux).

`diarize` устроен как `stt` до whisper: без фичи отдаётся заглушка, и
остальное собирается на Linux. Три отличия, каждое стоило открытия:

1. **Заглушка отказывает, а не подделывает.** `stt::mock` отдаёт текст
   `[final ru] фрагмент речи униффи` — за распознавание его никто не
   примет. Подделка под разделение голосов неотличима от правды.
2. **Фича `model` качает готовые библиотеки, а не собирает C++.** `cmake`
   не нужен, зато сборка ходит в сеть, и линкуется **весь** тулкит: 15
   статических библиотек, release-бинарь на 34 МБ с полным синтезом речи
   и вторым распознавателем внутри. Отсюда `default = []` жёстче обычного.
   Собирается и работает на Linux — Мак не нужен, в отличие от whisper.
3. **Синтетикой этот движок не проверить.** `diarize-probe` судит движок
   только по записям с известным числом людей
   (`scripts/fetch-diarize-models.sh` кладёт их в
   `models/diarize/check/`). Нет записей — прибор не судит вовсе. Первая
   версия проверяла тонами и объявила работающий движок слепым: модель
   голосов на тонах не работает.

### Граница UniFFI

`rust/crates/ffi/src/lib.rs` — ~4000 строк и **весь** контракт с Swift.
Три вещи, которые надо знать до правки:

1. **`MeetingCore` — один объект под одним `Mutex`.** Всё состояние в
   `MeetingCoreInner`. Долгие операции (пересбор финала) вынесены в
   `RebuildJobs` **вне** мьютекса — иначе проход держал бы ядро минутами.
   Приватные хелперы пишутся свободными функциями, а не методами:
   приватный метод внутри `#[uniffi::export]`-блока всё равно пытается
   пройти через границу.

2. **Ошибки едут строками.** Мутирующие методы возвращают `String`:
   пустая — успех, непустая — текст ошибки. Не `Result`. Меняешь такой
   метод — сохраняй соглашение, иначе Swift молча посчитает ошибку
   успехом.

3. **Поле, добавленное в `Ffi*`-запись, ломает каждый её конструктор в
   Swift.** У `uniffi::Record` memberwise-инициализатор, и пропущенный
   аргумент — ошибка компиляции. Живут такие конструкторы почти целиком в
   `apps/macos/Tests/`, поэтому на Linux не видны вовсе, а на Маке валят
   не сборку приложения, а сборку тестовой цели. То же и с методом,
   добавленным в протокол вроде `MeetingsCoreProviding`: его реализуют
   тестовые дубли. Меняешь запись или протокол — `grep -rn "FfiИмя("
   apps/macos/` и правь все места сразу.

4. **Ширина целого доезжает до интерфейса.** `u32` становится Swift
   `UInt32`, а его интерполяция в локализуемой строке даёт спецификатор
   `%u`, не `%lld`. Ключ не совпадёт с каталогом, и перевод не найдётся
   никогда. Числа в локализуемых строках подаются как `Int(...)`; за этим
   следит `scripts/check-localization.py`.

Swift **опрашивает**, а не подписывается: `RustCaptionStream` крутит цикл
с `drainEvents()` каждые 50 мс. Колбэков через границу нет.

Живой путь целиком: `AudioCaptureCoordinator` (микрофон + process tap) →
`PCMDownmixer` (48→16 kHz) → `ingest_audio_chunk` → `ChannelMixer` →
`SttPipeline` → нормализация глоссарием → SQLite + очередь →
`drainEvents()` → SwiftUI.

**Метка чанка идёт от общего начала записи, а не от нуля своего канала**
(Epic 25). Внутри канала считаются кадры — они точнее часов; начало канала
привязано к `mach_absolute_time` его первого буфера (`HostClock`), потому
что источники стартуют не одновременно и общего нуля у счётчиков кадров
нет. Пока этого не было, дорожки встречи расходились на 1150 мс, и каждая
заявляла, что началась в ноль. У сессий есть признак
`channel_clock_unified`: у записей до Epic 25 он `0` навсегда,
восстановить их сдвиг нечем.

### Swift (`apps/macos/Sources/`)

`App` (настройки, языки, каталог моделей), `Audio` (AVFoundation + Core
Audio tap), `LiveCaptions`, `Meetings`, `Glossary`, `Presence` (меню-бар,
оверлей, детектор встреч), `Settings`, `Shell`, `DesignSystem`,
`Resources` (каталоги локализации).

`.xcodeproj` и `Generated/` — генерируемые, в git не трекаются. Источники:
`project.yml` (xcodegen) и `generate-ffi.sh` (UniFFI).

**Строка, показанная человеку, локализуема не везде одинаково.** Внутри
`Text(`, `Button(`, `.help(`, `.alert(`, `.confirmationDialog(`,
`Menu(`, `TextField(`, `ContentUnavailableView(` литерал уже
`LocalizedStringKey`. Вне вьюхи — обычный `String`, и его надо обернуть в
`String(localized:)` явно. Склейка через `+` берёт `String`-перегрузку и
**не локализуется вовсе**: длинный текст пишется одной строкой.

## Whisper за фичей

`meetingraft-stt` собирается **без** whisper по умолчанию
(`default = []`). Обычный `cargo build`/`cargo test` компилирует движок
лишь частично — `whisper.rs` в такой сборке не проверяется даже
компилятором. Отсюда шаг 3 в `verify-mac.sh`.

Ускоритель выбирается по платформе, не по фиче: Metal на Apple,
CPU-сборка whisper.cpp (нужен `cmake`) в остальных случаях.

Без модели и без фичи `stt_backend()` отдаёт `mock` — живые субтитры идут
из `stt/src/mock.rs`, а не из Whisper. Это не поломка, но и не то, что
стоит принимать за работающий распознаватель.

`generate-ffi.sh` собирает с `whisper` по умолчанию; для CI/Linux —
`MEETINGRAFT_FFI_FEATURES= apps/macos/Scripts/generate-ffi.sh`.

## Guardrails for coding agents

- Do not bypass UniFFI with ad hoc FFI glue unless explicitly approved.
- Do not put Cocoa or AVFoundation types into Rust-facing contracts.
- Prefer small DTOs and explicit enums across boundaries.
- Keep state transitions explicit and testable.
- Add tests for any new state machine branch.
- Prefer repository interfaces over direct API calls in views or view models.
- Keep docs updated when changing domain boundaries or contracts.

## Agent roles

- Swift Shell Agent
- Swift Audio Agent
- Swift UI Agent
- Rust Core Agent
- Rust Sync Agent
- Rust UniFFI Agent
- Backend Agent
- QA Agent
- Docs/ADR Agent

## Done criteria

A feature is not done until:
- architecture boundaries are respected;
- tests cover core logic;
- docs changed if contracts changed;
- no UI layer contains direct business logic;
- glossary and transcript version impact is considered where relevant.

## Setup

**Swift собирается только на Mac.** На Linux-машине его нет вообще. Всё,
что писано под Swift, уходит на проверку человеку — не заявляй такой код
проверенным.

**Полный `cargo test` по workspace не влезает в память Linux-машины.**
Там гонять по крейтам:

```
cd rust && cargo test -p meetingraft-storage -p meetingraft-postcall
```

**Имена пакетов с префиксом**: крейт `storage` — это пакет
`meetingraft-storage`. Каталог и имя пакета не совпадают, `-p storage`
молча не найдёт ничего.

Один тест или модуль:

```
cd rust && cargo test -p meetingraft-postcall merge::   # фильтр по имени
cd apps/macos && xcodebuild -project MeetingRaft.xcodeproj -scheme MeetingRaft \
  -only-testing:MeetingRaftTests/PCMDownmixerTests test CODE_SIGNING_ALLOWED=NO
```

- Rust core (на Маке): `cd rust && cargo test` (workspace)
- Lint Rust: `cd rust && cargo fmt --check && cargo clippy --all-targets -- -D warnings`
- macOS shell: `cd apps/macos && xcodegen generate`, затем открыть
  `MeetingRaft.xcodeproj` в Xcode или
  `xcodebuild -project MeetingRaft.xcodeproj -scheme MeetingRaft -configuration Debug build CODE_SIGNING_ALLOWED=NO`
  (`.xcodeproj` генерируется, в git не трекается — источник `project.yml`)
- macOS tests: `xcodebuild -project MeetingRaft.xcodeproj -scheme MeetingRaft -configuration Debug test CODE_SIGNING_ALLOWED=NO`
- Lint Swift: `cd apps/macos && swiftformat Sources Tests --lint`
  (правила, невидимые на Linux, — ниже отдельным разделом)
- Локализация (гоняется и на Linux): `python3 scripts/check-localization.py`
- Pre-commit (локально, зеркало быстрых CI-линтов): `brew install pre-commit`
  (или `pipx install pre-commit`), затем из корня репо `pre-commit install`;
  разовый прогон `pre-commit run --all-files`. Хуки: `cargo fmt --check`,
  `swiftformat Sources Tests --lint`, `check-localization`, `ruff` для
  `backend/`. Clippy / полный `cargo test` / `xcodebuild` — только в CI.
- CI: автозапуск GitHub Actions отключён (`workflow_dispatch` only в
  `.github/workflows/ci.yml`). Проверки — локально: команды выше +
  `pre-commit run --all-files`
- UniFFI + Xcode project (из корня репо): `apps/macos/Scripts/generate-ffi.sh`
  (dylib → `rust/target/debug`, биндинги → `apps/macos/Generated/`, затем
  `xcodegen generate` в `apps/macos/`)
- Backend: `cd backend && uv sync --extra dev && uv run pytest`;
  `ruff check .` для линта; docker: `docker compose up --build`
  (API `:8080`, token `dev-token`); настройка в app:
  `docs/architecture-and-install.md` §2.5 (`#backend-setup`)
- Docs: architecture и ADR — в `docs/`; схемы + install —
  `docs/architecture-and-install.md`
- OpenAPI: `shared/openapi.yaml`

**На Маке всё вместе:** `scripts/verify-mac.sh` — 7 шагов: тесты Rust →
clippy/fmt → clippy `meetingraft-stt --features whisper` →
`generate-ffi.sh` → swiftformat → xcodebuild test → pre-commit.

## swiftformat rules that Linux cannot see

Swift собирается только на Mac, поэтому `swiftformat --lint` — шаг 5
`scripts/verify-mac.sh` — единственное место, где эти правила
обнаруживаются. Каждое стоит целого прогона, если ловить их по очереди,
поэтому список ведётся: пойманное сюда дописывается.

Правил в `apps/macos/.swiftformat` не перечислено вовсе — там один
`--swiftversion 6.0`, — значит действует то, что включено в установленной
версии. Предполагать по памяти, что рулится, а что нет, здесь не выходит;
только этот список и прогон.

- **Разделители в числах.** По умолчанию `--decimalgrouping 3,6`: до пяти
  цифр — **без** `_` (`50000`, `16000`), от шести — группами по три
  (`1_150_000`). Написанное на глаз `1_150` и `50_000` роняет шаг целиком.
  Сверяется по уже лежащим файлам:
  `grep -rnoP '(?<![_0-9.])[0-9]{5}(?![_0-9.])' apps/macos` показывает
  пятизначные без разделителей — так и надо.
- **`redundantAsync`.** `func … async` без единого `await` в теле —
  ошибка. Легко получается в тестах, где `await` убрался вместе с
  последней асинхронной строкой.
- **`preferKeyPath`.** `filter { $0.foo }` и `map { $0.foo }` обязаны быть
  `filter(\.foo)`. Только тривиальные: замыкания, которые сравнивают
  (`first { $0.id == other }`) или отрицают (`contains { !$0.ok }`), под
  правило не попадают.
- **`wrapFunctionBodies`.** Тело функции в одну строку
  (`func f() -> Float { 0.45 }`) — ошибка, включая тестовые заглушки.
- **`wrapPropertyBodies`.** То же для свойств: `var id: String {
  rawValue }` — ошибка. Поймано 2026-08-21 на `AppearanceSettingsStore`.
  Протоколов не касается: `var isAvailable: Bool { get }` законно.

Шаг 6 (`xcodebuild test`) ловит то же самое, но компилятором. Пойманное:

- **`String(localized:)` принимает только литерал.** Он берёт
  `String.LocalizationValue`, а склеенное через `+` — уже `String`, и
  преобразования нет. Длинный текст писать одной строкой; склейка через
  `+` законна у `Text`, `Label` и `.help` (у них есть перегрузка под
  `StringProtocol`) и незаконна здесь. Разница невидима, пока не собрать.

## Git

Работа идёт **веткой и pull request**, даже для мелочей — так устроена вся
история. Прямо в `main` не коммитить.

`apps/macos/Generated/` не трекается: биндинги генерирует
`generate-ffi.sh`, как `.xcodeproj` генерирует xcodegen из `project.yml`.

Учётные данные идут через `gh`
(`credential.https://github.com.helper`). Если пуш падает с отказом в
доступе при валидном `gh auth status` — дело в helper, а не в токене.

### Перед мерджем — сверить локальное с удалённым

**Обязательно, до `gh pr merge`:**

```
git log --oneline origin/<ветка>..<ветка>
```

Непустой вывод означает, что эти коммиты **есть только локально**. Мердж
возьмёт то, что лежит на GitHub, и они в `main` не попадут.

Проверять надо по имени ветки PR, а не по текущему `HEAD`: мердж обычно
запускается из `main`, и незапушенное в другой ветке иначе не видно вовсе.

`--delete-branch` в одной команде с мерджем не давать, пока сверка не
сделана: он сносит и удалённую ссылку, и локальную. После этого ветки нет,
а коммиты живут только как недостижимые объекты до сборки мусора.

Обжигались дважды. 2026-08-08: PR #36 смерджен без пяти локальных
коммитов, среди которых была починка молчаливой потери звука — того самого
дефекта, ради которого ветка и заводилась. 2026-08-21: спека подпроекта
локализации исчезла вместе с веткой, снесённой руками после того, как
работа переехала на другую. Оба раза восстановлено через `git reflog`.

## Порядок работы над крупным

Спека → план по задачам → выполнение задачами с разбором каждой. Спеки и
планы ложатся в `docs/superpowers/` и коммитятся: они часть истории
решений, а не черновик.

Что этот порядок ловит, и это проверено на Epic 19: **тест бывает
зелёным, проверяя не то**. Трижды за эпик тестовые данные обходили
проверяемое условие стороной, и обнаруживалось это только отдельным
разбором. Если тест должен что-то ловить — убедись, что он падает без
исправления.

### Способы проверить пустоту и не заметить

Пойманы 2026-08-08, 2026-08-09 и 2026-08-21, каждый — на работе, которая
выглядела законченной.

**Прибор верят только после заведомо положительного случая.**
`scripts/count-audio-taps.swift` показал ноль tap'ов после убийства
процесса, и это прочли как «утечки нет». Тот же ноль при заведомо идущей
записи показал, что скрипт слеп: приватные tap'ы
`kAudioHardwarePropertyTapList` не отдаёт. Тем же способом соврал замер
времени: `flush_timing` печатал 0.003 мс, потому что мерил уже опустевший
накопитель. **Сперва проверь, что прибор показывает ненулевое там, где оно
заведомо есть.**

**Защита, которая не может сработать, хуже её отсутствия.** Белый список в
`check-localization.py` не исключал ни одного файла: проверка смотрела
только на позиции вида `Text(`, куда исключаемое не попадает в принципе.
Список выглядел работающим и создавал уверенность. Прибор теперь проверяет
и сам список: послабление, которое ничего не исключает, — отдельный
провал.

**Прибор, сверяющий догадку сам с собой, зеленеет всегда.** Спецификатор
подстановки угадывался по имени переменной, и каталог переводов был
написан по той же догадке. Проверки сравнивали догадку с ней же и
проходили на всех 39 ключах — при том, что девять переводов были
недостижимы. Чинится не уточнением догадки, а её снятием: правило вместо
предположения.

**Закрытый список запретов не закрывает ничего.** Та же проверка
перечисляла числовые *имена* и всё неперечисленное молча считала строкой.
Мимо прошло `~\(size) MB`. Список должен быть разрешающим: всё, что не
опознано, — провал.

**Тест, утверждающий число, которое автор не может измерить, —
угадывание.** Тест на кадры ресемплера падал дважды подряд с разными
зашитыми константами, обе взяты из головы, обе не проверяемы на Linux.

**Сравнение двух дорог к одному результату константу не заменяет — у него
своя непроверенная посылка.** Здесь стояло, что тот же звук чанками и
одним куском даёт одно и то же. 2026-08-09 тест упал **третий** раз, уже в
этом виде: 15738 против 15013. Конвертер придерживает часть входа между
вызовами, и величина придержанного зависит от способа подачи.
Сравниваешь дороги — проверь сначала, что они и должны совпадать.

**Работает утверждение о свойстве, а не о значении.** Заработал четвёртый
заход: отставание не превышает чанка на сотне чанков. Ожидаемое выводится
из входа, единственная константа — граница из требования к задержке. Тем
же приёмом чинились тесты локализации: вместо сравнения с русским словом
— число частей отчёта и совпадение с тем же ключом.

**Кусок текста интерфейса — не свойство.** `contains("builtin")`
пережил перевод и упал на первом же прогоне за Маком, когда слово стало
`built-in`; `contains("Final")` в том же тесте вдобавок зависел от языка
машины. Прибор такое не ловит и поймать не может: латинский обрывок
строки от обычного слова не отличить без огромного числа ложных.
Ловится это только компилятором и прогоном — что и произошло.

**Ожидаемое выводится из фикстуры, а не вписывается числом.** Размер
частотной верхушки в тестах добычи терминов считается из самого
наполнителя: вписанное число пришлось бы подбирать под ответ и молча
ломалось бы от правки фикстуры.

**Зелёный тест проверяется заведомо положительным случаем.** Отдельной
веткой вернуть дефект (тут — `converter.reset()` на каждом чанке) и
убедиться, что тест краснеет: отставание стало 24000 кадров вместо ≤1600.
Ветка `debug/resampler-negative-control` жива именно для этого. И
отдельно: правка, объяснённая рассуждением вместо замера, в тот же день
ухудшила ровно то, что бралась улучшить, — откачена.

**Тест, который может пройти на пустом входе, ничего не значит.** «Ноль
помеченных окон» в детекторе эха выполнялось потому, что окон не было
вовсе. То же в тестах: `XCTAssertFalse(summary.contains("снято"))` под
английским интерфейсом выполняется само собой, что бы код ни делал.
Утверждай непустоту входа отдельной строкой — до утверждения о
результате.

## Где что искать

| Что | Где |
|---|---|
| Решения с обоснованиями | `docs/adr/` — читать до того, как предлагать альтернативу |
| Что делаем и почему | `docs/backlog.md`, `docs/roadmap.md` |
| Что и как проверять за Маком | `docs/mac-verification.md` — сценарии; состояние пунктов в беклоге |
| Спеки и планы работ | `docs/superpowers/specs/`, `docs/superpowers/plans/` |
| Схемы и установка | `docs/architecture-and-install.md` |
| Контракт с backend | `shared/openapi.yaml` (ADR-007) |
| Онбординг под Windows | `docs/windows/README.md` |
| Бюджеты задержки | `docs/architecture.md`, раздел про live caption latency |

## Stack & conventions

- Stack: SwiftUI + AVFoundation (macOS shell), Rust + UniFFI (domain core), backend workers/API (post-call)
- **Комментарии, документация в коде, файлы в `docs/` — по-русски.**
  Идентификаторы — по-английски.
- Prefer explicit types and small DTOs across UniFFI; keep state machines testable
- SQL (when present): keywords UPPERCASE, identifiers lowercase
- Commits: Conventional Commits, **English** subject and body (`feat:`,
  `fix:`, `docs:`, …). Тела PR — тоже по-английски. До 2026-08-04 коммиты
  были русскими; старую историю не переписывать. Язык кода и язык истории
  здесь разные намеренно.
- Интерфейс приложения: английский базовый, русский переводом
  (`Localizable.xcstrings`). Ключи английские.

## Skills & tooling

- graphify (CLI): `uv tool install graphifyy` — codebase knowledge graph
- Slash skills (agent side): `/agents-init`, graphify skill when answering structure questions
- Prefer UniFFI-facing contracts and ADRs in `docs/adr/` over ad hoc cross-layer glue

## hindsight (agent memory)

Long-term memory service (local Docker, `http://localhost:8888`, MCP at `/mcp`).
Bank id = repo name: `meetingraft`. Skip this section silently if the
service is not running or MCP tools are absent.

- **Recall at task start**: call `recall` with the task topic — past
  decisions, gotchas, and data quirks live there.
- **Retain on the way out**: after a significant decision, verified finding,
  or data gotcha, call `retain` with a short self-contained fact.
- Do not retain what the repo already records (code, git history, READMEs)
  or session-local noise.
- Recalled facts about code are historical context, not ground truth: verify
  against the current code. On mismatch the code wins — retire the stale
  fact via `invalidate_memory`.
- Connect in Claude Code:
  `claude mcp add --transport http --scope user hindsight http://localhost:8888/mcp`.

## graphify (knowledge graph)

Plain CLI (`uv tool install graphifyy`), works with any agent.

- When `graphify-out/graph.json` exists, answer codebase questions with
  `graphify query "<question>"` first; fall back to grep for exact strings.
- After modifying code, run `graphify update .` (AST-only, no API cost).
- Build initially with `graphify .` if the graph does not exist yet.
