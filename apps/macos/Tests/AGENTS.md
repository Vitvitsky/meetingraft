# AGENTS.md — apps/macos/Tests

Плоский XCTest-таргет, один файл на единицу/поведение. Тесты собираются
**только на Маке**; их поломки на Linux не видны.

## Дубли и протоколы

- Дули `private`, в каждом файле, под протоколы источника:
  `MeetingsCoreSpy`/`LibraryCoreSpy`, `RebuildCoreSpy`,
  `AttributionCoreSpy`, `GlossaryCoreSpy`, `FakeTap: AudioTapping`,
  `FakeFiles: FileDownloading`, `Locked<Value>`; часть —
  `@unchecked Sendable`.
- **Метод, добавленный в протокол источника, ломает каждый дубль** —
  падает сборка **тестовой** цели, не приложения, и на Linux это невидимо.
  Меняешь протокол — грепай конструкторы `Ffi*` и конформансы.

## Что утверждать

- Локализацию сверять с **тем же** ключом `String(localized:)`, не с
  обрывком текста: обрывок ломается от перевода и от языка машины.
- Отрицательный контроль обязателен: `ThemeContrast` требует, чтобы старые
  чернила **не** проходили; `OverlayAppearance` — чтобы панель существовала.
  Тест, который не может упасть, ничего не доказывает.
- `performAsCurrentDrawingAppearance` требует класса `@MainActor`;
  `MainActor.assumeIsolated` читает `@MainActor`-состояние из синхронного
  замыкания, про которое известно, что оно на главном акторе; тесты времени
  инъектируют наносекундный такт, а не берут настоящие часы.

## Настоящее ядро

`LiveCaptionsPresentationTests` и `AudioCaptureCoordinatorTests` гоняют
**настоящее** ядро с временным каталогом данных;
`RustCaptionStreamSmokeTests` тоже берёт настоящее ядро, но с каталогом по
умолчанию.
