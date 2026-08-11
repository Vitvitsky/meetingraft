//! Темп запусков живого распознавания.
//!
//! Стоимость одного прохода растёт с длиной буфера, а не с объёмом новой
//! речи: на длинной реплике каждую секунду переразбираются все её
//! тридцать. Пока LocalAgreement (ADR-010) фиксирует слова, это плата за
//! латентность и она оправдана.
//!
//! Но если согласия нет несколько кругов подряд — модель меняет мнение,
//! и учащённые прогоны не приближают результат, а только греют машину.
//! Тогда темп разжимается.
//!
//! Логика живёт отдельно от движка намеренно: она проверяется без
//! Whisper и без Mac, а `whisper.rs` собирается только под фичей.

/// Сколько речи должно накопиться, прежде чем движок вообще запустится.
///
/// Живёт здесь, а не в `whisper.rs`, по той же причине, что и сам темп:
/// `whisper.rs` собирается только под фичей и на Linux не существует
/// вовсе, а решение «сколько работы достаётся модели» надо считать и
/// снаружи — этим занят `gate-probe`. Копия константы в приборе
/// означала бы, что прибор меряет правило, которого движок уже не
/// придерживается, и разойтись они могли бы молча.
pub const MIN_SPEECH_FRAMES: usize = 16_000 / 5;
/// Сколько тишины закрывает реплику. По её концу идёт ещё один проход:
/// контекста больше не будет, остаток фиксируется принудительно.
pub const SILENCE_FRAMES: usize = 16_000 * 3 / 10;
/// Базовый темп partial-прогонов: не чаще раза в ~1 с.
pub const PARTIAL_MIN_FRAMES: usize = 16_000;

/// Через сколько кругов без фиксации разжимать темп.
const IDLE_ROUNDS_BEFORE_BACKOFF: u32 = 2;
/// Потолок разжатия. Больше — заметная задержка живых субтитров: текст
/// перестал бы появляться на несколько секунд, а это дороже экономии.
const MAX_MULTIPLIER: u32 = 3;

/// Сколько кадров ждать до следующего прохода.
#[derive(Debug, Clone)]
pub struct InferencePacer {
    base_frames: usize,
    multiplier: u32,
    idle_rounds: u32,
}

impl InferencePacer {
    pub fn new(base_frames: usize) -> Self {
        Self {
            base_frames: base_frames.max(1),
            multiplier: 1,
            idle_rounds: 0,
        }
    }

    /// Порог для следующего запуска.
    pub fn frames_until_next(&self) -> usize {
        self.base_frames * self.multiplier as usize
    }

    /// Во сколько раз темп сейчас реже базового — для замеров.
    pub fn multiplier(&self) -> u32 {
        self.multiplier
    }

    /// Учесть результат прохода.
    ///
    /// Возврат к базовому темпу — **немедленный**: как только слова пошли,
    /// задерживать их нельзя. Разжатие постепенное, возврат резкий.
    pub fn record(&mut self, committed_words: usize) {
        if committed_words > 0 {
            self.multiplier = 1;
            self.idle_rounds = 0;
            return;
        }
        self.idle_rounds += 1;
        if self.idle_rounds >= IDLE_ROUNDS_BEFORE_BACKOFF {
            self.idle_rounds = 0;
            self.multiplier = (self.multiplier + 1).min(MAX_MULTIPLIER);
        }
    }

    /// Реплика кончилась — следующая начинается с обычного темпа.
    pub fn reset(&mut self) {
        self.multiplier = 1;
        self.idle_rounds = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn starts_at_the_base_rate() {
        let pacer = InferencePacer::new(16_000);

        assert_eq!(pacer.frames_until_next(), 16_000);
        assert_eq!(pacer.multiplier(), 1);
    }

    /// Пока слова фиксируются, темп трогать нельзя: это плата за
    /// латентность, и она оправдана.
    #[test]
    fn productive_rounds_keep_the_base_rate() {
        let mut pacer = InferencePacer::new(16_000);

        for _ in 0..10 {
            pacer.record(3);
        }

        assert_eq!(pacer.frames_until_next(), 16_000);
    }

    /// Согласия нет несколько кругов — прогоны не приближают результат.
    #[test]
    fn idle_rounds_stretch_the_interval() {
        let mut pacer = InferencePacer::new(16_000);

        pacer.record(0);
        assert_eq!(pacer.multiplier(), 1, "одного круга мало для вывода");

        pacer.record(0);
        assert_eq!(pacer.multiplier(), 2);
    }

    /// Разжатие ограничено: иначе субтитры замирали бы на секунды, а это
    /// дороже сэкономленной энергии.
    #[test]
    fn backoff_is_capped() {
        let mut pacer = InferencePacer::new(16_000);

        for _ in 0..50 {
            pacer.record(0);
        }

        assert_eq!(pacer.multiplier(), MAX_MULTIPLIER);
        assert_eq!(pacer.frames_until_next(), 16_000 * MAX_MULTIPLIER as usize);
    }

    /// Возврат резкий: пошли слова — задерживать их нельзя.
    #[test]
    fn first_committed_word_restores_the_base_rate() {
        let mut pacer = InferencePacer::new(16_000);
        for _ in 0..10 {
            pacer.record(0);
        }
        assert!(pacer.multiplier() > 1);

        pacer.record(1);

        assert_eq!(pacer.multiplier(), 1);
        assert_eq!(pacer.frames_until_next(), 16_000);
    }

    #[test]
    fn reset_returns_to_the_base_rate() {
        let mut pacer = InferencePacer::new(16_000);
        for _ in 0..10 {
            pacer.record(0);
        }

        pacer.reset();

        assert_eq!(pacer.multiplier(), 1);
    }

    /// Нулевой базовый порог означал бы прогон на каждый кадр звука.
    #[test]
    fn zero_base_is_clamped() {
        assert_eq!(InferencePacer::new(0).frames_until_next(), 1);
    }
}
