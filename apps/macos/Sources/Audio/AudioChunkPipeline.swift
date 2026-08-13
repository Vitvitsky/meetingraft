import Foundation

/// Готовый чанк и его позиция в потоке канала.
struct AudioChunk: Equatable {
    /// PCM i16 LE.
    let data: Data
    /// Номер первого кадра чанка от начала записи канала.
    let startFrame: UInt64
    /// Насколько начало канала позже начала записи, мс.
    ///
    /// Ноль означает «канал начался вместе с записью», а не «начало
    /// неизвестно»: неизвестного здесь быть не должно — координатор
    /// привязывает канал до первого чанка.
    let startOffsetMs: UInt64

    /// Смещение от начала **записи**, мс — общее время обоих каналов.
    func timestampMs(sampleRate: Double = AudioChunkPipeline.targetSampleRate) -> UInt64 {
        startOffsetMs + startFrame * 1000 / UInt64(sampleRate)
    }
}

/// Упаковка Float32 mono → PCM i16 LE чанками фиксированной длительности.
///
/// Позиция чанка внутри канала считается по счётчику кадров, а не по
/// системным часам: выравнивание mic и system каналов (ADR-004) требует
/// меток, которые соответствуют самому звуку, а не моменту его обработки.
///
/// Но у счётчика кадров нет общего начала у двух каналов, и раньше оба
/// начинали с нуля, хотя стартуют с разницей около секунды. Поэтому к
/// кадрам прибавляется `startOffsetMs` — сдвиг начала канала от начала
/// записи, снятый с общих часов (`HostClock`) один раз, по первому буферу.
struct AudioChunkPipeline {
    /// Целевой sample rate для STT (ADR-005 / Phase 4).
    static let targetSampleRate: Double = 16000
    /// Длительность чанка, мс.
    static let chunkDurationMs: Int = 100

    private let framesPerChunk: Int
    private var pending: [Float] = []
    private var nextFrame: UInt64 = 0
    private var startOffsetMs: UInt64 = 0

    init(sampleRate: Double = AudioChunkPipeline.targetSampleRate) {
        framesPerChunk = Int(sampleRate * Double(Self.chunkDurationMs) / 1000.0)
    }

    /// Привязать канал к общему времени: на сколько его первый буфер
    /// позже начала записи.
    ///
    /// Зовётся один раз на запись, до первого `push`. Дальше метки идут
    /// от кадров: часы внутри канала точнее не сделают, а дрожать будут.
    mutating func anchor(startOffsetMs: UInt64) {
        self.startOffsetMs = startOffsetMs
    }

    /// Добавить сэмплы; вернуть готовые чанки со смещением каждого.
    mutating func push(samples: [Float]) -> [AudioChunk] {
        pending.append(contentsOf: samples)
        var out: [AudioChunk] = []
        while pending.count >= framesPerChunk {
            let slice = Array(pending.prefix(framesPerChunk))
            pending.removeFirst(framesPerChunk)
            out.append(AudioChunk(
                data: Self.encodeInt16LE(slice),
                startFrame: nextFrame,
                startOffsetMs: startOffsetMs
            ))
            nextFrame += UInt64(framesPerChunk)
        }
        return out
    }

    /// Сбросить хвост, счётчик кадров и привязку (новая запись).
    mutating func reset() {
        pending.removeAll(keepingCapacity: true)
        nextFrame = 0
        startOffsetMs = 0
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
