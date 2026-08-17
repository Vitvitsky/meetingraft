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

/// Календарная дата UTC из метки времени: `2026-08-14`.
///
/// Живёт в домене, а не в двух местах: считать её умеют и граница
/// UniFFI (подпись follow-up), и приборы (список встреч). Алгоритм
/// «days from civil», переписанный дважды, разъезжается на високосных —
/// а проверить это некому, потому что оба места пишут одно и то же
/// число разными путями.
pub fn utc_date_label(timestamp_ms: u64) -> String {
    let days_since_epoch = (timestamp_ms / 86_400_000) as i64;
    let shifted_days = days_since_epoch + 719_468;
    let era = shifted_days / 146_097;
    let day_of_era = shifted_days - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let mut year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_part = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_part + 2) / 5 + 1;
    let month = month_part + if month_part < 10 { 3 } else { -9 };
    year += i64::from(month <= 2);

    format!("{year:04}-{month:02}-{day:02}")
}

#[cfg(test)]
mod date_tests {
    use super::utc_date_label;

    #[test]
    fn formats_a_calendar_date() {
        assert_eq!(utc_date_label(0), "1970-01-01");
        assert_eq!(utc_date_label(1_785_628_800_000), "2026-08-02");
    }

    #[test]
    fn a_leap_day_stays_a_leap_day() {
        // 2024-02-29: то самое место, где переписанный алгоритм врёт.
        assert_eq!(utc_date_label(1_709_164_800_000), "2024-02-29");
    }
}
