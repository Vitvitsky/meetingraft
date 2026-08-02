import Foundation
import Observation

/// Presentation model экрана live captions.
@Observable
@MainActor
final class LiveCaptionsViewModel {
    private(set) var lines: [CaptionLine] = []
    private let stream: CaptionStreaming

    init(stream: CaptionStreaming = FakeCaptionStream()) {
        self.stream = stream
    }

    func start() {
        lines = []
        stream.start { [weak self] line in
            self?.append(line)
        }
    }

    func stop() {
        stream.stop()
    }

    private func append(_ line: CaptionLine) {
        if line.phase == .final, let last = lines.last, last.phase == .partial {
            lines[lines.count - 1] = line
        } else {
            lines.append(line)
        }
    }
}
