@preconcurrency import AVFoundation
import Foundation
import OSLog

/// Захват микрофона через AVAudioEngine → 16 kHz Float mono callbacks.
///
/// Голосовая обработка macOS (VPIO) по умолчанию **выключена**: она
/// несовместима с захватом системного звука. VPIO забирает устройство
/// вывода себе под опорный сигнал для эхоподавления, а `SystemAudioCapture`
/// строит на том же устройстве своё aggregate — и оно перестаёт
/// собираться (`AggregateDevice.mm`: не сходятся счётчики каналов).
/// Канал собеседника при этом пропадает молча, запись остаётся
/// одноканальной.
///
/// Замер 2026-08-04: с VPIO вход открывается девятиканальным, downlink DSP
/// падает около сотни раз в секунду, системный tap не стартует ни разу.
/// Без VPIO — один канал, tap поднимается, отказов нет.
///
/// Включается `MEETINGRAFT_VOICE_PROCESSING=1` — для замеров шумоподавления
/// на микрофоне, но тогда системного звука в записи не будет. Переменная,
/// а не настройка: это инструмент сравнения в прогонах, а не выбор
/// пользователя.
final class MicrophoneCapture: AudioTapping {
    private let engine = AVAudioEngine()
    private var downmixer: PCMDownmixer?
    private var onSamples: SamplesHandler?
    private let log = Logger(subsystem: "com.vitvitsky.meetingraft", category: "Microphone")

    /// Голосовая обработка включается только явным запросом.
    static var voiceProcessingEnabled: Bool {
        ProcessInfo.processInfo.environment["MEETINGRAFT_VOICE_PROCESSING"] == "1"
    }

    var isRunning: Bool {
        engine.isRunning
    }

    /// Старт tap на input. `onSamples` вызывается off-main.
    func start(onSamples: @escaping SamplesHandler) throws {
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
        input.installTap(onBus: 0, bufferSize: 2048, format: hwFormat) { [weak self] buffer, when in
            self?.emit(buffer: buffer, when: when)
        }
        try engine.start()
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

    private func emit(buffer: AVAudioPCMBuffer, when: AVAudioTime) {
        guard let downmixer, let onSamples else { return }
        let samples = downmixer.convert(buffer)
        guard !samples.isEmpty else { return }
        onSamples(samples, hostTime(of: when))
    }

    /// Момент записи буфера в тиках общих часов.
    ///
    /// `hostTime` у tap'а на входе валиден всегда, но проверка стоит
    /// дёшево, а её отсутствие — нет: невалидный ноль уехал бы в метку
    /// начала канала и вернул бы ту самую разницу стартов, ради которой
    /// эти часы и появились. Отказ виден в логе, подмена времени
    /// приёма — честная и худшая из двух: она запаздывает на длину
    /// буфера, зато не врёт на секунду.
    private func hostTime(of when: AVAudioTime) -> UInt64 {
        guard when.isHostTimeValid else {
            log.error("у буфера микрофона нет hostTime — метка канала взята по времени приёма")
            return HostClock.system.now()
        }
        return when.hostTime
    }

    enum CaptureError: Error {
        case invalidInputFormat
        case converterUnavailable
    }
}
