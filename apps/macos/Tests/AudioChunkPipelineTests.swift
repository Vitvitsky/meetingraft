@testable import MeetingRaft
import XCTest

final class AudioChunkPipelineTests: XCTestCase {
    func testEmitsChunkEveryHundredMsAt16k() {
        var pipeline = AudioChunkPipeline()
        let frames = Int(AudioChunkPipeline.targetSampleRate * 0.1)
        let samples = Array(repeating: Float(0.5), count: frames)
        let chunks = pipeline.push(samples: samples)
        XCTAssertEqual(chunks.count, 1)
        XCTAssertEqual(chunks[0].count, frames * 2)
    }

    func testBuffersUntilFullChunk() {
        var pipeline = AudioChunkPipeline()
        let frames = Int(AudioChunkPipeline.targetSampleRate * 0.1)
        XCTAssertTrue(pipeline.push(samples: Array(repeating: 0, count: frames / 2)).isEmpty)
        let chunks = pipeline.push(samples: Array(repeating: 0, count: frames / 2))
        XCTAssertEqual(chunks.count, 1)
    }
}
