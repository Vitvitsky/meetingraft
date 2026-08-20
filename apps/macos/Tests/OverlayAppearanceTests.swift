import AppKit
@testable import MeetingRaft
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

    /// Заведомо положительный случай для самого теста: без него все
    /// четыре проверки выше прошли бы и на панели, которой нет вовсе —
    /// `panelAppearanceName` у отсутствующей панели тоже `nil`.
    func testPanelActuallyExistsAfterShow() {
        let controller = OverlayWindowController()
        defer { controller.hide() }

        XCTAssertFalse(controller.isVisible)
        controller.show(content: Text("тест"), preference: .auto)

        XCTAssertTrue(controller.isVisible)
    }
}
