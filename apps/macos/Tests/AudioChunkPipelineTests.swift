@testable import MeetingRaft
import XCTest

final class AudioChunkPipelineTests: XCTestCase {
    private var framesPerChunk: Int {
        Int(AudioChunkPipeline.targetSampleRate * 0.1)
    }

    func testEmitsChunkEveryHundredMsAt16k() {
        var pipeline = AudioChunkPipeline()
        let samples = Array(repeating: Float(0.5), count: framesPerChunk)
        let chunks = pipeline.push(samples: samples)
        XCTAssertEqual(chunks.count, 1)
        XCTAssertEqual(chunks[0].data.count, framesPerChunk * 2)
    }

    func testBuffersUntilFullChunk() {
        var pipeline = AudioChunkPipeline()
        XCTAssertTrue(pipeline.push(samples: Array(repeating: 0, count: framesPerChunk / 2)).isEmpty)
        let chunks = pipeline.push(samples: Array(repeating: 0, count: framesPerChunk / 2))
        XCTAssertEqual(chunks.count, 1)
    }

    /// Смещения монотонны и не пересекаются, даже когда за один push
    /// выходит несколько чанков — от этого зависит выравнивание каналов.
    func testFrameOffsetsAreMonotonicAndContiguous() {
        var pipeline = AudioChunkPipeline()
        var offsets: [UInt64] = []
        for _ in 0 ..< 3 {
            let batch = pipeline.push(samples: Array(repeating: Float(0.2), count: framesPerChunk * 2))
            XCTAssertEqual(batch.count, 2)
            offsets.append(contentsOf: batch.map(\.startFrame))
        }

        let expected = (0 ..< 6).map { UInt64($0 * framesPerChunk) }
        XCTAssertEqual(offsets, expected)
    }

    /// Метка времени считается от кадров, а не от момента обработки.
    func testTimestampFollowsFrameOffset() {
        var pipeline = AudioChunkPipeline()
        let chunks = pipeline.push(samples: Array(repeating: Float(0.2), count: framesPerChunk * 3))
        XCTAssertEqual(chunks.map { $0.timestampMs() }, [0, 100, 200])
    }

    /// Привязка канала уезжает в метку каждого чанка.
    ///
    /// Без неё оба канала заявляли бы одно и то же время на звук,
    /// записанный с разницей около секунды: счётчик кадров у каждого свой
    /// и оба начинаются с нуля. Именно так на встрече `6CE19EC5` дорожки,
    /// разошедшиеся на 1150 мс, выглядели одновременными.
    func testAnchorShiftsEveryTimestampWithoutTouchingFrames() {
        var pipeline = AudioChunkPipeline()
        pipeline.anchor(startOffsetMs: 1150)
        let chunks = pipeline.push(samples: Array(repeating: Float(0.2), count: framesPerChunk * 3))

        XCTAssertEqual(chunks.count, 3, "вход непуст, иначе проверять нечего")
        XCTAssertEqual(chunks.map { $0.timestampMs() }, [1150, 1250, 1350])
        XCTAssertEqual(
            chunks.map(\.startFrame),
            (0 ..< 3).map { UInt64($0 * framesPerChunk) },
            "счёт кадров внутри канала остаётся от нуля"
        )
    }

    /// Непривязанный канал — это ноль, а не мусор от прошлой записи.
    func testResetDropsAnchor() {
        var pipeline = AudioChunkPipeline()
        pipeline.anchor(startOffsetMs: 1150)
        _ = pipeline.push(samples: Array(repeating: Float(0.2), count: framesPerChunk))
        pipeline.reset()

        let chunks = pipeline.push(samples: Array(repeating: Float(0.2), count: framesPerChunk))
        XCTAssertEqual(chunks.first?.timestampMs(), 0)
    }

    func testResetRewindsFrameCounter() {
        var pipeline = AudioChunkPipeline()
        _ = pipeline.push(samples: Array(repeating: Float(0.2), count: framesPerChunk * 2))
        pipeline.reset()

        let chunks = pipeline.push(samples: Array(repeating: Float(0.2), count: framesPerChunk))
        XCTAssertEqual(chunks.first?.startFrame, 0)
    }

    /// Неполный хвост не выдаётся и не сдвигает счётчик.
    func testPartialTailDoesNotAdvanceCounter() {
        var pipeline = AudioChunkPipeline()
        XCTAssertTrue(pipeline.push(samples: Array(repeating: Float(0.2), count: framesPerChunk - 1)).isEmpty)
        let chunks = pipeline.push(samples: [Float(0.2)])
        XCTAssertEqual(chunks.count, 1)
        XCTAssertEqual(chunks[0].startFrame, 0)
    }
}
