import AVFoundation
import Foundation

/// Запросы разрешений на захват (mic + system audio).
enum AudioPermissions {
    /// Запросить доступ к микрофону.
    static func requestMicrophone() async -> Bool {
        await withCheckedContinuation { continuation in
            AVCaptureDevice.requestAccess(for: .audio) { granted in
                continuation.resume(returning: granted)
            }
        }
    }

    /// Текущее разрешение микрофона — **без** запроса.
    ///
    /// Нужно до первой записи: иначе узнать, что микрофон запрещён, можно
    /// только нажав «запись». Отказ и запрет политикой сведены в один
    /// случай сознательно — оба означают «программа спросить не может, а
    /// человек может открыть настройки», и разное будущее у них не
    /// появляется.
    static func microphonePermission() -> MicrophonePermission {
        switch AVCaptureDevice.authorizationStatus(for: .audio) {
        case .authorized: .granted
        case .notDetermined: .notAsked
        default: .denied
        }
    }
}
