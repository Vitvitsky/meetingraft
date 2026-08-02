//! Трейт перевода — зеркало `SttEngine` для translation stream.

use domain::SpeechLanguage;

/// Ошибки translate path (не паникуем через UniFFI).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TranslateError {
    Disabled,
    NeedsHost,
    NotConfigured(&'static str),
    Failed(String),
}

impl std::fmt::Display for TranslateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Disabled => write!(f, "translation disabled"),
            Self::NeedsHost => write!(f, "translation requires host bridge"),
            Self::NotConfigured(msg) => write!(f, "translation not configured: {msg}"),
            Self::Failed(msg) => write!(f, "translation failed: {msg}"),
        }
    }
}

/// Синхронный перевод одной caption-строки.
pub trait TranslateEngine: Send {
    fn translate(
        &self,
        text: &str,
        source: SpeechLanguage,
        target: SpeechLanguage,
    ) -> Result<String, TranslateError>;
}
