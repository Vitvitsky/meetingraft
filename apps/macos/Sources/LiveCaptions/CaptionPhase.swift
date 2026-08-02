import Foundation

/// Визуальная фаза caption-события (live vs committed).
enum CaptionPhase: Equatable, Sendable {
    case partial
    case final
}
