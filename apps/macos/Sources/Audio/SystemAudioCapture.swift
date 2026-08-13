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
    private var onSamples: SamplesHandler?
    /// Разведка уже проходила успешно — повторять её не нужно.
    private var didProbeSuccessfully = false
    private(set) var lastStartSteps: [CaptureStartStep] = []

    private static let aggregateUid = "com.vitvitsky.meetingraft.aggregate"

    /// Страховка на случай, когда объект отпустили, не позвав `stop()`.
    /// Tap и aggregate живут в `coreaudiod`, а не в нас, и без явной
    /// уборки переживут наш процесс.
    ///
    /// SIGKILL этим не покрывается, и покрыть его нечем: подметание при
    /// старте здесь стояло и было снесено — наш tap приватный, а
    /// `kAudioHardwarePropertyTapList` приватные tap'ы не показывает даже
    /// собственному следующему запуску. Замер 2026-08-08: список пуст при
    /// заведомо идущей записи. Защита, которая не может сработать, хуже
    /// её отсутствия — она создаёт уверенность (Epic 24).
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

    func start(onSamples: @escaping SamplesHandler) throws {
        stop()
        // Шаги мерятся всегда: какой из этих вызовов стоит секунду, из кода
        // не видно — все они уходят в `coreaudiod` (задача 3 Epic 25).
        var timer = CaptureStepTimer()
        lastStartSteps = []

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
        timer.step("system:create_tap")

        guard let outputUid = Self.defaultOutputDeviceUid() else {
            stop()
            status = .noOutputDevice
            throw CaptureError.unavailable(.noOutputDevice)
        }
        timer.step("system:output_device")

        guard let aggregate = createAggregate(tapId: tap, outputUid: outputUid) else {
            stop()
            status = .aggregateFailed
            throw CaptureError.unavailable(.aggregateFailed)
        }
        aggregateId = aggregate
        timer.step("system:aggregate")

        guard let format = Self.streamFormat(of: aggregate),
              let downmixer = PCMDownmixer(from: format)
        else {
            stop()
            status = .aggregateFailed
            throw CaptureError.unavailable(.aggregateFailed)
        }
        self.downmixer = downmixer
        self.onSamples = onSamples
        timer.step("system:stream_format")

        var procId: AudioDeviceIOProcID?
        let createStatus = AudioDeviceCreateIOProcIDWithBlock(
            &procId,
            aggregate,
            nil
        ) { [weak self] _, inputData, inputTime, _, _ in
            self?.handle(inputData: inputData, inputTime: inputTime, format: format)
        }
        guard createStatus == noErr, let procId else {
            stop()
            status = .aggregateFailed
            throw CaptureError.unavailable(.aggregateFailed)
        }
        ioProcId = procId
        timer.step("system:io_proc")

        let startStatus = AudioDeviceStart(aggregate, procId)
        guard startStatus == noErr else {
            stop()
            status = .aggregateFailed
            throw CaptureError.unavailable(.aggregateFailed)
        }
        timer.step("system:device_start")

        isAvailable = true
        status = .granted
        lastStartSteps = timer.steps
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
            logFailure(
                AudioHardwareDestroyAggregateDevice(aggregateId),
                "AudioHardwareDestroyAggregateDevice"
            )
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
        return .success(id)
    }

    private func destroyTap(_ id: AudioObjectID) {
        logFailure(AudioHardwareDestroyProcessTap(id), "AudioHardwareDestroyProcessTap")
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
        return id
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

    private func handle(
        inputData: UnsafePointer<AudioBufferList>,
        inputTime: UnsafePointer<AudioTimeStamp>,
        format: AVAudioFormat
    ) {
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
        onSamples(samples, hostTime(of: inputTime))
    }

    /// Момент записи входных данных в тиках общих часов.
    ///
    /// `mHostTime` у IOProc идёт от `mach_absolute_time` — тех же часов,
    /// что `AVAudioTime.hostTime` у микрофона. Только поэтому два канала
    /// вообще можно свести к одному времени.
    ///
    /// Невалидную метку не подставляем молча: без неё канал получил бы
    /// начало по времени приёма, и это надо видеть в логе.
    private func hostTime(of inputTime: UnsafePointer<AudioTimeStamp>) -> UInt64 {
        let stamp = inputTime.pointee
        guard stamp.mFlags.contains(.hostTimeValid) else {
            log.error("у буфера системного канала нет mHostTime — метка взята по времени приёма")
            return HostClock.system.now()
        }
        return stamp.mHostTime
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
