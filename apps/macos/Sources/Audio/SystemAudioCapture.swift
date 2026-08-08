@preconcurrency import AVFoundation
import CoreAudio
import Foundation
import OSLog

/// Захват system playback через Core Audio process tap (ADR-004).
///
/// Схема: `AudioHardwareCreateProcessTap` даёт tap на весь вывод системы,
/// поверх него поднимается приватное aggregate-устройство, с которого
/// читает IOProc. ScreenCaptureKit сознательно не используется — он
/// требует разрешения Screen Recording ради звука.
final class SystemAudioCapture: AudioTapping {
    private let log = Logger(subsystem: "com.vitvitsky.meetingraft", category: "SystemAudio")

    private(set) var isAvailable = false
    private(set) var status: SystemAudioStatus = .unknown

    private var tapId: AudioObjectID = kAudioObjectUnknown
    private var aggregateId: AudioObjectID = kAudioObjectUnknown
    private var ioProcId: AudioDeviceIOProcID?
    private var downmixer: PCMDownmixer?
    private var onSamples: (([Float]) -> Void)?
    /// Разведка уже проходила успешно — повторять её не нужно.
    private var didProbeSuccessfully = false

    /// UID нашего aggregate. Фиксированный: по нему следующий запуск
    /// узнаёт брошенное устройство.
    private static let aggregateUid = "com.vitvitsky.meetingraft.aggregate"

    /// UID созданных нами объектов Core Audio → pid создателя.
    ///
    /// Единственный способ опознать свой мусор после SIGKILL: объект в
    /// `coreaudiod` жив, а связи с умершим процессом у него нет. Pid
    /// нужен, чтобы не снести объекты **живого** соседа — во время
    /// разработки рядом со сборкой из Xcode вполне работает
    /// установленная копия, и у неё тот же домен `UserDefaults`.
    private static let ownedObjectsKey = "com.vitvitsky.meetingraft.systemAudio.ownedObjects"
    private let defaults: UserDefaults

    init(defaults: UserDefaults = .standard) {
        self.defaults = defaults
        sweepLeftovers()
    }

    /// Страховка на случай, когда объект отпустили, не позвав `stop()`.
    /// Tap и aggregate живут в `coreaudiod`, а не в нас, и без явной
    /// уборки переживут наш процесс, ломая системный звук другим
    /// приложениям (Epic 24). SIGKILL этим не покрывается — там не
    /// выполняется вообще ничего.
    deinit {
        stop()
    }

    /// Разведка: создаём tap и сразу отпускаем. Первый вызов поднимает
    /// системный запрос разрешения «System Audio Recording» (macOS 15+).
    ///
    /// Удачная разведка запоминается: каждый созданный tap — это шанс
    /// оставить его в `coreaudiod` навсегда (Epic 24), и делать этот
    /// бросок на каждый `startRecording` незачем. Отказ не кэшируется:
    /// разрешение могли выдать между попытками, а неудавшийся
    /// `createTap` после себя ничего не оставляет.
    func prepare() {
        guard tapId == kAudioObjectUnknown else {
            isAvailable = true
            return
        }
        guard !didProbeSuccessfully else {
            isAvailable = true
            status = .granted
            return
        }
        switch createTap() {
        case let .success(id):
            destroyTap(id)
            didProbeSuccessfully = true
            isAvailable = true
            status = .granted
        case let .failure(error):
            isAvailable = false
            status = error
            log.info("System audio tap unavailable: \(String(describing: error), privacy: .public)")
        }
    }

    func start(onSamples: @escaping ([Float]) -> Void) throws {
        stop()

        let tap: AudioObjectID
        switch createTap() {
        case let .success(id):
            tap = id
        case let .failure(error):
            isAvailable = false
            status = error
            throw CaptureError.unavailable(error)
        }
        tapId = tap

        guard let outputUid = Self.defaultOutputDeviceUid() else {
            stop()
            status = .noOutputDevice
            throw CaptureError.unavailable(.noOutputDevice)
        }

        guard let aggregate = createAggregate(tapId: tap, outputUid: outputUid) else {
            stop()
            status = .aggregateFailed
            throw CaptureError.unavailable(.aggregateFailed)
        }
        aggregateId = aggregate

        guard let format = Self.streamFormat(of: aggregate),
              let downmixer = PCMDownmixer(from: format)
        else {
            stop()
            status = .aggregateFailed
            throw CaptureError.unavailable(.aggregateFailed)
        }
        self.downmixer = downmixer
        self.onSamples = onSamples

        var procId: AudioDeviceIOProcID?
        let createStatus = AudioDeviceCreateIOProcIDWithBlock(
            &procId,
            aggregate,
            nil
        ) { [weak self] _, inputData, _, _, _ in
            self?.handle(inputData: inputData, format: format)
        }
        guard createStatus == noErr, let procId else {
            stop()
            status = .aggregateFailed
            throw CaptureError.unavailable(.aggregateFailed)
        }
        ioProcId = procId

        let startStatus = AudioDeviceStart(aggregate, procId)
        guard startStatus == noErr else {
            stop()
            status = .aggregateFailed
            throw CaptureError.unavailable(.aggregateFailed)
        }

        isAvailable = true
        status = .granted
        log.info("System audio tap started")
    }

    /// Идемпотентно: повторный вызов не должен ронять Core Audio.
    ///
    /// Каждый отказ уборки идёт в лог. Оставленный tap или aggregate
    /// живёт в `coreaudiod` дольше нашего процесса и ломает системный
    /// звук соседним приложениям (Epic 24) — молчать про такое нельзя.
    func stop() {
        if aggregateId != kAudioObjectUnknown, let ioProcId {
            logFailure(AudioDeviceStop(aggregateId, ioProcId), "AudioDeviceStop")
            logFailure(
                AudioDeviceDestroyIOProcID(aggregateId, ioProcId),
                "AudioDeviceDestroyIOProcID"
            )
        }
        ioProcId = nil

        if aggregateId != kAudioObjectUnknown {
            let destroyStatus = AudioHardwareDestroyAggregateDevice(aggregateId)
            logFailure(destroyStatus, "AudioHardwareDestroyAggregateDevice")
            if destroyStatus == noErr {
                forget(Self.aggregateUid)
            }
            aggregateId = kAudioObjectUnknown
        }
        if tapId != kAudioObjectUnknown {
            destroyTap(tapId)
            tapId = kAudioObjectUnknown
        }
        downmixer = nil
        onSamples = nil
    }

    // MARK: - Core Audio

    private func createTap() -> Result<AudioObjectID, SystemAudioStatus> {
        let description = CATapDescription(stereoGlobalTapButExcludeProcesses: [])
        description.isPrivate = true
        description.muteBehavior = .unmuted

        var id = AudioObjectID(kAudioObjectUnknown)
        let result = AudioHardwareCreateProcessTap(description, &id)
        guard result == noErr, id != kAudioObjectUnknown else {
            // Отказ в TCC приходит как ошибка операции, отдельного кода нет.
            return .failure(result == kAudioHardwareIllegalOperationError ? .denied : .unsupported)
        }
        if let uid = Self.tapUid(of: id) {
            remember(uid)
        }
        return .success(id)
    }

    private func destroyTap(_ id: AudioObjectID) {
        // UID читается до уничтожения: после него объекта уже нет.
        let uid = Self.tapUid(of: id)
        let destroyStatus = AudioHardwareDestroyProcessTap(id)
        logFailure(destroyStatus, "AudioHardwareDestroyProcessTap")
        if destroyStatus == noErr, let uid {
            forget(uid)
        }
    }

    /// Отказ уборки Core Audio — в лог. Вернуть его наверх некуда:
    /// `stop()` зовётся из `deinit` и из аварийных веток `start`, где
    /// обрабатывать его уже нечем, но знать о нём надо.
    private func logFailure(_ status: OSStatus, _ call: String) {
        guard status != noErr else { return }
        log.error("\(call, privacy: .public) failed: \(status, privacy: .public)")
    }

    private func createAggregate(tapId: AudioObjectID, outputUid: String) -> AudioObjectID? {
        guard let tapUid = Self.tapUid(of: tapId) else { return nil }

        let description: [String: Any] = [
            kAudioAggregateDeviceNameKey as String: "MeetingRaft System Capture",
            kAudioAggregateDeviceUIDKey as String: Self.aggregateUid,
            kAudioAggregateDeviceMainSubDeviceKey as String: outputUid,
            kAudioAggregateDeviceIsPrivateKey as String: true,
            kAudioAggregateDeviceIsStackedKey as String: false,
            kAudioAggregateDeviceTapAutoStartKey as String: true,
            kAudioAggregateDeviceSubDeviceListKey as String: [
                [kAudioSubDeviceUIDKey as String: outputUid],
            ],
            kAudioAggregateDeviceTapListKey as String: [
                [
                    kAudioSubTapDriftCompensationKey as String: true,
                    kAudioSubTapUIDKey as String: tapUid,
                ],
            ],
        ]

        var id = AudioObjectID(kAudioObjectUnknown)
        let result = AudioHardwareCreateAggregateDevice(description as CFDictionary, &id)
        guard result == noErr, id != kAudioObjectUnknown else { return nil }
        remember(Self.aggregateUid)
        return id
    }

    // MARK: - Уборка за прошлыми запусками

    /// Снести объекты Core Audio, оставшиеся от мёртвых процессов.
    ///
    /// Штатный выход зовёт `stop()`, SIGKILL — нет, и тогда tap с
    /// aggregate остаются в `coreaudiod`, где мешают снимать системный
    /// звук уже всем, включая чужие приложения (Epic 24). Отсюда
    /// подметание при старте.
    ///
    /// Оно же и замер: ненулевая строка в логе — прямое доказательство,
    /// что утечка реальна. Если она не появляется никогда, разбор
    /// Epic 24 надо пересматривать.
    private func sweepLeftovers() {
        let owned = defaults.dictionary(forKey: Self.ownedObjectsKey) as? [String: Int] ?? [:]
        let abandoned = Set(owned.filter { !Self.isProcessAlive($0.value) }.keys)
        guard !abandoned.isEmpty else { return }

        var swept = 0
        for id in Self.tapIds() where Self.tapUid(of: id).map(abandoned.contains) == true {
            let status = AudioHardwareDestroyProcessTap(id)
            logFailure(status, "AudioHardwareDestroyProcessTap (leftover)")
            if status == noErr {
                swept += 1
            }
        }
        if abandoned.contains(Self.aggregateUid), let id = Self.deviceId(withUid: Self.aggregateUid) {
            let status = AudioHardwareDestroyAggregateDevice(id)
            logFailure(status, "AudioHardwareDestroyAggregateDevice (leftover)")
            if status == noErr {
                swept += 1
            }
        }

        // Записи снимаются независимо от исхода: не нашли — объекта уже
        // нет, не снесли — повторные попытки каждый запуск ничего не
        // изменят, а список будет расти вечно.
        defaults.set(owned.filter { !abandoned.contains($0.key) }, forKey: Self.ownedObjectsKey)
        log.error(
            "Core Audio leftovers from dead processes: \(abandoned.count, privacy: .public) known, \(swept, privacy: .public) destroyed"
        )
    }

    private func remember(_ uid: String) {
        var owned = defaults.dictionary(forKey: Self.ownedObjectsKey) as? [String: Int] ?? [:]
        owned[uid] = Int(ProcessInfo.processInfo.processIdentifier)
        defaults.set(owned, forKey: Self.ownedObjectsKey)
    }

    private func forget(_ uid: String) {
        var owned = defaults.dictionary(forKey: Self.ownedObjectsKey) as? [String: Int] ?? [:]
        owned.removeValue(forKey: uid)
        defaults.set(owned, forKey: Self.ownedObjectsKey)
    }

    /// Жив ли процесс. `EPERM` — жив, но чужой; для нас это тоже «жив».
    ///
    /// Pid переиспользуются, так что чужой процесс с тем же номером
    /// заставит нас пропустить уборку. Ошибка в безопасную сторону:
    /// лучше не убрать своё, чем убить чужое живое.
    private static func isProcessAlive(_ pid: Int) -> Bool {
        kill(pid_t(pid), 0) == 0 || errno == EPERM
    }

    /// Все process tap'ы системы. macOS 14.4+.
    private static func tapIds() -> [AudioObjectID] {
        var address = AudioObjectPropertyAddress(
            mSelector: kAudioHardwarePropertyTapList,
            mScope: kAudioObjectPropertyScopeGlobal,
            mElement: kAudioObjectPropertyElementMain
        )
        return objectIds(of: AudioObjectID(kAudioObjectSystemObject), at: &address)
    }

    private static func deviceId(withUid uid: String) -> AudioObjectID? {
        var address = AudioObjectPropertyAddress(
            mSelector: kAudioHardwarePropertyDevices,
            mScope: kAudioObjectPropertyScopeGlobal,
            mElement: kAudioObjectPropertyElementMain
        )
        return objectIds(of: AudioObjectID(kAudioObjectSystemObject), at: &address)
            .first { deviceUid(of: $0) == uid }
    }

    private static func objectIds(
        of owner: AudioObjectID,
        at address: inout AudioObjectPropertyAddress
    ) -> [AudioObjectID] {
        var size: UInt32 = 0
        guard AudioObjectGetPropertyDataSize(owner, &address, 0, nil, &size) == noErr else {
            return []
        }
        let count = Int(size) / MemoryLayout<AudioObjectID>.size
        guard count > 0 else { return [] }

        var ids = [AudioObjectID](repeating: kAudioObjectUnknown, count: count)
        guard AudioObjectGetPropertyData(owner, &address, 0, nil, &size, &ids) == noErr else {
            return []
        }
        return ids
    }

    private static func deviceUid(of deviceId: AudioObjectID) -> String? {
        var address = AudioObjectPropertyAddress(
            mSelector: kAudioDevicePropertyDeviceUID,
            mScope: kAudioObjectPropertyScopeGlobal,
            mElement: kAudioObjectPropertyElementMain
        )
        var uid: CFString = "" as CFString
        var size = UInt32(MemoryLayout<CFString>.size)
        let result = withUnsafeMutablePointer(to: &uid) { pointer in
            AudioObjectGetPropertyData(deviceId, &address, 0, nil, &size, pointer)
        }
        return result == noErr ? uid as String : nil
    }

    private func handle(inputData: UnsafePointer<AudioBufferList>, format: AVAudioFormat) {
        guard let downmixer, let onSamples else { return }
        let list = UnsafeMutableAudioBufferListPointer(UnsafeMutablePointer(mutating: inputData))
        guard let first = list.first, first.mDataByteSize > 0 else { return }

        let frames = first.mDataByteSize / max(1, format.streamDescription.pointee.mBytesPerFrame)
        guard frames > 0,
              let buffer = AVAudioPCMBuffer(pcmFormat: format, frameCapacity: AVAudioFrameCount(frames))
        else {
            return
        }
        buffer.frameLength = AVAudioFrameCount(frames)

        let destination = UnsafeMutableAudioBufferListPointer(buffer.mutableAudioBufferList)
        for (index, source) in list.enumerated() where index < destination.count {
            guard let sourceData = source.mData, let destinationData = destination[index].mData else {
                continue
            }
            let bytes = min(source.mDataByteSize, destination[index].mDataByteSize)
            memcpy(destinationData, sourceData, Int(bytes))
        }

        let samples = downmixer.convert(buffer)
        guard !samples.isEmpty else { return }
        onSamples(samples)
    }

    private static func tapUid(of tapId: AudioObjectID) -> String? {
        var address = AudioObjectPropertyAddress(
            mSelector: kAudioTapPropertyUID,
            mScope: kAudioObjectPropertyScopeGlobal,
            mElement: kAudioObjectPropertyElementMain
        )
        var uid: CFString = "" as CFString
        var size = UInt32(MemoryLayout<CFString>.size)
        let result = withUnsafeMutablePointer(to: &uid) { pointer in
            AudioObjectGetPropertyData(tapId, &address, 0, nil, &size, pointer)
        }
        return result == noErr ? uid as String : nil
    }

    private static func defaultOutputDeviceUid() -> String? {
        var address = AudioObjectPropertyAddress(
            mSelector: kAudioHardwarePropertyDefaultOutputDevice,
            mScope: kAudioObjectPropertyScopeGlobal,
            mElement: kAudioObjectPropertyElementMain
        )
        var deviceId = AudioObjectID(kAudioObjectUnknown)
        var size = UInt32(MemoryLayout<AudioObjectID>.size)
        guard AudioObjectGetPropertyData(
            AudioObjectID(kAudioObjectSystemObject),
            &address,
            0,
            nil,
            &size,
            &deviceId
        ) == noErr, deviceId != kAudioObjectUnknown else {
            return nil
        }
        return deviceUid(of: deviceId)
    }

    private static func streamFormat(of deviceId: AudioObjectID) -> AVAudioFormat? {
        var address = AudioObjectPropertyAddress(
            mSelector: kAudioDevicePropertyStreamFormat,
            mScope: kAudioObjectPropertyScopeInput,
            mElement: kAudioObjectPropertyElementMain
        )
        var description = AudioStreamBasicDescription()
        var size = UInt32(MemoryLayout<AudioStreamBasicDescription>.size)
        guard AudioObjectGetPropertyData(deviceId, &address, 0, nil, &size, &description) == noErr else {
            return nil
        }
        return AVAudioFormat(streamDescription: &description)
    }

    enum CaptureError: Error {
        case unavailable(SystemAudioStatus)
    }
}

/// Почему системный звук недоступен — определяет, что показывать в UI.
enum SystemAudioStatus: Error, Equatable, Sendable {
    /// Разведка ещё не проводилась.
    case unknown
    /// Tap создаётся — разрешение есть.
    case granted
    /// Пользователь отказал в «System Audio Recording».
    case denied
    /// API недоступен на этой системе.
    case unsupported
    /// Нет устройства вывода, поверх которого строить aggregate.
    case noOutputDevice
    /// Tap создался, но aggregate-устройство поднять не удалось.
    case aggregateFailed
}
