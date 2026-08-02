//! Demo/CI dictionary engine.

use domain::SpeechLanguage;

use crate::engine::{TranslateEngine, TranslateError};

/// Демо/CI перевод: известные фразы + fallback-маркер.
pub struct StubTranslateEngine;

impl TranslateEngine for StubTranslateEngine {
    fn translate(
        &self,
        text: &str,
        _source: SpeechLanguage,
        target: SpeechLanguage,
    ) -> Result<String, TranslateError> {
        Ok(stub_translate(text, target))
    }
}

/// Демо/CI перевод: известные фразы + fallback-маркер.
pub fn stub_translate(text: &str, target: SpeechLanguage) -> String {
    if let Some(mapped) = lookup(text, target) {
        return mapped.to_string();
    }
    format!("[{}·stub] {text}", target.code())
}

fn lookup(text: &str, target: SpeechLanguage) -> Option<&'static str> {
    match (text, target) {
        ("Добро пожаловать", SpeechLanguage::En) => Some("Welcome"),
        ("Добро пожаловать в MeetingRaft", SpeechLanguage::En) => {
            Some("Welcome to MeetingRaft")
        }
        ("Язык сессии — русский", SpeechLanguage::En) => {
            Some("Session language is Russian")
        }
        ("Язык сессии — русский по умолчанию", SpeechLanguage::En) => {
            Some("Session language is Russian by default")
        }
        ("English terms are fine", SpeechLanguage::En) => Some("English terms are fine"),
        ("English terms are fine in mixed meetings", SpeechLanguage::En) => {
            Some("English terms are fine in mixed meetings")
        }
        ("Добро пожаловать", SpeechLanguage::Es) => Some("Bienvenido"),
        ("Добро пожаловать в MeetingRaft", SpeechLanguage::Es) => {
            Some("Bienvenido a MeetingRaft")
        }
        ("Язык сессии — русский", SpeechLanguage::Es) => {
            Some("El idioma de la sesión es ruso")
        }
        ("Язык сессии — русский по умолчанию", SpeechLanguage::Es) => {
            Some("El idioma de la sesión es ruso por defecto")
        }
        ("Welcome", SpeechLanguage::Ru) => Some("Добро пожаловать"),
        ("Welcome to MeetingRaft", SpeechLanguage::Ru) => Some("Добро пожаловать в MeetingRaft"),
        ("Session language is English", SpeechLanguage::Ru) => Some("Язык сессии — английский"),
        ("Session language is English for this meeting", SpeechLanguage::Ru) => {
            Some("Язык сессии — английский для этой встречи")
        }
        ("Welcome", SpeechLanguage::Es) => Some("Bienvenido"),
        ("Welcome to MeetingRaft", SpeechLanguage::Es) => Some("Bienvenido a MeetingRaft"),
        ("Bienvenido", SpeechLanguage::En) => Some("Welcome"),
        ("Bienvenido a MeetingRaft", SpeechLanguage::En) => Some("Welcome to MeetingRaft"),
        ("Bienvenido", SpeechLanguage::Ru) => Some("Добро пожаловать"),
        ("Bienvenido a MeetingRaft", SpeechLanguage::Ru) => Some("Добро пожаловать в MeetingRaft"),
        ("[partial ru] речь…", SpeechLanguage::En) => Some("[partial] speaking…"),
        ("[final ru] фрагмент речи униффи", SpeechLanguage::En) => {
            Some("[final] speech fragment uniffi")
        }
        ("[partial en] speaking…", SpeechLanguage::Ru) => Some("[partial] речь…"),
        ("[final en] speech fragment uniffi", SpeechLanguage::Ru) => {
            Some("[final] фрагмент речи униффи")
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_known_russian_demo_line() {
        assert_eq!(
            stub_translate("Добро пожаловать", SpeechLanguage::En),
            "Welcome"
        );
    }

    #[test]
    fn unknown_gets_stub_prefix() {
        let out = stub_translate("xyz", SpeechLanguage::En);
        assert!(out.starts_with("[en·stub]"));
    }
}
