# Apple Translation вместо заглушки — спека

**Цель:** включённый синхронный перевод должен переводить, а не помечать
текст `[en·apple]`. Сегодня `HostTranslationBridge` возвращает исходную
строку с меткой — это заглушка из ADR-008, но снаружи она выглядит
сломанной функцией (`docs/backlog.md:1837`, Epic 15).

Заведено 2026-09-13. Обвязка (политика, очередь host-запросов, FFI,
вторая колонка) уже стоит; меняется только то, что стоит за мостом.

## Что стоит сейчас

- Rust `translate` умеет политику, `resolve_effective` и очередь
  host-запросов (`rust/crates/translate/src/`).
- FFI отдаёт `drainHostTranslationRequests()` и принимает
  `completeHostTranslation()`; флаг `setHostTranslationAvailable()` уже
  есть (`rust/crates/ffi/src/lib.rs:1518-1565`).
- Swift: `HostTranslationBridge` (42 строки) поллит очередь каждые 50 мс и
  подставляет `[\(targetCode)·apple] \(text)`
  (`apps/macos/Sources/LiveCaptions/HostTranslationBridge.swift:39-41`).
  Мост создаётся и стартует в `LiveCaptionsViewModel.init`
  (`LiveCaptionsViewModel.swift:19,25-26`).
- Колонка перевода и переключатели есть (`LiveCaptionsView.swift:98-110`,
  `:187-200`), отказ ядра виден в строке состояния (`:245-254`).

## Чем ограничена платформа (проверено по SDK, не по памяти)

- `TranslationSession` не имеет headless-API: выдаётся модификатором
  `.translationTask` (macOS 15.0+). Перегрузки с `id:` **нет**.
- Сессия **не `Sendable`**, но хранится в `@MainActor`-классе законно.
  **Рантайм-ограничение Apple: обращение к сессии после исчезновения вью
  или смены конфигурации — `fatalError`.**
- `LanguageAvailability.status(from:to:)` — `async`, **не бросает**;
  `Status` = `installed` / `supported` / `unsupported`. Скачивать не
  умеет.
- Скачать пару можно **только через сессию**: `prepareTranslation()` либо
  неявно через `translate`.
- Entitlements и ключи Info.plist не нужны. В симуляторе не работает —
  проверка на железе.

## Решение 1 — цикл живёт внутри замыкания сессии

Мост не хранит сессию дольше её жизни. `TranslationHostView` (нулевая вью
в `AppShellView`) строит `TranslationSession.Configuration(source:target:)`
и в замыкании зовёт `await bridge.run(session:)`; цикл
drain → translate → complete живёт ровно столько, сколько валидна сессия.
На выходе ссылка очищается — `fatalError` недостижим по построению.

Конфигурация `nil`, когда перевод выключен или `source == target`: сессии
и `run` нет вовсе. Флаг в Rust ставится на входе в `run` и снимается на
выходе (`defer`), поэтому выключение перевода само снимает `available` и
host-запросы перестают копиться.

Скачивание кнопка не зовёт напрямую: она ставит флаг, цикл его
подхватывает и зовёт `prepareTranslation()`. Вся работа с сессией — внутри
`run`.

Смена пары пересоздаёт конфигурацию → SwiftUI отменяет `run` → цикл
дренит и завершает пустыми стухшие host-запросы (иначе новая сессия
перевела бы старый запрос на новый target) → стартует новый `run` с новой
сессией.

## Решение 2 — только финальные реплики (host-путь)

`translate(_:)` асинхронен, частичные меняются каждые ~100–200 мс:
переводить их — копить очередь и получать обратный порядок. Host-путь
ставит в очередь **только `CaptionPhase::Final`** (правка в
`maybe_enqueue_translation`, `rust/crates/ffi/src/lib.rs:716-755`).

Не-host движки (`stub`, `http`, `local_llm`) переводят все фазы как
раньше: они синхронные, и demo-скрипт на них держится. Разница
задокументирована в беклоге.

Тест `apple_backend_uses_host_bridge_queue`
(`rust/crates/ffi/src/lib.rs:5580-5602`) ждёт первую Partial — станет
ждать первую Final. Тест stub-пути (`:5564-5578`) не трогается.

## Решение 3 — auto только для установленной пары

Пробник на `LanguageAvailability` считает статус пары (язык сессии →
target) и кормит `setHostTranslationAvailable(status == .installed)`:

- `.installed` → `auto` берёт Apple;
- `.supported` → `auto` уходит на backend/stub, в UI «нужна загрузка» и
  кнопка;
- `.unsupported` → Apple недоступен, в UI «пара не поддерживается».

Скачивание **никогда не стартует само посреди встречи** — только по
явному нажатию. После успешного `prepareTranslation()` пробник
повторяется, `available` становится `true`, `auto` переключается на
Apple.

## Решение 4 — состояние видно в колонке и в строке состояния

Пустая колонка обязана объяснять себя (правило «молчаливый отказ хуже
видимого»). Мост публикует состояние:

- `off` — перевод выключен;
- `ready` — Apple готов;
- `needsDownload` — нужна загрузка (кнопка);
- `downloading` — качается;
- `unsupported` — пара не поддерживается;
- `failed(текст)` — ошибка перевода или загрузки.

Колонка перевода показывает причину вместо нейтральной заглушки; та же
строка дублируется в статус-баре. Кнопка «Скачать» — в колонке и в
настройках (`SettingsProviderSections.swift:179-220`), чтобы скачать
заранее, до встречи.

## Поток данных

1. Включили перевод, выбрали target → сторы обновились.
2. `TranslationHostView` строит конфигурацию; `.translationTask` отдаёт
   сессию в `bridge.run`.
3. `run`: пробник → `setHostTranslationAvailable` → публикация состояния.
4. Rust: `auto` → Apple только при `installed`.
5. Live: финальное событие → Rust кладёт `HostTranslationRequest`.
6. `run`: drain → `session.translate(text)` → `completeHostTranslation`.
7. Rust → `pending_translations`; Swift дренит `drainLiveTranslations()` →
   колонка.
8. Кнопка → флаг → `prepareTranslation()` → повторный пробник → Apple.

## Компоненты и швы для тестов

- `TranslationHostView` (новый) — только SwiftUI-обвязка сессии, никакой
  логики.
- `HostTranslationBridge` (переписан) — `@Observable @MainActor`:
  `run(session:)`, `requestDownload()`, публикуемое состояние. Сессию не
  хранит вне `run`.
- `TranslationAvailabilityChecking` (новый протокол) +
  `SystemTranslationAvailability` над `LanguageAvailability` + фейк.
- `TranslationSessioning` (новый протокол: `translate`,
  `prepareTranslation`) + адаптер над `TranslationSession` + фейк — чтобы
  мост тестировался без Apple-фреймворка.
- Мост переезжает из `LiveCaptionsViewModel` в `AppShellView` (в
  `.environment`): сессия обязана пережить уход с вкладки, а состояние
  нужно двум экранам — субтитрам и настройкам.

## Обработка ошибок

| Случай | Что делаем |
|---|---|
| `unsupported` | `available = false`, `auto` → stub/backend, колонка «пара не поддерживается» |
| `needsDownload` | `available = false`, колонка + кнопка |
| `prepareTranslation` бросил | состояние `failed`, `available` остаётся `false` |
| `translate` бросил | состояние `failed`, запрос завершается **пустым** (чтобы `awaiting` не рос), следующая реплика продолжает |
| `run` отменён (смена пары или выключение) | снять `available` (`defer`), drain и пустое завершение стухших |
| `targetCode` запроса ≠ язык сессии | завершить пустым, не переводить не туда |
| `target == язык сессии` | режет Rust, видно как `translationIssue` (как сейчас) |

## Тестирование

- **Rust:** host-путь ставит только финальные (правка существующего
  теста); stub-путь неизменен.
- **Swift:** мост с фейковой сессией и фейковым пробником — цикл переводит
  и завершает; ошибка → состояние + пустое завершение; отмена дренит;
  пробник маппит три статуса; `setHostTranslationAvailable` зовётся с
  верным bool. Отрицательный контроль: тест краснеет, если вернётся
  старая заглушка `[target·apple]`.
- **За Маком руками:** настоящий перевод RU→EN и RU→ES на macOS 15,
  включая первую загрузку пары.

## Что не делаем

`HttpTranslateEngine` и локальную LLM (ждут ADR-007); перевод частичных
для host-пути; перевод в оверлее (там только субтитры); правок контракта
FFI (только фильтр по фазе в Rust).

## Проверено и не проверено

**Проверено** по `swiftinterface` установленного SDK и компилятором под
Swift 6: отсутствие `id:`-перегрузки; наследование `@MainActor` замыканием
из `View.body`; не-Sendable сессии; точные подписи `translate(_:)`,
`prepareTranslation()`, `status(from:to:)`; отсутствие требований к
entitlements.

**Не подтверждено** документацией Apple: явная аннотация `@MainActor` у
замыкания (проверена только компилятором); зависимость от системного
приложения Translate (отчёты на форумах, для macOS не подтверждено);
точная семантика «что считается установленным пакетом» (есть отчёты
Code 16 при `status == .installed`).

## Критерии готовности

- Включённый перевод даёт настоящий перевод, а не метку.
- Пара не скачана → видно причину и кнопку; скачивание не стартует само.
- Смена target или выключение не оставляют стухших запросов и не роняют
  процесс.
- Тесты Rust и Swift зелёные; `scripts/verify-mac.sh` проходит.
- Живая проверка RU→EN и RU→ES на Маке записана в беклог.
