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

    /// 48 kHz стерео → 16 kHz моно: кадров втрое меньше.
    func testDownmixesStereo48kToMono16k() throws {
        let (format, buffer) = makeBuffer(sampleRate: 48000, channels: 2, frames: 4800)
        let downmixer = try XCTUnwrap(PCMDownmixer(from: format))

        let samples = downmixer.convert(buffer)

        XCTAssertFalse(samples.isEmpty)
        // Ресемплер даёт небольшой разброс на границах окна.
        XCTAssertEqual(Double(samples.count), 1600, accuracy: 64)
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
