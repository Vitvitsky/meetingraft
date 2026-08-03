@testable import MeetingRaft
import XCTest

/// Подменный источник PCM: тесты дёргают `emit` вручную.
private final class FakeTap: AudioTapping {
    var isAvailable: Bool
    var prepareCalls = 0
    var startCalls = 0
    var stopCalls = 0
    var startError: Error?

    private var onSamples: (([Float]) -> Void)?

    init(isAvailable: Bool = true) {
        self.isAvailable = isAvailable
    }

    func prepare() {
        prepareCalls += 1
    }

    func start(onSamples: @escaping ([Float]) -> Void) throws {
        startCalls += 1
        if let startError {
            throw startError
        }
        self.onSamples = onSamples
    }

    func stop() {
        stopCalls += 1
        onSamples = nil
    }

    func emit(_ samples: [Float]) {
        onSamples?(samples)
    }
}

private struct FakeTapError: Error {}

@MainActor
final class AudioCaptureCoordinatorTests: XCTestCase {
    private func makeCoordinator(
        microphone: FakeTap,
        systemAudio: FakeTap
    ) -> AudioCaptureCoordinator {
        let root = FileManager.default.temporaryDirectory
            .appendingPathComponent("meetingraft-coordinator-\(UUID().uuidString)")
        let core = MeetingCore.withDataRoot(dataRoot: root.path)
        return AudioCaptureCoordinator(
            core: core,
            microphone: microphone,
            systemAudio: systemAudio,
            requestMicrophonePermission: { true }
        )
    }

    /// ingest уходит через `Task { @MainActor }`, поэтому ждём результат.
    private func waitForChunks(_ coordinator: AudioCaptureCoordinator) async {
        for _ in 0 ..< 50 where coordinator.chunkCount == 0 {
            try? await Task.sleep(nanoseconds: 10_000_000)
        }
    }

    /// Оба канала стартуют, когда системный звук доступен.
    func testStartsBothChannelsWhenSystemAudioAvailable() async {
        let microphone = FakeTap()
        let systemAudio = FakeTap(isAvailable: true)
        let coordinator = makeCoordinator(microphone: microphone, systemAudio: systemAudio)

        await coordinator.startRecording()

        XCTAssertTrue(coordinator.isRecording)
        XCTAssertTrue(coordinator.systemAudioAvailable)
        XCTAssertEqual(microphone.startCalls, 1)
        XCTAssertEqual(systemAudio.startCalls, 1)
    }

    /// Недоступный системный канал не мешает записи с микрофона.
    func testRecordsMicOnlyWhenSystemAudioUnavailable() async {
        let microphone = FakeTap()
        let systemAudio = FakeTap(isAvailable: false)
        let coordinator = makeCoordinator(microphone: microphone, systemAudio: systemAudio)

        await coordinator.startRecording()

        XCTAssertTrue(coordinator.isRecording)
        XCTAssertFalse(coordinator.systemAudioAvailable)
        XCTAssertEqual(microphone.startCalls, 1)
        XCTAssertEqual(systemAudio.startCalls, 0)
    }

    /// Сбой старта системного tap не должен ронять сессию целиком.
    func testSystemAudioStartFailureDegradesToMicOnly() async {
        let microphone = FakeTap()
        let systemAudio = FakeTap(isAvailable: true)
        systemAudio.startError = FakeTapError()
        let coordinator = makeCoordinator(microphone: microphone, systemAudio: systemAudio)

        await coordinator.startRecording()

        XCTAssertTrue(coordinator.isRecording)
        XCTAssertFalse(coordinator.systemAudioAvailable)
        XCTAssertEqual(microphone.startCalls, 1)
    }

    /// Сбой микрофона откатывает сессию.
    func testMicrophoneFailureRollsBackSession() async {
        let microphone = FakeTap()
        microphone.startError = FakeTapError()
        let systemAudio = FakeTap(isAvailable: true)
        let coordinator = makeCoordinator(microphone: microphone, systemAudio: systemAudio)

        await coordinator.startRecording()

        XCTAssertFalse(coordinator.isRecording)
        XCTAssertNil(coordinator.sessionId)
        XCTAssertNotNil(coordinator.lastError)
        XCTAssertEqual(coordinator.sttBackend, "idle")
    }

    /// Сэмплы обоих каналов доходят до ядра — чанки считаются.
    func testIngestsSamplesFromBothChannels() async {
        let microphone = FakeTap()
        let systemAudio = FakeTap(isAvailable: true)
        let coordinator = makeCoordinator(microphone: microphone, systemAudio: systemAudio)
        await coordinator.startRecording()
        XCTAssertTrue(coordinator.isRecording)

        let frames = Int(AudioChunkPipeline.targetSampleRate * 0.1)
        microphone.emit(Array(repeating: Float(0.1), count: frames))
        systemAudio.emit(Array(repeating: Float(0.1), count: frames))
        await waitForChunks(coordinator)

        XCTAssertGreaterThan(coordinator.chunkCount, 0)
        coordinator.stopRecording()
        XCTAssertEqual(microphone.stopCalls, 1)
        XCTAssertEqual(systemAudio.stopCalls, 1)
    }
}
