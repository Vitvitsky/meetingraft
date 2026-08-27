//! Silero VAD через sherpa-onnx — тот, что обещан ADR-005.
//!
//! В живом пути его **нет**. Здесь он существует ради одного:
//! `gate-probe` ставит его рядом с нынешним гейтом по энергии на одних и
//! тех же записях и печатает, во что обходится каждый. До этих чисел
//! менять живой путь нечего — гейт уже дважды правился по замеру, и оба
//! раза догадка о нём расходилась с числом.
//!
//! ## Почему собственная сегментация VAD выключена
//!
//! У Silero свои `min_speech_duration` и `min_silence_duration`, и они
//! делают ровно то же, что наши `MIN_SPEECH_FRAMES` и `SILENCE_FRAMES`:
//! собирают кадры в реплики. Оставить оба слоя — значит сравнивать не
//! решателей, а две разные цепочки, и приписать разницу VAD будет
//! нельзя.
//!
//! Поэтому здесь они выставлены в минимум: VAD отвечает только на
//! «речь ли это сейчас», а реплики собирает наш код, тот же, что при
//! гейте.
//!
//! ## Чем он ошибается иначе, чем гейт
//!
//! Гейт мгновенен и глуп: громче фона — речь. VAD умён и **запаздывает**
//! — ему нужно накопить окно, чтобы решить. Отсюда его известная плата,
//! записанная в беклоге ещё по другому поводу: срезанное начало тихой
//! реплики не восстановить, и никто о нём не узнает. Прибор поэтому
//! печатает не только запуски модели, но и места расхождения — их можно
//! послушать.

use std::path::Path;

use sherpa_onnx::{SileroVadModelConfig, VadModelConfig, VoiceActivityDetector};

use crate::speech_decider::SpeechDecider;
use crate::vad_path::resolve_vad_model;

/// Порог вероятности речи. Умолчание Silero; своего у нас нет и
/// взяться ему пока неоткуда — его назначит замер.
const THRESHOLD: f32 = 0.5;
/// Окно Silero: 512 отсчётов при 16 кГц (32 мс). Значение модели, не наш
/// выбор.
const WINDOW_SIZE: i32 = 512;
/// Сколько секунд отсчётов держит внутренний буфер детектора.
const BUFFER_SECONDS: f32 = 30.0;
/// Наименьшая речь и наименьшая тишина, которые видит сам VAD.
///
/// Не ноль: sherpa на нуле собирает сегменты из отдельных окон и
/// захлёбывается. Одно окно — тот минимум, при котором своя сегментация
/// VAD ещё не спорит с нашей.
const MIN_DURATION: f32 = 0.032;

/// Решатель «есть ли речь» на Silero VAD.
pub struct SileroGate {
    detector: VoiceActivityDetector,
}

impl SileroGate {
    /// Открыть модель из `<data_root>/models/vad/`.
    pub fn open(data_root: impl AsRef<Path>, sample_rate: u32) -> Result<Self, String> {
        let model = resolve_vad_model(data_root)?;
        let config = VadModelConfig {
            silero_vad: SileroVadModelConfig {
                model: Some(model.to_string_lossy().into_owned()),
                threshold: THRESHOLD,
                min_silence_duration: MIN_DURATION,
                min_speech_duration: MIN_DURATION,
                window_size: WINDOW_SIZE,
                max_speech_duration: BUFFER_SECONDS,
            },
            sample_rate: sample_rate as i32,
            num_threads: 1,
            ..VadModelConfig::default()
        };
        let detector = VoiceActivityDetector::create(&config, BUFFER_SECONDS)
            .ok_or_else(|| "sherpa-onnx не открыл модель VAD".to_string())?;
        Ok(Self { detector })
    }
}

impl SpeechDecider for SileroGate {
    fn accepts_frame(&mut self, frame: &[i16]) -> bool {
        if frame.is_empty() {
            return false;
        }
        let samples: Vec<f32> = frame.iter().map(|s| *s as f32 / 32768.0).collect();
        self.detector.accept_waveform(&samples);
        let speaking = self.detector.detected();
        // Накопленные сегменты выбрасываются: нас интересует признак
        // «сейчас речь», а не собранные реплики — их собирает наш код,
        // тот же, что при гейте.
        self.detector.clear();
        speaking
    }

    fn name(&self) -> &'static str {
        "silero vad"
    }

    fn reset(&mut self) {
        self.detector.reset();
    }
}
