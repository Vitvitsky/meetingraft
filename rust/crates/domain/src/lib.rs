//! Доменные модели MeetingRaft.

mod audio;
mod caption;
mod diagnostics;
mod glossary;
mod language;
mod postcall;
mod session;
mod speaker;

pub use audio::AudioChannel;
pub use caption::{CaptionEvent, CaptionPhase};
pub use diagnostics::{SttDiagnostic, SttDiagnosticKind};
pub use glossary::{GlossaryKind, GlossaryScope, GlossaryTerm};
pub use language::{LanguagePolicy, SpeechLanguage};
pub use postcall::{
    Artifact, ArtifactKind, EditOrigin, EditPosition, FinalSegment, FinalTranscript,
    MeetingSummary, SearchHit, SearchHitKind, SegmentEdit, SpeakerSource, TranscriptSegment,
    body_fingerprint, edits_by_position,
};
pub use session::SessionState;
pub use speaker::{KnownVoice, Speaker, StoredVoicePrint};

/// Версия доменного крейта; используется smoke-тестом сборки.
pub const CRATE_VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg(test)]
mod tests {
    use super::*;

    /// Smoke-тест сборки workspace: версия крейта совпадает с манифестом.
    #[test]
    fn crate_version_matches_manifest() {
        assert_eq!(CRATE_VERSION, "0.1.0");
    }

    #[test]
    fn default_language_policy_is_russian_first() {
        let policy = LanguagePolicy::default_v1();
        assert_eq!(policy.primary, SpeechLanguage::Ru);
        assert_eq!(
            policy.allowed,
            vec![SpeechLanguage::Ru, SpeechLanguage::En, SpeechLanguage::Es]
        );
        assert!(policy.is_allowed(SpeechLanguage::Ru));
        assert_eq!(SpeechLanguage::default(), SpeechLanguage::Ru);
        assert_eq!(SpeechLanguage::Ru.code(), "ru");
    }
}
