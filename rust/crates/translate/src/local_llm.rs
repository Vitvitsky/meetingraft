//! Local LLM engine (Ollama / GGUF) — скелет.

use domain::SpeechLanguage;

use crate::engine::{TranslateEngine, TranslateError};

/// Будущий local LLM translate (не MT-NLLB).
pub struct LocalLlmTranslateEngine;

impl TranslateEngine for LocalLlmTranslateEngine {
    fn translate(
        &self,
        text: &str,
        source: SpeechLanguage,
        target: SpeechLanguage,
    ) -> Result<String, TranslateError> {
        Ok(format!(
            "[{}→{}·local_llm] {text}",
            source.code(),
            target.code()
        ))
    }
}
