import AppKit
import SwiftUI

/// Содержимое пункта в строке меню (ТЗ редизайна §4.7).
///
/// Пока главное окно свёрнуто ради накладки, строка меню — единственный
/// способ вернуться к приложению и остановить запись, если накладка
/// потерялась за чужим окном.
struct MenuBarView: View {
    let isRecording: Bool
    let detectedApp: MeetingApp?
    let onToggleRecording: () -> Void
    let onOpenWindow: () -> Void

    var body: some View {
        if let detectedApp, !isRecording {
            Text("\(detectedApp.displayName) is running")
            Button("Record this meeting") {
                onToggleRecording()
            }
        }

        Button(isRecording ? "Stop recording" : "Start recording") {
            onToggleRecording()
        }
        .keyboardShortcut("r", modifiers: [.command, .shift])

        Divider()

        Button("Open MeetingRaft") {
            onOpenWindow()
        }

        Divider()

        Button("Quit") {
            NSApplication.shared.terminate(nil)
        }
        .keyboardShortcut("q")
    }
}

/// Значок в строке меню по состоянию.
///
/// Значок обязан отличаться формой, а не только цветом: в монохромной
/// строке меню цвет не передаётся вовсе.
enum MenuBarIcon {
    static func systemImage(isRecording: Bool, hasDetectedMeeting: Bool) -> String {
        if isRecording {
            return "record.circle.fill"
        }
        return hasDetectedMeeting ? "waveform.badge.exclamationmark" : "waveform"
    }
}
