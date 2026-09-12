# AGENTS.md — apps/macos

Swift-слой: правила границ, опрос ядра, локализация, тесты и сборка.
Список каталогов и правило локализации-литерала — в корневом `AGENTS.md`;
здесь то, что видно рядом с кодом.

## Границы

- Вьюхи не содержат сети и бизнес-правил. Доступ к ядру — в
  `@Observable @MainActor`-моделях, зависящих от **узких протоколов**
  (`MeetingsCoreProviding`, `FinalRebuildCoreProviding`,
  `SpeakerAttributionCoreProviding`, `GlossaryCoreProviding`); каждый
  удовлетворяется `extension MeetingCore: X {}` — это и есть шов для
  тестов. Вьюха получает `MeetingCore` только чтобы отдать его модели.
- AVFoundation/Core Audio — только в `Sources/Audio/` и
  `Meetings/SegmentAudioPlayer.swift`. Окна AppKit — в `Sources/Presence/`,
  и только там; исключение — `App/DirectoryPicker.swift` (системный
  `NSOpenPanel` для выбора папки). Остальные `import AppKit` — ради
  цвета/геометрии, не окон. Сеть — только в `Sources/App/` (URLSession) и в
  Rust-ядре.
- Форматирование (даты, длительности, размеры) остаётся в Swift. Но строки,
  которые становятся **хранимыми данными** (заголовок встречи, имя
  спикера по умолчанию), намеренно не локализуются.
- Строковые коды с границы разбираются в одном месте: `SpeakerSource(code:)`
  (неизвестное → `.none`), `CaptionSpeaker(channelCode:)` (→ `.you`),
  `CaptionLine.init(event:)`.

## Опрос и конкурентность

Swift **опрашивает** Rust, колбэков нет. Такты:
- `RustCaptionStream` — `drainEvents()` + `drainLiveTranslations()` каждые
  50 мс;
- `LiveCaptionsViewModel.startLive` — 50 мс;
- `HostTranslationBridge` — 50 мс;
- `FinalRebuildViewModel.finalRebuildProgress` — 500 мс (инъектируется);
- `MeetingsViewModel` — debounce поиска 200 мс.

Шаблон: `Task { @MainActor in while !Task.isCancelled { …; try? await
Task.sleep(…) } }`.

Swift 6 (`SWIFT_VERSION 6.0`). `@Observable`-сторы; `@MainActor` на моделях
и координаторах. `@unchecked Sendable` — на мостах к не-Sendable ядру:
`RustCaptionStream`, `FakeCaptionStream`, `ContinuationBox`, тестовые
дубли. `@preconcurrency import AVFoundation`. Блокирующие вызовы ядра
(сеть) идут через `offMainThread` (`App/BlockingCore.swift`), а он
использует `DispatchQueue.global`, **не** `Task.detached`: кооперативный
пул рассчитан на неблокирующие задачи.

## Локализация

- Автопозиции: `Text(`, `Button(`, `.help(`, `.alert(`,
  `.confirmationDialog(`, `Menu(`, `TextField(`, `ContentUnavailableView(`,
  `Picker(`, `Toggle(`, `Label(`, `.navigationTitle(`. Вне вьюхи — только
  `String(localized:)` с литералом; склейка через `+` не локализуется.
- Числа подаются как `Int(...)`: `UInt32` даёт спецификатор `%u`.
  Литеральный `%` в строке удваивается (`%%`).
- `// loc:allow` — построчное послабление; сейчас единственное место —
  `Meetings/SpeakerAttributionViewModel.swift` (имя спикера следует языку
  встречи, не интерфейса). Отдельно в `scripts/check-localization.py` есть
  список `CYRILLIC_ALLOWED` — послабление на весь файл
  (`LiveCaptions/FakeCaptionStream.swift`, `App/SpeechLanguage.swift`).
- Проверка — `scripts/check-localization.py` (6 проверок), гоняется и на
  Linux.

## Карта каталогов

- `App/` — сетевой слой, загрузчики моделей, сторы (см.
  `Sources/App/AGENTS.md`).
- `Audio/` — адаптеры захвата (см. `Sources/Audio/AGENTS.md`).
- `DesignSystem/` — токены и компоненты (см.
  `Sources/DesignSystem/AGENTS.md`).
- `Glossary/` — `GlossaryFilter`, `GlossaryView`; `kind` (подсказка против
  замены) переносится без изменений.
- `LiveCaptions/` — `CaptionStreaming` + `FakeCaptionStream` +
  `RustCaptionStream`, `HostTranslationBridge` (цикл Apple за очередью
  хоста),
  `CaptionLine`/`CaptionSpeaker`/`CaptionPhase`. **Уход со вкладки не
  останавливает запись.**
- `Meetings/` — список, детали, пересбор, атрибуция (см.
  `Sources/Meetings/AGENTS.md`).
- `Presence/` — меню-бар, оверлей, детектор встреч (см.
  `Sources/Presence/AGENTS.md`).
- `Settings/` — экран настроек со своим ядром (см.
  `Sources/Settings/AGENTS.md`).
- `Shell/` — `AppShellView`, композиционный корень, одно общее ядро.
- `Resources/` — `Localizable.xcstrings` (база en + ru),
  `InfoPlist.xcstrings`.

Тесты — `Tests/AGENTS.md` (плоский XCTest-таргет, дубли под протоколы,
отрицательные контроли).

## Сборка

- `project.yml` — источник xcodegen (`.xcodeproj` генерируется, в git не
  трекается). Deployment macOS 15.0, `SWIFT_VERSION 6.0`,
  `SWIFT_EMIT_LOC_STRINGS YES`. Схема объявлена явно: иначе xcodegen её не
  создаёт и `xcodebuild -scheme` падает на чистом клоне. Линкуется
  `-lmeetingraft_ffi` из `rust/target/debug`.
- `generate-ffi.sh` собирает `meetingraft-ffi` с фичами по умолчанию
  `whisper,diarize,gigaam` (переопределение `MEETINGRAFT_FFI_FEATURES=`),
  гоняет uniffi-bindgen в `Generated/`, затем xcodegen.
- `.swiftformat` задаёт только `--swiftversion 6.0`.
- Swift собирается **только на Маке** — на Linux его нет вовсе.
