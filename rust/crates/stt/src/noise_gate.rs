//! Гейт речи с адаптивным уровнем фона.
//!
//! Постоянный порог RMS не работает, и это измерено (Epic 18, замер
//! 2026-08-04): при открытом окне городской шум пробивает константу, и
//! Whisper молотит непрерывно — 6.9 ватта на GPU при том, что никто не
//! говорит.
//!
//! Поднять константу нельзя: тихий собеседник из системного канала до
//! неё не дотягивает, и это отдельная измеренная жалоба. Одно число не
//! обслуживает обе ситуации, потому что описывает не то: значение имеет
//! **превышение речи над фоном комнаты**, а не абсолютная громкость.
//!
//! Фон оценивается на ходу. Шум улицы становится фоном и сам себя не
//! пробивает; тихая речь превышает его с запасом.

/// Во сколько раз речь должна превышать фон.
///
/// Речь обычно на 10–15 дБ громче фона; втрое по амплитуде — нижний край
/// этого диапазона, выбранный в пользу чувствительности: пропустить
/// тихую реплику хуже, чем лишний раз запустить модель.
const SPEECH_MARGIN: f32 = 3.0;
/// Ниже этого уровня речи не бывает ни при каком фоне.
///
/// Нужен, чтобы в идеально тихой комнате оценка фона не сползла к нулю:
/// тогда порог стал бы нулевым и гейт открывался бы от чего угодно.
const ABSOLUTE_FLOOR: f32 = 120.0;
/// Скорость подъёма оценки фона: медленно, иначе фоном станет речь.
const RISE: f32 = 0.02;
/// Скорость спада: быстро, иначе после громкого места гейт надолго
/// оглохнет и начало следующей реплики потеряется.
const FALL: f32 = 0.25;

/// Уровень кадра, по которому судит гейт.
///
/// Живёт рядом с гейтом, а не у каждого вызывающего: приборы обязаны
/// считать ровно то же, что живой путь. Своя копия у прибора означала
/// бы, что он меряет не тот гейт, и разошлись бы они молча.
pub fn frame_rms(frame: &[i16]) -> f32 {
    if frame.is_empty() {
        return 0.0;
    }
    let sum: f64 = frame.iter().map(|s| f64::from(*s) * f64::from(*s)).sum();
    ((sum / frame.len() as f64).sqrt()) as f32
}

/// Решает, есть ли речь, по уровню кадра и накопленной оценке фона.
#[derive(Debug, Clone)]
pub struct NoiseGate {
    floor: Option<f32>,
    margin: f32,
}

impl Default for NoiseGate {
    fn default() -> Self {
        Self::new()
    }
}

impl NoiseGate {
    pub fn new() -> Self {
        Self {
            floor: None,
            margin: SPEECH_MARGIN,
        }
    }

    /// Текущая оценка фона — для журнала диагностики и замеров.
    pub fn noise_floor(&self) -> f32 {
        self.floor.unwrap_or(0.0)
    }

    /// Порог, который сейчас нужно превысить.
    pub fn threshold(&self) -> f32 {
        (self.noise_floor() * self.margin).max(ABSOLUTE_FLOOR)
    }

    /// Есть ли речь в кадре с таким уровнем.
    pub fn accepts(&mut self, level: f32) -> bool {
        // Первый кадр задаёт отправную точку: без него оценка полползёт
        // от нуля и первые секунды шума будут считаться речью.
        let floor = *self.floor.get_or_insert(level);
        let is_speech = level > (floor * self.margin).max(ABSOLUTE_FLOOR);

        if is_speech {
            // Во время речи фон вверх не тянем — иначе он дорастёт до
            // уровня говорящего и гейт закроется сам собой посреди фразы.
            if level < floor {
                self.floor = Some(level);
            }
        } else {
            let rate = if level < floor { FALL } else { RISE };
            self.floor = Some(floor + (level - floor) * rate);
        }
        is_speech
    }

    /// Забыть накопленное: новая сессия — новая комната.
    pub fn reset(&mut self) {
        self.floor = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Прогнать кадры одного уровня и вернуть, сколько признано речью.
    fn feed(gate: &mut NoiseGate, level: f32, frames: usize) -> usize {
        (0..frames).filter(|_| gate.accepts(level)).count()
    }

    /// Главный измеренный случай: открытое окно, никто не говорит.
    /// Постоянный порог 450 такой шум пропускал, и модель работала
    /// непрерывно.
    #[test]
    fn steady_city_noise_stops_being_speech() {
        let mut gate = NoiseGate::new();

        let accepted = feed(&mut gate, 600.0, 200);

        assert!(
            accepted < 10,
            "шум должен стать фоном, а не речью: принято {accepted} из 200"
        );
    }

    /// Обратный измеренный случай: тихий собеседник из системного канала
    /// не дотягивал до постоянного порога.
    #[test]
    fn quiet_speech_over_quiet_room_is_accepted() {
        let mut gate = NoiseGate::new();
        feed(&mut gate, 60.0, 100);

        assert!(gate.accepts(500.0), "порог: {}", gate.threshold());
    }

    /// Та же тихая речь, но в шумной комнате: превышения нет, и честнее
    /// не пустить — распознавать под уличный гул нечего.
    #[test]
    fn quiet_speech_under_loud_noise_is_rejected() {
        let mut gate = NoiseGate::new();
        feed(&mut gate, 600.0, 200);

        assert!(!gate.accepts(700.0));
    }

    /// Нормальная речь пробивает и городской шум.
    #[test]
    fn normal_speech_beats_city_noise() {
        let mut gate = NoiseGate::new();
        feed(&mut gate, 600.0, 200);

        assert!(gate.accepts(2_500.0));
    }

    /// Фон не должен догонять говорящего: иначе гейт закроется посреди
    /// фразы и вторая половина реплики пропадёт.
    #[test]
    fn long_utterance_does_not_raise_the_floor() {
        let mut gate = NoiseGate::new();
        feed(&mut gate, 200.0, 100);
        let before = gate.noise_floor();

        let accepted = feed(&mut gate, 2_000.0, 300);

        assert_eq!(accepted, 300, "речь оборвалась на середине");
        assert!(gate.noise_floor() <= before + 1.0);
    }

    /// В идеально тихой комнате оценка фона сползает к нулю; без
    /// абсолютного минимума гейт открывался бы от чего угодно.
    #[test]
    fn absolute_floor_protects_a_silent_room() {
        let mut gate = NoiseGate::new();
        feed(&mut gate, 1.0, 500);

        assert!(!gate.accepts(100.0));
        assert!(gate.threshold() >= ABSOLUTE_FLOOR);
    }

    /// После громкого места гейт обязан быстро вернуть чувствительность,
    /// иначе начало следующей реплики потеряется.
    #[test]
    fn floor_falls_quickly_after_loud_background() {
        let mut gate = NoiseGate::new();
        feed(&mut gate, 900.0, 200);

        feed(&mut gate, 80.0, 30);

        assert!(gate.accepts(400.0), "порог: {}", gate.threshold());
    }

    #[test]
    fn reset_forgets_the_room() {
        let mut gate = NoiseGate::new();
        feed(&mut gate, 900.0, 200);

        gate.reset();

        assert_eq!(gate.noise_floor(), 0.0);
    }
}
