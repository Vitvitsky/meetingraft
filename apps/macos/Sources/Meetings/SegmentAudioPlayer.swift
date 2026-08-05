import AVFoundation
import Observation

/// Проигрывание фрагмента реплики.
///
/// Отдельный объект, а не метод presentation model: держит
/// `AVAudioEngine`, который обязан пережить перерисовку списка. Внутри
/// модели движок дёргался бы на каждое обновление строки.
@Observable
@MainActor
final class SegmentAudioPlayer {
    private(set) var isPlaying = false

    private let engine = AVAudioEngine()
    private let node = AVAudioPlayerNode()
    private var isAttached = false

    /// `nil`, когда играть нечего: `sampleRate == 0` — это ответ ядра
    /// «записи за диапазон нет», а не сбой.
    static func buffer(from fragment: FfiAudioFragment) -> AVAudioPCMBuffer? {
        guard fragment.sampleRate > 0 else { return nil }
        let frames = fragment.pcm.count / 2
        guard frames > 0,
              let format = AVAudioFormat(
                  commonFormat: .pcmFormatFloat32,
                  sampleRate: Double(fragment.sampleRate),
                  channels: 1,
                  interleaved: false
              ),
              let buffer = AVAudioPCMBuffer(
                  pcmFormat: format,
                  frameCapacity: AVAudioFrameCount(frames)
              )
        else { return nil }

        buffer.frameLength = AVAudioFrameCount(frames)
        guard let target = buffer.floatChannelData?[0] else { return nil }
        fragment.pcm.withUnsafeBytes { raw in
            for index in 0 ..< frames {
                let low = UInt16(raw[index * 2])
                let high = UInt16(raw[index * 2 + 1])
                let sample = Int16(bitPattern: low | (high << 8))
                target[index] = Float(sample) / Float(Int16.max)
            }
        }
        return buffer
    }

    func play(fragment: FfiAudioFragment) {
        guard let buffer = Self.buffer(from: fragment) else { return }
        stop()
        if !isAttached {
            engine.attach(node)
            isAttached = true
        }
        engine.connect(node, to: engine.mainMixerNode, format: buffer.format)
        do {
            try engine.start()
        } catch {
            return
        }
        isPlaying = true
        node.scheduleBuffer(buffer, completionCallbackType: .dataPlayedBack) { [weak self] _ in
            Task { @MainActor in self?.stop() }
        }
        node.play()
    }

    func stop() {
        if node.isPlaying {
            node.stop()
        }
        if engine.isRunning {
            engine.stop()
        }
        isPlaying = false
    }
}
