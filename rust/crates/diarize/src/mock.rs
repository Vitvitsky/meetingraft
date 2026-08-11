//! Заглушка вместо движка: отказ, а не выдуманные отрезки.
//!
//! Отличие от `stt::MockSttEngine` намеренное. Тот отдаёт подделку под
//! текст, и это безобидно: `[final ru] фрагмент речи униффи` никто за
//! распознавание не примет. Подделка под диаризацию неотличима от правды
//! — отрезки с метками выглядят одинаково, кто бы их ни насчитал, — и
//! человек, увидев в списке «голос A» и «голос B», поверит, что запись
//! разделена.
//!
//! Поэтому заглушка не делит ничего и говорит почему.

use crate::{DiarizeReport, Diarizer};

/// Причина отказа. Текст уезжает в интерфейс и в прибор как есть, так что
/// он обязан объяснять, а не сообщать код.
const NO_MODEL: &str = "модель разделения голосов не выбрана: крейт собран без фичи `model` \
     (решение принимается замером, задача 3 плана 2026-08-11-voice-clustering)";

/// Движок, которого нет.
///
/// Причину носит с собой: «модели нет» и «модель есть, но не поднялась на
/// этих файлах» — разные беды с разными действиями человека, и свести их
/// к одному тексту значило бы отправить его чинить не то.
#[derive(Debug)]
pub struct MockDiarizer {
    reason: String,
}

impl Default for MockDiarizer {
    fn default() -> Self {
        Self::new()
    }
}

impl MockDiarizer {
    /// Сборка без движка вовсе.
    pub fn new() -> Self {
        Self::because(NO_MODEL)
    }

    /// Движок есть, но не поднялся: причина уже известна вызывающему.
    pub fn because(reason: impl Into<String>) -> Self {
        Self {
            reason: reason.into(),
        }
    }
}

impl Diarizer for MockDiarizer {
    fn diarize(&mut self, _pcm: &[i16], _sample_rate: u32) -> DiarizeReport {
        DiarizeReport::refused(self.reason.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Отказ не зависит от материала: заглушке нечем отличить монолог от
    /// диалога, и притворяться, будто она их различает, — худшее, что она
    /// может сделать.
    #[test]
    fn the_stub_refuses_on_any_material() {
        let mut engine = MockDiarizer::new();

        let silence = engine.diarize(&vec![0i16; 16_000], 16_000);
        let loud = engine.diarize(&vec![3_000i16; 16_000], 16_000);

        assert!(silence.is_refused());
        assert!(loud.is_refused());
        assert_eq!(silence.refused, loud.refused, "причина обязана быть одна");
    }

    /// Причина называет то, что человек может исправить.
    #[test]
    fn the_reason_names_the_missing_piece() {
        let mut engine = MockDiarizer::new();

        let reason = engine
            .diarize(&[], 16_000)
            .refused
            .expect("заглушка обязана отказать");

        assert!(
            reason.contains("model"),
            "причина не называет фичу: {reason}"
        );
    }

    /// Своя причина доезжает целиком: «модели нет» и «модель не поднялась»
    /// человек чинит по-разному.
    #[test]
    fn a_given_reason_is_carried_through() {
        let mut engine = MockDiarizer::because("в каталоге нет embedding.onnx");

        let reason = engine
            .diarize(&[], 16_000)
            .refused
            .expect("заглушка обязана отказать");

        assert_eq!(reason, "в каталоге нет embedding.onnx");
    }
}
