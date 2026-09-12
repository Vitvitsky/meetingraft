# Apple Translation Engine Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Включённый синхронный перевод переводит через Apple Translation, а не помечает текст `[en·apple]`.

**Architecture:** Цикл перевода живёт внутри замыкания `.translationTask` — ровно столько, сколько валидна `TranslationSession` (Apple бросает `fatalError` при обращении к мёртвой сессии). Мост — `@Observable @MainActor`, создан на уровне `App` и привязан к ядру главного окна, чтобы состояние видели и субтитры, и Settings. Host-путь ставит в очередь только финальные реплики.

**Tech Stack:** SwiftUI + Translation framework (macOS 15.0), Rust + UniFFI (`meetingraft-ffi`), XCTest.

**Spec:** `docs/superpowers/specs/2026-09-13-apple-translation-engine-design.md`

## Global Constraints

- Deployment target macOS 15.0, `SWIFT_VERSION 6.0` (`apps/macos/project.yml:9,26`).
- Никаких новых entitlements и ключей Info.plist — Translation framework их не требует.
- `TranslationSession` **не `Sendable`**; обращение к ней после исчезновения вью или смены конфигурации — `fatalError`. Сессию не хранить вне замыкания `.translationTask`.
- `LanguageAvailability.status(from:to:)` — `async`, **не `throws`**.
- Скачивание пары — только через сессию (`prepareTranslation()`).
- Комментарии и документация — по-русски, идентификаторы — по-английски.
- Строки интерфейса локализуемы: базовый en, перевод ru в `Localizable.xcstrings`. Вне вьюхи — `String(localized:)` с литералом.
- Мутирующие методы FFI возвращают `String`: пустая — успех, непустая — ошибка.
- Rust-тесты: `cd rust && cargo test -p meetingraft-ffi`. Swift-тесты (только Мак): `cd apps/macos && xcodebuild -project MeetingRaft.xcodeproj -scheme MeetingRaft -configuration Debug test CODE_SIGNING_ALLOWED=NO`.
- Ветка работы: `feat/apple-translation-engine`. Коммиты — Conventional Commits, английский subject.

---

### Task 1: Rust — host-путь ставит в очередь только финальные реплики

**Files:**
- Modify: `rust/crates/ffi/src/lib.rs:716-755` (`maybe_enqueue_translation`)
- Test: `rust/crates/ffi/src/lib.rs` (модуль `#[cfg(test)] mod tests`, рядом с тестом `apple_backend_uses_host_bridge_queue`)

**Interfaces:**
- Consumes: `translate::resolve_effective`, `HostPendingQueue::enqueue`, `domain::CaptionPhase`.
- Produces: при `EffectiveBackend::AppleHost` в `host_translation_queue` попадают только `CaptionPhase::Final`. Не-host движки (`stub`, `http`, `local_llm`) не меняются.

- [ ] **Step 1: Написать падающий тест на фильтр**

В `rust/crates/ffi/src/lib.rs`, в `mod tests`, после теста `apple_backend_uses_host_bridge_queue`:

```rust
    /// Host-путь переводит только законченные реплики: частичные меняются
    /// чаще, чем Apple успевает, и копят очередь.
    #[test]
    fn host_path_enqueues_finals_only() {
        let core = MeetingCore::new();
        core.set_host_translation_available(true);
        assert!(core.set_live_translation(true, "en".into()).is_empty());
        assert!(
            core.set_translation_backend("apple".into(), String::new())
                .is_empty()
        );
        let mut guard = core.inner.lock().expect("meeting core poisoned");
        let partial = domain::CaptionEvent::new(
            "p1".into(),
            "Добро пожаловать".into(),
            CaptionPhase::Partial,
        );
        let final_event = domain::CaptionEvent::new(
            "f1".into(),
            "Добро пожаловать в MeetingRaft".into(),
            CaptionPhase::Final,
        );
        maybe_enqueue_translation(&mut guard, &partial);
        maybe_enqueue_translation(&mut guard, &final_event);
        let reqs = guard.host_translation_queue.drain();
        assert_eq!(reqs.len(), 1, "в очередь должен попасть только final");
        assert_eq!(reqs[0].text, "Добро пожаловать в MeetingRaft");
    }
```

- [ ] **Step 2: Убедиться, что тест падает**

Run: `cd rust && cargo test -p meetingraft-ffi host_path_enqueues_finals_only`
Expected: FAIL — `assertion left == right failed`, `left: 2`, `right: 1` (сейчас в очередь попадают обе реплики).

- [ ] **Step 3: Отфильтровать host-путь по фазе**

В `maybe_enqueue_translation` заменить ветку `EffectiveBackend::AppleHost`:

```rust
        EffectiveBackend::AppleHost => {
            // Apple-перевод асинхронен и дорог: частичные реплики меняются
            // каждые ~100–200 мс, их перевод копит очередь и приходит
            // вразнобой. Host-путь переводит только законченные.
            if matches!(event.phase, CaptionPhase::Final) {
                inner.host_translation_queue.enqueue(
                    &event.text,
                    source,
                    target,
                    event.phase,
                    event.channel,
                );
            }
        }
```

- [ ] **Step 4: Обновить существующий тест под новое поведение**

Demo-скрипт первым отдаёт `Partial` (`rust/crates/session/src/fake_captions.rs:16-26`), а `drain_events` на быстром тесте успевает выдать только его. Поэтому в тесте `apple_backend_uses_host_bridge_queue` финальную реплику ставим напрямую. Заменить тело теста на:

```rust
    #[test]
    fn apple_backend_uses_host_bridge_queue() {
        let core = MeetingCore::new();
        core.set_host_translation_available(true);
        assert!(core.set_live_translation(true, "en".into()).is_empty());
        assert!(
            core.set_translation_backend("apple".into(), String::new())
                .is_empty()
        );
        assert_eq!(core.effective_translation_backend(), "apple");
        // Demo-скрипт первым отдаёт partial, а host-путь его не ставит:
        // финальную реплику кладём напрямую, без ожидания такта скрипта.
        {
            let mut guard = core.inner.lock().expect("meeting core poisoned");
            let final_event = domain::CaptionEvent::new(
                "f1".into(),
                "Добро пожаловать в MeetingRaft".into(),
                CaptionPhase::Final,
            );
            maybe_enqueue_translation(&mut guard, &final_event);
        }
        let reqs = core.drain_host_translation_requests();
        assert_eq!(reqs.len(), 1);
        assert_eq!(reqs[0].text, "Добро пожаловать в MeetingRaft");
        assert!(
            core.complete_host_translation(reqs[0].id.clone(), "Welcome to MeetingRaft".into())
                .is_empty()
        );
        let translations = core.drain_live_translations();
        assert_eq!(translations[0].text, "Welcome to MeetingRaft");
        core.stop();
    }
```

- [ ] **Step 5: Прогнать тесты и линт**

Run: `cd rust && cargo test -p meetingraft-ffi && cargo fmt --check && cargo clippy -p meetingraft-ffi --all-targets -- -D warnings`
Expected: PASS; clippy без замечаний.

- [ ] **Step 6: Коммит**

```bash
git add rust/crates/ffi/src/lib.rs
git commit -m "feat: the Apple path translates finished utterances only"
```

---

### Task 2: Swift — швы сессии и доступности

**Files:**
- Create: `apps/macos/Sources/LiveCaptions/TranslationSessioning.swift`
- Create: `apps/macos/Sources/LiveCaptions/TranslationAvailability.swift`
- Create: `apps/macos/Sources/LiveCaptions/TranslationHostQueue.swift`

**Interfaces:**
- Produces: `TranslationSessioning` (`translate(_:) async throws -> String`, `prepareTranslation() async throws`), `SystemTranslationSession`; `TranslationAvailabilityStatus` (`.installed` / `.needsDownload` / `.unsupported`), `TranslationAvailabilityChecking.status(source:target:)`, `SystemTranslationAvailability`; `TranslationHostQueue` (`setHostTranslationAvailable(available:)`, `drainHostTranslationRequests()`, `completeHostTranslation(id:translatedText:)`) и пустой `extension MeetingCore: TranslationHostQueue {}`.

- [ ] **Step 1: Написать протокол сессии и адаптер**

`apps/macos/Sources/LiveCaptions/TranslationSessioning.swift`:

```swift
import Translation

/// Шов для тестов: мост не должен зависеть от Apple-фреймворка напрямую.
@MainActor
protocol TranslationSessioning: AnyObject {
    func translate(_ text: String) async throws -> String
    func prepareTranslation() async throws
}

/// Настоящая сессия из `.translationTask`.
///
/// Живёт только внутри замыкания `.translationTask`: Apple бросает
/// `fatalError`, если обратиться к ней после исчезновения вью или смены
/// конфигурации. Адаптер создаётся там же и умирает вместе с замыканием.
@MainActor
final class SystemTranslationSession: TranslationSessioning {
    private let session: TranslationSession

    init(_ session: TranslationSession) {
        self.session = session
    }

    func translate(_ text: String) async throws -> String {
        try await session.translate(text).targetText
    }

    func prepareTranslation() async throws {
        try await session.prepareTranslation()
    }
}
```

- [ ] **Step 2: Написать протокол доступности и настоящую проверку**

`apps/macos/Sources/LiveCaptions/TranslationAvailability.swift`:

```swift
import Foundation
import Translation

/// Доступность языковой пары для Apple-перевода.
enum TranslationAvailabilityStatus: Equatable, Sendable {
    /// Пара поддерживается и уже скачана.
    case installed
    /// Пара поддерживается, но требует загрузки.
    case needsDownload
    /// Пара не поддерживается вовсе.
    case unsupported
}

/// Шов для тестов: `LanguageAvailability` не должна протекать в мост.
protocol TranslationAvailabilityChecking: Sendable {
    func status(source: SpeechLanguage, target: SpeechLanguage) async
        -> TranslationAvailabilityStatus
}

/// Настоящая проверка через Translation framework.
struct SystemTranslationAvailability: TranslationAvailabilityChecking {
    func status(source: SpeechLanguage, target: SpeechLanguage) async
        -> TranslationAvailabilityStatus
    {
        let availability = LanguageAvailability()
        let status = await availability.status(
            from: Locale.Language(identifier: source.rawValue),
            to: Locale.Language(identifier: target.rawValue)
        )
        switch status {
        case .installed: return .installed
        case .supported: return .needsDownload
        case .unsupported: return .unsupported
        @unknown default: return .unsupported
        }
    }
}
```

- [ ] **Step 3: Написать шов очереди и конформанс ядра**

`apps/macos/Sources/LiveCaptions/TranslationHostQueue.swift`:

```swift
import Foundation

/// Часть `MeetingCore`, которой пользуется мост. Шов для тестов: мост
/// проверяется без настоящего ядра и без ожидания тактов demo-скрипта.
@MainActor
protocol TranslationHostQueue: AnyObject {
    func setHostTranslationAvailable(available: Bool)
    func drainHostTranslationRequests() -> [FfiHostTranslationRequest]
    func completeHostTranslation(id: String, translatedText: String) -> String
}

extension MeetingCore: TranslationHostQueue {}
```

- [ ] **Step 4: Собрать Rust-ядро и биндинги, проверить компиляцию**

Run: `apps/macos/Scripts/generate-ffi.sh`
Expected: успешная генерация `apps/macos/Generated/` (без этого шага новые файлы не увидят `FfiHostTranslationRequest`).

- [ ] **Step 5: Проверить, что типы видны компилятору**

Run: `cd apps/macos && xcodegen generate && xcodebuild -project MeetingRaft.xcodeproj -scheme MeetingRaft -configuration Debug build CODE_SIGNING_ALLOWED=NO`
Expected: BUILD SUCCEEDED. Ошибка `cannot find type 'TranslationSession' in scope` означает, что `import Translation` не подхватился — проверить, что SDK macOS 15+ и deployment target 15.0.

- [ ] **Step 6: Коммит**

```bash
git add apps/macos/Sources/LiveCaptions/TranslationSessioning.swift apps/macos/Sources/LiveCaptions/TranslationAvailability.swift apps/macos/Sources/LiveCaptions/TranslationHostQueue.swift
git commit -m "feat: seams for the Apple translation session and availability"
```

---

### Task 3: Мост перевода — цикл, доступность, скачивание

**Files:**
- Modify: `apps/macos/Sources/LiveCaptions/HostTranslationBridge.swift` (полностью переписать)
- Test: `apps/macos/Tests/HostTranslationBridgeTests.swift`

**Interfaces:**
- Consumes: `TranslationSessioning`, `TranslationAvailabilityChecking`, `TranslationHostQueue` (Task 2).
- Produces: `TranslationHostState` (`.off` / `.ready` / `.needsDownload` / `.downloading` / `.unsupported` / `.failed(String)`); `HostTranslationBridge` с `bind(queue:)`, `requestDownload()`, `prepare(source:target:) async`, `pumpOnce(session:) async`, `performDownload(session:source:target:) async`, `run(session:source:target:) async`, `state`, `hasSession`.

- [ ] **Step 1: Написать падающие тесты**

`apps/macos/Tests/HostTranslationBridgeTests.swift`:

```swift
@testable import MeetingRaft
import XCTest

@MainActor
final class HostTranslationBridgeTests: XCTestCase {
    private struct FakeAvailability: TranslationAvailabilityChecking {
        let status: TranslationAvailabilityStatus

        func status(source: SpeechLanguage, target: SpeechLanguage) async
            -> TranslationAvailabilityStatus
        {
            status
        }
    }

    private final class FakeHostQueue: TranslationHostQueue {
        var requests: [FfiHostTranslationRequest] = []
        var completed: [String: String] = [:]
        var availabilityFlags: [Bool] = []

        func setHostTranslationAvailable(available: Bool) {
            availabilityFlags.append(available)
        }

        func drainHostTranslationRequests() -> [FfiHostTranslationRequest] {
            defer { requests = [] }
            return requests
        }

        func completeHostTranslation(id: String, translatedText: String) -> String {
            completed[id] = translatedText
            return ""
        }
    }

    private final class FakeSession: TranslationSessioning {
        var failingText: String?
        var prepared = 0
        private(set) var translated: [String] = []

        func translate(_ text: String) async throws -> String {
            if text == failingText {
                throw NSError(domain: "test", code: 1)
            }
            translated.append(text)
            return "перевод: \(text)"
        }

        func prepareTranslation() async throws {
            prepared += 1
        }
    }

    private func request(
        id: String,
        text: String,
        source: String = "ru",
        target: String = "en"
    ) -> FfiHostTranslationRequest {
        FfiHostTranslationRequest(
            id: id,
            text: text,
            sourceCode: source,
            targetCode: target,
            phase: .final
        )
    }

    /// Apple-путь обязан отдать перевод сессии, а не метку заглушки.
    func testPumpOnceCompletesTranslation() async {
        let queue = FakeHostQueue()
        queue.requests = [request(id: "1", text: "Добро пожаловать")]
        let bridge = HostTranslationBridge(availability: FakeAvailability(status: .installed))
        bridge.bind(queue: queue)
        await bridge.prepare(source: .ru, target: .en)

        await bridge.pumpOnce(session: FakeSession())

        XCTAssertEqual(queue.completed["1"], "перевод: Добро пожаловать")
        XCTAssertEqual(bridge.state, .ready)
    }

    /// Отрицательный контроль: старая заглушка не должна вернуться.
    func testPumpOnceIsNotTheStub() async {
        let queue = FakeHostQueue()
        queue.requests = [request(id: "1", text: "Добро пожаловать")]
        let bridge = HostTranslationBridge(availability: FakeAvailability(status: .installed))
        bridge.bind(queue: queue)
        await bridge.prepare(source: .ru, target: .en)

        await bridge.pumpOnce(session: FakeSession())

        XCTAssertNotEqual(queue.completed["1"], "[en·apple] Добро пожаловать")
    }

    /// Упавшая реплика завершается пустой: очередь не растёт, следующая живёт.
    func testPumpOnceOnErrorCompletesEmptyAndContinues() async {
        let queue = FakeHostQueue()
        queue.requests = [
            request(id: "1", text: "раз"),
            request(id: "2", text: "два"),
        ]
        let session = FakeSession()
        session.failingText = "раз"
        let bridge = HostTranslationBridge(availability: FakeAvailability(status: .installed))
        bridge.bind(queue: queue)
        await bridge.prepare(source: .ru, target: .en)

        await bridge.pumpOnce(session: session)

        XCTAssertEqual(queue.completed["1"], "")
        XCTAssertEqual(queue.completed["2"], "перевод: два")
    }

    /// Ошибка видна в состоянии, а не проглатывается.
    func testPumpOnceOnErrorPublishesFailure() async {
        let queue = FakeHostQueue()
        queue.requests = [request(id: "1", text: "раз")]
        let session = FakeSession()
        session.failingText = "раз"
        let bridge = HostTranslationBridge(availability: FakeAvailability(status: .installed))
        bridge.bind(queue: queue)
        await bridge.prepare(source: .ru, target: .en)

        await bridge.pumpOnce(session: session)

        guard case .failed = bridge.state else {
            return XCTFail("ожидалось состояние failed, получено \(bridge.state)")
        }
    }

    /// Запрос от другой пары не переводим: сессия настроена не на неё.
    func testPumpOnceDropsRequestForAnotherPair() async {
        let queue = FakeHostQueue()
        queue.requests = [request(id: "1", text: "hola", target: "es")]
        let session = FakeSession()
        let bridge = HostTranslationBridge(availability: FakeAvailability(status: .installed))
        bridge.bind(queue: queue)
        await bridge.prepare(source: .ru, target: .en)

        await bridge.pumpOnce(session: session)

        XCTAssertEqual(queue.completed["1"], "")
        XCTAssertTrue(session.translated.isEmpty)
    }

    /// Установленная пара разрешает Apple в ядре.
    func testPrepareInstalledRegistersHostAvailable() async {
        let queue = FakeHostQueue()
        let bridge = HostTranslationBridge(availability: FakeAvailability(status: .installed))
        bridge.bind(queue: queue)

        await bridge.prepare(source: .ru, target: .en)

        XCTAssertEqual(queue.availabilityFlags.last, true)
        XCTAssertEqual(bridge.state, .ready)
    }

    /// Нескачанная пара запрещает Apple: auto уйдёт на stub/backend.
    func testPrepareNeedsDownloadDisablesHost() async {
        let queue = FakeHostQueue()
        let bridge = HostTranslationBridge(availability: FakeAvailability(status: .needsDownload))
        bridge.bind(queue: queue)

        await bridge.prepare(source: .ru, target: .en)

        XCTAssertEqual(queue.availabilityFlags.last, false)
        XCTAssertEqual(bridge.state, .needsDownload)
    }

    func testPrepareUnsupportedDisablesHost() async {
        let queue = FakeHostQueue()
        let bridge = HostTranslationBridge(availability: FakeAvailability(status: .unsupported))
        bridge.bind(queue: queue)

        await bridge.prepare(source: .ru, target: .en)

        XCTAssertEqual(queue.availabilityFlags.last, false)
        XCTAssertEqual(bridge.state, .unsupported)
    }

    /// Скачивание идёт через сессию и пересчитывает доступность.
    func testPerformDownloadPreparesAndRechecks() async {
        let queue = FakeHostQueue()
        let session = FakeSession()
        let bridge = HostTranslationBridge(availability: FakeAvailability(status: .installed))
        bridge.bind(queue: queue)
        await bridge.prepare(source: .ru, target: .en)

        await bridge.performDownload(session: session, source: .ru, target: .en)

        XCTAssertEqual(session.prepared, 1)
        XCTAssertEqual(bridge.state, .ready)
    }
}
```

- [ ] **Step 2: Убедиться, что тесты падают**

Run: `cd apps/macos && xcodebuild -project MeetingRaft.xcodeproj -scheme MeetingRaft -configuration Debug test CODE_SIGNING_ALLOWED=NO -only-testing:MeetingRaftTests/HostTranslationBridgeTests`
Expected: FAIL на сборке — `cannot find 'HostTranslationBridge' in scope` (старый класс не имеет нужных методов) или ошибки типов.

- [ ] **Step 3: Переписать мост**

`apps/macos/Sources/LiveCaptions/HostTranslationBridge.swift`:

```swift
import Foundation
import Observation

/// Что видно в UI про Apple-перевод.
enum TranslationHostState: Equatable, Sendable {
    case off
    case ready
    case needsDownload
    case downloading
    case unsupported
    case failed(String)
}

/// Мост к Apple Translation (ADR-008).
///
/// Сессию не хранит: цикл живёт внутри замыкания `.translationTask`
/// (`run(session:)`) ровно столько, сколько валидна сессия. Хранить её
/// дольше нельзя — Apple бросает `fatalError` при обращении к мёртвой.
@Observable
@MainActor
final class HostTranslationBridge {
    private(set) var state: TranslationHostState = .off
    /// Есть ли живая сессия: без неё скачать пару нечем.
    private(set) var hasSession = false

    private let availability: TranslationAvailabilityChecking
    private var queue: TranslationHostQueue?
    private var downloadRequested = false
    private var targetCode = ""
    private var installed = false

    init(availability: TranslationAvailabilityChecking = SystemTranslationAvailability()) {
        self.availability = availability
    }

    /// Главное окно отдаёт свою очередь: мост создан на уровне App, чтобы
    /// его видела и Settings-сцена, а ядро живёт в окне.
    func bind(queue: TranslationHostQueue) {
        self.queue = queue
    }

    /// Кнопка «скачать» только ставит флаг: работу с сессией делает цикл,
    /// у которого сессия жива.
    func requestDownload() {
        downloadRequested = true
    }

    /// Настроить пару и спросить доступность. Вынесено из `run`, чтобы
    /// проверяться тестом без бесконечного цикла.
    func prepare(source: SpeechLanguage, target: SpeechLanguage) async {
        targetCode = target.rawValue
        let status = await availability.status(source: source, target: target)
        installed = status == .installed
        state = Self.displayState(for: status)
        queue?.setHostTranslationAvailable(available: installed)
    }

    /// Цикл живёт, пока живёт замыкание `.translationTask`.
    func run(session: TranslationSessioning, source: SpeechLanguage, target: SpeechLanguage) async {
        hasSession = true
        await prepare(source: source, target: target)
        defer {
            hasSession = false
            installed = false
            queue?.setHostTranslationAvailable(available: false)
            drainAndDropStale()
            state = .off
        }
        while !Task.isCancelled {
            if downloadRequested {
                downloadRequested = false
                await performDownload(session: session, source: source, target: target)
            }
            if installed {
                await pumpOnce(session: session)
            }
            try? await Task.sleep(nanoseconds: 50_000_000)
        }
    }

    /// Один проход: забрать запросы, перевести, вернуть. Цикл вокруг него
    /// только спит, поэтому он и тестируется.
    func pumpOnce(session: TranslationSessioning) async {
        guard let queue else { return }
        for request in queue.drainHostTranslationRequests() {
            guard request.targetCode == targetCode else {
                // Стухший запрос: сессия настроена на другую пару.
                _ = queue.completeHostTranslation(id: request.id, translatedText: "")
                continue
            }
            do {
                let translated = try await session.translate(request.text)
                _ = queue.completeHostTranslation(id: request.id, translatedText: translated)
                state = .ready
            } catch {
                // Реплика теряется, но очередь не растёт и следующая живёт.
                _ = queue.completeHostTranslation(id: request.id, translatedText: "")
                state = .failed(error.localizedDescription)
            }
        }
    }

    /// Скачать пару через живую сессию и пересчитать доступность.
    func performDownload(
        session: TranslationSessioning,
        source: SpeechLanguage,
        target: SpeechLanguage
    ) async {
        state = .downloading
        do {
            try await session.prepareTranslation()
            await prepare(source: source, target: target)
        } catch {
            state = .failed(error.localizedDescription)
        }
    }

    /// Снять с очереди всё, что осталось от прошлой пары.
    private func drainAndDropStale() {
        guard let queue else { return }
        for request in queue.drainHostTranslationRequests() {
            _ = queue.completeHostTranslation(id: request.id, translatedText: "")
        }
    }

    private static func displayState(for status: TranslationAvailabilityStatus) -> TranslationHostState {
        switch status {
        case .installed: .ready
        case .needsDownload: .needsDownload
        case .unsupported: .unsupported
        }
    }
}
```

- [ ] **Step 4: Прогнать тесты**

Run: `cd apps/macos && xcodebuild -project MeetingRaft.xcodeproj -scheme MeetingRaft -configuration Debug test CODE_SIGNING_ALLOWED=NO -only-testing:MeetingRaftTests/HostTranslationBridgeTests`
Expected: PASS, 9 тестов.

- [ ] **Step 5: Линт Swift**

Run: `cd apps/macos && swiftformat Sources Tests --lint`
Expected: без замечаний (следить за `wrapFunctionBodies` и `preferKeyPath`).

- [ ] **Step 6: Коммит**

```bash
git add apps/macos/Sources/LiveCaptions/HostTranslationBridge.swift apps/macos/Tests/HostTranslationBridgeTests.swift
git commit -m "feat: the host bridge runs a session-bound translation loop"
```

---

### Task 4: Шелл — невидимая вью, app-level мост, чистка VM

**Files:**
- Create: `apps/macos/Sources/LiveCaptions/TranslationHostView.swift`
- Modify: `apps/macos/Sources/MeetingRaftApp.swift:6-14,17-28,49-56`
- Modify: `apps/macos/Sources/Shell/AppShellView.swift:8-36,85-88`
- Modify: `apps/macos/Sources/LiveCaptions/LiveCaptionsViewModel.swift:19,22-27`

**Interfaces:**
- Consumes: `HostTranslationBridge` (Task 3).
- Produces: `TranslationHostView(bridge:source:target:isEnabled:)`; мост доступен через `@Environment(HostTranslationBridge.self)` в обеих сценах.

- [ ] **Step 1: Написать невидимую вью**

`apps/macos/Sources/LiveCaptions/TranslationHostView.swift`:

```swift
import SwiftUI
import Translation

/// Невидимая вью, за которой живёт `TranslationSession` (ADR-008).
///
/// Сессия выдаётся только модификатором `.translationTask` и умирает
/// вместе с вью: Apple бросает `fatalError` при обращении к ней после
/// исчезновения вью или смены конфигурации. Поэтому цикл перевода живёт
/// в замыкании, а не в отдельном объекте.
struct TranslationHostView: View {
    let bridge: HostTranslationBridge
    let source: SpeechLanguage
    let target: SpeechLanguage
    let isEnabled: Bool

    private var configuration: TranslationSession.Configuration? {
        guard isEnabled, source != target else { return nil }
        return TranslationSession.Configuration(
            source: Locale.Language(identifier: source.rawValue),
            target: Locale.Language(identifier: target.rawValue)
        )
    }

    var body: some View {
        Color.clear
            .frame(width: 0, height: 0)
            .translationTask(configuration) { session in
                await bridge.run(
                    session: SystemTranslationSession(session),
                    source: source,
                    target: target
                )
            }
    }
}
```

- [ ] **Step 2: Создать мост на уровне App и раздать обеим сценам**

В `MeetingRaftApp.swift` добавить стор рядом с остальными:

```swift
    /// Общий для окна и Settings: состояние перевода нужно обоим, а ядро
    /// живёт в главном окне и привязывается к мосту на его появлении.
    @State private var translationBridge = HostTranslationBridge()
```

В `WindowGroup` добавить инъекцию после `.environment(recordingBridge)`:

```swift
                .environment(translationBridge)
```

В сцене `Settings` добавить после `.environment(appearanceStore)`:

```swift
                .environment(translationBridge)
```

- [ ] **Step 3: Привязать ядро и поставить вью в шелл**

В `AppShellView.swift` добавить окружение:

```swift
    @Environment(HostTranslationBridge.self) private var translationBridge
```

В `body`, сразу после `.background(Theme.surfaceRoot)`, добавить:

```swift
        .background(
            TranslationHostView(
                bridge: translationBridge,
                source: languageStore.primary,
                target: translationStore.target,
                isEnabled: translationStore.enabled && translationStore.backend != .off
            )
        )
```

В существующий `.onAppear` (строка 85) добавить первой строкой:

```swift
            translationBridge.bind(queue: core)
```

- [ ] **Step 4: Убрать старый мост из VM**

В `LiveCaptionsViewModel.swift` удалить строку 19 (`private let hostBridge: HostTranslationBridge`), строки 25-26 (`hostBridge = HostTranslationBridge(core: core)` и `hostBridge.start()`).

- [ ] **Step 5: Собрать и прогнать тесты**

Run: `cd apps/macos && xcodegen generate && xcodebuild -project MeetingRaft.xcodeproj -scheme MeetingRaft -configuration Debug test CODE_SIGNING_ALLOWED=NO`
Expected: BUILD SUCCEEDED, тесты PASS.

- [ ] **Step 6: Коммит**

```bash
git add apps/macos/Sources/LiveCaptions/TranslationHostView.swift apps/macos/Sources/MeetingRaftApp.swift apps/macos/Sources/Shell/AppShellView.swift apps/macos/Sources/LiveCaptions/LiveCaptionsViewModel.swift
git commit -m "feat: the translation session lives in the shell, not the tab"
```

---

### Task 5: Состояние в колонке, статус-баре и настройках

**Files:**
- Create: `apps/macos/Sources/LiveCaptions/TranslationHostState+Presentation.swift`
- Modify: `apps/macos/Sources/LiveCaptions/LiveCaptionsView.swift:98-110,239-259`
- Modify: `apps/macos/Sources/Settings/SettingsProviderSections.swift:179-220`
- Modify: `apps/macos/Sources/Resources/Localizable.xcstrings`
- Test: `apps/macos/Tests/TranslationHostStateTests.swift`

**Interfaces:**
- Consumes: `TranslationHostState` (Task 3).
- Produces: `TranslationHostState.columnMessage: String?`, `TranslationHostState.canDownload: Bool`.

- [ ] **Step 1: Написать падающий тест презентации**

`apps/macos/Tests/TranslationHostStateTests.swift`:

```swift
@testable import MeetingRaft
import XCTest

final class TranslationHostStateTests: XCTestCase {
    /// Ключ сверяем с тем же ключом, а не с обрывком текста: обрывок
    /// ломается от перевода и от языка машины.
    func testNeedsDownloadExplainsItself() {
        XCTAssertEqual(
            TranslationHostState.needsDownload.columnMessage,
            String(localized: "Apple translation needs a language download")
        )
    }

    func testUnsupportedExplainsItself() {
        XCTAssertEqual(
            TranslationHostState.unsupported.columnMessage,
            String(localized: "This language pair is not supported")
        )
    }

    func testDownloadingExplainsItself() {
        XCTAssertEqual(
            TranslationHostState.downloading.columnMessage,
            String(localized: "Downloading language…")
        )
    }

    func testFailureShowsItsMessage() {
        XCTAssertEqual(
            TranslationHostState.failed("boom").columnMessage,
            "boom"
        )
    }

    /// Готовая пара и выключенный перевод не пишут ничего: колонка
    /// показывает обычную заглушку.
    func testReadyAndOffStaySilent() {
        XCTAssertNil(TranslationHostState.ready.columnMessage)
        XCTAssertNil(TranslationHostState.off.columnMessage)
    }

    func testDownloadOfferedOnlyWhenItCanHelp() {
        XCTAssertTrue(TranslationHostState.needsDownload.canDownload)
        XCTAssertTrue(TranslationHostState.failed("boom").canDownload)
        XCTAssertFalse(TranslationHostState.ready.canDownload)
        XCTAssertFalse(TranslationHostState.unsupported.canDownload)
        XCTAssertFalse(TranslationHostState.downloading.canDownload)
        XCTAssertFalse(TranslationHostState.off.canDownload)
    }
}
```

- [ ] **Step 2: Убедиться, что тест падает**

Run: `cd apps/macos && xcodebuild -project MeetingRaft.xcodeproj -scheme MeetingRaft -configuration Debug test CODE_SIGNING_ALLOWED=NO -only-testing:MeetingRaftTests/TranslationHostStateTests`
Expected: FAIL на сборке — `value of type 'TranslationHostState' has no member 'columnMessage'`.

- [ ] **Step 3: Написать презентацию**

`apps/macos/Sources/LiveCaptions/TranslationHostState+Presentation.swift`:

```swift
import Foundation

/// Как состояние Apple-перевода выглядит в интерфейсе. Пустая колонка
/// обязана объяснять себя: молчаливый отказ хуже видимого.
extension TranslationHostState {
    /// Текст для пустой колонки перевода; `nil` — обычная заглушка.
    var columnMessage: String? {
        switch self {
        case .off, .ready:
            nil
        case .needsDownload:
            String(localized: "Apple translation needs a language download")
        case .downloading:
            String(localized: "Downloading language…")
        case .unsupported:
            String(localized: "This language pair is not supported")
        case .failed(let message):
            message
        }
    }

    /// Предлагать ли кнопку скачивания.
    var canDownload: Bool {
        switch self {
        case .needsDownload, .failed:
            true
        case .off, .ready, .downloading, .unsupported:
            false
        }
    }
}
```

- [ ] **Step 4: Показать состояние в колонке и статус-баре**

В `LiveCaptionsView.swift` добавить окружение рядом с другими:

```swift
    @Environment(HostTranslationBridge.self) private var translationBridge
```

Заменить блок колонки перевода в `captionStage` (строки 101-107) на:

```swift
            if translationStore.enabled {
                Divider().overlay(Theme.borderSubtle)
                VStack(spacing: Theme.Space.sm) {
                    stage(
                        lines: Array(viewModel.translationLines.suffix(3)),
                        placeholder: translationPlaceholder
                    )
                    if translationBridge.state.canDownload {
                        Button(String(localized: "Download language")) {
                            translationBridge.requestDownload()
                        }
                        .buttonStyle(.themedPrimary)
                        .padding(.bottom, Theme.Space.md)
                    }
                }
            }
```

Добавить рядом с `placeholderText`:

```swift
    /// Причину пустой колонки показываем вместо нейтральной заглушки.
    private var translationPlaceholder: String {
        translationBridge.state.columnMessage
            ?? String(localized: "Translation appears here")
    }
```

Заменить блок состояния в `statusBar` (строки 245-254) на:

```swift
            if translationStore.enabled {
                if !viewModel.translationIssue.isEmpty {
                    Text(viewModel.translationIssue)
                        .font(Theme.Text.mono(size: 11))
                        .foregroundStyle(Theme.warning)
                        .textSelection(.enabled)
                } else if let message = translationBridge.state.columnMessage {
                    Text(message)
                        .font(Theme.Text.mono(size: 11))
                        .foregroundStyle(Theme.warning)
                } else if !viewModel.effectiveTranslationBackend.isEmpty {
                    Text(viewModel.effectiveTranslationBackend)
                        .font(Theme.Text.mono(size: 11))
                        .foregroundStyle(Theme.textTertiary)
                }
            }
```

- [ ] **Step 5: Показать состояние и кнопку в настройках**

В `SettingsProviderSections.swift` в `TranslationSettingsSection` добавить окружение:

```swift
    @Environment(HostTranslationBridge.self) private var translationBridge
```

Внутри секции, после пикера backend, добавить:

```swift
            if let message = translationBridge.state.columnMessage {
                SettingsRow(title: String(localized: "Status")) {
                    Text(message)
                        .font(Theme.Text.bodySmall)
                        .foregroundStyle(Theme.textSecondary)
                }
            }
            if translationBridge.state.canDownload, translationBridge.hasSession {
                Button(String(localized: "Download language")) {
                    translationBridge.requestDownload()
                }
            }
```

- [ ] **Step 6: Добавить переводы**

Собрать приложение, чтобы Xcode извлёк новые ключи:

Run: `cd apps/macos && xcodebuild -project MeetingRaft.xcodeproj -scheme MeetingRaft -configuration Debug build CODE_SIGNING_ALLOWED=NO`

Затем в `apps/macos/Sources/Resources/Localizable.xcstrings` добавить русские значения для ключей:
- `Apple translation needs a language download` → `Apple-перевод требует загрузки языка`
- `Downloading language…` → `Загрузка языка…`
- `This language pair is not supported` → `Эта пара языков не поддерживается`
- `Download language` → `Скачать язык`
- `Status` → `Состояние`

- [ ] **Step 7: Проверить локализацию и прогнать тесты**

Run: `python3 scripts/check-localization.py && cd apps/macos && swiftformat Sources Tests --lint && xcodebuild -project MeetingRaft.xcodeproj -scheme MeetingRaft -configuration Debug test CODE_SIGNING_ALLOWED=NO`
Expected: локализация полна, формат чист, тесты PASS.

- [ ] **Step 8: Коммит**

```bash
git add apps/macos/Sources/LiveCaptions/TranslationHostState+Presentation.swift apps/macos/Sources/LiveCaptions/LiveCaptionsView.swift apps/macos/Sources/Settings/SettingsProviderSections.swift apps/macos/Tests/TranslationHostStateTests.swift apps/macos/Sources/Resources/Localizable.xcstrings
git commit -m "feat: the translation column says why it is empty, and offers the download"
```

---

### Task 6: Беклог, ADR-заметка и полная проверка за Маком

**Files:**
- Modify: `docs/backlog.md` (Epic 15, строки 1837-1856)
- Modify: `docs/adr/ADR-008-live-translation-backends.md` — **нельзя**: ADR неизменяем. Вместо правки — строка в беклоге, что Apple-путь реализован.

**Interfaces:**
- Consumes: всё предыдущее.
- Produces: обновлённый беклог и зелёный `verify-mac.sh`.

- [ ] **Step 1: Отметить сделанное в беклоге**

В `docs/backlog.md`, в Epic 15, заменить строки 1849-1854:

```markdown
- [x] Apple Translation вместо заглушки: цикл живёт в замыкании
  `.translationTask`, сессия не переживает вью (2026-09-13)
- [x] Скачивание языковых пар: состояние и кнопка, скачивание не стартует
  само (2026-09-13)
- Живая проверка за Маком: настоящий перевод RU→EN и RU→ES, включая
  первую загрузку пары
```

- [ ] **Step 2: Прогнать полную проверку за Маком**

Run: `scripts/verify-mac.sh`
Expected: 7 шагов зелёные. Если падает `swiftformat` — поправить формат; если `check-localization` — дописать перевод.

- [ ] **Step 3: Коммит**

```bash
git add docs/backlog.md
git commit -m "docs: the Apple translator is wired, and the live check is named"
```

---

## Self-Review

**Spec coverage:**
- «цикл внутри замыкания сессии» → Task 3 (`run` + `defer`) и Task 4 (`TranslationHostView`).
- «только финальные реплики» → Task 1.
- «auto только для установленной пары» → Task 3 (`prepare` → `setHostTranslationAvailable`) и Task 2 (`SystemTranslationAvailability`).
- «состояние в колонке и статус-баре» → Task 5.
- «скачивание только через сессию, по кнопке» → Task 3 (`requestDownload` + `performDownload`) и Task 5 (кнопки).
- «тесты Rust и Swift, отрицательный контроль» → Task 1 и Task 3.
- «не делаем HttpTranslateEngine и LLM» → ни одна задача их не трогает.
- «без entitlements» → Global Constraints; Task 2 не добавляет ключей.

**Placeholder scan:** TBD/TODO нет; каждый шаг с кодом содержит код.

**Type consistency:** `TranslationHostState` объявлен в Task 3 и используется в Task 5; `pumpOnce(session:)`, `prepare(source:target:)`, `performDownload(session:source:target:)`, `bind(queue:)` совпадают по именам в Task 3 и его тестах; `TranslationHostQueue` из Task 2 совпадает с использованием в Task 3; `columnMessage`/`canDownload` из Task 5 совпадают с тестом.
