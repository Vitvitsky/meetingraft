import AVFoundation
@testable import MeetingRaft
import XCTest

final class PCMDownmixerTests: XCTestCase {
    private func makeBuffer(
        sampleRate: Double,
        channels: AVAudioChannelCount,
        frames: AVAudioFrameCount,
        level: Float = 0.25
    ) -> (AVAudioFormat, AVAudioPCMBuffer) {
        let format = AVAudioFormat(
            commonFormat: .pcmFormatFloat32,
            sampleRate: sampleRate,
            channels: channels,
            interleaved: false
        )!
        let buffer = AVAudioPCMBuffer(pcmFormat: format, frameCapacity: frames)!
        buffer.frameLength = frames
        for channel in 0 ..< Int(channels) {
            let data = buffer.floatChannelData![channel]
            for frame in 0 ..< Int(frames) {
                data[frame] = frame.isMultiple(of: 2) ? level : -level
            }
        }
        return (format, buffer)
    }

    /// Буфер, залитый одним значением: самый простой сигнал для фильтра.
    private func makeConstantBuffer(
        sampleRate: Double,
        frames: AVAudioFrameCount,
        level: Float
    ) -> (AVAudioFormat, AVAudioPCMBuffer) {
        let format = AVAudioFormat(
            commonFormat: .pcmFormatFloat32,
            sampleRate: sampleRate,
            channels: 1,
            interleaved: false
        )!
        let buffer = AVAudioPCMBuffer(pcmFormat: format, frameCapacity: frames)!
        buffer.frameLength = frames
        let data = buffer.floatChannelData![0]
        for frame in 0 ..< Int(frames) {
            data[frame] = level
        }
        return (format, buffer)
    }

    /// На длинном потоке отставание не растёт и не превышает чанка.
    ///
    /// Меряется не длина результата, а **отставание**: сколько кадров из
    /// поданных ещё не вышло. Ожидаемая длина считается из входа (4800
    /// кадров на 48 кГц — это 1600 на 16 кГц), константа тут одна —
    /// граница в один чанк, и она взята из требования к задержке: больше
    /// 100 мс внутри конвертера живой путь не терпит.
    ///
    /// Отставание не обязано быть нулевым и не обязано быть постоянным:
    /// конвертер отдаёт блоками (замер на Маке 2026-08-08: по 4096
    /// входных кадров), поэтому в любом фиксированном окне сумма гуляет
    /// в обе стороны. Существенно, что **накопленное** отставание не
    /// уходит в рост: сброс состояния на каждой границе, ради которого
    /// тест и существует, дал бы потерю на каждом чанке, и на сотне
    /// чанков граница была бы снесена с запасом.
    ///
    /// Прошлые заходы: три упавших теста подряд, каждый утверждал число,
    /// которого автор измерить не мог, — сумму с допуском на глаз, потом
    /// установившийся режим с допуском 16, потом равенство нарезки
    /// одному куску (посылка «задержка одинакова» оказалась ложной:
    /// 15738 против 15013).
    func testLongStreamHoldsBackLessThanOneChunk() throws {
        let chunkFrames: AVAudioFrameCount = 4800
        let chunks = 100
        let (format, chunk) = makeBuffer(sampleRate: 48000, channels: 2, frames: chunkFrames)
        let perChunkExpected = Int(
            Double(chunkFrames) * AudioChunkPipeline.targetSampleRate / 48000
        )
        let downmixer = try XCTUnwrap(PCMDownmixer(from: format))

        var total = 0
        var worst = 0
        var worstAt = 0
        for index in 1 ... chunks {
            total += downmixer.convert(chunk).count
            let behind = index * perChunkExpected - total
            XCTAssertGreaterThanOrEqual(
                behind,
                0,
                "кадров вышло больше, чем было подано, на чанке \(index): \(total)"
            )
            if behind > worst {
                worst = behind
                worstAt = index
            }
        }

        XCTAssertGreaterThan(total, 0, "поток пуст — мерить нечего")
        XCTAssertLessThan(
            worst,
            perChunkExpected,
            "конвертер придерживает больше чанка: \(worst) кадров на чанке \(worstAt) "
                + "из \(chunks); всего вышло \(total) из \(chunks * perChunkExpected)"
        )
    }

    /// Поток без разрывов на границах чанков.
    ///
    /// Тест, ради которого работа существует. Постоянный уровень —
    /// самое простое, что фильтр обязан пропускать без изменений; если
    /// состояние ресемплера сбрасывать на каждом чанке, он каждый раз
    /// разгоняется заново и на границах появляются провалы.
    ///
    /// Проверяется разброс, а не абсолютное значение: так тест не зависит
    /// от того, какое усиление даёт конвертер.
    func testKeepsResamplerStateAcrossChunks() throws {
        let (format, buffer) = makeConstantBuffer(sampleRate: 48000, frames: 4800, level: 0.5)
        let downmixer = try XCTUnwrap(PCMDownmixer(from: format))

        var stream: [Float] = []
        for _ in 0 ..< 10 {
            stream.append(contentsOf: downmixer.convert(buffer))
        }

        // Первый чанк отбрасывается: фильтр в начале потока разгоняется
        // законно, и это не разрыв.
        let steady = Array(stream.dropFirst(1600))
        XCTAssertGreaterThan(steady.count, 8000, "потока не набралось — мерить нечего")

        let spread = (steady.max() ?? 0) - (steady.min() ?? 0)
        XCTAssertLessThan(
            spread,
            0.01,
            "уровень гуляет на \(spread) — состояние ресемплера теряется между чанками"
        )
    }

    /// Тот же sample rate и один канал — конвертация не теряет кадры.
    func testKeepsFrameCountWhenFormatsMatch() throws {
        let (format, buffer) = makeBuffer(sampleRate: 16000, channels: 1, frames: 1600)
        let downmixer = try XCTUnwrap(PCMDownmixer(from: format))

        let samples = downmixer.convert(buffer)

        XCTAssertEqual(samples.count, 1600)
    }

    func testEmptyBufferProducesNoSamples() throws {
        let (format, buffer) = makeBuffer(sampleRate: 44100, channels: 2, frames: 1024)
        buffer.frameLength = 0
        let downmixer = try XCTUnwrap(PCMDownmixer(from: format))

        XCTAssertEqual(downmixer.convert(buffer).count, 0)
    }
}
