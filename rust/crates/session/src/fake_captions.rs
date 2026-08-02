//! Fake caption producer для live demo (скрипт Phase 1).

use domain::{CaptionEvent, CaptionPhase};
use uuid::Uuid;

/// Скриптованный producer: на каждом tick отдаёт следующий event по времени.
pub struct FakeCaptionProducer {
    script: Vec<(u64, CaptionEvent)>,
    next_index: usize,
}

impl FakeCaptionProducer {
    /// Дефолтный русскоязычный demo-скрипт.
    pub fn default_script() -> Self {
        let lines: [(&str, CaptionPhase); 6] = [
            ("Добро пожаловать", CaptionPhase::Partial),
            ("Добро пожаловать в MeetingRaft", CaptionPhase::Final),
            ("Язык сессии — русский", CaptionPhase::Partial),
            ("Язык сессии — русский по умолчанию", CaptionPhase::Final),
            ("English terms are fine", CaptionPhase::Partial),
            (
                "English terms are fine in mixed meetings",
                CaptionPhase::Final,
            ),
        ];
        let script = lines
            .into_iter()
            .enumerate()
            .map(|(i, (text, phase))| {
                (
                    (i as u64) * 800,
                    CaptionEvent {
                        id: Uuid::new_v4().to_string(),
                        text: text.to_string(),
                        phase,
                    },
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
