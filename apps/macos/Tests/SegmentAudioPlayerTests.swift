@testable import MeetingRaft
import AVFoundation
import XCTest

@MainActor
final class SegmentAudioPlayerTests: XCTestCase {
    /// i16 little-endian разбирается в кадры без сдвига и потерь.
    func testBufferKeepsFrameCountAndScale() {
        let samples: [Int16] = [0, 1, -1, 32767]
        var bytes: [UInt8] = []
        for sample in samples {
            bytes.append(UInt8(truncatingIfNeeded: sample))
            bytes.append(UInt8(truncatingIfNeeded: sample >> 8))
        }
        let fragment = FfiAudioFragment(pcm: Data(bytes), sampleRate: 16000, durationMs: 1)

        let buffer = SegmentAudioPlayer.buffer(from: fragment)

        XCTAssertEqual(buffer?.frameLength, 4)
        XCTAssertEqual(buffer?.format.sampleRate, 16000)
        XCTAssertEqual(buffer?.floatChannelData?[0][3] ?? 0, 1.0, accuracy: 0.001)
        XCTAssertEqual(buffer?.floatChannelData?[0][2] ?? 0, -0.00003, accuracy: 0.0001)
    }

    /// `sampleRate == 0` — ответ ядра «записи здесь нет», а не сбой.
    /// Кнопка воспроизведения на это не показывается вовсе.
    func testEmptyFragmentYieldsNoBuffer() {
        let fragment = FfiAudioFragment(pcm: Data(), sampleRate: 0, durationMs: 0)
        XCTAssertNil(SegmentAudioPlayer.buffer(from: fragment))
    }

    /// Обрезанный хвост не должен ронять разбор.
    func testOddByteCountIsTruncatedNotCrashed() {
        let fragment = FfiAudioFragment(pcm: Data([0, 0, 5]), sampleRate: 16000, durationMs: 1)
        XCTAssertEqual(SegmentAudioPlayer.buffer(from: fragment)?.frameLength, 1)
    }
}
