//! Live translation engines (ADR-008) — отдельный поток от STT captions.

mod engine;
mod host;
mod http;
mod local_llm;
mod policy;
mod stub;

pub use engine::{TranslateEngine, TranslateError};
pub use host::{HostPendingQueue, HostTranslationRequest};
pub use http::HttpTranslateEngine;
pub use local_llm::LocalLlmTranslateEngine;
pub use policy::{EffectiveBackend, TranslationBackendKind, TranslationPolicy};
pub use stub::{StubTranslateEngine, stub_translate};

use domain::SpeechLanguage;

/// Резолв `auto` → конкретный backend (ADR-008).
pub fn resolve_effective(policy: &TranslationPolicy, host_available: bool) -> EffectiveBackend {
    if !policy.enabled {
        return EffectiveBackend::Off;
    }
    match policy.backend {
        TranslationBackendKind::Off => EffectiveBackend::Off,
        TranslationBackendKind::Stub => EffectiveBackend::Stub,
        TranslationBackendKind::Apple => EffectiveBackend::AppleHost,
        TranslationBackendKind::Backend => EffectiveBackend::BackendHttp,
        TranslationBackendKind::LocalLlm => EffectiveBackend::LocalLlm,
        TranslationBackendKind::Auto => {
            if host_available {
                EffectiveBackend::AppleHost
            } else if policy
                .backend_base_url
                .as_ref()
                .is_some_and(|u| !u.trim().is_empty())
            {
                EffectiveBackend::BackendHttp
            } else {
                EffectiveBackend::Stub
            }
        }
    }
}

/// Синхронный перевод через выбранный non-host engine.
pub fn translate_now(
    effective: EffectiveBackend,
    policy: &TranslationPolicy,
    text: &str,
    source: SpeechLanguage,
    target: SpeechLanguage,
) -> Result<String, TranslateError> {
    match effective {
        EffectiveBackend::Off => Err(TranslateError::Disabled),
        EffectiveBackend::AppleHost => Err(TranslateError::NeedsHost),
        EffectiveBackend::Stub => StubTranslateEngine.translate(text, source, target),
        EffectiveBackend::BackendHttp => {
            let url = policy.backend_base_url.clone().unwrap_or_default();
            HttpTranslateEngine::new(url).translate(text, source, target)
        }
        EffectiveBackend::LocalLlm => LocalLlmTranslateEngine.translate(text, source, target),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auto_prefers_host_when_available() {
        let policy = TranslationPolicy {
            enabled: true,
            target: SpeechLanguage::En,
            backend: TranslationBackendKind::Auto,
            backend_base_url: Some("http://localhost:8080".into()),
        };
        assert_eq!(
            resolve_effective(&policy, true),
            EffectiveBackend::AppleHost
        );
        assert_eq!(
            resolve_effective(&policy, false),
            EffectiveBackend::BackendHttp
        );
    }

    #[test]
    fn auto_falls_back_to_stub_without_host_or_url() {
        let policy = TranslationPolicy::default_enabled(SpeechLanguage::En);
        assert_eq!(resolve_effective(&policy, false), EffectiveBackend::Stub);
    }
}
