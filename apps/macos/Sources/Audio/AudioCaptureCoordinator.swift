import Foundation
import Observation

/// Координатор mic (+ system если available) → UniFFI ingest + drain live STT.
@Observable
@MainActor
final class AudioCaptureCoordinator {
    private(set) var isRecording = false
    private(set) var lastError: String?
    private(set) var systemAudioAvailable = false
    /// Почему системный звук недоступен — для actionable-состояния в UI.
    private(set) var systemAudioStatus: SystemAudioStatus = .unknown
    private(set) var sessionId: String?
    /// Обновляется при каждом успешном ingest — чтобы UI видел рост.
    private(set) var chunkCount: UInt64 = 0
    /// `idle` | `mock` | `whisper` после startRecording.
    private(set) var sttBackend: String = "idle"
    private(set) var captionEventCount: UInt64 = 0
    /// Сглаженный уровень микрофона, 0…1 — для индикатора на экране.
    ///
    /// Считается из тех же сэмплов, что уходят в распознавание, поэтому
    /// показывает реальный вход, а не анимацию.
    private(set) var inputLevel: Double = 0
    /// Насколько позже начала записи начался каждый канал, мс. `nil` —
    /// канал ещё не отдал ни одного буфера.
    ///
    /// Этим и сведены метки чанков к общему времени: у каналов больше нет
    /// каждого своего нуля.
    private(set) var micStartOffsetMs: UInt64?
    private(set) var systemStartOffsetMs: UInt64?

    private let core: MeetingCore
    private let microphone: any AudioTapping
    private let systemAudio: any AudioTapping
    private let clock: HostClock
    private var micPipeline = AudioChunkPipeline()
    private var systemPipeline = AudioChunkPipeline()
    /// Начало записи в тиках общих часов: снимается перед запуском первого
    /// источника, поэтому ни один буфер не может быть раньше него.
    private var recordingAnchor: UInt64?
    /// Когда вернулся `start()` каждого источника, в тиках общих часов.
    ///
    /// Отдельно от якоря записи: между «вызов вернулся» и «пришёл первый
    /// буфер» может лежать своё время, и оно измеряется само по себе.
    /// Иначе шаги сложатся в сотню миллисекунд при разнице стартов в
    /// секунду, и остаток окажется приписан неизвестно чему.
    private var micStartedAt: UInt64?
    private var systemStartedAt: UInt64?

    /// Запрос разрешения вынесен в зависимость: в тестовом бандле
    /// системный промпт недоступен и подвесил бы тест.
    private let requestMicrophonePermission: @Sendable () async -> Bool

    init(
        core: MeetingCore,
        microphone: any AudioTapping = MicrophoneCapture(),
        systemAudio: any AudioTapping = SystemAudioCapture(),
        clock: HostClock = .system,
        requestMicrophonePermission: @escaping @Sendable () async -> Bool = {
            await AudioPermissions.requestMicrophone()
        }
    ) {
        self.core = core
        self.microphone = microphone
        self.systemAudio = systemAudio
        self.clock = clock
        self.requestMicrophonePermission = requestMicrophonePermission
    }

    init(dataRoot: String? = nil) {
        microphone = MicrophoneCapture()
        systemAudio = SystemAudioCapture()
        clock = .system
        requestMicrophonePermission = { await AudioPermissions.requestMicrophone() }
        if let dataRoot {
            core = MeetingCore.withDataRoot(dataRoot: dataRoot)
        } else {
            let support = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask).first!
            let root = support.appendingPathComponent("meetingraft", isDirectory: true)
            try? FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
            core = MeetingCore.withDataRoot(dataRoot: root.path)
        }
    }

    /// Путь к ggml-модели (пусто если нет).
    var whisperModelPath: String {
        core.whisperModelPath()
    }

    var modelsDirectory: String {
        core.modelsDirectory()
    }

    /// Разведать системный канал заранее — при открытии окна.
    ///
    /// Первый вызов поднимает системный запрос «System Audio Recording».
    /// Пусть он приходит при открытии приложения, а не по нажатию
    /// «запись»: там он ложится в самое начало встречи, и первые слова
    /// созвона в системный канал не попадают вовсе.
    ///
    /// Идемпотентно: удачная разведка запоминается внутри источника, и
    /// повторный вызов tap'а не создаёт. Во время записи не делает ничего
    /// — tap уже поднят, и лезть в него незачем.
    ///
    /// **Главный поток при этом занят**, пока запрос на экране: вызов
    /// `AudioHardwareCreateProcessTap` синхронный. Это цена не выросла, а
    /// переехала — раньше окно замирало на нажатии записи. Убрать её
    /// целиком значит звать разведку вне главного актора, а это требует
    /// точки сериализации: `prepare()` и `start()` не должны пересекаться
    /// никогда, иначе tap утечёт в `coreaudiod` (Epic 24). Такую правку
    /// делать без Мака под рукой нельзя.
    func warmUpSystemAudio() {
        guard !isRecording else { return }
        systemAudio.prepare()
        systemAudioAvailable = systemAudio.isAvailable
        systemAudioStatus = (systemAudio as? SystemAudioCapture)?.status ?? .unknown
    }

    /// Старт recording: permission → Rust session → taps.
    func startRecording() async {
        lastError = nil
        chunkCount = 0
        captionEventCount = 0
        sttBackend = "idle"
        let granted = await requestMicrophonePermission()
        guard granted else {
            lastError = "Доступ к микрофону запрещён"
            return
        }

        // Секундомер с этой точки, а не с начала метода: ожидание
        // разрешения — время человека, а не наша цена.
        var timer = CaptureStepTimer(clock: clock)

        // Разведка системного канала — **до** открытия сессии. Первый её
        // вызов поднимает системный запрос «System Audio Recording», и
        // пока этот запрос на экране, звук не идёт ни по одному каналу:
        // микрофон стартует ниже. Открытая сессия и `isRecording = true`
        // означали бы в это время «идёт запись» при нуле записанного, а
        // человек, уверенный, что встреча пишется, — худший исход из
        // возможных.
        //
        // Разведка создаёт и сразу отпускает tap, поэтому её цена — цена
        // ожидания перед записью. Платится она только за первую встречу
        // после запуска приложения: удачная разведка запоминается.
        systemAudio.prepare()
        systemAudioAvailable = systemAudio.isAvailable
        systemAudioStatus = (systemAudio as? SystemAudioCapture)?.status ?? .unknown
        timer.step("system_prepare")

        let id = UUID().uuidString
        let err = core.startRecording(sessionId: id, title: MeetingTitle.forNewMeeting())
        guard err.isEmpty else {
            lastError = err
            return
        }
        sessionId = id
        micPipeline.reset()
        systemPipeline.reset()
        micStartOffsetMs = nil
        systemStartOffsetMs = nil
        recordingAnchor = nil
        micStartedAt = nil
        systemStartedAt = nil
        sttBackend = core.sttBackend()
        // Пока tap не запущен, микшер не должен ждать системный канал.
        core.setSystemAudioExpected(expected: false)
        // До start mic: иначе ранние буферы отбрасываются в ingest.
        isRecording = true
        timer.step("session_open")

        // Начало записи — до запуска первого источника: якорь канала
        // отсчитывается от него, и буфер, записанный раньше собственного
        // старта, невозможен.
        recordingAnchor = clock.now()

        do {
            try microphone.start { [weak self] samples, hostTime in
                Task { @MainActor in
                    self?.ingest(samples: samples, hostTime: hostTime, channel: .mic)
                }
            }
        } catch {
            lastError = "Не удалось запустить микрофон: \(error.localizedDescription)"
            microphone.stop()
            // lastError уже содержит настоящую причину — не перетирать.
            // Записывать нечего: запись не началась.
            _ = core.stopRecording()
            sessionId = nil
            isRecording = false
            sttBackend = "idle"
            return
        }
        micStartedAt = clock.now()
        timer.step("mic_start")

        if systemAudioAvailable {
            do {
                try systemAudio.start { [weak self] samples, hostTime in
                    Task { @MainActor in
                        self?.ingest(samples: samples, hostTime: hostTime, channel: .system)
                    }
                }
                core.setSystemAudioExpected(expected: true)
            } catch {
                systemAudioAvailable = false
                systemAudioStatus = (systemAudio as? SystemAudioCapture)?.status ?? .unsupported
            }
            systemStartedAt = clock.now()
            timer.step("system_start")
        }

        logStartSteps(timer.steps)
    }

    /// Записать замер подъёма захвата в журнал диагностики.
    ///
    /// Шаги координатора и шаги самих источников идут одним списком: вопрос
    /// «куда девается секунда до первого системного буфера» разбирается по
    /// всей цепочке, а не по её половине.
    private func logStartSteps(_ steps: [CaptureStartStep]) {
        for step in steps + microphone.lastStartSteps + systemAudio.lastStartSteps {
            core.logCaptureStartStep(name: step.name, elapsedMs: step.elapsedMs)
        }
    }

    func stopRecording() {
        microphone.stop()
        systemAudio.stop()
        let error = core.stopRecording()
        if !error.isEmpty {
            lastError = "Не удалось сохранить хвост записи: \(error)"
        }
        core.setSystemAudioExpected(expected: false)
        isRecording = false
        sttBackend = "idle"
        inputLevel = 0
    }

    /// Live STT events с того же MeetingCore, что и ingest.
    func drainLiveCaptions() -> [FfiCaptionEvent] {
        let events = core.drainLiveCaptions()
        if let sessionId {
            captionEventCount = core.captionEventCount(sessionId: sessionId)
        }
        return events
    }

    /// Сбросить сообщение об ошибке (UI alert).
    func clearError() {
        lastError = nil
    }

    /// Насколько быстро индикатор следует за звуком. Ниже — плавнее, но
    /// заметно отстаёт от речи.
    private static let levelSmoothing = 0.3
    /// Опорный уровень: обычная речь около этого значения RMS.
    private static let levelReference = 0.15

    private func updateLevel(samples: [Float]) {
        let sum = samples.reduce(0.0) { $0 + Double($1) * Double($1) }
        let rms = (sum / Double(samples.count)).squareRoot()
        let normalized = min(1, rms / Self.levelReference)
        inputLevel += (normalized - inputLevel) * Self.levelSmoothing
    }

    /// Имя канала для журнала — то же, каким его знает ядро.
    private func channelCode(_ channel: FfiAudioChannel) -> String {
        switch channel {
        case .mic: "mic"
        case .system: "system"
        }
    }

    /// Привязать канал к общему времени по его первому буферу.
    ///
    /// Сдвиг считается от начала записи, а метки чанков внутри канала —
    /// по-прежнему от кадров. Оба числа уходят в журнал диагностики:
    /// молчащая разница стартов уже раз стоила недели разбора, и знать её
    /// нужно по каждой записи, а не по той, где что-то заподозрили.
    private func anchorIfNeeded(channel: FfiAudioChannel, hostTime: UInt64) {
        guard let recordingAnchor else { return }
        let offset = clock.elapsedMs(from: recordingAnchor, to: hostTime)
        let startedAt: UInt64?
        switch channel {
        case .mic:
            guard micStartOffsetMs == nil else { return }
            micStartOffsetMs = offset
            micPipeline.anchor(startOffsetMs: offset)
            startedAt = micStartedAt
        case .system:
            guard systemStartOffsetMs == nil else { return }
            systemStartOffsetMs = offset
            systemPipeline.anchor(startOffsetMs: offset)
            startedAt = systemStartedAt
        }
        core.logCaptureChannelStart(channel: channel, offsetMs: offset)

        // Последний шаг цепочки, и единственный, который кончается не
        // возвратом из вызова, а приходом звука.
        if let startedAt {
            core.logCaptureStartStep(
                name: "\(channelCode(channel)):first_buffer",
                elapsedMs: clock.elapsedMs(from: startedAt, to: hostTime)
            )
        }

        // Разницу пишем, когда стали известны оба конца — раньше её просто
        // нет.
        if let mic = micStartOffsetMs, let system = systemStartOffsetMs {
            core.logCaptureChannelSkew(
                laterChannel: system >= mic ? .system : .mic,
                skewMs: system >= mic ? system - mic : mic - system
            )
        }
    }

    private func ingest(samples: [Float], hostTime: UInt64, channel: FfiAudioChannel) {
        guard isRecording, sessionId != nil, !samples.isEmpty else { return }
        anchorIfNeeded(channel: channel, hostTime: hostTime)
        if channel == .mic {
            updateLevel(samples: samples)
        }
        let chunks: [AudioChunk] = switch channel {
        case .mic:
            micPipeline.push(samples: samples)
        case .system:
            systemPipeline.push(samples: samples)
        }
        for chunk in chunks {
            let err = core.ingestAudioChunk(
                channel: channel,
                pcm: chunk.data,
                sampleRate: UInt32(AudioChunkPipeline.targetSampleRate),
                timestampMs: chunk.timestampMs()
            )
            if err.isEmpty {
                chunkCount += 1
            } else {
                lastError = err
            }
        }
    }
}
