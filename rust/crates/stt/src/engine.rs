//! Контракт STT-движка.

use domain::{CaptionEvent, LanguagePolicy};

/// On-device / swappable STT (Whisper сейчас, cloud позже).
pub trait SttEngine: Send {
    fn set_language_policy(&mut self, policy: LanguagePolicy);

    /// Принять PCM i16 mono @ sample_rate; вернуть новые caption events.
    fn push_pcm(&mut self, pcm: &[i16], sample_rate: u32) -> Vec<CaptionEvent>;

    /// Сбросить хвост окна (конец сегмента / stop).
    fn flush(&mut self) -> Vec<CaptionEvent>;
}
