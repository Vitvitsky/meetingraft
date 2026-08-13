//! Спикеры встречи.

use crate::AudioChannel;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Speaker {
    pub id: String,
    pub meeting_id: String,
    pub display_name: String,
    pub sort_index: i64,
}

impl Speaker {
    /// Идентификатор спикера по умолчанию для канала встречи.
    ///
    /// Детерминированный, а не случайный: повторный пересбор обязан
    /// попадать в того же спикера, иначе список плодил бы дубликаты, а
    /// переименование терялось бы при каждом проходе.
    pub fn default_id(meeting_id: &str, channel: AudioChannel) -> String {
        format!("{meeting_id}:{}", channel.code())
    }

    /// Спикер по умолчанию для канала; имя приходит из презентационного
    /// слоя, как и название встречи.
    pub fn default_for(meeting_id: &str, channel: AudioChannel, display_name: &str) -> Self {
        Self {
            id: Self::default_id(meeting_id, channel),
            meeting_id: meeting_id.to_owned(),
            display_name: display_name.to_owned(),
            // Владелец машины идёт первым: его реплики чаще ищут.
            sort_index: match channel {
                AudioChannel::Mic => 0,
                AudioChannel::System => 1,
            },
        }
    }
}

/// Слепок голоса участника, как он лежит в базе (ADR-013).
///
/// Арифметика слепков живёт в `meetingraft-diarize`, а этот тип — только
/// то, что хранится. Разделение не формальное: `diarize` собирается за
/// фичей и тянет модель, а хранилище обязано собираться всегда.
///
/// Область — встреча. Слепки между встречами включаются отдельной
/// настройкой и отдельным решением; здесь их нет.
#[derive(Debug, Clone, PartialEq)]
pub struct StoredVoicePrint {
    pub meeting_id: String,
    /// Чей слепок. Тот же идентификатор, что у спикера встречи: имя
    /// человек даёт один раз, и слепок цепляется к нему, а не к своему
    /// собственному номеру.
    pub speaker_id: String,
    /// Какой моделью посчитан.
    ///
    /// Обязателен и сравнивается перед каждым использованием. Векторы
    /// разных моделей несравнимы, а похожесть между ними получается не
    /// нулевая, а **правдоподобная** — то есть неверные подписи выглядели
    /// бы уверенно. Смена модели однажды не доехала до машины молча, и
    /// замер шёл прежней (Epic 9, 2026-08-11).
    pub model_id: String,
    /// Усреднённый вектор единичной длины.
    pub vector: Vec<f32>,
    /// Из скольки кусков усреднён.
    pub samples: u32,
    /// Сколько секунд материала в нём. Слепок на четырёх секундах и
    /// слепок на четырёх минутах — разной надёжности, и человеку это
    /// показывают, а не прячут.
    pub seconds: f32,
    pub updated_at_ms: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_id_is_stable_per_channel() {
        assert_eq!(
            Speaker::default_id("m1", AudioChannel::Mic),
            Speaker::default_id("m1", AudioChannel::Mic)
        );
        assert_ne!(
            Speaker::default_id("m1", AudioChannel::Mic),
            Speaker::default_id("m1", AudioChannel::System)
        );
        assert_ne!(
            Speaker::default_id("m1", AudioChannel::Mic),
            Speaker::default_id("m2", AudioChannel::Mic)
        );
    }

    #[test]
    fn owner_sorts_before_others() {
        let mine = Speaker::default_for("m1", AudioChannel::Mic, "Вы");
        let theirs = Speaker::default_for("m1", AudioChannel::System, "Собеседник");

        assert!(mine.sort_index < theirs.sort_index);
    }
}
