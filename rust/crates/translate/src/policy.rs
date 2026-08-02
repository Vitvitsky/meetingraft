//! Политика выбора translation backend (ADR-008).

use domain::SpeechLanguage;

/// Пользовательский / settings выбор backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TranslationBackendKind {
    Off,
    #[default]
    Auto,
    Stub,
    Apple,
    Backend,
    LocalLlm,
}

impl TranslationBackendKind {
    pub fn code(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Auto => "auto",
            Self::Stub => "stub",
            Self::Apple => "apple",
            Self::Backend => "backend",
            Self::LocalLlm => "local_llm",
        }
    }

    pub fn from_code(code: &str) -> Option<Self> {
        match code {
            "off" => Some(Self::Off),
            "auto" => Some(Self::Auto),
            "stub" => Some(Self::Stub),
            "apple" => Some(Self::Apple),
            "backend" => Some(Self::Backend),
            "local_llm" => Some(Self::LocalLlm),
            _ => None,
        }
    }
}

/// После резолва `auto` — конкретный путь исполнения.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EffectiveBackend {
    Off,
    Stub,
    AppleHost,
    BackendHttp,
    LocalLlm,
}

impl EffectiveBackend {
    pub fn code(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Stub => "stub",
            Self::AppleHost => "apple",
            Self::BackendHttp => "backend",
            Self::LocalLlm => "local_llm",
        }
    }
}

/// Политика sync-перевода (отдельно от LanguagePolicy STT).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TranslationPolicy {
    pub enabled: bool,
    pub target: SpeechLanguage,
    pub backend: TranslationBackendKind,
    /// Base URL для `backend` / `auto→backend`, например `http://127.0.0.1:8080`.
    pub backend_base_url: Option<String>,
}

impl TranslationPolicy {
    pub fn disabled() -> Self {
        Self {
            enabled: false,
            target: SpeechLanguage::En,
            backend: TranslationBackendKind::Off,
            backend_base_url: None,
        }
    }

    pub fn default_enabled(target: SpeechLanguage) -> Self {
        Self {
            enabled: true,
            target,
            backend: TranslationBackendKind::Auto,
            backend_base_url: None,
        }
    }
}
