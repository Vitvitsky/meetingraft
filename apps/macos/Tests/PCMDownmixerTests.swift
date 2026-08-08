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

    /// В установившемся режиме чанк на входе даёт ровно свою долю на выходе.
    ///
    /// Ожидаемое число не зашито, а посчитано из входа: 4800 кадров на
    /// 48 кГц — это 1600 кадров на 16 кГц, и никакой другой ответ не
    /// является «без потерь». Разгон в начале потока из счёта выпадает:
    /// меряются вторые десять чанков, первые десять отброшены.
    ///
    /// Сравнивать нарезку с одним куском — а такой заход тут был —
    /// нельзя: конвертер придерживает часть входа до следующего чанка,
    /// и величина придержанного зависит от того, как подавали вход
    /// (замер на Маке 2026-08-08: 15738 чанками против 15013 одним
    /// куском). Одинаковой задержки, которая «выпадает из сравнения»,
    /// у двух дорог нет.
    ///
    /// Измерение **одного** вызова когда-то дало вывод «теряется 15%», на
    /// котором держался сброс конвертера после каждого чанка.
    func testSteadyStateKeepsEveryFrame() throws {
        let chunkFrames: AVAudioFrameCount = 4800
        let (format, chunk) = makeBuffer(sampleRate: 48000, channels: 2, frames: chunkFrames)
        let perChunkExpected = Int(
            Double(chunkFrames) * AudioChunkPipeline.targetSampleRate / 48000
        )
        let downmixer = try XCTUnwrap(PCMDownmixer(from: format))

        var perChunk: [Int] = []
        for _ in 0 ..< 20 {
            perChunk.append(downmixer.convert(chunk).count)
        }
        let steady = perChunk.dropFirst(10)

        XCTAssertTrue(
            steady.allSatisfy { $0 > 0 },
            "в установившемся режиме есть пустые вызовы — считать нечего: \(perChunk)"
        )
        XCTAssertEqual(
            steady.reduce(0, +),
            10 * perChunkExpected,
            "кадры теряются на границах чанков: \(perChunk)"
        )
        XCTAssertLessThanOrEqual(
            perChunk[0],
            perChunkExpected,
            "первый чанк не короче установившегося — задержки фильтра нет, "
                + "то есть конвертер всё-таки сбрасывается: \(perChunk)"
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
