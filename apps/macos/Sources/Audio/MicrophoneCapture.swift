@preconcurrency import AVFoundation
import CoreAudio
import Foundation

/// Захват микрофона через AVAudioEngine → 16 kHz Float mono callbacks.
///
/// Включена голосовая обработка macOS (VPIO): подавление шума,
/// автоусиление и эхоподавление на выделенном железе. Причины все три
/// измеренные:
///
/// - городской шум за окном пробивал гейт речи и заставлял Whisper
///   работать непрерывно (Epic 18, замер 2026-08-04);
/// - тихий собеседник из системного канала не дотягивал до порога;
/// - без наушников голос из динамиков попадает в микрофонный канал и
///   ломает атрибуцию по каналам (ADR-012) — оба канала оказываются с
///   собеседником.
///
/// Системного звука это не касается: он идёт отдельным tap'ом и приходит
/// уже чистым цифровым потоком, обрабатывать его нечем и незачем.
///
/// Отключается `MEETINGRAFT_VOICE_PROCESSING=0` — обработка меняет
/// сигнал, и её влияние на точность распознавания надо мерить, а не
/// предполагать. Переменная, а не настройка: сравнивать надо в замерах,
/// а не в интерфейсе.
final class MicrophoneCapture: AudioTapping {
    private let engine = AVAudioEngine()
    private var downmixer: PCMDownmixer?
    private var onSamples: (([Float]) -> Void)?

    /// Голосовая обработка включена, если её не выключили явно.
    static var voiceProcessingEnabled: Bool {
        ProcessInfo.processInfo.environment["MEETINGRAFT_VOICE_PROCESSING"] != "0"
    }

    var isRunning: Bool {
        engine.isRunning
    }

    /// Старт tap на input. `onSamples` вызывается off-main.
    func start(onSamples: @escaping ([Float]) -> Void) throws {
        stop()
        self.onSamples = onSamples

        let input = engine.inputNode
        // Включать до чтения формата: VPIO меняет формат входа, и
        // прочитанный заранее оказался бы не тем, под который ставится tap.
        if Self.voiceProcessingEnabled {
            do {
                try input.setVoiceProcessingEnabled(true)
            } catch {
                // Не на всех устройствах VPIO доступен. Отказ здесь — не
                // повод не записать встречу.
                NSLog("MeetingRaft: голосовая обработка недоступна (\(error))")
            }
        }
        NSLog("MeetingRaft/diag: VPIO=\(input.isVoiceProcessingEnabled)")
        // nil format = hardware format; иначе -10877 / пустой stream.
        let hwFormat = input.inputFormat(forBus: 0)
        guard hwFormat.sampleRate > 0, hwFormat.channelCount > 0 else {
            throw CaptureError.invalidInputFormat
        }

        guard let downmixer = PCMDownmixer(from: hwFormat) else {
            throw CaptureError.converterUnavailable
        }
        self.downmixer = downmixer

        engine.prepare()
        input.installTap(onBus: 0, bufferSize: 2048, format: hwFormat) { [weak self] buffer, _ in
            self?.emit(buffer: buffer)
        }
        try engine.start()
        NSLog("MeetingRaft/diag: движок микрофона запущен, формат входа \(hwFormat)")
        NSLog("MeetingRaft/diag: устройство входа \(Self.currentInputDeviceDescription(of: input))")
    }

    /// Какое устройство реально открыл движок: имя, UID, число входных
    /// каналов. Число каналов у `inputFormat` не сходится ни с одним
    /// устройством в системе, и гадать по нему бесполезно.
    private static func currentInputDeviceDescription(of input: AVAudioInputNode) -> String {
        guard let unit = input.audioUnit else { return "audioUnit недоступен" }
        var deviceId = AudioDeviceID(kAudioObjectUnknown)
        var size = UInt32(MemoryLayout<AudioDeviceID>.size)
        let status = AudioUnitGetProperty(
            unit,
            kAudioOutputUnitProperty_CurrentDevice,
            kAudioUnitScope_Global,
            0,
            &deviceId,
            &size
        )
        guard status == noErr, deviceId != kAudioObjectUnknown else {
            return "не прочиталось (status \(status))"
        }
        let name = stringProperty(kAudioObjectPropertyName, of: deviceId) ?? "без имени"
        let uid = stringProperty(kAudioDevicePropertyDeviceUID, of: deviceId) ?? "без UID"
        return "id \(deviceId), «\(name)», UID \(uid), входных каналов \(inputChannelCount(of: deviceId))"
    }

    private static func stringProperty(
        _ selector: AudioObjectPropertySelector,
        of deviceId: AudioObjectID
    ) -> String? {
        var address = AudioObjectPropertyAddress(
            mSelector: selector,
            mScope: kAudioObjectPropertyScopeGlobal,
            mElement: kAudioObjectPropertyElementMain
        )
        var value: CFString = "" as CFString
        var size = UInt32(MemoryLayout<CFString>.size)
        let result = withUnsafeMutablePointer(to: &value) { pointer in
            AudioObjectGetPropertyData(deviceId, &address, 0, nil, &size, pointer)
        }
        return result == noErr ? value as String : nil
    }

    /// Сумма каналов по всем входным потокам устройства.
    private static func inputChannelCount(of deviceId: AudioObjectID) -> Int {
        var address = AudioObjectPropertyAddress(
            mSelector: kAudioDevicePropertyStreamConfiguration,
            mScope: kAudioObjectPropertyScopeInput,
            mElement: kAudioObjectPropertyElementMain
        )
        var size = UInt32(0)
        guard AudioObjectGetPropertyDataSize(deviceId, &address, 0, nil, &size) == noErr, size > 0 else {
            return -1
        }
        let raw = UnsafeMutableRawPointer.allocate(byteCount: Int(size), alignment: 16)
        defer { raw.deallocate() }
        guard AudioObjectGetPropertyData(deviceId, &address, 0, nil, &size, raw) == noErr else {
            return -1
        }
        let list = UnsafeMutableAudioBufferListPointer(
            raw.assumingMemoryBound(to: AudioBufferList.self)
        )
        return list.reduce(0) { $0 + Int($1.mNumberChannels) }
    }

    func stop() {
        if engine.inputNode.numberOfInputs >= 0 {
            engine.inputNode.removeTap(onBus: 0)
        }
        if engine.isRunning {
            engine.stop()
        }
        downmixer = nil
        onSamples = nil
    }

    private func emit(buffer: AVAudioPCMBuffer) {
        guard let downmixer, let onSamples else { return }
        let samples = downmixer.convert(buffer)
        guard !samples.isEmpty else { return }
        onSamples(samples)
    }

    enum CaptureError: Error {
        case invalidInputFormat
        case converterUnavailable
    }
}
