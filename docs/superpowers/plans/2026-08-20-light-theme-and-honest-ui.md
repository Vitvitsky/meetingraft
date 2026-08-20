# Светлая тема и честный интерфейс — план работ

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Приложение открывается светлым, если так стоит в системе, и не
показывает ни одной кнопки, за которой заглушка.

**Architecture:** Токены остаются `static let Color`, но за каждым цветом
стоит `NSColor(name:dynamicProvider:)`, отдающий вариант по `NSAppearance`.
Правится один файл `Theme.swift`; ~250 обращений к токенам в экранах не
меняются. Тема выбирается `AppearanceSettingsStore` (Dark / Light / Auto),
оверлей подчиняется ему отдельно, потому что живёт вне иерархии SwiftUI.

**Tech Stack:** SwiftUI, AppKit (`NSColor`, `NSAppearance`, `NSPanel`),
XCTest. Сборка — только macOS.

Спека: `docs/superpowers/specs/2026-08-20-light-theme-and-honest-ui-design.md`.

## Состояние на 2026-08-20

**Написаны все семь задач.** Ветка `feat/light-theme-and-honest-ui`,
девять коммитов.

**Ни один шаг «прогнать тест» не выполнен.** Swift на машине, где план
исполнялся, отсутствует целиком — ни компилятора, ни возможности
запустить. Галочки ниже поэтому расставлены **только на шагах, которые
состоят в написании кода**; шаги вида «убедиться, что тест падает» и
«убедиться, что тест зелёный» оставлены пустыми и ждут Мака. Ставить их
означало бы заявить проверенным то, что никто не проверял.

Из этого следует и то, что порядок TDD соблюдён лишь на бумаге: тест
писался раньше кода, но красным его никто не видел. Первый прогон на
Маке — это одновременно и первая компиляция.

Отдельно ждёт **отрицательный контроль теста контраста** (задача 2,
шаги 4–6): числа посчитаны заранее, подмена палитры не делалась.
Сценарий — `docs/mac-verification.md`, раздел «Тема и контраст».

Что отклонилось от плана по ходу, и оба раза в сторону строгости:

- в `ThemeContrastTests` добавлена калибровка самого счётчика на
  величинах, не зависящих от палитры (чёрное на белом — ровно 21:1,
  белое на белом — 1:1). Без неё зелёный тест мог бы означать, что
  формула всегда возвращает большое число;
- в `OverlayAppearanceTests` добавлен заведомо положительный случай:
  `panelAppearanceName` у несозданной панели тоже `nil`, то есть три
  проверки темы прошли бы при полном отсутствии панели.

Задача 6 вышла шире плана: вместе с кнопкой удалены три метода
протокола `MeetingsCoreProviding` и их реализации в двух тестовых
дублях — они существовали только ради refine.

## Global Constraints

- Комментарии и документация в коде — **по-русски**; сообщения коммитов и
  тела PR — **по-английски** (`CLAUDE.md`).
- **Swift на Linux не собирается вовсе.** Ни один шаг этого плана нельзя
  объявить проверенным без прогона на Маке. Формулировка результата —
  «написано, ждёт Мака», а не «работает».
- Проверка целиком: `scripts/verify-mac.sh` (шаг 5 swiftformat, шаг 6
  `xcodebuild test`). Отдельный тестовый класс:
  `xcodebuild -project MeetingRaft.xcodeproj -scheme MeetingRaft
  -only-testing:MeetingRaftTests/ThemeContrastTests test CODE_SIGNING_ALLOWED=NO`
  из `apps/macos`.
- **Шрифт не трогать.** `Theme.Text` остаётся на `Font.system` (SF Pro);
  пункт ТЗ §2.2 про Inter отменён решением 2026-08-20.
- **Радиусы и отступы не трогать.** `Theme.Radius` уже совпадает с ТЗ §2.3
  дословно.
- Работа идёт веткой и pull request, прямо в `main` не коммитить.
- Ветка этого плана: `feat/light-theme-and-honest-ui`.

---

### Task 1: Токены отдают вариант по теме

**Files:**
- Modify: `apps/macos/Sources/DesignSystem/Theme.swift` (весь блок цветов,
  строки 9–47 и расширение `Color` в конце файла)
- Test: `apps/macos/Tests/ThemeTests.swift:33-40` (существующий
  `testAccentMatchesSpec` меняет ожидание) плюс новые тесты в том же файле

**Interfaces:**
- Consumes: ничего.
- Produces: `Theme.dynamic(light:dark:opacity:) -> Color` (приватный),
  все существующие имена токенов (`Theme.surfaceRoot`, `Theme.accent`,
  `Theme.textTertiary`, `Theme.borderSubtle` и прочие) сохраняют тип
  `Color` и сигнатуру. Task 2 опирается на то, что имена не изменились.

- [ ] **Step 1: Написать падающий тест на две темы**

Заменить `testAccentMatchesSpec` (`ThemeTests.swift:33-40`) и добавить
рядом. Класс пометить `@MainActor` — `performAsCurrentDrawingAppearance`
трогает состояние отрисовки:

```swift
@MainActor
final class ThemeTests: XCTestCase {
    /// Разрешить цвет в конкретной теме: динамический токен сам по себе
    /// значения не имеет, пока не назначена `NSAppearance.current`.
    private func components(
        _ color: Color,
        in name: NSAppearance.Name
    ) -> (r: CGFloat, g: CGFloat, b: CGFloat, a: CGFloat) {
        var out: (r: CGFloat, g: CGFloat, b: CGFloat, a: CGFloat) = (0, 0, 0, 0)
        NSAppearance(named: name)!.performAsCurrentDrawingAppearance {
            let native = NSColor(color).usingColorSpace(.sRGB) ?? .black
            native.getRed(&out.r, green: &out.g, blue: &out.b, alpha: &out.a)
        }
        return out
    }

    private func assertHex(
        _ color: Color,
        in name: NSAppearance.Name,
        equals hex: UInt32,
        file: StaticString = #filePath,
        line: UInt = #line
    ) {
        let got = components(color, in: name)
        XCTAssertEqual(got.r, CGFloat((hex >> 16) & 0xFF) / 255, accuracy: 0.004, file: file, line: line)
        XCTAssertEqual(got.g, CGFloat((hex >> 8) & 0xFF) / 255, accuracy: 0.004, file: file, line: line)
        XCTAssertEqual(got.b, CGFloat(hex & 0xFF) / 255, accuracy: 0.004, file: file, line: line)
    }

    /// Акцент переехал на системный синий Apple (Epic 23), и в светлой
    /// теме он на шаг темнее системного: `#007AFF` даёт на белом 4.02:1,
    /// то есть ниже порога для обычного текста.
    func testAccentIsSystemBlueInBothAppearances() {
        assertHex(Theme.accent, in: .darkAqua, equals: 0x0A84FF)
        assertHex(Theme.accent, in: .aqua, equals: 0x0069D9)
    }

    /// Главный признак того, что тема вообще подключена: фон окна обязан
    /// быть разным. Одинаковый означает, что провайдер не спросили.
    func testSurfaceRootDiffersBetweenAppearances() {
        assertHex(Theme.surfaceRoot, in: .darkAqua, equals: 0x0D0D0F)
        assertHex(Theme.surfaceRoot, in: .aqua, equals: 0xFFFFFF)
    }

    /// Жёлтый на белом читается как 1.41:1 — в светлой теме
    /// предупреждение оранжевое. Единственный статусный цвет, который
    /// нельзя было перенести.
    func testWarningTurnsOrangeOnLight() {
        assertHex(Theme.warning, in: .darkAqua, equals: 0xFFD60A)
        assertHex(Theme.warning, in: .aqua, equals: 0xB25000)
    }

    /// Границы меняют не только яркость, но и сам цвет: белый на 6%
    /// поверх белого фона не виден вовсе.
    func testBordersInvertWithAppearance() {
        XCTAssertGreaterThan(components(Theme.borderSubtle, in: .darkAqua).r, 0.9)
        XCTAssertLessThan(components(Theme.borderSubtle, in: .aqua).r, 0.1)
    }
```

Остальные тесты файла (`testHexInitParsesChannelsInOrder`,
`testHexInitAppliesOpacity`, `testSpacingScaleIsAscending`,
`testRadiusScaleIsAscending`, `testStatusKindsUseDistinctColors`)
остаются как есть.

- [ ] **Step 2: Убедиться, что тест падает**

Из `apps/macos`:

```
xcodebuild -project MeetingRaft.xcodeproj -scheme MeetingRaft \
  -only-testing:MeetingRaftTests/ThemeTests test CODE_SIGNING_ALLOWED=NO
```

Ожидается провал `testAccentIsSystemBlueInBothAppearances`,
`testSurfaceRootDiffersBetweenAppearances`, `testWarningTurnsOrangeOnLight`,
`testBordersInvertWithAppearance`: токены пока статические и в обеих темах
дают одно и то же (акцент — `#4A9FD8`).

- [ ] **Step 3: Перевести токены на динамический провайдер**

`Theme.swift` целиком до `enum Text` включительно заменяется на:

```swift
import AppKit
import SwiftUI

/// Токены оформления (ТЗ редизайна §2).
///
/// Значения заданы один раз и только здесь: экраны не имеют права
/// придумывать свои цвета и отступы, иначе визуальный язык расходится
/// быстрее, чем его успевают править.
///
/// Тем две. За каждым цветом стоит `NSColor` с провайдером: значение
/// выбирается по `NSAppearance` той вьюхи, где цвет рисуется, поэтому ни
/// одному экрану не нужно знать о теме. Светлая палитра посчитана по
/// контрасту, а не подобрана — разбор в спеке
/// `docs/superpowers/specs/2026-08-20-light-theme-and-honest-ui-design.md`.
enum Theme {
    // MARK: - Поверхности

    /// Фон окна.
    static let surfaceRoot = dynamic(light: 0xFFFFFF, dark: 0x0D0D0F)
    /// Панели и toolbar.
    static let surface = dynamic(light: 0xF5F5F7, dark: 0x141416)
    /// Карточки и приподнятые строки.
    static let surfaceElevated = dynamic(light: 0xFFFFFF, dark: 0x1C1C1F)
    /// Всплывающие панели.
    static let surfaceOverlay = dynamic(light: 0xFFFFFF, dark: 0x242428)
    /// Стекло поверх контента. В светлой теме белое и куда плотнее:
    /// чёрная вуаль на светлом фоне читается как грязь, а не как стекло.
    static let surfaceGlass = dynamic(
        light: 0xFFFFFF, dark: 0x000000,
        lightOpacity: 0.72, darkOpacity: 0.19
    )

    // MARK: - Текст

    static let textPrimary = dynamic(light: 0x1D1D1F, dark: 0xFFFFFF)
    static let textSecondary = dynamic(light: 0x6E6E73, dark: 0xA1A1A6)
    static let textTertiary = dynamic(light: 0x7C7C82, dark: 0x6E6E73)
    static let textDisabled = dynamic(light: 0xC7C7CC, dark: 0x48484D)

    // MARK: - Акцент и статусы

    /// Системный синий Apple. В светлой теме на шаг темнее системного
    /// `#007AFF`: тот даёт на белом 4.02:1, ниже порога 4.5:1 для
    /// обычного текста, а этим цветом красятся подписи в 10–13 пунктов.
    static let accent = dynamic(light: 0x0069D9, dark: 0x0A84FF)
    /// Подложка выбранного элемента.
    static let accentBackground = dynamic(
        light: 0x0069D9, dark: 0x0A84FF,
        lightOpacity: 0.10, darkOpacity: 0.09
    )
    static let success = dynamic(light: 0x1D7A32, dark: 0x30D158)
    /// В светлой теме оранжевое: жёлтое на белом даёт 1.41:1.
    static let warning = dynamic(light: 0xB25000, dark: 0xFFD60A)
    static let error = dynamic(light: 0xD70015, dark: 0xFF453A)
    static let info = dynamic(light: 0x0071A4, dark: 0x64D2FF)

    // MARK: - Границы

    static let borderSubtle = dynamic(
        light: 0x000000, dark: 0xFFFFFF, lightOpacity: 0.06, darkOpacity: 0.06
    )
    static let borderDefault = dynamic(
        light: 0x000000, dark: 0xFFFFFF, lightOpacity: 0.10, darkOpacity: 0.10
    )
    static let borderStrong = dynamic(
        light: 0x000000, dark: 0xFFFFFF, lightOpacity: 0.14, darkOpacity: 0.14
    )

    // MARK: - Разрешение по теме

    /// Цвет, знающий обе темы.
    ///
    /// `NSColor` собирается из компонент напрямую, а не через
    /// `NSColor(Color(hex:))`: провайдер зовётся при отрисовке, и путь
    /// через SwiftUI протаскивал бы туда лишний слой преобразования.
    private static func dynamic(
        light: UInt32,
        dark: UInt32,
        lightOpacity: Double = 1,
        darkOpacity: Double = 1
    ) -> Color {
        Color(nsColor: NSColor(name: nil) { appearance in
            let isDark = appearance.bestMatch(from: [.aqua, .darkAqua]) == .darkAqua
            return srgb(isDark ? dark : light, isDark ? darkOpacity : lightOpacity)
        })
    }

    private static func srgb(_ hex: UInt32, _ opacity: Double) -> NSColor {
        NSColor(
            srgbRed: CGFloat((hex >> 16) & 0xFF) / 255,
            green: CGFloat((hex >> 8) & 0xFF) / 255,
            blue: CGFloat(hex & 0xFF) / 255,
            alpha: CGFloat(opacity)
        )
    }
```

Дальше по файлу `enum Text`, `enum Space`, `enum Radius` и расширение
`Color` с `init(hex:opacity:)` остаются **без единой правки**: `Color(hex:)`
всё ещё нужен тестам и одноразовым цветам вроде `StatusKind`.

Комментарий про единственную тёмную тему (строки 9–11 старого файла)
уходит вместе с заменой.

- [ ] **Step 4: Убедиться, что тест зелёный**

```
xcodebuild -project MeetingRaft.xcodeproj -scheme MeetingRaft \
  -only-testing:MeetingRaftTests/ThemeTests test CODE_SIGNING_ALLOWED=NO
```

Ожидается PASS всех тестов класса.

- [ ] **Step 5: Прогнать форматтер**

```
swiftformat --lint apps/macos/Sources apps/macos/Tests
```

- [ ] **Step 6: Коммит**

```bash
git add apps/macos/Sources/DesignSystem/Theme.swift apps/macos/Tests/ThemeTests.swift
git commit -m "feat: every colour token knows both appearances"
```

---

### Task 2: Прибор, судящий палитру, и его отрицательный контроль

Зелёный тест здесь ничего не значит сам по себе: за Epic 19 трижды
тестовые данные обходили проверяемое условие стороной. Поэтому тест
пишется вместе с заведомо провальной палитрой и проверяется на ней.

**Files:**
- Create: `apps/macos/Tests/ThemeContrastTests.swift`
- Modify: `apps/macos/Sources/DesignSystem/Theme.swift` — **только на время
  шага 5**, правка откатывается

**Interfaces:**
- Consumes: `Theme.textPrimary`, `Theme.textSecondary`, `Theme.textTertiary`,
  `Theme.accent`, `Theme.success`, `Theme.warning`, `Theme.error`,
  `Theme.info`, `Theme.surfaceRoot`, `Theme.surface`, `Theme.surfaceElevated`
  из Task 1.
- Produces: ничего для следующих задач.

- [ ] **Step 1: Написать тест контраста**

Создать `apps/macos/Tests/ThemeContrastTests.swift`:

```swift
@testable import MeetingRaft
import SwiftUI
import XCTest

/// Контраст токенов по WCAG 2.1 в обеих темах.
///
/// Тест существует потому, что палитру нельзя проверить глазом на
/// Linux-машине, где она пишется, и потому что глазом её плохо видно и
/// на Маке: жёлтое `#FFD60A` на белом даёт 1.41:1, и заметно это не
/// раньше, чем на такой надписи споткнётся человек.
///
/// Пороги тирами, а не одним числом. Сплошной порог 4.5:1 покраснел бы на
/// работающей тёмной теме: третичный текст в ней 3.35:1, и это осознанно
/// — им набраны необязательные подписи. Выключенный текст не проверяется
/// вовсе: он обязан читаться выключенным, и WCAG выводит его из-под
/// требования.
@MainActor
final class ThemeContrastTests: XCTestCase {
    private struct Token {
        let name: String
        let color: Color
        /// Порог WCAG для этого цвета.
        let floor: Double
    }

    private var foregrounds: [Token] {
        [
            Token(name: "textPrimary", color: Theme.textPrimary, floor: 4.5),
            Token(name: "textSecondary", color: Theme.textSecondary, floor: 4.5),
            Token(name: "textTertiary", color: Theme.textTertiary, floor: 3.0),
            Token(name: "accent", color: Theme.accent, floor: 4.5),
            Token(name: "success", color: Theme.success, floor: 4.5),
            Token(name: "warning", color: Theme.warning, floor: 4.5),
            Token(name: "error", color: Theme.error, floor: 4.5),
            Token(name: "info", color: Theme.info, floor: 4.5),
        ]
    }

    private var surfaces: [(name: String, color: Color)] {
        [
            ("surfaceRoot", Theme.surfaceRoot),
            ("surface", Theme.surface),
            ("surfaceElevated", Theme.surfaceElevated),
        ]
    }

    /// Относительная яркость по WCAG 2.1.
    private func luminance(_ color: Color, in name: NSAppearance.Name) -> Double {
        var r: CGFloat = 0
        var g: CGFloat = 0
        var b: CGFloat = 0
        var a: CGFloat = 0
        NSAppearance(named: name)!.performAsCurrentDrawingAppearance {
            let native = NSColor(color).usingColorSpace(.sRGB) ?? .black
            native.getRed(&r, green: &g, blue: &b, alpha: &a)
        }
        func channel(_ value: CGFloat) -> Double {
            let x = Double(value)
            return x <= 0.03928 ? x / 12.92 : pow((x + 0.055) / 1.055, 2.4)
        }
        return 0.2126 * channel(r) + 0.7152 * channel(g) + 0.0722 * channel(b)
    }

    private func ratio(_ foreground: Color, on background: Color, in name: NSAppearance.Name) -> Double {
        let a = luminance(foreground, in: name)
        let b = luminance(background, in: name)
        return (max(a, b) + 0.05) / (min(a, b) + 0.05)
    }

    private func assertPalette(_ name: NSAppearance.Name, file: StaticString = #filePath, line: UInt = #line) {
        for token in foregrounds {
            for surface in surfaces {
                let value = ratio(token.color, on: surface.color, in: name)
                XCTAssertGreaterThanOrEqual(
                    value,
                    token.floor,
                    "\(name.rawValue): \(token.name) на \(surface.name) даёт \(String(format: "%.2f", value)):1",
                    file: file,
                    line: line
                )
            }
        }
    }

    func testDarkPaletteMeetsContrastFloors() {
        assertPalette(.darkAqua)
    }

    func testLightPaletteMeetsContrastFloors() {
        assertPalette(.aqua)
    }

    /// Прозрачные токены яркостью не судятся: у стекла и границ фон
    /// просвечивает, и число вышло бы про несуществующий цвет. Тест
    /// стоит здесь, чтобы список проверяемых не разъехался с палитрой
    /// молча — новый непрозрачный цвет обязан попасть в `foregrounds`.
    func testEveryOpaqueForegroundTokenIsAudited() {
        XCTAssertEqual(foregrounds.count, 8)
        XCTAssertEqual(surfaces.count, 3)
    }
}
```

- [ ] **Step 2: Убедиться, что тест собирается и зелёный**

```
xcodebuild -project MeetingRaft.xcodeproj -scheme MeetingRaft \
  -only-testing:MeetingRaftTests/ThemeContrastTests test CODE_SIGNING_ALLOWED=NO
```

Ожидается PASS. Худшая пара — вторичный текст на `surface` в светлой
теме, 4.66:1.

- [ ] **Step 3: Коммит теста**

```bash
git add apps/macos/Tests/ThemeContrastTests.swift
git commit -m "test: the palette is judged by contrast, not by eye"
```

- [ ] **Step 4: Подставить заведомо провальную палитру**

Временно, **без коммита**, заменить в `Theme.swift` пять светлых значений
на перенесённые из тёмной темы как есть:

```swift
    static let textTertiary = dynamic(light: 0x8E8E93, dark: 0x6E6E73)
    static let success = dynamic(light: 0x30D158, dark: 0x30D158)
    static let warning = dynamic(light: 0xFFD60A, dark: 0xFFD60A)
    static let error = dynamic(light: 0xFF453A, dark: 0xFF453A)
    static let info = dynamic(light: 0x64D2FF, dark: 0x64D2FF)
```

- [ ] **Step 5: Убедиться, что тест краснеет ровно на пяти цветах**

```
xcodebuild -project MeetingRaft.xcodeproj -scheme MeetingRaft \
  -only-testing:MeetingRaftTests/ThemeContrastTests test CODE_SIGNING_ALLOWED=NO
```

Ожидается провал `testLightPaletteMeetsContrastFloors` с сообщениями про
`textTertiary` (3.26 на белом), `success` (2.02), `warning` (1.41),
`error` (3.41), `info` (1.72). `testDarkPaletteMeetsContrastFloors`
обязан остаться зелёным — иначе тест ловит не тему, а что-то другое.

Числа посчитаны заранее и расходиться не должны; расхождение означает,
что `performAsCurrentDrawingAppearance` не назначает тему и цвет
разрешается не тот, — тогда тест не годен и разбираться надо с ним, а не
с палитрой.

- [ ] **Step 6: Откатить подставную палитру**

```bash
git checkout apps/macos/Sources/DesignSystem/Theme.swift
```

Убедиться, что тест снова зелёный. Числа отрицательного контроля выписать
себе — они лягут в `docs/mac-verification.md` задачей 7, где заводится
сам раздел.

---

### Task 3: Хранилище выбранной темы

**Files:**
- Create: `apps/macos/Sources/App/AppearanceSettingsStore.swift`
- Test: `apps/macos/Tests/AppearanceSettingsStoreTests.swift`

**Interfaces:**
- Consumes: ничего.
- Produces: `AppearanceSettingsStore` (`@Observable`, `@MainActor` не
  нужен) с полем `preference: AppearancePreference` и вычисляемым
  `colorScheme: ColorScheme?`; `enum AppearancePreference: String,
  CaseIterable, Identifiable { case auto, light, dark }` с `title: String`.
  Task 4 и Task 5 берут отсюда `colorScheme` и `preference`.

- [ ] **Step 1: Написать падающий тест**

Создать `apps/macos/Tests/AppearanceSettingsStoreTests.swift` по образцу
`TranslationSettingsStoreTests`:

```swift
@testable import MeetingRaft
import SwiftUI
import XCTest

final class AppearanceSettingsStoreTests: XCTestCase {
    private func makeDefaults(_ name: String = #function) -> UserDefaults {
        let defaults = UserDefaults(suiteName: "test.appearance.\(name)")!
        defaults.removePersistentDomain(forName: "test.appearance.\(name)")
        return defaults
    }

    /// Умолчание — системная тема. Приложение, навязывающее свою при
    /// первом запуске, спорит с настройкой, которую человек уже сделал в
    /// системе.
    func testDefaultsToAuto() {
        let store = AppearanceSettingsStore(defaults: makeDefaults())

        XCTAssertEqual(store.preference, .auto)
        XCTAssertNil(store.colorScheme)
    }

    func testExplicitChoiceMapsToScheme() {
        let store = AppearanceSettingsStore(defaults: makeDefaults())

        store.preference = .light
        XCTAssertEqual(store.colorScheme, .light)

        store.preference = .dark
        XCTAssertEqual(store.colorScheme, .dark)
    }

    func testChoiceSurvivesRestart() {
        let defaults = makeDefaults()
        let first = AppearanceSettingsStore(defaults: defaults)
        first.preference = .light

        let second = AppearanceSettingsStore(defaults: defaults)

        XCTAssertEqual(second.preference, .light)
    }

    /// Мусор в `UserDefaults` не должен ронять запуск и не должен молча
    /// становиться тёмной темой: неизвестное значение — это «не выбрано».
    func testUnknownStoredValueFallsBackToAuto() {
        let defaults = makeDefaults()
        defaults.set("chartreuse", forKey: "appearance.preference")

        let store = AppearanceSettingsStore(defaults: defaults)

        XCTAssertEqual(store.preference, .auto)
    }
}
```

- [ ] **Step 2: Убедиться, что тест падает**

```
xcodebuild -project MeetingRaft.xcodeproj -scheme MeetingRaft \
  -only-testing:MeetingRaftTests/AppearanceSettingsStoreTests test CODE_SIGNING_ALLOWED=NO
```

Ожидается ошибка сборки: `cannot find 'AppearanceSettingsStore' in scope`.

- [ ] **Step 3: Написать хранилище**

Создать `apps/macos/Sources/App/AppearanceSettingsStore.swift`:

```swift
import Foundation
import Observation
import SwiftUI

/// Тема оформления: системная, светлая или тёмная.
///
/// До этого окно было принудительно тёмным, потому что светлой палитры не
/// существовало. Теперь существует, и выбор возвращается человеку.
enum AppearancePreference: String, CaseIterable, Identifiable {
    case auto
    case light
    case dark

    var id: String { rawValue }

    var title: String {
        switch self {
        case .auto: String(localized: "System")
        case .light: String(localized: "Light")
        case .dark: String(localized: "Dark")
        }
    }
}

/// Выбранная тема. Переживает перезапуск: настройка делается один раз.
@Observable
final class AppearanceSettingsStore {
    var preference: AppearancePreference {
        didSet {
            defaults.set(preference.rawValue, forKey: Keys.preference)
        }
    }

    /// `nil` означает «как в системе» — именно это `preferredColorScheme`
    /// понимает как отсутствие навязанной темы.
    var colorScheme: ColorScheme? {
        switch preference {
        case .auto: nil
        case .light: .light
        case .dark: .dark
        }
    }

    private let defaults: UserDefaults

    init(defaults: UserDefaults = .standard) {
        self.defaults = defaults
        let stored = defaults.string(forKey: Keys.preference)
        // Неизвестное значение — это «не выбрано», а не повод упасть или
        // молча назначить тёмную.
        preference = stored.flatMap(AppearancePreference.init(rawValue:)) ?? .auto
    }

    private enum Keys {
        static let preference = "appearance.preference"
    }
}
```

- [ ] **Step 4: Убедиться, что тест зелёный**

```
xcodebuild -project MeetingRaft.xcodeproj -scheme MeetingRaft \
  -only-testing:MeetingRaftTests/AppearanceSettingsStoreTests test CODE_SIGNING_ALLOWED=NO
```

Ожидается PASS.

- [ ] **Step 5: Коммит**

```bash
git add apps/macos/Sources/App/AppearanceSettingsStore.swift \
        apps/macos/Tests/AppearanceSettingsStoreTests.swift
git commit -m "feat: the chosen appearance outlives the launch"
```

---

### Task 4: Переключатель в настройках и снятие жёсткой темы

Здесь тестов не появляется, и это осознанно: проверяемая часть — отображение
выбора в `ColorScheme?` — уже покрыта Task 3, а `.preferredColorScheme` на
вьюхе проверяется только глазом. Врать про покрытие хуже, чем его не иметь.

**Files:**
- Modify: `apps/macos/Sources/MeetingRaftApp.swift:6-13, 17-26, 47-53`
- Modify: `apps/macos/Sources/Shell/AppShellView.swift:56-64`
- Modify: `apps/macos/Sources/Settings/SettingsView.swift:8-14, 39`
- Modify: `apps/macos/Sources/Settings/SettingsSections.swift:48-51, 66-68`

**Interfaces:**
- Consumes: `AppearanceSettingsStore`, `AppearancePreference` из Task 3.
- Produces: `AppearanceSettingsStore` в окружении обеих сцен — Task 5
  читает его из `AppShellView`.

- [ ] **Step 1: Завести хранилище в точке входа**

В `MeetingRaftApp.swift` после строки 9 добавить:

```swift
    @State private var appearanceStore = AppearanceSettingsStore()
```

И протащить в обе сцены — в `WindowGroup` после `.environment(presenceStore)`
(строка 21) и в `Settings` после `.environment(presenceStore)` (строка 52),
в обоих местах одной строкой:

```swift
                .environment(appearanceStore)
```

- [ ] **Step 2: Снять жёсткую тему с главного окна**

В `AppShellView.swift` добавить к остальным `@Environment` в начале
структуры:

```swift
    @Environment(AppearanceSettingsStore.self) private var appearanceStore
```

Заменить строку 64 `.preferredColorScheme(.dark)` на:

```swift
        .preferredColorScheme(appearanceStore.colorScheme)
```

Комментарий выше (строки 56–58, «Тема принудительно тёмная: светлая
палитра вынесена за скобки») заменить на:

```swift
        // Оболочка окна переведена на токены (ТЗ редизайна, D1, шаг 3).
        // Тема идёт из настроек; `nil` означает системную.
```

- [ ] **Step 3: Снять жёсткую тему с настроек**

В `SettingsView.swift` добавить к `@Environment` (после строки 10):

```swift
    @Environment(AppearanceSettingsStore.self) private var appearanceStore
```

Заменить строку 39 `.preferredColorScheme(.dark)` на:

```swift
        .preferredColorScheme(appearanceStore.colorScheme)
```

- [ ] **Step 4: Добавить переключатель в General**

В `SettingsSections.swift` в `GeneralSettingsSection` добавить к
`@Environment` (после строки 50):

```swift
    @Environment(AppearanceSettingsStore.self) private var appearanceStore
```

И вставить строку сразу после `Divider()` на строке 67, то есть между
языком сессии и списком распознаваемых языков:

```swift
            SettingsRow(
                title: String(localized: "Appearance"),
                caption: String(localized: "System follows the macOS setting.")
            ) {
                Picker("", selection: Bindable(appearanceStore).preference) {
                    ForEach(AppearancePreference.allCases) { option in
                        Text(option.title).tag(option)
                    }
                }
                .labelsHidden()
                .frame(width: 160)
            }

            Divider().overlay(Theme.borderSubtle)
```

- [ ] **Step 5: Собрать и прогнать тесты целиком**

```
xcodebuild -project MeetingRaft.xcodeproj -scheme MeetingRaft \
  test CODE_SIGNING_ALLOWED=NO
```

Ожидается PASS. Сборка — главное, что здесь проверяется: `@Environment`
без соответствующего `.environment()` падает не на компиляции, а в
рантайме при открытии экрана.

- [ ] **Step 6: Проверить глазом**

Запустить приложение, в Settings → General переключить Appearance:
System → Light → Dark. Убедиться, что светлеет **и** главное окно, **и**
окно настроек, и что при System тема идёт за системной (переключить в
Системных настройках при открытом приложении).

- [ ] **Step 7: Коммит**

```bash
git add apps/macos/Sources/MeetingRaftApp.swift \
        apps/macos/Sources/Shell/AppShellView.swift \
        apps/macos/Sources/Settings/SettingsView.swift \
        apps/macos/Sources/Settings/SettingsSections.swift
git commit -m "feat: the window stops forcing its own darkness"
```

---

### Task 5: Накладка подчиняется той же теме

`NSPanel` живёт вне иерархии SwiftUI, и `preferredColorScheme` до него не
доходит. Забыть о нём — значит получить тёмную плашку поверх светлого
приложения, причём именно в том сценарии, ради которого накладка и
существует: главное окно свёрнуто, видна только она.

**Files:**
- Modify: `apps/macos/Sources/Presence/OverlayWindowController.swift:11-53`
- Modify: `apps/macos/Sources/Shell/AppShellView.swift:122-127` (реакция на
  смену настройки) и `:175-185` (вызов `overlay.show` внутри
  `applyPresence()`)
- Test: `apps/macos/Tests/OverlayAppearanceTests.swift` (создать)

**Interfaces:**
- Consumes: `AppearancePreference` из Task 3, `AppearanceSettingsStore` из
  окружения `AppShellView` (Task 4).
- Produces: `OverlayWindowController.show(content:preference:)` и
  `var panelAppearanceName: NSAppearance.Name?`.

- [ ] **Step 1: Написать падающий тест**

Создать `apps/macos/Tests/OverlayAppearanceTests.swift`:

```swift
@testable import MeetingRaft
import AppKit
import SwiftUI
import XCTest

/// Накладка — единственное окно вне иерархии SwiftUI, и тема до неё сама
/// не доходит. Тест держит именно эту связь.
@MainActor
final class OverlayAppearanceTests: XCTestCase {
    func testLightPreferenceGivesAquaPanel() {
        let controller = OverlayWindowController()
        defer { controller.hide() }

        controller.show(content: Text("тест"), preference: .light)

        XCTAssertEqual(controller.panelAppearanceName, .aqua)
    }

    func testDarkPreferenceGivesDarkAquaPanel() {
        let controller = OverlayWindowController()
        defer { controller.hide() }

        controller.show(content: Text("тест"), preference: .dark)

        XCTAssertEqual(controller.panelAppearanceName, .darkAqua)
    }

    /// Системная тема — это отсутствие навязанной, а не тёмная: панель с
    /// `nil` берёт тему приложения.
    func testAutoPreferenceLeavesPanelToTheSystem() {
        let controller = OverlayWindowController()
        defer { controller.hide() }

        controller.show(content: Text("тест"), preference: .auto)

        XCTAssertNil(controller.panelAppearanceName)
    }

    /// Повторный показ обновляет уже открытую панель — тема обязана
    /// переехать вместе с содержимым, иначе смена настройки во время
    /// записи оставит панель прежней.
    func testReshowUpdatesAppearanceOfTheLivePanel() {
        let controller = OverlayWindowController()
        defer { controller.hide() }

        controller.show(content: Text("тест"), preference: .dark)
        controller.show(content: Text("тест"), preference: .light)

        XCTAssertEqual(controller.panelAppearanceName, .aqua)
    }
}
```

- [ ] **Step 2: Убедиться, что тест падает**

```
xcodebuild -project MeetingRaft.xcodeproj -scheme MeetingRaft \
  -only-testing:MeetingRaftTests/OverlayAppearanceTests test CODE_SIGNING_ALLOWED=NO
```

Ожидается ошибка сборки: у `show` нет параметра `preference`, и нет
`panelAppearanceName`.

- [ ] **Step 3: Научить контроллер теме**

В `OverlayWindowController.swift` после свойства `isVisible` (строка 19)
добавить:

```swift
    /// Тема панели. `nil` — панель идёт за темой приложения.
    var panelAppearanceName: NSAppearance.Name? {
        panel?.appearance?.name
    }
```

Заменить сигнатуру и начало `show` (строки 21–29) на:

```swift
    /// Показать накладку с новым содержимым; повторный вызов обновляет
    /// уже открытую панель, а не создаёт вторую.
    ///
    /// Тема передаётся явно: панель живёт вне иерархии SwiftUI, и
    /// `preferredColorScheme` до неё не доходит.
    func show(content: some View, preference: AppearancePreference) {
        let hosting = NSHostingView(rootView: AnyView(content))
        if let panel {
            panel.appearance = Self.appearance(for: preference)
            panel.contentView = hosting
            panel.orderFrontRegardless()
            return
        }
```

В создании панели, сразу после `panel.level = .floating` (строка 37),
добавить:

```swift
        panel.appearance = Self.appearance(for: preference)
```

И в конце типа, перед `positionNearBottom`, добавить:

```swift
    private static func appearance(for preference: AppearancePreference) -> NSAppearance? {
        switch preference {
        case .auto: nil
        case .light: NSAppearance(named: .aqua)
        case .dark: NSAppearance(named: .darkAqua)
        }
    }
```

- [ ] **Step 4: Передать тему из оболочки**

В `AppShellView.swift` в методе `applyPresence()` вызов `overlay.show(`
(строки 176–185) получает второй аргумент. Целиком после правки:

```swift
            overlay.show(
                content: CaptionOverlayView(
                    lines: captionsViewModel.recentLines(limit: 2),
                    isRecording: captureCoordinator.isRecording,
                    showsSpeaker: captureCoordinator.systemAudioAvailable,
                    opacity: presenceStore.overlayOpacity
                ) {
                    captionsViewModel.stopLive(capture: captureCoordinator)
                },
                preference: appearanceStore.preference
            )
```

И реакция на смену настройки — иначе переключение темы во время записи не
доедет до открытой панели. Добавить сразу после `onChange` на
`presenceStore.minimizesMainWindow` (строки 125–127), рядом с остальными:

```swift
        .onChange(of: appearanceStore.preference) { _, _ in
            // Накладка живёт вне иерархии SwiftUI: сама она о смене темы
            // не узнает, её пересобирает тот же путь, что и всё остальное
            // присутствие.
            if overlay.isVisible {
                applyPresence()
            }
        }
```

- [ ] **Step 5: Убедиться, что тест зелёный**

```
xcodebuild -project MeetingRaft.xcodeproj -scheme MeetingRaft \
  -only-testing:MeetingRaftTests/OverlayAppearanceTests test CODE_SIGNING_ALLOWED=NO
```

Ожидается PASS всех четырёх.

- [ ] **Step 6: Коммит**

```bash
git add apps/macos/Sources/Presence/OverlayWindowController.swift \
        apps/macos/Sources/Shell/AppShellView.swift \
        apps/macos/Tests/OverlayAppearanceTests.swift
git commit -m "feat: the floating strip follows the chosen appearance"
```

---

### Task 6: Заглушка backend refine убирается целиком

`backend/app/main.py:95` пропускает через настоящую LLM только `brief` и
`follow_up`; `refine` отдаёт заглушечный markdown. Кнопка с честной
подписью «(stub)» — ровно то, что правило продукта запрещает показывать.

Удаляется джоб `refine` из интерфейса, **не** путь к backend: Brief и
Follow-up через backend остаются, они настоящие. `JobKind::Refine` в
`rust/crates/sync/src/dto.rs` и в `shared/openapi.yaml` остаётся тоже —
контракт описывает backend, а не наш интерфейс.

**Files:**
- Modify: `apps/macos/Sources/Meetings/MeetingDetailView.swift:407-414`
  (кнопка), `:500-540` (панель `backendRefinePanel` и её вызов)
- Modify: `apps/macos/Sources/Meetings/MeetingsViewModel.swift:66-67`,
  `:369-470` (весь блок refine), плюс объявление `backendRefineTask`,
  `backendArtifactMarkdown` и `enum BackendRefineStatus`
- Modify: `apps/macos/Tests/MeetingsViewModelTests.swift:160-215` (тесты
  refine-джоба)

**Interfaces:**
- Consumes: ничего.
- Produces: ничего.

- [ ] **Step 1: Найти всё, что держит эту ветку**

```bash
cd apps/macos && grep -rn "backendJob\|backendRefine\|backendArtifactMarkdown\|BackendRefineStatus\|submitBackendRefine\|resetBackendRefineSession" Sources/ Tests/
```

Выписать список мест. Ожидается: `MeetingsViewModel` (объявления,
`submitBackendRefine`, `performBackendRefine`, `resetBackendRefineSession`,
опрос джоба), `MeetingDetailView` (кнопка и панель), `MeetingsViewModelTests`
(тесты успеха и отказа джоба).

- [ ] **Step 2: Удалить тесты refine-джоба**

Убрать из `MeetingsViewModelTests.swift` тесты, обращающиеся к
`viewModel.backendJobStatus`, `viewModel.backendJobId`,
`viewModel.backendArtifactMarkdown` и к `kind: "refine"`. Остальные тесты
файла — про Brief, Follow-up, экспорт и список встреч — остаются.

- [ ] **Step 3: Удалить кнопку и панель**

В `MeetingDetailView.swift` убрать `Button("Submit refine (stub)", …)`
целиком вместе с `.help`, `.disabled` (строки 407–414), убрать
`backendRefinePanel` и предшествующий ему `Divider()` из тела экрана, и
удалить само вычисляемое свойство `backendRefinePanel`.

- [ ] **Step 4: Удалить машинерию из вью-модели**

В `MeetingsViewModel.swift` убрать `backendJobStatus`, `backendJobId`,
`backendArtifactMarkdown`, `backendRefineTask`, `submitBackendRefine`,
`performBackendRefine`, `resetBackendRefineSession`, `enum
BackendRefineStatus` и константы опроса (`maxPollAttempts`,
`pollDelayNanoseconds`), **если** последние не используются брифом и
follow-up. Проверить `grep -n "maxPollAttempts\|pollDelayNanoseconds"`
перед удалением: используются обоими — оставить.

Все вызовы `resetBackendRefineSession()` из других мест вью-модели убрать
вместе с ней.

- [ ] **Step 5: Собрать и прогнать тесты целиком**

```
xcodebuild -project MeetingRaft.xcodeproj -scheme MeetingRaft \
  test CODE_SIGNING_ALLOWED=NO
```

Ожидается PASS. Отсутствие ошибок «unused» здесь ничего не гарантирует —
Swift не ругается на мёртвый `private`, — поэтому шаг 1 делается grep'ом,
а не на глаз.

- [ ] **Step 6: Проверить глазом**

Открыть встречу → Artifacts. Убедиться, что кнопки Submit refine нет,
панели Backend refine нет, а Generate Brief и Generate Follow-up на месте
и работают.

- [ ] **Step 7: Коммит**

```bash
git add apps/macos/Sources/Meetings/MeetingDetailView.swift \
        apps/macos/Sources/Meetings/MeetingsViewModel.swift \
        apps/macos/Tests/MeetingsViewModelTests.swift
git commit -m "fix: a button that honestly says stub is still a stub"
```

---

### Task 7: Документы догоняют код

**Files:**
- Modify: `docs/backlog.md` — Epic 23 (строки 2593–2639)
- Modify: `docs/roadmap.md` — Phase 15 (строки 358–371)
- Modify: `docs/mac-verification.md` — новый раздел
- Modify: `docs/ui-redesign-macos-2026-08-03.md` — §2.1, §2.2

**Interfaces:**
- Consumes: результаты Task 1–6.
- Produces: ничего.

- [ ] **Step 1: Закрыть Epic 23 числами**

В `docs/backlog.md` в разделе «Работы» Epic 23 отметить сделанное и
записать три вещи, которых в эпике не было: решение по шрифту (SF Pro
остаётся, пункт ТЗ про Inter отменён), что радиусы уже совпадали с ТЗ и не
трогались, и что светлый системный синий Apple пришлось затемнить —
`#007AFF` даёт на белом 4.02:1.

- [ ] **Step 2: Обновить Phase 15 в роадмапе**

Отметить, что телеметрия, номера ADR и демо-кнопка убраны раньше, а
последняя заглушка (`Submit refine`) снята этой работой. Локализация
остаётся открытой и уезжает в подпроект 1b.

- [ ] **Step 3: Записать сценарий проверки**

В `docs/mac-verification.md` добавить раздел «Тема и контраст»: как
гонять `ThemeContrastTests`, какие числа даёт отрицательный контроль
(пять провалов: 3.26 / 2.02 / 1.41 / 3.41 / 1.72), и что проверяется
глазом — переключение System / Light / Dark на трёх поверхностях
(главное окно, настройки, накладка поверх встречи).

- [ ] **Step 4: Поправить ТЗ редизайна**

В `docs/ui-redesign-macos-2026-08-03.md` §2.1 дописать колонку светлых
значений, в §2.2 заменить предписание Inter на «SF Pro, решение
2026-08-20», сославшись на спеку.

- [ ] **Step 5: Коммит**

```bash
git add docs/backlog.md docs/roadmap.md docs/mac-verification.md \
        docs/ui-redesign-macos-2026-08-03.md
git commit -m "docs: the light palette and what it cost to compute it"
```

- [ ] **Step 6: Полная проверка и PR**

```
scripts/verify-mac.sh
```

Все семь шагов зелёные. Затем сверить локальное с удалённым **до** мерджа:

```bash
git log --oneline origin/feat/light-theme-and-honest-ui..feat/light-theme-and-honest-ui
```

Пустой вывод — можно `gh pr merge`. Непустой — сперва `git push`.

---

## Что этот план сознательно не содержит

- **Локализации.** Каталог `Localizable.xcstrings` и вычистка
  захардкоженного русского — подпроект 1b, отдельная спека и отдельный
  план. Решение пользователя 2026-08-20.
- **Перекомпоновки экранов** по ТЗ §4. Здесь только палитра, тема и одна
  удалённая заглушка.
- **Правки радиусов, отступов и шрифта.** Разбор — в спеке.
- **Светлых вариантов для `StatusKind`** и прочих одноразовых цветов вне
  `Theme`: если такие найдутся при работе, они записываются в беклог, а не
  правятся мимоходом.
