//! Fake caption producer для live demo (скрипт Phase 1).

use domain::{CaptionEvent, CaptionPhase, SpeechLanguage};
use uuid::Uuid;

/// Скриптованный producer: на каждом tick отдаёт следующий event по времени.
pub struct FakeCaptionProducer {
    script: Vec<(u64, CaptionEvent)>,
    next_index: usize,
}

impl FakeCaptionProducer {
    /// Demo-скрипт на языке распознавания сессии.
    pub fn for_language(language: SpeechLanguage) -> Self {
        let lines: [(&str, CaptionPhase); 6] = match language {
            SpeechLanguage::Ru => [
                ("Добро пожаловать", CaptionPhase::Partial),
                ("Добро пожаловать в MeetingRaft", CaptionPhase::Final),
                ("Язык сессии — русский", CaptionPhase::Partial),
                ("Язык сессии — русский по умолчанию", CaptionPhase::Final),
                ("English terms are fine", CaptionPhase::Partial),
                (
                    "English terms are fine in mixed meetings",
                    CaptionPhase::Final,
                ),
            ],
            SpeechLanguage::En => [
                ("Welcome", CaptionPhase::Partial),
                ("Welcome to MeetingRaft", CaptionPhase::Final),
                ("Session language is English", CaptionPhase::Partial),
                (
                    "Session language is English for this meeting",
                    CaptionPhase::Final,
                ),
                ("Russian terms are fine", CaptionPhase::Partial),
                (
                    "Russian terms are fine in mixed meetings",
                    CaptionPhase::Final,
                ),
            ],
            SpeechLanguage::Es => [
                ("Bienvenido", CaptionPhase::Partial),
                ("Bienvenido a MeetingRaft", CaptionPhase::Final),
                ("El idioma de la sesión es español", CaptionPhase::Partial),
                (
                    "El idioma de la sesión es español por defecto",
                    CaptionPhase::Final,
                ),
                ("Los términos en inglés están bien", CaptionPhase::Partial),
                (
                    "Los términos en inglés están bien en reuniones mixtas",
                    CaptionPhase::Final,
                ),
            ],
        };
        Self::from_lines(&lines)
    }

    /// Дефолтный русскоязычный demo-скрипт.
    pub fn default_script() -> Self {
        Self::for_language(SpeechLanguage::Ru)
    }

    fn from_lines(lines: &[(&str, CaptionPhase); 6]) -> Self {
        let script = lines
            .iter()
            .enumerate()
            .map(|(i, (text, phase))| {
                (
                    (i as u64) * 800,
                    CaptionEvent::new(Uuid::new_v4().to_string(), (*text).to_string(), *phase),
                )
            })
            .collect();
        Self {
            script,
            next_index: 0,
        }
    }

    /// События, готовые к `elapsed_ms` (включительно), ещё не выданные.
    pub fn drain_due(&mut self, elapsed_ms: u64) -> Vec<CaptionEvent> {
        let mut out = Vec::new();
        while self.next_index < self.script.len() {
            let (at, _) = &self.script[self.next_index];
            if *at > elapsed_ms {
                break;
            }
            out.push(self.script[self.next_index].1.clone());
            self.next_index += 1;
        }
        out
    }
}
