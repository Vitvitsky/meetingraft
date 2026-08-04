//! Контракт STT-движка.

use domain::{CaptionEvent, LanguagePolicy, SttDiagnostic};

/// On-device / swappable STT (Whisper сейчас, cloud позже).
pub trait SttEngine: Send {
    fn set_language_policy(&mut self, policy: LanguagePolicy);

    /// Установить подсказку с терминами для движков, которые её поддерживают.
    fn set_initial_prompt(&mut self, _prompt: &str) {}

    /// Принять PCM i16 mono @ sample_rate; вернуть новые caption events.
    fn push_pcm(&mut self, pcm: &[i16], sample_rate: u32) -> Vec<CaptionEvent>;

    /// Сбросить хвост окна (конец сегмента / stop).
    fn flush(&mut self) -> Vec<CaptionEvent>;

    /// Забрать накопленные записи о решениях движка.
    ///
    /// Движок ничего не пишет на диск сам: он лишь рассказывает, что
    /// сделал. Куда это девать — решает слой выше, который и владеет
    /// каталогом данных.
    ///
    /// По умолчанию пусто: движку, который ничего не выбрасывает,
    /// объясняться не в чем.
    fn take_diagnostics(&mut self) -> Vec<SttDiagnostic> {
        Vec::new()
    }
}
