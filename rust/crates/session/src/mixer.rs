//! Выравнивание и микширование каналов захвата (ADR-004, ADR-009).
//!
//! Захват отдаёт два независимых потока (mic = «я», system = «остальные»)
//! чанками фиксированной длительности с меткой от начала записи. Live-STT
//! потребляет один поток, поэтому здесь потоки сводятся по слотам времени,
//! а принадлежность речи сохраняется отдельным полем `dominant`.
//!
//! На диске каналы остаются раздельными — микс существует только для
//! live-распознавания.

use std::collections::BTreeMap;

use domain::AudioChannel;

/// Длительность слота по умолчанию; совпадает с чанком захвата.
pub const DEFAULT_SLOT_MS: u64 = 100;
/// Сколько слотов ждать недостающий канал, прежде чем отдать с тишиной.
pub const DEFAULT_TOLERANCE_SLOTS: u64 = 2;
/// Во сколько раз канал должен быть громче, чтобы перехватить доминирование.
const HYSTERESIS_RATIO: f32 = 1.5;

/// Выровненный кадр: микс каналов и канал говорящего.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MixedFrame {
    pub pcm: Vec<i16>,
    pub dominant: AudioChannel,
    /// Смещение кадра от начала записи, мс.
    pub timestamp_ms: u64,
}

/// Слот времени: до одного чанка на канал.
#[derive(Default)]
struct Slot {
    mic: Option<Vec<i16>>,
    system: Option<Vec<i16>>,
}

/// Сводит два канала в один поток кадров, сохраняя порядок по времени.
pub struct ChannelMixer {
    slot_ms: u64,
    tolerance_slots: u64,
    system_expected: bool,
    slots: BTreeMap<u64, Slot>,
    highest_slot: Option<u64>,
    /// Первый ещё не отданный слот; всё раньше — опоздавшее.
    next_slot: u64,
    dominant: AudioChannel,
    late_chunks: u64,
}

impl Default for ChannelMixer {
    fn default() -> Self {
        Self::new()
    }
}

impl ChannelMixer {
    pub fn new() -> Self {
        Self::with_params(DEFAULT_SLOT_MS, DEFAULT_TOLERANCE_SLOTS)
    }

    pub fn with_params(slot_ms: u64, tolerance_slots: u64) -> Self {
        Self {
            slot_ms: slot_ms.max(1),
            tolerance_slots,
            system_expected: false,
            slots: BTreeMap::new(),
            highest_slot: None,
            next_slot: 0,
            dominant: AudioChannel::Mic,
            late_chunks: 0,
        }
    }

    /// Ждать ли системный канал. Пока tap не запущен — не ждём, иначе
    /// каждый слот простаивал бы допуск впустую.
    pub fn set_system_expected(&mut self, expected: bool) {
        self.system_expected = expected;
    }

    /// Сколько чанков пришло в уже отданные слоты (диагностика).
    pub fn late_chunks(&self) -> u64 {
        self.late_chunks
    }

    /// Начать заново (новая запись).
    pub fn reset(&mut self) {
        self.slots.clear();
        self.highest_slot = None;
        self.next_slot = 0;
        self.dominant = AudioChannel::Mic;
        self.late_chunks = 0;
    }

    /// Положить чанк канала в слот по его метке времени.
    pub fn push(&mut self, channel: AudioChannel, pcm: &[i16], timestamp_ms: u64) {
        let index = timestamp_ms / self.slot_ms;
        if index < self.next_slot {
            self.late_chunks += 1;
            return;
        }
        let slot = self.slots.entry(index).or_default();
        let target = match channel {
            AudioChannel::Mic => &mut slot.mic,
            AudioChannel::System => &mut slot.system,
        };
        target.get_or_insert_with(Vec::new).extend_from_slice(pcm);
        self.highest_slot = Some(match self.highest_slot {
            Some(highest) => highest.max(index),
            None => index,
        });
    }

    /// Отдать готовые слоты по порядку. Слот готов, когда пришли все
    /// ожидаемые каналы либо истёк допуск ожидания.
    pub fn drain(&mut self) -> Vec<MixedFrame> {
        let mut out = Vec::new();
        loop {
            let ready = match self.slots.iter().next() {
                Some((&index, slot)) => self.is_ready(index, slot),
                None => false,
            };
            if !ready {
                break;
            }
            let (index, slot) = self.slots.pop_first().expect("слот проверен выше");
            out.push(self.mix(index, slot));
        }
        out
    }

    /// Отдать всё накопленное, не дожидаясь допуска (остановка записи).
    pub fn flush(&mut self) -> Vec<MixedFrame> {
        let mut out = Vec::new();
        while let Some((index, slot)) = self.slots.pop_first() {
            out.push(self.mix(index, slot));
        }
        out
    }

    fn is_ready(&self, index: u64, slot: &Slot) -> bool {
        let complete = slot.mic.is_some() && (!self.system_expected || slot.system.is_some());
        if complete {
            return true;
        }
        match self.highest_slot {
            Some(highest) => index + self.tolerance_slots <= highest,
            None => false,
        }
    }

    fn mix(&mut self, index: u64, slot: Slot) -> MixedFrame {
        let mic = slot.mic.unwrap_or_default();
        let system = slot.system.unwrap_or_default();
        let len = mic.len().max(system.len());
        let mut pcm = Vec::with_capacity(len);
        for position in 0..len {
            let left = i32::from(mic.get(position).copied().unwrap_or(0));
            let right = i32::from(system.get(position).copied().unwrap_or(0));
            // Клампим, а не делим пополам: деление глушило бы монолог вдвое.
            pcm.push((left + right).clamp(i32::from(i16::MIN), i32::from(i16::MAX)) as i16);
        }

        self.dominant = next_dominant(self.dominant, rms(&mic), rms(&system));
        self.next_slot = index + 1;

        MixedFrame {
            pcm,
            dominant: self.dominant,
            timestamp_ms: index * self.slot_ms,
        }
    }
}

/// Доминирующий канал с гистерезисом: смена только при заметном перевесе,
/// иначе атрибуция дребезжала бы на близких уровнях.
fn next_dominant(current: AudioChannel, mic: f32, system: f32) -> AudioChannel {
    if system > mic * HYSTERESIS_RATIO {
        AudioChannel::System
    } else if mic > system * HYSTERESIS_RATIO {
        AudioChannel::Mic
    } else {
        current
    }
}

fn rms(samples: &[i16]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    let sum: f64 = samples.iter().map(|s| f64::from(*s) * f64::from(*s)).sum();
    (sum / samples.len() as f64).sqrt() as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(level: i16, len: usize) -> Vec<i16> {
        (0..len)
            .map(|i| if i % 2 == 0 { level } else { -level })
            .collect()
    }

    #[test]
    fn mic_only_passes_through_without_waiting() {
        let mut mixer = ChannelMixer::new();
        let mic = tone(1000, 4);

        mixer.push(AudioChannel::Mic, &mic, 0);
        let frames = mixer.drain();

        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0].pcm, mic);
        assert_eq!(frames[0].dominant, AudioChannel::Mic);
        assert_eq!(frames[0].timestamp_ms, 0);
    }

    #[test]
    fn louder_system_takes_dominance() {
        let mut mixer = ChannelMixer::new();
        mixer.set_system_expected(true);

        mixer.push(AudioChannel::Mic, &tone(100, 4), 0);
        mixer.push(AudioChannel::System, &tone(5000, 4), 0);
        let frames = mixer.drain();

        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0].dominant, AudioChannel::System);
    }

    #[test]
    fn dominance_does_not_flap_on_similar_levels() {
        let mut mixer = ChannelMixer::new();
        mixer.set_system_expected(true);

        mixer.push(AudioChannel::Mic, &tone(1000, 4), 0);
        mixer.push(AudioChannel::System, &tone(1200, 4), 0);
        let frames = mixer.drain();

        // Перевес меньше HYSTERESIS_RATIO — доминант не меняется.
        assert_eq!(frames[0].dominant, AudioChannel::Mic);
    }

    #[test]
    fn missing_system_channel_is_filled_with_silence_after_tolerance() {
        let mut mixer = ChannelMixer::with_params(100, 2);
        mixer.set_system_expected(true);
        let mic = tone(1000, 4);

        mixer.push(AudioChannel::Mic, &mic, 0);
        assert!(mixer.drain().is_empty(), "слот ждёт системный канал");

        mixer.push(AudioChannel::Mic, &mic, 100);
        mixer.push(AudioChannel::Mic, &mic, 200);
        let frames = mixer.drain();

        assert_eq!(frames.len(), 1, "по допуску отдан только слот 0");
        assert_eq!(frames[0].pcm, mic, "недостающий канал — тишина");
        assert_eq!(frames[0].timestamp_ms, 0);
    }

    #[test]
    fn late_chunk_is_dropped_and_counted() {
        let mut mixer = ChannelMixer::new();
        mixer.push(AudioChannel::Mic, &tone(1000, 4), 0);
        assert_eq!(mixer.drain().len(), 1);

        mixer.push(AudioChannel::System, &tone(1000, 4), 0);

        assert_eq!(mixer.late_chunks(), 1);
        assert!(mixer.drain().is_empty());
    }

    #[test]
    fn frames_keep_slot_order() {
        let mut mixer = ChannelMixer::new();
        mixer.push(AudioChannel::Mic, &tone(1000, 2), 200);
        mixer.push(AudioChannel::Mic, &tone(1000, 2), 0);
        mixer.push(AudioChannel::Mic, &tone(1000, 2), 100);

        let stamps: Vec<u64> = mixer.drain().iter().map(|f| f.timestamp_ms).collect();

        assert_eq!(stamps, vec![0, 100, 200]);
    }

    #[test]
    fn flush_emits_pending_slots_without_tolerance() {
        let mut mixer = ChannelMixer::new();
        mixer.set_system_expected(true);
        mixer.push(AudioChannel::Mic, &tone(1000, 2), 0);
        mixer.push(AudioChannel::Mic, &tone(1000, 2), 100);
        assert!(mixer.drain().is_empty());

        let stamps: Vec<u64> = mixer.flush().iter().map(|f| f.timestamp_ms).collect();

        assert_eq!(stamps, vec![0, 100]);
    }

    #[test]
    fn mix_clamps_instead_of_wrapping() {
        let mut mixer = ChannelMixer::new();
        mixer.set_system_expected(true);
        mixer.push(AudioChannel::Mic, &[i16::MAX, i16::MIN], 0);
        mixer.push(AudioChannel::System, &[i16::MAX, i16::MIN], 0);

        let frames = mixer.drain();

        assert_eq!(frames[0].pcm, vec![i16::MAX, i16::MIN]);
    }

    #[test]
    fn channels_of_different_length_align_to_the_longer() {
        let mut mixer = ChannelMixer::new();
        mixer.set_system_expected(true);
        mixer.push(AudioChannel::Mic, &[100, 100], 0);
        mixer.push(AudioChannel::System, &[10, 10, 10, 10], 0);

        let frames = mixer.drain();

        assert_eq!(frames[0].pcm, vec![110, 110, 10, 10]);
    }

    #[test]
    fn reset_rewinds_slots_and_dominance() {
        let mut mixer = ChannelMixer::new();
        mixer.set_system_expected(true);
        mixer.push(AudioChannel::System, &tone(5000, 4), 0);
        mixer.push(AudioChannel::Mic, &tone(100, 4), 0);
        assert_eq!(mixer.drain()[0].dominant, AudioChannel::System);

        mixer.push(AudioChannel::Mic, &tone(100, 4), 0);
        assert_eq!(mixer.late_chunks(), 1, "чанк в отданный слот");

        mixer.reset();
        mixer.push(AudioChannel::Mic, &tone(1000, 4), 0);
        mixer.push(AudioChannel::System, &tone(1000, 4), 0);
        let frames = mixer.drain();

        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0].timestamp_ms, 0);
        // Уровни равны — гистерезис держит доминанта, сброшенного в Mic.
        assert_eq!(frames[0].dominant, AudioChannel::Mic);
        assert_eq!(mixer.late_chunks(), 0);
    }
}
