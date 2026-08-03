@preconcurrency import AVFoundation
import Foundation

/// Захват микрофона через AVAudioEngine → 16 kHz Float mono callbacks.
final class MicrophoneCapture: AudioTapping {
    private let engine = AVAudioEngine()
    private var downmixer: PCMDownmixer?
    private var onSamples: (([Float]) -> Void)?

    var isRunning: Bool {
        engine.isRunning
    }

    /// Старт tap на input. `onSamples` вызывается off-main.
    func start(onSamples: @escaping ([Float]) -> Void) throws {
        stop()
        self.onSamples = onSamples

        let input = engine.inputNode
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
