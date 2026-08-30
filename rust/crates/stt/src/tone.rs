//! Потоковый русский распознаватель T-one через sherpa-onnx.
//!
//! Conformer-CTC от T-Software DC под Apache 2.0. От двух соседних
//! движков отличается не качеством, а **устройством**: он потоковый.
//! GigaAM и parakeet офлайновые — им подают кусок и ждут ответа целиком;
//! этот принимает звук чанками, держит состояние между ними и сам
//! говорит, где кончилась реплика.
//!
//! ## Отсюда — то, чего у него нет
//!
//! **У него нет оси нарезки.** Границы реплик даёт его собственный
//! эндпойнтинг: тишина после речи закрывает реплику, и `is_endpoint`
//! говорит об этом. Подать ему заранее нарезанные куски можно, но тогда
//! мерился бы не он: у потокового движка отбирали бы ровно то, что в нём
//! ценно. Поэтому стенд с ним ось нарезки **не сочетает**, а отказывает.
//!
//! ## Что здесь стоит померить, а не предположить
//!
//! Модель специализирована под **телефонию** — 8 кГц, узкая полоса. У нас
//! широкополосный звук встречи. Сколько она на этом теряет — вопрос, ради
//! которого движок и заведён; ответ даёт стенд.
//!
//! Токены посимвольные (35 позиций: русский алфавит, пробел, `<blk>`),
//! поэтому слова собираются [`words_from_char_tokens`], как у GigaAM, а
//! не как у Whisper. Ошибка здесь молчалива: текст остаётся верным,
//! расходится только число слов с тайм-кодами.

use std::path::Path;

use domain::TranscriptSegment;
use sherpa_onnx::{
    OnlineRecognizer, OnlineRecognizerConfig, OnlineStream, OnlineToneCtcModelConfig,
};

use crate::batch::BatchTranscribeError;
use crate::local_agreement::words_from_char_tokens;
use crate::tone_path::{TONE_MODEL_ID, resolve_tone_model};

/// Чанк подачи, мс.
///
/// Триста миллисекунд — шаг, которым работает сам T-one (его splitter
/// логвероятностей смотрит на такие сегменты). Своё число этот параметр
/// получит на встречах, если получит вовсе.
pub const CHUNK_MS: u64 = 300;

/// Сколько тишины дописывается в конец записи, мс.
///
/// **Замер, а не осторожность.** На контрольной записи последнее слово
/// («зеленый») терялось: `input_finished()` — это «вход кончился», а не
/// «досчитай»; потоковому энкодеру нужны настоящие кадры **после**
/// последнего слова, чтобы его выдать. Та же запись с двумя секундами
/// тишины в хвосте вернула слово, и WER упал с 0.115 до 0.077.
///
/// Секунда — с запасом больше чанка подачи. Живого пути это не касается:
/// там звук идёт дальше сам.
const TAIL_SILENCE_MS: u64 = 1000;

/// Сколько тишины после речи закрывает реплику, секунды.
///
/// Правило 2 sherpa: молчание после **распознанного** текста. Взято 1.2 с
/// — примерно столько длится пауза между репликами в разговоре, и вдвое
/// больше межсловной.
const RULE2_TRAILING_SILENCE: f32 = 1.2;
/// То же для тишины **до** первого слова: реплику не открывает.
const RULE1_TRAILING_SILENCE: f32 = 2.4;
/// Реплика длиннее этого закрывается принудительно, секунды.
///
/// Без потолка монолог без пауз стал бы одной репликой на всю встречу —
/// то самое, от чего затевался стенд.
const RULE3_UTTERANCE_LENGTH: f32 = 20.0;

fn num_threads() -> i32 {
    if let Some(value) = std::env::var_os("MEETINGRAFT_TONE_THREADS")
        && let Some(parsed) = value.to_str().and_then(|text| text.parse::<i32>().ok())
        && parsed > 0
    {
        return parsed;
    }
    std::thread::available_parallelism()
        .map(|count| (count.get() as i32).min(4))
        .unwrap_or(2)
}

/// Открытый потоковый распознаватель.
pub struct ToneStreamer {
    recognizer: OnlineRecognizer,
}

impl ToneStreamer {
    /// Открыть модель из `<data_root>/models/tone/`.
    pub fn open(data_root: impl AsRef<Path>) -> Result<Self, BatchTranscribeError> {
        let model =
            resolve_tone_model(data_root).map_err(|_| BatchTranscribeError::ModelMissing {
                model_id: TONE_MODEL_ID.to_string(),
            })?;

        let mut config = OnlineRecognizerConfig {
            enable_endpoint: true,
            rule1_min_trailing_silence: RULE1_TRAILING_SILENCE,
            rule2_min_trailing_silence: RULE2_TRAILING_SILENCE,
            rule3_min_utterance_length: RULE3_UTTERANCE_LENGTH,
            ..OnlineRecognizerConfig::default()
        };
        config.model_config.t_one_ctc = OnlineToneCtcModelConfig {
            model: Some(model.model.to_string_lossy().into_owned()),
        };
        config.model_config.tokens = Some(model.tokens.to_string_lossy().into_owned());
        config.model_config.num_threads = num_threads();

        let recognizer = OnlineRecognizer::create(&config).ok_or_else(|| {
            BatchTranscribeError::ModelLoad(
                "sherpa-onnx не открыл модель T-one: проверь, что файлы докачались целиком"
                    .to_string(),
            )
        })?;
        Ok(Self { recognizer })
    }

    /// Прогнать запись целиком, подавая её чанками, и собрать реплики.
    ///
    /// Запись уже кончилась — это не живой путь; чанки здесь ради того,
    /// чтобы движок работал так, как он устроен, а не получал всё разом.
    ///
    /// Границы реплик — его собственные. Наши сюда не подставляются.
    pub fn transcribe_stream(
        &self,
        pcm: &[i16],
        sample_rate: u32,
    ) -> Result<Vec<TranscriptSegment>, BatchTranscribeError> {
        if pcm.is_empty() || sample_rate == 0 {
            return Ok(Vec::new());
        }

        let stream = self.recognizer.create_stream();
        let chunk_frames = (CHUNK_MS * u64::from(sample_rate) / 1000) as usize;
        let mut segments = Vec::new();
        // Смещение реплики внутри записи движок сообщает сам
        // (`start_time`), но только пока реплика открыта; после `reset`
        // оно обнуляется. Поэтому позиция ведётся ещё и здесь — по
        // поданным отсчётам, а не по часам.
        let mut fed_frames = 0usize;

        // Тишина в хвосте — часть подачи, а не подгонка: без неё
        // последнее слово записи не выдаётся вовсе (см. TAIL_SILENCE_MS).
        let tail_frames = (TAIL_SILENCE_MS * u64::from(sample_rate) / 1000) as usize;
        let tail = vec![0i16; tail_frames];
        let fed: Vec<&[i16]> = pcm
            .chunks(chunk_frames)
            .chain(tail.chunks(chunk_frames))
            .collect();

        for chunk in fed {
            let audio: Vec<f32> = chunk.iter().map(|s| *s as f32 / 32768.0).collect();
            stream.accept_waveform(sample_rate as i32, &audio);
            fed_frames += chunk.len();

            while self.recognizer.is_ready(&stream) {
                self.recognizer.decode(&stream);
            }
            if self.recognizer.is_endpoint(&stream) {
                self.take(&stream, fed_frames, sample_rate, &mut segments);
                self.recognizer.reset(&stream);
            }
        }

        // Хвост: без этого последняя реплика теряется целиком, если
        // запись кончилась на речи. Молча потерянный конец встречи —
        // худший исход из возможных.
        stream.input_finished();
        while self.recognizer.is_ready(&stream) {
            self.recognizer.decode(&stream);
        }
        self.take(&stream, fed_frames, sample_rate, &mut segments);

        Ok(segments)
    }

    /// Забрать текущую гипотезу как законченную реплику.
    fn take(
        &self,
        stream: &OnlineStream,
        fed_frames: usize,
        sample_rate: u32,
        out: &mut Vec<TranscriptSegment>,
    ) {
        let Some(result) = self.recognizer.get_result(stream) else {
            return;
        };
        let text = result.text.trim().to_string();
        if text.is_empty() {
            return;
        }

        let now_ms = fed_frames as u64 * 1000 / u64::from(sample_rate);
        // `start_time` — начало открытой реплики от начала потока.
        // Отсутствие его не выдумывается: тогда реплика начинается там,
        // где кончилась предыдущая.
        let start_ms = result
            .start_time
            .map(|seconds| (seconds.max(0.0) * 1000.0) as u64)
            .unwrap_or_else(|| out.last().map(|last| last.end_ms).unwrap_or(0));

        // Конец реплики — по последнему тайм-коду, если он есть; иначе по
        // тому, сколько звука уже подано.
        let end_ms = match (&result.timestamps, result.start_time) {
            (Some(times), Some(_)) if !times.is_empty() => {
                let last = times.last().copied().unwrap_or(0.0).max(0.0);
                (start_ms + (last * 1000.0) as u64).min(now_ms)
            }
            _ => now_ms,
        };

        out.push(TranscriptSegment::new(start_ms, end_ms.max(start_ms), text));
    }

    /// Слова с тайм-кодами из текущей гипотезы — для проверки прибором.
    pub fn words(&self, stream: &OnlineStream) -> Vec<crate::local_agreement::HypothesisWord> {
        let Some(result) = self.recognizer.get_result(stream) else {
            return Vec::new();
        };
        let Some(times) = result
            .timestamps
            .as_ref()
            .filter(|times| times.len() == result.tokens.len())
        else {
            return Vec::new();
        };
        let tokens: Vec<(String, u64)> = result
            .tokens
            .iter()
            .cloned()
            .zip(
                times
                    .iter()
                    .map(|seconds| (seconds.max(0.0) * 1000.0) as u64),
            )
            .collect();
        words_from_char_tokens(&tokens)
    }
}
