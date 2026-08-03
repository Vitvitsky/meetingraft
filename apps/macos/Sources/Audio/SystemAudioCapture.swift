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

    /// Разведка: создаём tap и сразу отпускаем. Первый вызов поднимает
    /// системный запрос разрешения «System Audio Recording» (macOS 15+).
    func prepare() {
        guard tapId == kAudioObjectUnknown else {
            isAvailable = true
            return
        }
        switch createTap() {
        case let .success(id):
            destroyTap(id)
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
    func stop() {
        if aggregateId != kAudioObjectUnknown, let ioProcId {
            AudioDeviceStop(aggregateId, ioProcId)
            AudioDeviceDestroyIOProcID(aggregateId, ioProcId)
        }
        ioProcId = nil

        if aggregateId != kAudioObjectUnknown {
            AudioHardwareDestroyAggregateDevice(aggregateId)
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
        AudioHardwareDestroyProcessTap(id)
    }

    private func createAggregate(tapId: AudioObjectID, outputUid: String) -> AudioObjectID? {
        guard let tapUid = Self.tapUid(of: tapId) else { return nil }

        let description: [String: Any] = [
            kAudioAggregateDeviceNameKey as String: "MeetingRaft System Capture",
            kAudioAggregateDeviceUIDKey as String: "com.vitvitsky.meetingraft.aggregate",
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

        var uidAddress = AudioObjectPropertyAddress(
            mSelector: kAudioDevicePropertyDeviceUID,
            mScope: kAudioObjectPropertyScopeGlobal,
            mElement: kAudioObjectPropertyElementMain
        )
        var uid: CFString = "" as CFString
        var uidSize = UInt32(MemoryLayout<CFString>.size)
        let result = withUnsafeMutablePointer(to: &uid) { pointer in
            AudioObjectGetPropertyData(deviceId, &uidAddress, 0, nil, &uidSize, pointer)
        }
        return result == noErr ? uid as String : nil
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
