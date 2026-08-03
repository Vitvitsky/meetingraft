import Observation

/// Мост между строкой меню и окном приложения.
///
/// `MenuBarExtra` — отдельная сцена: у неё нет доступа ни к координатору
/// захвата, ни к presentation-моделям окна. Вместо того чтобы поднимать
/// всю запись на уровень приложения, окно кладёт сюда флаг и два
/// действия.
@Observable
@MainActor
final class RecordingBridge {
    private(set) var isRecording = false

    /// Начать или остановить запись; ставит окно.
    var toggle: (() -> Void)?
    /// Показать главное окно.
    var openWindow: (() -> Void)?

    func setRecording(_ value: Bool) {
        isRecording = value
    }
}
