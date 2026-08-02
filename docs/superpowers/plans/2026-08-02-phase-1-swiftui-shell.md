# Phase 1 — SwiftUI Shell with Fake Subtitle Stream Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Кликнутый shell MeetingRaft: sidebar, toolbar, settings с языком сессии (default `ru`), menu commands и live-captions экран с fake subtitle stream (partial/final).

**Architecture:** Только presentation-слой в Swift. Views не содержат бизнес-логики: язык и captions живут в `@Observable` stubs/view models. Fake stream — `Timer`/`Task` в `FakeCaptionStream`, не в View. Rust/UniFFI не трогаем (Phase 2).

**Tech Stack:** SwiftUI, Swift 6, macOS 15.0+, XcodeGen, XCTest (unit target), SwiftFormat.

## Global Constraints

- Минимальная macOS: **15.0**; bundle id: `com.vitvitsky.meetingraft`.
- Идентификаторы английские, комментарии/docstrings русские (AGENTS.md).
- Коммиты: Conventional Commits с русской темой.
- SwiftUI без networking/business rules; session language policy stub: primary `ru`, allowed `{ru, en, es}` (ADR-003).
- `.xcodeproj` и `Sources/Info.plist` не коммитятся.
- Fake captions — Swift-local; в Phase 2 переедут в Rust.

## File map

| Path | Responsibility |
|------|----------------|
| `apps/macos/Sources/App/SpeechLanguage.swift` | Enum `ru`/`en`/`es` + display names |
| `apps/macos/Sources/App/SessionLanguageStore.swift` | Stub store: primary language, default `ru` |
| `apps/macos/Sources/App/AppDestination.swift` | Sidebar destinations |
| `apps/macos/Sources/App/MeetingRaftApp.swift` | Scenes, Settings, Commands (modify) |
| `apps/macos/Sources/Shell/AppShellView.swift` | NavigationSplitView + toolbar (replaces ContentView) |
| `apps/macos/Sources/Shell/SidebarView.swift` | Sidebar list |
| `apps/macos/Sources/LiveCaptions/CaptionPhase.swift` | `.partial` / `.final` |
| `apps/macos/Sources/LiveCaptions/CaptionLine.swift` | Display model for one line |
| `apps/macos/Sources/LiveCaptions/FakeCaptionStream.swift` | Timer-driven stub producer |
| `apps/macos/Sources/LiveCaptions/LiveCaptionsViewModel.swift` | Holds lines + start/stop |
| `apps/macos/Sources/LiveCaptions/LiveCaptionsView.swift` | Renders lines with partial/final styling |
| `apps/macos/Sources/Settings/SettingsView.swift` | Language picker bound to store |
| `apps/macos/Sources/ContentView.swift` | Delete after shell lands |
| `apps/macos/Tests/SessionLanguageStoreTests.swift` | Default `ru`, allowed set |
| `apps/macos/Tests/FakeCaptionStreamTests.swift` | Partial then final sequence |
| `apps/macos/project.yml` | Add `MeetingRaftTests` target |

---

### Task 1: Speech language stub + unit tests

**Files:**
- Create: `apps/macos/Sources/App/SpeechLanguage.swift`
- Create: `apps/macos/Sources/App/SessionLanguageStore.swift`
- Create: `apps/macos/Tests/SessionLanguageStoreTests.swift`
- Modify: `apps/macos/project.yml` (add test target)

**Interfaces:**
- Consumes: ничего.
- Produces:
  - `enum SpeechLanguage: String, CaseIterable, Identifiable, Hashable` — cases `ru`, `en`, `es`; `var id: String { rawValue }`; `var displayName: String`
  - `@Observable final class SessionLanguageStore` — `var primary: SpeechLanguage` (default `.ru`); `let allowed: [SpeechLanguage]` = `[.ru, .en, .es]`

- [ ] **Step 1: Добавить test target в `project.yml`**

Заменить `apps/macos/project.yml` на:

```yaml
name: MeetingRaft
options:
  bundleIdPrefix: com.vitvitsky
  deploymentTarget:
    macOS: "15.0"
targets:
  MeetingRaft:
    type: application
    platform: macOS
    sources:
      - path: Sources
        excludes:
          - Info.plist
    settings:
      base:
        PRODUCT_BUNDLE_IDENTIFIER: com.vitvitsky.meetingraft
        MACOSX_DEPLOYMENT_TARGET: "15.0"
        SWIFT_VERSION: "6.0"
    info:
      path: Sources/Info.plist
      properties:
        CFBundleDisplayName: MeetingRaft
  MeetingRaftTests:
    type: bundle.unit-test
    platform: macOS
    sources:
      - Tests
    dependencies:
      - target: MeetingRaft
    settings:
      base:
        PRODUCT_BUNDLE_IDENTIFIER: com.vitvitsky.meetingraft.tests
        MACOSX_DEPLOYMENT_TARGET: "15.0"
        SWIFT_VERSION: "6.0"
        GENERATE_INFOPLIST_FILE: YES
```

- [ ] **Step 2: Падающий тест**

Создать `apps/macos/Tests/SessionLanguageStoreTests.swift`:

```swift
import XCTest
@testable import MeetingRaft

final class SessionLanguageStoreTests: XCTestCase {
    func testDefaultPrimaryIsRussian() {
        let store = SessionLanguageStore()
        XCTAssertEqual(store.primary, .ru)
    }

    func testAllowedLanguagesAreRuEnEs() {
        let store = SessionLanguageStore()
        XCTAssertEqual(store.allowed, [.ru, .en, .es])
    }
}
```

- [ ] **Step 3: Убедиться, что тест не собирается / падает**

Run:
```bash
cd apps/macos && xcodegen generate && \
xcodebuild -project MeetingRaft.xcodeproj -scheme MeetingRaft \
  -configuration Debug test CODE_SIGNING_ALLOWED=NO
```
Expected: FAIL — `SessionLanguageStore` / `SpeechLanguage` отсутствуют.

- [ ] **Step 4: Минимальная реализация**

`apps/macos/Sources/App/SpeechLanguage.swift`:

```swift
import Foundation

/// Язык распознавания речи (ADR-003).
enum SpeechLanguage: String, CaseIterable, Identifiable, Hashable, Sendable {
    case ru
    case en
    case es

    var id: String { rawValue }

    /// Локализованное имя для UI.
    var displayName: String {
        switch self {
        case .ru: "Русский"
        case .en: "English"
        case .es: "Español"
        }
    }
}
```

`apps/macos/Sources/App/SessionLanguageStore.swift`:

```swift
import Foundation
import Observation

/// Stub политики языка сессии; в Phase 2 заменяется Rust/UniFFI.
@Observable
final class SessionLanguageStore {
    /// Primary language; по умолчанию русский.
    var primary: SpeechLanguage = .ru

    /// Разрешённый набор v1.
    let allowed: [SpeechLanguage] = [.ru, .en, .es]
}
```

- [ ] **Step 5: Тесты зелёные**

Run: та же `xcodebuild ... test`
Expected: `TEST SUCCEEDED`, оба теста PASS.

- [ ] **Step 6: Commit**

```bash
git add apps/macos/
git commit -m "feat: stub политики языка сессии (ru primary)"
```

---

### Task 2: Fake caption stream + unit tests

**Files:**
- Create: `apps/macos/Sources/LiveCaptions/CaptionPhase.swift`
- Create: `apps/macos/Sources/LiveCaptions/CaptionLine.swift`
- Create: `apps/macos/Sources/LiveCaptions/FakeCaptionStream.swift`
- Create: `apps/macos/Sources/LiveCaptions/LiveCaptionsViewModel.swift`
- Create: `apps/macos/Tests/FakeCaptionStreamTests.swift`

**Interfaces:**
- Consumes: ничего из Task 1 (независимый captions path).
- Produces:
  - `enum CaptionPhase { case partial, final }`
  - `struct CaptionLine: Identifiable, Equatable` — `id: UUID`, `text: String`, `phase: CaptionPhase`
  - `protocol CaptionStreaming: AnyObject` — `func start(onEvent: @escaping @MainActor (CaptionLine) -> Void)`, `func stop()`
  - `final class FakeCaptionStream: CaptionStreaming` — scripted sequence, injectable clock via `tickInterval`
  - `@Observable final class LiveCaptionsViewModel` — `var lines: [CaptionLine]`, `func start()`, `func stop()`

- [ ] **Step 1: Падающий тест последовательности**

`apps/macos/Tests/FakeCaptionStreamTests.swift`:

```swift
import XCTest
@testable import MeetingRaft

@MainActor
final class FakeCaptionStreamTests: XCTestCase {
    func testEmitsPartialThenFinalForFirstSegment() async throws {
        let stream = FakeCaptionStream(
            script: [
                .init(text: "Привет", phase: .partial),
                .init(text: "Привет, команда", phase: .final),
            ],
            tickNanoseconds: 1_000_000
        )
        var received: [CaptionLine] = []
        let done = expectation(description: "two events")
        done.expectedFulfillmentCount = 2

        stream.start { line in
            received.append(line)
            done.fulfill()
        }

        await fulfillment(of: [done], timeout: 2.0)
        stream.stop()

        XCTAssertEqual(received.map(\.phase), [.partial, .final])
        XCTAssertEqual(received.map(\.text), ["Привет", "Привет, команда"])
    }
}
```

- [ ] **Step 2: Run — FAIL**

Run: `cd apps/macos && xcodegen generate && xcodebuild -project MeetingRaft.xcodeproj -scheme MeetingRaft -configuration Debug test CODE_SIGNING_ALLOWED=NO`
Expected: compile error — missing types.

- [ ] **Step 3: Реализация**

`CaptionPhase.swift`:

```swift
import Foundation

/// Визуальная фаза caption-события (live vs committed).
enum CaptionPhase: Equatable, Sendable {
    case partial
    case final
}
```

`CaptionLine.swift`:

```swift
import Foundation

/// Presentation-модель одной строки субтитров.
struct CaptionLine: Identifiable, Equatable, Sendable {
    let id: UUID
    let text: String
    let phase: CaptionPhase

    init(id: UUID = UUID(), text: String, phase: CaptionPhase) {
        self.id = id
        self.text = text
        self.phase = phase
    }
}
```

`FakeCaptionStream.swift`:

```swift
import Foundation

/// Контракт источника captions для UI (Phase 2 — Rust facade).
protocol CaptionStreaming: AnyObject {
    func start(onEvent: @escaping @MainActor (CaptionLine) -> Void)
    func stop()
}

/// Скриптованный fake stream на Task.sleep.
final class FakeCaptionStream: CaptionStreaming, @unchecked Sendable {
    private let script: [CaptionLine]
    private let tickNanoseconds: UInt64
    private var task: Task<Void, Never>?

    init(script: [CaptionLine]? = nil, tickNanoseconds: UInt64 = 800_000_000) {
        self.script = script ?? Self.defaultScript
        self.tickNanoseconds = tickNanoseconds
    }

    func start(onEvent: @escaping @MainActor (CaptionLine) -> Void) {
        stop()
        let script = self.script
        let tick = tickNanoseconds
        task = Task { @MainActor in
            for line in script {
                if Task.isCancelled { return }
                onEvent(line)
                try? await Task.sleep(nanoseconds: tick)
            }
        }
    }

    func stop() {
        task?.cancel()
        task = nil
    }

    private static let defaultScript: [CaptionLine] = [
        .init(text: "Добро пожаловать", phase: .partial),
        .init(text: "Добро пожаловать в MeetingRaft", phase: .final),
        .init(text: "Язык сессии — русский", phase: .partial),
        .init(text: "Язык сессии — русский по умолчанию", phase: .final),
        .init(text: "English terms are fine", phase: .partial),
        .init(text: "English terms are fine in mixed meetings", phase: .final),
    ]
}
```

`LiveCaptionsViewModel.swift`:

```swift
import Foundation
import Observation

/// Presentation model экрана live captions.
@Observable
@MainActor
final class LiveCaptionsViewModel {
    private(set) var lines: [CaptionLine] = []
    private let stream: CaptionStreaming

    init(stream: CaptionStreaming = FakeCaptionStream()) {
        self.stream = stream
    }

    func start() {
        lines = []
        stream.start { [weak self] line in
            self?.append(line)
        }
    }

    func stop() {
        stream.stop()
    }

    private func append(_ line: CaptionLine) {
        if line.phase == .final, let last = lines.last, last.phase == .partial {
            lines[lines.count - 1] = line
        } else {
            lines.append(line)
        }
    }
}
```

- [ ] **Step 4: Run — PASS**

Expected: `TEST SUCCEEDED`.

- [ ] **Step 5: Commit**

```bash
git add apps/macos/
git commit -m "feat: fake caption stream с partial/final"
```

---

### Task 3: App shell — sidebar, toolbar, captions UI

**Files:**
- Create: `apps/macos/Sources/App/AppDestination.swift`
- Create: `apps/macos/Sources/Shell/SidebarView.swift`
- Create: `apps/macos/Sources/Shell/AppShellView.swift`
- Create: `apps/macos/Sources/LiveCaptions/LiveCaptionsView.swift`
- Modify: `apps/macos/Sources/MeetingRaftApp.swift` → move to `Sources/App/MeetingRaftApp.swift` (or keep path and update imports)
- Delete: `apps/macos/Sources/ContentView.swift`

**Interfaces:**
- Consumes: `LiveCaptionsViewModel`, `SessionLanguageStore`, `SpeechLanguage`
- Produces: runnable shell with `NavigationSplitView`; destinations `.liveCaptions`, `.meetingsPlaceholder`

Keep `MeetingRaftApp.swift` at `Sources/MeetingRaftApp.swift` (XcodeGen already globs Sources) — do not move unless needed. Create new files under subfolders; XcodeGen includes them recursively.

- [ ] **Step 1: Destination + sidebar + shell + captions view**

`AppDestination.swift`:

```swift
import Foundation

/// Пункты боковой навигации.
enum AppDestination: String, Hashable, CaseIterable, Identifiable {
    case liveCaptions
    case meetings

    var id: String { rawValue }

    var title: String {
        switch self {
        case .liveCaptions: "Live Captions"
        case .meetings: "Meetings"
        }
    }

    var systemImage: String {
        switch self {
        case .liveCaptions: "captions.bubble"
        case .meetings: "calendar"
        }
    }
}
```

`SidebarView.swift`:

```swift
import SwiftUI

/// Боковая навигация приложения.
struct SidebarView: View {
    @Binding var selection: AppDestination?

    var body: some View {
        List(AppDestination.allCases, selection: $selection) { destination in
            Label(destination.title, systemImage: destination.systemImage)
                .tag(destination)
        }
        .navigationTitle("MeetingRaft")
    }
}
```

`LiveCaptionsView.swift`:

```swift
import SwiftUI

/// Экран live captions; логика в ViewModel.
struct LiveCaptionsView: View {
    @Bindable var viewModel: LiveCaptionsViewModel
    let primaryLanguage: SpeechLanguage

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            HStack {
                Text("Session language: \(primaryLanguage.displayName)")
                    .foregroundStyle(.secondary)
                Spacer()
                Button("Start demo") { viewModel.start() }
                Button("Stop") { viewModel.stop() }
            }
            .padding(.horizontal)

            List(viewModel.lines) { line in
                Text(line.text)
                    .font(line.phase == .partial ? .body.italic() : .body)
                    .foregroundStyle(line.phase == .partial ? .secondary : .primary)
                    .accessibilityLabel("\(line.phase == .partial ? "Partial" : "Final"): \(line.text)")
            }
        }
        .navigationTitle("Live Captions")
        .onDisappear { viewModel.stop() }
    }
}
```

`AppShellView.swift`:

```swift
import SwiftUI

/// Корневой shell: sidebar + detail + toolbar.
struct AppShellView: View {
    @Environment(SessionLanguageStore.self) private var languageStore
    @State private var selection: AppDestination? = .liveCaptions
    @State private var captionsViewModel = LiveCaptionsViewModel()

    var body: some View {
        NavigationSplitView {
            SidebarView(selection: $selection)
        } detail: {
            switch selection ?? .liveCaptions {
            case .liveCaptions:
                LiveCaptionsView(
                    viewModel: captionsViewModel,
                    primaryLanguage: languageStore.primary
                )
            case .meetings:
                ContentUnavailableView(
                    "Meetings",
                    systemImage: "calendar",
                    description: Text("Появится в следующих фазах")
                )
            }
        }
        .toolbar {
            ToolbarItemGroup(placement: .primaryAction) {
                Picker("Language", selection: Bindable(languageStore).primary) {
                    ForEach(languageStore.allowed) { language in
                        Text(language.displayName).tag(language)
                    }
                }
                .frame(width: 140)
                Button("Start Captions", systemImage: "play.fill") {
                    selection = .liveCaptions
                    captionsViewModel.start()
                }
                .keyboardShortcut("r", modifiers: [.command])
            }
        }
    }
}
```

- [ ] **Step 2: Подключить в App, удалить ContentView**

`MeetingRaftApp.swift`:

```swift
import SwiftUI

/// Точка входа приложения MeetingRaft.
@main
struct MeetingRaftApp: App {
    @State private var languageStore = SessionLanguageStore()

    var body: some Scene {
        WindowGroup {
            AppShellView()
                .environment(languageStore)
        }
        .commands {
            CommandGroup(replacing: .newItem) {}
            CommandMenu("Session") {
                Button("Start Demo Captions") {
                    NotificationCenter.default.post(name: .startDemoCaptions, object: nil)
                }
                .keyboardShortcut("r", modifiers: [.command])
            }
        }

        Settings {
            SettingsView()
                .environment(languageStore)
        }
    }
}

extension Notification.Name {
    static let startDemoCaptions = Notification.Name("startDemoCaptions")
}
```

Note: menu command via NotificationCenter is a thin shell hook; prefer wiring through the same view model later if brittle. Alternative for Step 2: omit NotificationCenter and keep only toolbar shortcut — **prefer toolbar-only in Step 2, add Settings in Task 4, add Commands that duplicate toolbar action via focused value in Task 4**.

Revised `MeetingRaftApp` for this task (commands deferred to Task 4):

```swift
import SwiftUI

@main
struct MeetingRaftApp: App {
    @State private var languageStore = SessionLanguageStore()

    var body: some Scene {
        WindowGroup {
            AppShellView()
                .environment(languageStore)
        }

        Settings {
            SettingsView()
                .environment(languageStore)
        }
    }
}
```

Create stub `SettingsView` so App compiles (full UI in Task 4):

```swift
import SwiftUI

struct SettingsView: View {
    var body: some View {
        Text("Settings")
            .padding()
    }
}
```

Delete `ContentView.swift`.

- [ ] **Step 3: Сборка**

```bash
cd apps/macos && swiftformat --lint Sources Tests && xcodegen generate && \
xcodebuild -project MeetingRaft.xcodeproj -scheme MeetingRaft \
  -configuration Debug build CODE_SIGNING_ALLOWED=NO && \
xcodebuild -project MeetingRaft.xcodeproj -scheme MeetingRaft \
  -configuration Debug test CODE_SIGNING_ALLOWED=NO
```
Expected: BUILD/TEST SUCCEEDED.

- [ ] **Step 4: Commit**

```bash
git add apps/macos/ && git rm apps/macos/Sources/ContentView.swift
git commit -m "feat: SwiftUI shell с sidebar и live captions UI"
```

---

### Task 4: Settings language picker + menu commands

**Files:**
- Modify: `apps/macos/Sources/Settings/SettingsView.swift`
- Modify: `apps/macos/Sources/MeetingRaftApp.swift`
- Modify: `apps/macos/Sources/Shell/AppShellView.swift` (focused value / notification for menu)

**Interfaces:**
- Consumes: `SessionLanguageStore`
- Produces: Settings scene with `Picker` over `allowed`; menu `Session → Start Demo Captions` (⌘R)

- [ ] **Step 1: Settings UI**

```swift
import SwiftUI

/// Окно настроек: язык сессии (stub).
struct SettingsView: View {
    @Environment(SessionLanguageStore.self) private var languageStore

    var body: some View {
        Form {
            Picker("Session language", selection: Bindable(languageStore).primary) {
                ForEach(languageStore.allowed) { language in
                    Text(language.displayName).tag(language)
                }
            }
            Text("Default is Russian (ADR-003).")
                .foregroundStyle(.secondary)
        }
        .padding()
        .frame(width: 360, height: 140)
    }
}
```

- [ ] **Step 2: Focused action для menu**

Add `apps/macos/Sources/App/StartCaptionsAction.swift`:

```swift
import SwiftUI

/// Focused action: старт demo captions из меню.
struct StartCaptionsKey: FocusedValueKey {
    typealias Value = () -> Void
}

extension FocusedValues {
    var startCaptions: (() -> Void)? {
        get { self[StartCaptionsKey.self] }
        set { self[StartCaptionsKey.self] = newValue }
    }
}
```

In `AppShellView`, after toolbar:

```swift
.focusedValue(\.startCaptions) {
    selection = .liveCaptions
    captionsViewModel.start()
}
```

In `MeetingRaftApp`:

```swift
@FocusedValue(\.startCaptions) private var startCaptions

// inside WindowGroup scene:
.commands {
    CommandMenu("Session") {
        Button("Start Demo Captions") {
            startCaptions?()
        }
        .keyboardShortcut("r", modifiers: [.command])
    }
}
```

- [ ] **Step 3: Build + test**

Same xcodebuild build/test as Task 3.
Expected: SUCCEEDED.

- [ ] **Step 4: Commit**

```bash
git commit -am "feat: settings языка сессии и Session menu"
```

---

### Task 5: CI test step + docs

**Files:**
- Modify: `.github/workflows/ci.yml` — после build добавить `xcodebuild ... test`
- Modify: `docs/roadmap.md` — Phase 1 status done when exit criteria met
- Modify: `docs/backlog.md` — Epic 2 items checked or annotated

- [ ] **Step 1: CI**

In macos job, after build step add:

```yaml
      - name: Юнит-тесты
        run: >
          xcodebuild -project MeetingRaft.xcodeproj -scheme MeetingRaft
          -configuration Debug test CODE_SIGNING_ALLOWED=NO
```

- [ ] **Step 2: Локальная проверка полного CI-эквивалента**

```bash
(cd rust && cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test) && \
(cd apps/macos && swiftformat --lint Sources Tests && xcodegen generate && \
 xcodebuild -project MeetingRaft.xcodeproj -scheme MeetingRaft -configuration Debug build CODE_SIGNING_ALLOWED=NO && \
 xcodebuild -project MeetingRaft.xcodeproj -scheme MeetingRaft -configuration Debug test CODE_SIGNING_ALLOWED=NO)
```

- [ ] **Step 3: Commit + PR**

```bash
git add .github/workflows/ci.yml docs/
git commit -m "ci: гонять macOS unit tests; закрыть Phase 1 в docs"
git push -u origin HEAD
gh pr create --title "feat: Phase 1 — SwiftUI shell и fake captions" --body "..."
```

---

## Exit criteria Phase 1 (roadmap)

- [ ] App builds from Xcode / xcodebuild
- [ ] Fake captions render with partial (italic/secondary) and final (primary) styles
- [ ] Settings shows session language selector, default `ru`
- [ ] Unit tests for language store and fake stream pass in CI

## Spec coverage check

| Requirement | Task |
|-------------|------|
| Sidebar | 3 |
| Toolbar | 3 |
| Settings scene + language selector default ru | 1, 4 |
| Menu commands + shortcuts | 4 |
| Fake subtitle stream partial/final | 2, 3 |
| Presentation models only | 1–4 |
| No Rust/UniFFI | all |
