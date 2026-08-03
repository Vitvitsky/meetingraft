import AppKit
import SwiftUI

/// Окно накладки: плавающая панель поверх остальных приложений.
///
/// `NSPanel`, а не `Window`: нужна панель, которая не забирает фокус у
/// Zoom, видна на всех рабочих столах и переживает полноэкранный режим
/// чужого приложения. Средствами SwiftUI это не настраивается.
@MainActor
final class OverlayWindowController {
    private var panel: NSPanel?
    /// Окно, свёрнутое ради накладки. Помним именно то, что прятали:
    /// разворачивать наугад первое попавшееся — верный способ поднять
    /// чужое окно вместо своего.
    private var hiddenWindow: NSWindow?

    var isVisible: Bool {
        panel?.isVisible ?? false
    }

    /// Показать накладку с новым содержимым; повторный вызов обновляет
    /// уже открытую панель, а не создаёт вторую.
    func show(content: some View) {
        let hosting = NSHostingView(rootView: AnyView(content))
        if let panel {
            panel.contentView = hosting
            panel.orderFrontRegardless()
            return
        }

        let panel = NSPanel(
            contentRect: NSRect(x: 0, y: 0, width: 720, height: 120),
            styleMask: [.borderless, .nonactivatingPanel, .utilityWindow],
            backing: .buffered,
            defer: false
        )
        panel.level = .floating
        panel.isOpaque = false
        panel.backgroundColor = .clear
        panel.hasShadow = true
        // Тащится за любое место: заголовка у панели нет.
        panel.isMovableByWindowBackground = true
        // Видна на всех рабочих столах и поверх чужого полноэкранного
        // окна — без этого в Zoom на весь экран накладки не будет.
        panel.collectionBehavior = [.canJoinAllSpaces, .fullScreenAuxiliary, .stationary]
        panel.hidesOnDeactivate = false
        panel.isFloatingPanel = true
        panel.becomesKeyOnlyIfNeeded = true
        panel.contentView = hosting
        positionNearBottom(panel)
        panel.orderFrontRegardless()
        self.panel = panel
    }

    func hide() {
        panel?.orderOut(nil)
        panel = nil
    }

    /// Свернуть главное окно приложения.
    func hideMainWindow() {
        guard hiddenWindow == nil else { return }
        // Панель накладки исключаем явно: она тоже окно приложения.
        let target = NSApp.windows.first { window in
            window.isVisible && !(window is NSPanel) && window.canBecomeMain
        }
        guard let target else { return }
        hiddenWindow = target
        target.miniaturize(nil)
    }

    /// Вернуть ранее свёрнутое окно.
    func restoreMainWindow() {
        guard let hiddenWindow else { return }
        self.hiddenWindow = nil
        if hiddenWindow.isMiniaturized {
            hiddenWindow.deminiaturize(nil)
        }
    }

    /// Внизу по центру экрана: там накладка меньше всего перекрывает
    /// собеседников в сетке видео.
    private func positionNearBottom(_ panel: NSPanel) {
        guard let screen = NSScreen.main else { return }
        let visible = screen.visibleFrame
        let size = panel.frame.size
        let origin = NSPoint(
            x: visible.midX - size.width / 2,
            y: visible.minY + visible.height * 0.08
        )
        panel.setFrameOrigin(origin)
    }
}
