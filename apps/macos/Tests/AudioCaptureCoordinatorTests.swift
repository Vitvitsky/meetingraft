@testable import MeetingRaft
import XCTest

/// Подменный источник PCM: тесты дёргают `emit` вручную.
private final class FakeTap: AudioTapping {
    var isAvailable: Bool
    var prepareCalls = 0
    var startCalls = 0
    var stopCalls = 0
    var startError: Error?

    private var onSamples: SamplesHandler?

    init(isAvailable: Bool = true) {
        self.isAvailable = isAvailable
    }

    func prepare() {
        prepareCalls += 1
    }

    func start(onSamples: @escaping SamplesHandler) throws {
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

    /// `hostTime` в тиках; координатору отдают часы 1:1, поэтому тик здесь
    /// равен наносекунде.
    func emit(_ samples: [Float], hostTime: UInt64 = 0) {
        onSamples?(samples, hostTime)
    }
}

private struct FakeTapError: Error {}

/// Начало записи в тиках подменных часов. Тик = наносекунда, поэтому
/// смещения буферов в тестах читаются как миллисекунды × 1e6.
///
/// На уровне файла, а не в классе: замыкание часов `@Sendable`, а
/// статическое свойство `@MainActor`-класса изолировано вместе с ним.
private let anchorTicks: UInt64 = 1_000_000_000

@MainActor
final class AudioCaptureCoordinatorTests: XCTestCase {
    private func makeCore() -> MeetingCore {
        let root = FileManager.default.temporaryDirectory
            .appendingPathComponent("meetingraft-coordinator-\(UUID().uuidString)")
        return MeetingCore.withDataRoot(dataRoot: root.path)
    }

    private func makeCoordinator(
        microphone: FakeTap,
        systemAudio: FakeTap,
        core: MeetingCore? = nil
    ) -> AudioCaptureCoordinator {
        let core = core ?? makeCore()
        return AudioCaptureCoordinator(
            core: core,
            microphone: microphone,
            systemAudio: systemAudio,
            clock: HostClock(numerator: 1, denominator: 1, now: { anchorTicks }),
            requestMicrophonePermission: { true }
        )
    }

    /// Смещение буфера от начала записи в тиках подменных часов.
    private func hostTime(atMs offset: UInt64) -> UInt64 {
        anchorTicks + offset * 1_000_000
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

    /// У каналов одно общее начало отсчёта, а не по своему нулю у каждого.
    ///
    /// Системный tap поднимается позже микрофона — на встрече `6CE19EC5`
    /// на 1150 мс. Пока каждый канал считал от своего первого буфера, эта
    /// секунда с четвертью пропадала, и сопоставление дорожек было
    /// смещено ровно на неё.
    func testChannelStartsAreMeasuredFromOneAnchor() async {
        let microphone = FakeTap()
        let systemAudio = FakeTap(isAvailable: true)
        let coordinator = makeCoordinator(microphone: microphone, systemAudio: systemAudio)
        await coordinator.startRecording()
        XCTAssertTrue(coordinator.isRecording)

        let frames = Int(AudioChunkPipeline.targetSampleRate * 0.1)
        let samples = Array(repeating: Float(0.1), count: frames)
        microphone.emit(samples, hostTime: hostTime(atMs: 12))
        systemAudio.emit(samples, hostTime: hostTime(atMs: 1_162))
        await waitForChunks(coordinator)
        for _ in 0 ..< 50 where coordinator.systemStartOffsetMs == nil {
            try? await Task.sleep(nanoseconds: 10_000_000)
        }

        XCTAssertEqual(coordinator.micStartOffsetMs, 12)
        XCTAssertEqual(coordinator.systemStartOffsetMs, 1_162, "разница стартов — 1150 мс")

        // Привязка одноразовая: дальше метки идут от кадров, иначе внутри
        // канала появилось бы дрожание часов. Ждём именно роста счётчика:
        // без него утверждение выполнялось бы просто потому, что второй
        // буфер ещё не дошёл.
        let processed = coordinator.chunkCount
        microphone.emit(samples, hostTime: hostTime(atMs: 9_000))
        for _ in 0 ..< 50 where coordinator.chunkCount == processed {
            try? await Task.sleep(nanoseconds: 10_000_000)
        }
        XCTAssertGreaterThan(coordinator.chunkCount, processed, "второй буфер дошёл")
        XCTAssertEqual(coordinator.micStartOffsetMs, 12)

        coordinator.stopRecording()
    }

    /// Разница стартов обязана попадать в журнал диагностики.
    ///
    /// Молча она уже раз стоила недели разбора: оба канала помечали своё
    /// начало нулём, приборы этому верили, и разошедшиеся дорожки никто не
    /// заподозрил.
    func testStartSkewGoesToTheDiagnosticsLog() async throws {
        let core = makeCore()
        let microphone = FakeTap()
        let systemAudio = FakeTap(isAvailable: true)
        let coordinator = makeCoordinator(
            microphone: microphone,
            systemAudio: systemAudio,
            core: core
        )
        await coordinator.startRecording()

        let frames = Int(AudioChunkPipeline.targetSampleRate * 0.1)
        let samples = Array(repeating: Float(0.1), count: frames)
        microphone.emit(samples, hostTime: hostTime(atMs: 12))
        systemAudio.emit(samples, hostTime: hostTime(atMs: 1_162))
        for _ in 0 ..< 50 where coordinator.systemStartOffsetMs == nil {
            try? await Task.sleep(nanoseconds: 10_000_000)
        }
        coordinator.stopRecording()

        let log = try String(contentsOfFile: core.diagnosticsLogPath(), encoding: .utf8)
        let lines = log.split(separator: "\n").map(String.init)
        XCTAssertFalse(lines.isEmpty, "журнал непуст, иначе проверять нечего")
        XCTAssertTrue(
            lines.contains { $0.contains("capture_channel_skew") && $0.contains("\"buffer_ms\":1150") },
            "в журнале нет разницы стартов: \(log)"
        )
        XCTAssertTrue(
            lines.contains {
                $0.contains("capture_channel_start") && $0.contains("\"text\":\"system\"")
                    && $0.contains("\"buffer_ms\":1162")
            },
            "в журнале нет старта системного канала: \(log)"
        )
    }
}
