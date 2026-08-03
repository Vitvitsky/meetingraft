import Foundation

/// Идентификатор on-device Whisper ggml-модели (parity с Rust `whisper_filename_for_id`).
enum WhisperModelId: String, CaseIterable, Identifiable, Sendable {
    case auto
    case base
    case small
    case largeV3Turbo = "large-v3-turbo"

    var id: String {
        rawValue
    }

    /// Имя файла в `models/`; `auto` не скачивается.
    var filename: String? {
        switch self {
        case .auto:
            nil
        case .base:
            "ggml-base.bin"
        case .small:
            "ggml-small.bin"
        case .largeV3Turbo:
            "ggml-large-v3-turbo.bin"
        }
    }

    /// HF resolve URL (`ggerganov/whisper.cpp`, ветка `main`).
    var downloadURL: URL? {
        guard let filename else { return nil }
        return URL(string: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/\(filename)")
    }

    var displayName: String {
        switch self {
        case .auto:
            "Auto (best installed)"
        case .base:
            "Whisper base"
        case .small:
            "Whisper small"
        case .largeV3Turbo:
            "Whisper large-v3-turbo"
        }
    }
}
