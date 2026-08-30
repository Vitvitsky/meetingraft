//! Отрезки речи от Silero VAD.
//!
//! Отдельно от [`crate::SileroGate`], и это не дублирование. Гейт
//! отвечает на вопрос «сейчас речь?» покадрово и **выбрасывает**
//! накопленные сегменты (`detector.clear()` после каждого кадра); здесь
//! нужен ровно противоположный режим — собрать границы реплик и вернуть
//! их все. Один объект с двумя режимами означал бы, что живой путь и
//! пакетный проход делят состояние детектора, а ходят они по звуку
//! по-разному.
//!
//! Живого пути это не касается: здесь пакетная нарезка записи, которая
//! уже кончилась.

use std::path::Path;

use sherpa_onnx::{SileroVadModelConfig, VadModelConfig, VoiceActivityDetector};

use crate::vad_path::resolve_vad_model;

/// Окно Silero: 512 отсчётов при 16 кГц (32 мс). Значение модели, не наш
/// выбор.
const WINDOW_SIZE: i32 = 512;
/// Сколько секунд отсчётов держит внутренний буфер детектора.
///
/// Он же потолок длины одного отрезка: речь длиннее буфера детектор
/// закрывает сам. Тридцать секунд взяты равными потолку куска у стенда —
/// иначе непрерывный монолог давал бы отрезок, который всё равно
/// пришлось бы резать.
const BUFFER_SECONDS: f32 = 30.0;
/// Порог решения. Тот же, что у гейта: сравнивать нарезку с гейтом можно
/// только при одинаковом пороге.
const THRESHOLD: f32 = 0.5;
/// Пауза короче этой реплику не разрывает.
///
/// У гейта здесь одно окно (32 мс): ему нужно мгновенное «речь или нет».
/// Нарезке нужно другое — граница между репликами, а не между словами, и
/// 350 мс это межсловную паузу переживают.
const MIN_SILENCE: f32 = 0.35;
/// Речь короче этой отрезком не считается.
const MIN_SPEECH: f32 = 0.2;

/// Границы отрезков речи, мс от начала записи.
///
/// Пустой вход даёт пустой список — это законно и означает «речи нет».
/// Отказ движка едет `Err`: сломанный проход, выглядящий как тишина, —
/// худшее, что может отдать нарезка, потому что дальше он превращается в
/// пустую расшифровку.
pub fn speech_segments(
    data_root: impl AsRef<Path>,
    pcm: &[i16],
    sample_rate: u32,
) -> Result<Vec<(u64, u64)>, String> {
    if pcm.is_empty() || sample_rate == 0 {
        return Ok(Vec::new());
    }
    let model = resolve_vad_model(data_root)?;
    let config = VadModelConfig {
        silero_vad: SileroVadModelConfig {
            model: Some(model.to_string_lossy().into_owned()),
            threshold: THRESHOLD,
            min_silence_duration: MIN_SILENCE,
            min_speech_duration: MIN_SPEECH,
            window_size: WINDOW_SIZE,
            max_speech_duration: BUFFER_SECONDS,
        },
        sample_rate: sample_rate as i32,
        num_threads: 1,
        ..VadModelConfig::default()
    };
    let detector = VoiceActivityDetector::create(&config, BUFFER_SECONDS)
        .ok_or_else(|| "sherpa-onnx не открыл модель VAD".to_string())?;

    let samples: Vec<f32> = pcm.iter().map(|sample| *sample as f32 / 32768.0).collect();
    let mut out = Vec::new();
    for chunk in samples.chunks(WINDOW_SIZE as usize) {
        detector.accept_waveform(chunk);
        drain(&detector, sample_rate, &mut out);
    }
    // Без этого последняя реплика теряется целиком, если запись кончилась
    // на речи: детектор держит её в буфере и ждёт тишины, которой уже не
    // будет. Молча потерянный хвост встречи — ровно тот молчаливый отказ,
    // который здесь считается худшим исходом.
    detector.flush();
    drain(&detector, sample_rate, &mut out);
    Ok(out)
}

/// Забрать всё, что детектор уже собрал.
fn drain(detector: &VoiceActivityDetector, sample_rate: u32, out: &mut Vec<(u64, u64)>) {
    while let Some(segment) = detector.front() {
        let start_ms = segment.start() as u64 * 1000 / u64::from(sample_rate);
        let end_ms = start_ms + segment.n() as u64 * 1000 / u64::from(sample_rate);
        out.push((start_ms, end_ms));
        // `front` держит заимствование сегмента; уронить его надо до
        // `pop`, иначе читаем освобождённую память.
        drop(segment);
        detector.pop();
    }
}
