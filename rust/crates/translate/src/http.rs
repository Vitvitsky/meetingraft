//! HTTP backend engine (NLLB / ADR-007) — скелет без сетевого вызова.

use domain::SpeechLanguage;

use crate::engine::{TranslateEngine, TranslateError};

/// Будущий клиент `POST {base}/v1/translate`.
pub struct HttpTranslateEngine {
    base_url: String,
}

impl HttpTranslateEngine {
    pub fn new(base_url: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into(),
        }
    }
}

impl TranslateEngine for HttpTranslateEngine {
    fn translate(
        &self,
        text: &str,
        source: SpeechLanguage,
        target: SpeechLanguage,
    ) -> Result<String, TranslateError> {
        if self.base_url.trim().is_empty() {
            return Err(TranslateError::NotConfigured("backend_base_url empty"));
        }
        // Скелет: контракт зафиксируем в shared/openapi при реализации ADR-007.
        Ok(format!(
            "[{}→{}·backend@{}] {text}",
            source.code(),
            target.code(),
            self.base_url.trim_end_matches('/')
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_empty_url() {
        let eng = HttpTranslateEngine::new("");
        assert!(matches!(
            eng.translate("hi", SpeechLanguage::Ru, SpeechLanguage::En),
            Err(TranslateError::NotConfigured(_))
        ));
    }

    #[test]
    fn skeleton_marks_backend() {
        let eng = HttpTranslateEngine::new("http://127.0.0.1:8080");
        let out = eng
            .translate("привет", SpeechLanguage::Ru, SpeechLanguage::En)
            .unwrap();
        assert!(out.contains("backend@http://127.0.0.1:8080"));
    }
}
