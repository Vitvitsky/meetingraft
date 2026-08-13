@testable import MeetingRaft
import XCTest

/// Подменный источник PCM: тесты дёргают `emit` вручную.
private final class FakeTap: AudioTapping {
    var isAvailable: Bool
    var prepareCalls = 0
    var startCalls = 0
    var stopCalls = 0
    var startError: Error?
    /// Что «источник» отчитался о цене своего подъёма.
    var lastStartSteps: [CaptureStartStep] = []

    private var onSamples: SamplesHandler?

    init(isAvailable: Bool = true) {
        self.isAvailable = isAvailable
    }

    /// Что делать во время разведки. Настоящая разведка держит на экране
    /// системный запрос разрешения, и тест подсматривает состояние ровно
    /// в этот момент.
    var onPrepare: (() -> Void)?

    func prepare() {
        prepareCalls += 1
        onPrepare?()
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

    /// Разведка идёт при открытии окна, а не по нажатию записи.
    ///
    /// Ноль вызовов до неё — заведомо отрицательный случай: без него
    /// «разведка была» выполнялось бы и тем, что её делает `startRecording`.
    func testWarmUpProbesSystemAudioBeforeAnyRecording() async {
        let microphone = FakeTap()
        let systemAudio = FakeTap(isAvailable: true)
        let coordinator = makeCoordinator(microphone: microphone, systemAudio: systemAudio)

        XCTAssertEqual(systemAudio.prepareCalls, 0, "до открытия окна разведки быть не должно")

        coordinator.warmUpSystemAudio()

        XCTAssertEqual(systemAudio.prepareCalls, 1)
        XCTAssertTrue(coordinator.systemAudioAvailable, "доступность известна до записи")
        XCTAssertFalse(coordinator.isRecording, "разведка сама записи не начинает")
    }

    /// Во время записи разведка не трогает уже поднятый tap.
    ///
    /// Окно может открыться посреди сессии — запись живёт вне экрана
    /// субтитров, — и повторная разведка полезла бы в работающий tap.
    func testWarmUpDoesNothingWhileRecording() async {
        let microphone = FakeTap()
        let systemAudio = FakeTap(isAvailable: true)
        let coordinator = makeCoordinator(microphone: microphone, systemAudio: systemAudio)
        await coordinator.startRecording()
        XCTAssertTrue(coordinator.isRecording)
        let probes = systemAudio.prepareCalls
        XCTAssertGreaterThan(probes, 0, "запись обязана была разведать сама")

        coordinator.warmUpSystemAudio()

        XCTAssertEqual(systemAudio.prepareCalls, probes, "разведка полезла в работающий tap")
        coordinator.stopRecording()
    }

    /// Пока на экране запрос разрешения, приложение не заявляет запись.
    ///
    /// Первый вызов разведки поднимает системный запрос «System Audio
    /// Recording», и до её конца звук не идёт ни по одному каналу —
    /// микрофон стартует после. Открытая сессия и `isRecording` в это
    /// время означали бы «идёт запись» при нуле записанного.
    ///
    /// Проверяется именно состояние **внутри** разведки: после неё оно
    /// уже правильное само собой, и утверждение про «после» прошло бы и
    /// на старом порядке вызовов.
    func testNothingClaimsRecordingWhileThePermissionDialogIsUp() async {
        let microphone = FakeTap()
        let systemAudio = FakeTap(isAvailable: true)
        let coordinator = makeCoordinator(microphone: microphone, systemAudio: systemAudio)

        var claimedRecording: Bool?
        var hadSession: Bool?
        systemAudio.onPrepare = {
            // Разведка зовётся синхронно из `startRecording()`, то есть с
            // главного актора: читать его состояние здесь безопасно.
            MainActor.assumeIsolated {
                claimedRecording = coordinator.isRecording
                hadSession = coordinator.sessionId != nil
            }
        }

        await coordinator.startRecording()

        XCTAssertEqual(systemAudio.prepareCalls, 1, "разведки не было — проверять нечего")
        XCTAssertEqual(claimedRecording, false, "заявлена запись, пока звука нет ни на одном канале")
        XCTAssertEqual(hadSession, false, "сессия открыта раньше, чем начался захват")
        XCTAssertTrue(coordinator.isRecording, "после разведки запись обязана идти")
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
        systemAudio.emit(samples, hostTime: hostTime(atMs: 1162))
        await waitForChunks(coordinator)
        for _ in 0 ..< 50 where coordinator.systemStartOffsetMs == nil {
            try? await Task.sleep(nanoseconds: 10_000_000)
        }

        XCTAssertEqual(coordinator.micStartOffsetMs, 12)
        XCTAssertEqual(coordinator.systemStartOffsetMs, 1162, "разница стартов — 1150 мс")

        // Привязка одноразовая: дальше метки идут от кадров, иначе внутри
        // канала появилось бы дрожание часов. Ждём именно роста счётчика:
        // без него утверждение выполнялось бы просто потому, что второй
        // буфер ещё не дошёл.
        let processed = coordinator.chunkCount
        microphone.emit(samples, hostTime: hostTime(atMs: 9000))
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
        systemAudio.emit(samples, hostTime: hostTime(atMs: 1162))
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

    /// Замер подъёма захвата — целой цепочкой, а не половиной.
    ///
    /// Задача 3 Epic 25 спрашивает, какой шаг съедает секунду до первого
    /// системного буфера. Ответить можно только если в журнале есть все
    /// звенья: шаги координатора, шаги самого источника и последнее звено —
    /// ожидание звука, которое не кончается возвратом из вызова.
    func testStartTimingCoversTheWholeChain() async throws {
        let core = makeCore()
        let microphone = FakeTap()
        let systemAudio = FakeTap(isAvailable: true)
        // Так отчитался бы настоящий tap: цена сидит в одном шаге.
        systemAudio.lastStartSteps = [
            CaptureStartStep(name: "system:create_tap", elapsedMs: 700),
            CaptureStartStep(name: "system:aggregate", elapsedMs: 30),
        ]
        let coordinator = makeCoordinator(
            microphone: microphone,
            systemAudio: systemAudio,
            core: core
        )
        await coordinator.startRecording()

        let frames = Int(AudioChunkPipeline.targetSampleRate * 0.1)
        let samples = Array(repeating: Float(0.1), count: frames)
        systemAudio.emit(samples, hostTime: hostTime(atMs: 1162))
        for _ in 0 ..< 50 where coordinator.systemStartOffsetMs == nil {
            try? await Task.sleep(nanoseconds: 10_000_000)
        }
        coordinator.stopRecording()

        let log = try String(contentsOfFile: core.diagnosticsLogPath(), encoding: .utf8)
        let steps = log
            .split(separator: "\n")
            .map(String.init)
            .filter { $0.contains("capture_start_step") }
        XCTAssertFalse(steps.isEmpty, "шагов нет вовсе — проверять нечего")

        for name in ["session_open", "system_prepare", "mic_start", "system_start"] {
            XCTAssertTrue(
                steps.contains { $0.contains("\"text\":\"\(name)\"") },
                "в цепочке нет шага \(name): \(log)"
            )
        }
        XCTAssertTrue(
            steps.contains {
                $0.contains("\"text\":\"system:create_tap\"") && $0.contains("\"buffer_ms\":700")
            },
            "замер самого источника не доехал до журнала: \(log)"
        )
        // Последнее звено: от возврата из `start()` до первого буфера.
        // Часы в тесте стоят, поэтому это ровно метка буфера.
        XCTAssertTrue(
            steps.contains {
                $0.contains("\"text\":\"system:first_buffer\"") && $0.contains("\"buffer_ms\":1162")
            },
            "ожидание первого буфера не измерено: \(log)"
        )
    }
}
