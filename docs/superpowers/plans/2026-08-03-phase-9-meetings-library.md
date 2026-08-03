# Phase 9 — Meetings library

Детальный план фазы. Основание: `docs/product-ux-review-2026-08-03.md` §2.2,
§3.3, §5.1. Фаза делает Meetings домом продукта: встреча становится
опознаваемой (название, длительность), находимой (полнотекстовый поиск) и
удаляемой.

Правила фазы — из `AGENTS.md`: SwiftUI без бизнес-логики, домен в Rust за
UniFFI, TDD на логику ядра, Conventional Commits с русским subject. Язык
интерфейса — английский основной (решение 2026-08-03), русский добавляется
в Phase 12; новые строки пишем сразу через `String(localized:)`.

## Exit criteria

- У встречи есть редактируемое название, длительность и дата окончания;
  список встреч читается без раскрытия.
- Полнотекстовый поиск по Final-транскриптам, live-финалам и артефактам
  возвращает встречу со сниппетом; работает на русском.
- Meetings — стартовый раздел приложения.
- Удаление встречи стирает чанки, manifest, captions, finals, artifacts,
  speakers и поисковый индекс (обещание `architecture.md:109` выполнено).
- Существующая база пользователя обновляется миграцией без потери данных.
- Первый запуск больше не приводит к Mock-субтитрам молча.

## T0 — первый запуск: скачивание модели вне Settings — **код готов, не проверен**

`FirstRunModelBootstrap`, вызов из `AppShellView.task`; из `SettingsView`
триггер удалён.


Сейчас `maybeFirstRunDownload()` вызывается только из
`SettingsView.onAppear` (`SettingsView.swift:272, 287`); пользователь,
не открывший настройки, получает Mock.

- `apps/macos/Sources/App/FirstRunModelBootstrap.swift` — новый тип с
  единственным методом `ensureModel(core:)`: если `listLocalWhisperModels()`
  пуст и загрузка не идёт — качает `ggml-base.bin`.
- Вызов из `AppShellView.task`, не из Settings; Settings продолжает
  показывать статус, но перестаёт быть триггером.
- В `LiveCaptionsView` при `sttBackend == "mock"` показать явную плашку
  «Speech model is downloading — captions are placeholders», вместо тихой
  подмены.
- Тест: `FirstRunModelBootstrapTests` на фейковом `WhisperDownloading` —
  вызов при пустом каталоге, отсутствие вызова при непустом.

Коммит: `fix: скачивание STT-модели при первом запуске вне Settings`.

## T1 — механизм миграций в storage — **перенесён в Phase 8 (T0)**

Phase 8 добавляет колонку `channel` в `caption_events` и упирается в ту же
проблему раньше, поэтому механизм миграций реализуется там:
`docs/superpowers/plans/2026-08-03-phase-8-system-audio.md`, T0. К моменту
старта этой фазы `PRAGMA user_version` уже работает, и задачи T2, T5, T6
просто добавляют свои шаги.

## T2 — sessions: title и ended_at_ms — **сделано**

Миграция 3, `end_session(ended_at_ms)`, `set_meeting_title`,
`MeetingSummary::duration_ms`.


- Шаг миграции 2: `ALTER TABLE sessions ADD COLUMN title TEXT NOT NULL
  DEFAULT ''`; `ALTER TABLE sessions ADD COLUMN ended_at_ms INTEGER`.
- `domain::MeetingSummary` (`domain/src/postcall.rs:32`) получает
  `title: String` и `ended_at_ms: Option<u64>`.
- storage: `set_session_title(id, title)`, `finish_session(id, ended_at_ms)`;
  `end_session()` перестаёт быть только сбросом `active_session` и пишет
  время окончания; `list_meetings` отдаёт новые поля.
- Тесты storage: begin → append → finish → `list_meetings` возвращает
  `ended_at_ms` и вычислимую длительность; переименование переживает
  переоткрытие соединения; пустое имя допустимо (fallback — на стороне UI).

Коммит: `feat: название и время окончания встречи в storage`.

## T3 — название по умолчанию задаёт Swift — **код готов, не проверен**

`MeetingTitle.forNewMeeting()`; `start_recording` принимает `title`.


Формат даты локале-зависим, поэтому генерация дефолтного имени — забота
презентационного слоя (`AGENTS.md`: форматирование не уезжает в Rust).

- `MeetingCore::start_recording(session_id, title)` — сигнатура получает
  название; пустая строка допустима.
- `AudioCaptureCoordinator` формирует `Date.now.formatted(...)` →
  «Meeting Aug 3, 14:30».
- Поле остаётся редактируемым; в Phase 9 LLM сможет предложить осмысленное
  имя по Final — переименование не ломается.
- Тест: `AudioCaptureCoordinatorTests` — старт записи прокидывает непустое
  название.

Коммит: `feat: имя встречи по умолчанию из shell при старте записи`.

## T4 — FFI: переименование и расширенная сводка — **сделано**


- `FfiMeetingSummary` получает `title: String`, `endedAtMs: u64`
  (0 = не завершена — совместимо с текущим стилем `UInt64`-полей).
- `rename_meeting(meeting_id: String, title: String) -> String` — строка
  ошибки, пустая при успехе (конвенция как у `delete_speaker`).
- Тесты в `ffi`: rename → `list_meetings` отражает; rename несуществующей
  встречи возвращает непустую ошибку.

Коммит: `feat: UniFFI rename_meeting и расширенная MeetingSummary`.

## T5 — FTS5-индекс — **сделано**

Миграция 4 с backfill; индексация в тех же методах, что и запись строк;
запрос экранируется и делается префиксным.


- Шаг миграции 3:
  ```sql
  CREATE VIRTUAL TABLE meeting_fts USING fts5(
      meeting_id UNINDEXED,
      kind UNINDEXED,      -- caption | final | artifact
      ref_id UNINDEXED,
      body,
      tokenize = 'unicode61 remove_diacritics 2'
  );
  ```
  плюс backfill существующих `caption_events` (только `phase = 'final'`),
  `final_transcripts` и `artifacts`.
- Запись в индекс — в тех же методах storage, что пишут исходные строки;
  никаких триггеров (пути записи все в одном крейте, триггеры усложняют
  миграции).
- `search(query, limit) -> Vec<SearchHit>` c `bm25()` для ранжирования и
  `snippet(meeting_fts, 3, '⟦', '⟧', '…', 12)` для фрагмента.
  Пользовательский запрос экранируется и превращается в префиксный:
  `"термин"*` — иначе спецсимволы FTS дают синтаксическую ошибку.
- `domain::SearchHit { meeting_id, kind, ref_id, snippet, score }`.
- Тесты: русский запрос находит фрагмент; префиксный поиск («биллин»
  находит «биллинга»); удаление артефакта убирает строку из индекса;
  запрос со спецсимволами (`"`, `*`, `(`) не паникует.

Коммит: `feat: FTS5-индекс и поиск по материалам встреч`.

## T6 — удаление встречи — **сделано** (Rust); UI-подтверждение не проверено


`architecture.md:109` обещает каскадное удаление; функции нет нигде.

- storage: `delete_meeting(id)` — строки `sessions`, `audio_manifest`,
  `caption_events`, `final_transcripts`, `artifacts`, `speakers`,
  `meeting_fts`, плюс каталог `sessions/{id}/` с PCM-чанками; всё в одной
  транзакции, файлы — после успешного коммита.
- FFI: `delete_meeting(meeting_id) -> String`.
- UI: контекстное меню строки и ⌫ в `MeetingsListView`, подтверждающий
  `confirmationDialog` с явным перечислением того, что будет удалено.
- Тесты: после удаления `list_meetings` пуст, поиск ничего не находит,
  каталог чанков отсутствует; удаление активной сессии запрещено
  (непустая ошибка).

Коммит: `feat: каскадное удаление встречи`.

## T7 — Meetings как дом и читаемый список — **код готов, не проверен**

Поиск с дебаунсом, переименование, удаление с подтверждением,
`AppDestination` начинается с `meetings`.


- `AppDestination` (`apps/macos/Sources/App/AppDestination.swift`): порядок
  `meetings, liveCaptions, glossary`; `AppShellView.swift:9` —
  `@State private var selection: AppDestination? = .meetings`.
- `MeetingsListView`: строка = название (headline), под ним дата,
  длительность и бейджи Final / artifact count; UUID уходит из UI целиком.
- Переименование: контекстное меню «Rename» → inline `TextField`.
- `.searchable(text: $viewModel.query)`; при непустом запросе список
  заменяется результатами со сниппетом и переходом в нужную вкладку
  детали (`kind` → `Final` / `Live` / `Artifacts`).
- `MeetingsViewModel`: `query`, дебаунс 200 мс, `searchHits`; поиск не
  трогает `meetings`.
- Тесты `MeetingsViewModelTests`: дебаунс схлопывает серию нажатий; пустой
  запрос возвращает полный список; ошибка поиска попадает в `errorMessage`.

Коммит: `feat: Meetings как стартовый раздел с поиском и переименованием`.

## T8 — синхронизация документации — **сделано**


- `docs/architecture.md` — раздел про библиотеку встреч и поиск.
- `docs/backlog.md` — новый Epic 11 «Meetings library» с отметками.
- `docs/roadmap.md` — статус Phase 8.
- `shared/openapi.yaml` не трогаем: поиск локальный.

Коммит: `docs: библиотека встреч и поиск`.

## Порядок и зависимости

T0 независим — можно делать первым и мержить отдельно.
T1 → T2 → {T4, T5, T6} → T7. T3 после T2. T8 последним.

Проверки перед закрытием фазы: `cd rust && cargo test`, `cargo clippy
--all-targets -- -D warnings`, `cargo fmt --check`, `swiftformat Sources
Tests --lint`, `xcodebuild ... test`, `pre-commit run --all-files`.
