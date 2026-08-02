import Foundation

/// Упаковка Float32 mono → PCM i16 LE чанками фиксированной длительности.
struct AudioChunkPipeline {
    /// Целевой sample rate для STT (ADR-005 / Phase 4).
    static let targetSampleRate: Double = 16000
    /// Длительность чанка, мс.
    static let chunkDurationMs: Int = 100

    private let framesPerChunk: Int
    private var pending: [Float] = []

    init(sampleRate: Double = AudioChunkPipeline.targetSampleRate) {
        framesPerChunk = Int(sampleRate * Double(Self.chunkDurationMs) / 1000.0)
    }

    /// Добавить сэмплы; вернуть готовые чанки (каждый — i16 LE bytes).
    mutating func push(samples: [Float]) -> [Data] {
        pending.append(contentsOf: samples)
        var out: [Data] = []
        while pending.count >= framesPerChunk {
            let slice = Array(pending.prefix(framesPerChunk))
            pending.removeFirst(framesPerChunk)
            out.append(Self.encodeInt16LE(slice))
        }
        return out
    }

    /// Сбросить хвост без паддинга.
    mutating func reset() {
        pending.removeAll(keepingCapacity: true)
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
