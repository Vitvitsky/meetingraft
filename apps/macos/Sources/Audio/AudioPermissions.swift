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

    /// Текущий статус микрофона без prompt.
    static func microphoneAuthorized() -> Bool {
        AVCaptureDevice.authorizationStatus(for: .audio) == .authorized
    }
}
