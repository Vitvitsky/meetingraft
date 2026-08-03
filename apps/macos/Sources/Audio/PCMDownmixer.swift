@preconcurrency import AVFoundation
import Foundation

/// Приведение произвольного аппаратного формата к 16 kHz mono Float32.
///
/// Общий код для микрофона (`AVAudioEngine`) и системного tap (Core Audio):
/// оба отдают буферы в формате устройства — обычно 44.1/48 kHz и стерео,
/// а STT ждёт моно 16 kHz (ADR-005).
final class PCMDownmixer {
    private let converter: AVAudioConverter
    private let targetFormat: AVAudioFormat

    /// `nil`, если формат источника непригоден или конвертер недоступен.
    init?(from sourceFormat: AVAudioFormat) {
        guard sourceFormat.sampleRate > 0, sourceFormat.channelCount > 0 else {
            return nil
        }
        guard
            let target = AVAudioFormat(
                commonFormat: .pcmFormatFloat32,
                sampleRate: AudioChunkPipeline.targetSampleRate,
                channels: 1,
                interleaved: false
            ),
            let converter = AVAudioConverter(from: sourceFormat, to: target)
        else {
            return nil
        }
        self.converter = converter
        targetFormat = target
    }

    /// Пустой массив, если конвертация не дала кадров.
    func convert(_ buffer: AVAudioPCMBuffer) -> [Float] {
        guard buffer.frameLength > 0 else { return [] }

        let ratio = targetFormat.sampleRate / buffer.format.sampleRate
        let capacity = AVAudioFrameCount(Double(buffer.frameLength) * ratio) + 64
        guard let out = AVAudioPCMBuffer(pcmFormat: targetFormat, frameCapacity: capacity) else {
            return []
        }

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

        guard error == nil, out.frameLength > 0, let channel = out.floatChannelData?[0] else {
            return []
        }
        return Array(UnsafeBufferPointer(start: channel, count: Int(out.frameLength)))
    }
}
