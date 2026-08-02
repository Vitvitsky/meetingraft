import Foundation

/// Presentation-модель одной строки субтитров.
struct CaptionLine: Identifiable, Equatable, Sendable {
    let id: UUID
    let text: String
    let phase: CaptionPhase

    init(id: UUID = UUID(), text: String, phase: CaptionPhase) {
        self.id = id
        self.text = text
        self.phase = phase
    }
}
