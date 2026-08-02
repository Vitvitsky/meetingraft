//! Языковая политика распознавания (ADR-003).

/// Код языка речи.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum SpeechLanguage {
    #[default]
    Ru,
    En,
    Es,
}

impl SpeechLanguage {
    /// ISO-подобный код для контрактов и UI.
    pub fn code(self) -> &'static str {
        match self {
            Self::Ru => "ru",
            Self::En => "en",
            Self::Es => "es",
        }
    }
}

/// Политика языков сессии: primary + allowed set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LanguagePolicy {
    pub primary: SpeechLanguage,
    pub allowed: Vec<SpeechLanguage>,
}

impl LanguagePolicy {
    /// Политика v1: русский primary, ru/en/es allowed.
    pub fn default_v1() -> Self {
        Self {
            primary: SpeechLanguage::Ru,
            allowed: vec![SpeechLanguage::Ru, SpeechLanguage::En, SpeechLanguage::Es],
        }
    }

    /// Проверка, что язык входит в allowed.
    pub fn is_allowed(&self, language: SpeechLanguage) -> bool {
        self.allowed.contains(&language)
    }
}
