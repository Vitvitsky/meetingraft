import Foundation
import OSLog

/// Захват system playback (ADR-004). Process tap; при недоступности — isAvailable=false.
final class SystemAudioCapture {
    private let log = Logger(subsystem: "com.vitvitsky.meetingraft", category: "SystemAudio")
    private(set) var isAvailable = false
    private var onSamples: (([Float]) -> Void)?

    /// Попытка подготовить global stereo process tap.
    func prepare() {
        // Полный aggregate-device wiring — follow-up; контракт и mic-only path готовы.
        isAvailable = false
        log.info("System audio process tap not wired yet; mic-only recording active")
    }

    func start(onSamples: @escaping ([Float]) -> Void) throws {
        prepare()
        guard isAvailable else {
            throw CaptureError.unavailable
        }
        self.onSamples = onSamples
    }

    func stop() {
        onSamples = nil
    }

    enum CaptureError: Error {
        case unavailable
    }
}
