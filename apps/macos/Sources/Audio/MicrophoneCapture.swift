@preconcurrency import AVFoundation
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
