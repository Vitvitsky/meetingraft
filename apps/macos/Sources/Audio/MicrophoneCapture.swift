@preconcurrency import AVFoundation
import Foundation

/// Захват микрофона через AVAudioEngine → 16 kHz Float mono callbacks.
final class MicrophoneCapture {
    private let engine = AVAudioEngine()
    private var converter: AVAudioConverter?
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

        let targetFormat = AVAudioFormat(
            commonFormat: .pcmFormatFloat32,
            sampleRate: AudioChunkPipeline.targetSampleRate,
            channels: 1,
            interleaved: false
        )!
        converter = AVAudioConverter(from: hwFormat, to: targetFormat)
        guard converter != nil else {
            throw CaptureError.converterUnavailable
        }

        engine.prepare()
        input.installTap(onBus: 0, bufferSize: 2048, format: hwFormat) { [weak self] buffer, _ in
            self?.convert(buffer: buffer, targetFormat: targetFormat)
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
        converter = nil
        onSamples = nil
    }

    private func convert(buffer: AVAudioPCMBuffer, targetFormat: AVAudioFormat) {
        guard let converter, let onSamples, buffer.frameLength > 0 else { return }

        let ratio = targetFormat.sampleRate / buffer.format.sampleRate
        let capacity = AVAudioFrameCount(Double(buffer.frameLength) * ratio) + 64
        guard let out = AVAudioPCMBuffer(pcmFormat: targetFormat, frameCapacity: capacity) else { return }

        // Конвертер должен получить буфер ровно один раз, иначе out.frameLength == 0.
        var consumed = false
        var error: NSError?
        converter.convert(to: out, error: &error) { _, status in
            if consumed {
                status.pointee = .noDataNow
                return nil
            }
            consumed = true
            status.pointee = .haveData
            return buffer
        }

        guard error == nil, out.frameLength > 0, let channel = out.floatChannelData?[0] else { return }
        let samples = Array(UnsafeBufferPointer(start: channel, count: Int(out.frameLength)))
        onSamples(samples)
    }

    enum CaptureError: Error {
        case invalidInputFormat
        case converterUnavailable
    }
}
