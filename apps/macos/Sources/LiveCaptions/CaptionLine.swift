import Foundation

/// Кто произнёс строку: канал захвата в терминах пользователя (ADR-009).
enum CaptionSpeaker: String, Equatable, Sendable {
    case you = "mic"
    case others = "system"

    /// Неизвестный код считается микрофоном — так же, как в Rust.
    init(channelCode: String) {
        self = CaptionSpeaker(rawValue: channelCode) ?? .you
    }

    var label: String {
        switch self {
        case .you: String(localized: "You")
        case .others: String(localized: "Others")
        }
    }
}

/// Presentation-модель одной строки субтитров.
struct CaptionLine: Identifiable, Equatable, Sendable {
    let id: UUID
    let text: String
    let phase: CaptionPhase
    let speaker: CaptionSpeaker

    init(id: UUID = UUID(), text: String, phase: CaptionPhase, speaker: CaptionSpeaker = .you) {
        self.id = id
        self.text = text
        self.phase = phase
        self.speaker = speaker
    }

    /// Единственное место разбора события из Rust — чтобы live-путь и
    /// demo-путь не разъезжались в трактовке полей.
    init(event: FfiCaptionEvent) {
        let phase: CaptionPhase = switch event.phase {
        case .partial: .partial
        case .final: .final
        }
        self.init(
            id: UUID(uuidString: event.id) ?? UUID(),
            text: event.text,
            phase: phase,
            speaker: CaptionSpeaker(channelCode: event.channel)
        )
    }
}
