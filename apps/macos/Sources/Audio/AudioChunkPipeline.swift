import Foundation

/// Готовый чанк и его позиция в потоке канала.
struct AudioChunk: Equatable {
    /// PCM i16 LE.
    let data: Data
    /// Номер первого кадра чанка от начала записи канала.
    let startFrame: UInt64

    /// Смещение от начала записи канала, мс.
    func timestampMs(sampleRate: Double = AudioChunkPipeline.targetSampleRate) -> UInt64 {
        startFrame * 1000 / UInt64(sampleRate)
    }
}

/// Упаковка Float32 mono → PCM i16 LE чанками фиксированной длительности.
///
/// Позиция чанка считается по счётчику кадров, а не по системным часам:
/// выравнивание mic и system каналов (ADR-004) требует меток, которые
/// соответствуют самому звуку, а не моменту его обработки.
struct AudioChunkPipeline {
    /// Целевой sample rate для STT (ADR-005 / Phase 4).
    static let targetSampleRate: Double = 16000
    /// Длительность чанка, мс.
    static let chunkDurationMs: Int = 100

    private let framesPerChunk: Int
    private var pending: [Float] = []
    private var nextFrame: UInt64 = 0

    init(sampleRate: Double = AudioChunkPipeline.targetSampleRate) {
        framesPerChunk = Int(sampleRate * Double(Self.chunkDurationMs) / 1000.0)
    }

    /// Добавить сэмплы; вернуть готовые чанки со смещением каждого.
    mutating func push(samples: [Float]) -> [AudioChunk] {
        pending.append(contentsOf: samples)
        var out: [AudioChunk] = []
        while pending.count >= framesPerChunk {
            let slice = Array(pending.prefix(framesPerChunk))
            pending.removeFirst(framesPerChunk)
            out.append(AudioChunk(data: Self.encodeInt16LE(slice), startFrame: nextFrame))
            nextFrame += UInt64(framesPerChunk)
        }
        return out
    }

    /// Сбросить хвост и счётчик кадров (новая запись).
    mutating func reset() {
        pending.removeAll(keepingCapacity: true)
        nextFrame = 0
    }

    static func encodeInt16LE(_ samples: [Float]) -> Data {
        var data = Data(capacity: samples.count * 2)
        for sample in samples {
            let clipped = max(-1.0, min(1.0, sample))
            var value = Int16(clipped * Float(Int16.max))
            withUnsafeBytes(of: &value) { data.append(contentsOf: $0) }
        }
        return data
    }
}
