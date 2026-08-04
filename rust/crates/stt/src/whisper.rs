//! On-device Whisper (whisper-rs + Metal).

use domain::{CaptionEvent, CaptionPhase, LanguagePolicy, SttDiagnostic, SttDiagnosticKind};
use uuid::Uuid;
use whisper_rs::{
    FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters, WhisperState,
    WhisperTokenId, convert_integer_to_float_audio, install_logging_hooks,
};

use crate::local_agreement::{HypothesisWord, LocalAgreement, backfill_end_ms, words_from_tokens};
use crate::{Stabilized, SttEngine};
use crate::{is_hallucination_prefix, is_whisper_hallucination};

/// Порог RMS для «есть речь» (выше → меньше галлюцинаций на тишине).
const ENERGY_THRESHOLD: f32 = 450.0;
const SILENCE_FRAMES: usize = 16_000 * 3 / 10;
const MIN_SPEECH_FRAMES: usize = 16_000 / 5;
/// Не гоняем Whisper чаще чем раз в ~1 с на partial.
const PARTIAL_MIN_FRAMES: usize = 16_000;
/// Сегмент с no_speech_prob выше порога отбрасываем.
const NO_SPEECH_PROB_MAX: f32 = 0.55;
/// Потолок неустойчивого хвоста: без него согласие может не наступить
/// на шумной речи и хвост будет расти бесконечно.
const MAX_PENDING_WORDS: usize = 24;
/// Потолок буфера — окно Whisper. Дальше режем принудительно.
const MAX_BUFFER_FRAMES: usize = 16_000 * 30;
/// Режем с запасом назад: тайм-коды на границе неточны, потерять контекст
/// дешевле, чем обрезать слово посередине.
const TRIM_GUARD_MS: u64 = 200;
/// Переменная окружения для замеров латентности (ADR-010, T6).
const TIMING_ENV: &str = "MEETINGRAFT_STT_TIMING";
/// Потолок несобранных записей диагностики между вычитками.
const MAX_PENDING_DIAGNOSTICS: usize = 256;

/// Глушим вывод whisper.cpp один раз на процесс.
///
/// Без хуков whisper.cpp пишет в stderr потокенный дамп декодера, и на
/// каждой итерации LocalAgreement это сотни строк. `log_backend` не
/// подключён, поэтому логи никуда не идут.
static SILENCE_LOGS: std::sync::Once = std::sync::Once::new();

/// Whisper STT с energy-VAD сегментацией.
pub struct WhisperSttEngine {
    ctx: WhisperContext,
    /// Переиспользуемое состояние декодера.
    ///
    /// `create_state` выделяет KV-кэши и compute-буферы (~240 МБ на
    /// `base`) и переинициализирует Metal. Создавать его на каждой
    /// итерации — а их теперь одна в секунду — значит платить эту цену
    /// целиком на каждый partial.
    state: Option<WhisperState>,
    policy: LanguagePolicy,
    buffer: Vec<i16>,
    speech_frames: usize,
    silence_frames: usize,
    in_speech: bool,
    frames_since_partial: usize,
    initial_prompt: String,
    agreement: LocalAgreement,
    /// Начало фразы, похожей на титры, придержанное до следующей порции.
    held_final: String,
    /// Что движок выбросил или придержал — для журнала слоем выше.
    diagnostics: Vec<SttDiagnostic>,
}

impl WhisperSttEngine {
    pub fn open(model_path: impl AsRef<std::path::Path>) -> Result<Self, String> {
        let path = model_path.as_ref();
        let ctx = WhisperContext::new_with_params(
            path.to_string_lossy().as_ref(),
            WhisperContextParameters::default(),
        )
        .map_err(|e| format!("whisper load: {e}"))?;
        SILENCE_LOGS.call_once(install_logging_hooks);
        Ok(Self {
            ctx,
            state: None,
            policy: LanguagePolicy::default_v1(),
            buffer: Vec::new(),
            speech_frames: 0,
            silence_frames: 0,
            in_speech: false,
            frames_since_partial: 0,
            initial_prompt: String::new(),
            agreement: LocalAgreement::new(MAX_PENDING_WORDS),
            held_final: String::new(),
            diagnostics: Vec::new(),
        })
    }

    fn rms(pcm: &[i16]) -> f32 {
        if pcm.is_empty() {
            return 0.0;
        }
        let sum: f64 = pcm.iter().map(|s| f64::from(*s) * f64::from(*s)).sum();
        ((sum / pcm.len() as f64).sqrt()) as f32
    }

    fn event(text: String, phase: CaptionPhase) -> CaptionEvent {
        CaptionEvent::new(Uuid::new_v4().to_string(), text.to_string(), phase)
    }

    fn accept_text(text: &str) -> Option<String> {
        let trimmed = text.trim();
        if trimmed.is_empty() || is_whisper_hallucination(trimmed) {
            None
        } else {
            Some(trimmed.to_string())
        }
    }

    /// Фильтр окончательного текста с памятью на один шаг.
    ///
    /// LocalAgreement фиксирует текст порциями (ADR-010), поэтому
    /// «Субтитры сделал DimaTorzok» приходит по кускам, и каждый кусок по
    /// отдельности фильтр проходит. Здесь начало известной фразы
    /// придерживается до следующей порции, где становится ясно, речь это
    /// была или титры.
    fn accept_final(&mut self, text: &str) -> Option<String> {
        let trimmed = text.trim();
        if trimmed.is_empty() {
            return None;
        }
        let candidate = if self.held_final.is_empty() {
            trimmed.to_string()
        } else {
            format!("{} {trimmed}", self.held_final)
        };
        self.held_final.clear();
        let buffer_ms = (self.buffer.len() / 16) as u64;
        if is_whisper_hallucination(&candidate) {
            self.note(
                SttDiagnosticKind::DroppedHallucination,
                &candidate,
                buffer_ms,
            );
            return None;
        }
        if is_hallucination_prefix(&candidate) {
            self.note(SttDiagnosticKind::HeldPrefix, &candidate, buffer_ms);
            self.held_final = candidate;
            return None;
        }
        Some(candidate)
    }

    /// Запомнить решение. Потолок не даёт журналу расти без предела,
    /// если движок «поехал»: тогда важны первые записи, а не последние.
    fn note(&mut self, kind: SttDiagnosticKind, text: &str, buffer_ms: u64) {
        if self.diagnostics.len() >= MAX_PENDING_DIAGNOSTICS {
            return;
        }
        self.diagnostics
            .push(SttDiagnostic::new(kind, text, buffer_ms));
    }

    /// Отдать придержанный текст: реплика кончилась, продолжения не будет.
    ///
    /// Молча его терять нельзя — это оказалась настоящая речь, просто
    /// похожая началом на титры.
    fn release_held(&mut self) -> Option<CaptionEvent> {
        let held = std::mem::take(&mut self.held_final);
        let buffer_ms = (self.buffer.len() / 16) as u64;
        let text = Self::accept_text(&held)?;
        self.note(SttDiagnosticKind::ReleasedHeld, &text, buffer_ms);
        Some(Self::event(text, CaptionPhase::Final))
    }

    /// Гипотеза по буферу: слова с временем окончания.
    ///
    /// Ассоциированная функция, а не метод: состояние берётся отдельным
    /// мутабельным заимствованием, чтобы буфер можно было передать рядом.
    fn hypothesis(
        state: &mut WhisperState,
        pcm: &[i16],
        language: &str,
        prompt: &str,
        special_token_floor: WhisperTokenId,
    ) -> Vec<HypothesisWord> {
        if pcm.len() < MIN_SPEECH_FRAMES / 2 {
            return Vec::new();
        }
        let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
        params.set_language(Some(language));
        params.set_print_special(false);
        params.set_print_progress(false);
        params.set_print_realtime(false);
        params.set_print_timestamps(false);
        // Тайм-коды нужны для обрезки буфера.
        params.set_single_segment(false);
        params.set_token_timestamps(true);
        // Контекст между итерациями НЕ переносим: на коротком буфере
        // Whisper пересказывает промпт, и зафиксированный текст возвращался
        // бы как новая гипотеза — одна фраза повторялась бы бесконечно
        // (ADR-010, раздел «Откат»).
        params.set_no_context(true);
        params.set_suppress_blank(true);
        params.set_temperature(0.0);
        // Документация whisper-rs: no_speech_thold historically stub — всё равно
        // фильтруем по segment.no_speech_probability() ниже.
        params.set_no_speech_thold(0.6);
        if !prompt.is_empty() {
            params.set_initial_prompt(prompt);
        }

        let mut audio = vec![0.0f32; pcm.len()];
        if convert_integer_to_float_audio(pcm, &mut audio).is_err() {
            return Vec::new();
        }
        if state.full(params, &audio).is_err() {
            return Vec::new();
        }

        let mut words: Vec<HypothesisWord> = Vec::new();
        for index in 0..state.full_n_segments() {
            let Some(segment) = state.get_segment(index) else {
                continue;
            };
            if segment.no_speech_probability() > NO_SPEECH_PROB_MAX {
                continue;
            }
            if segment
                .to_str()
                .is_ok_and(|text| is_whisper_hallucination(text.trim()))
            {
                continue;
            }
            // Время сегмента whisper.cpp заполняет всегда, время токенов —
            // нет. Первое служит запасным для второго.
            let segment_end_ms = (segment.end_timestamp().max(0) as u64) * 10;
            let mut segment_tokens: Vec<(String, u64)> = Vec::new();
            for token_index in 0..segment.n_tokens() {
                let Some(token) = segment.get_token(token_index) else {
                    continue;
                };
                // Служебные токены (в т.ч. тайм-коды `<|1.20|>`) идут с id не
                // меньше eot — иначе они попали бы в текст как «слова».
                if token.token_id() >= special_token_floor {
                    continue;
                }
                let Ok(text) = token.to_str() else {
                    continue;
                };
                // t1 в сотых долях секунды от начала буфера.
                let end_ms = (token.token_data().t1.max(0) as u64) * 10;
                segment_tokens.push((text.to_string(), end_ms));
            }
            let mut segment_words = words_from_tokens(&segment_tokens);
            backfill_end_ms(&mut segment_words, segment_end_ms);
            words.append(&mut segment_words);
        }
        // Между сегментами время тоже обязано расти.
        backfill_end_ms(&mut words, 0);
        // Титры whisper.cpp иногда режет по тайм-кодам на два сегмента —
        // «Субтитры сделал» и имя, — и каждый по отдельности проходит
        // посегментную проверку выше. Гипотеза целиком её не проходит.
        let joined = words
            .iter()
            .map(|word| word.text.as_str())
            .collect::<Vec<_>>()
            .join(" ");
        if is_whisper_hallucination(&joined) {
            return Vec::new();
        }
        words
    }

    /// Прогнать текущий буфер через переиспользуемое состояние.
    fn current_hypothesis(&mut self) -> Vec<HypothesisWord> {
        if self.state.is_none() {
            self.state = self.ctx.create_state().ok();
        }
        let language = self.policy.primary.code();
        // Только глоссарий: зафиксированный текст в промпт не идёт.
        let prompt = self.initial_prompt.clone();
        let special_token_floor = self.ctx.token_eot();
        // Расщепление заимствований по полям: state — мутабельно,
        // buffer — нет.
        let Some(state) = self.state.as_mut() else {
            return Vec::new();
        };
        Self::hypothesis(state, &self.buffer, language, &prompt, special_token_floor)
    }

    /// Выбросить из буфера аудио до зафиксированной границы.
    fn trim_buffer(&mut self, until_ms: u64) {
        let cut_ms = until_ms.saturating_sub(TRIM_GUARD_MS);
        let frames = (cut_ms as usize) * 16;
        if frames == 0 || frames >= self.buffer.len() {
            return;
        }
        self.buffer.drain(0..frames);
        self.agreement.rebase(cut_ms);
    }

    /// Замеры для ADR-010: включаются `MEETINGRAFT_STT_TIMING=1`.
    ///
    /// Инструментовка держится за env, а не за фичу: числа снимают на
    /// собранном приложении, пересобирать ради этого не нужно.
    fn log_timing(inference_ms: u128, buffer_frames: usize, stabilized: &Stabilized) {
        if std::env::var_os(TIMING_ENV).is_none() {
            return;
        }
        let buffer_ms = buffer_frames / 16;
        let committed = stabilized.committed_text.split_whitespace().count();
        let pending = stabilized.pending_text.split_whitespace().count();
        eprintln!(
            "meetingraft-stt timing: inference={inference_ms}ms buffer={buffer_ms}ms \
             committed_words={committed} pending_words={pending}"
        );
    }

    /// События из результата стабилизации.
    fn events(&mut self, stabilized: &Stabilized) -> Vec<CaptionEvent> {
        let mut out = Vec::new();
        if let Some(text) = self.accept_final(&stabilized.committed_text) {
            out.push(Self::event(text, CaptionPhase::Final));
        }
        if let Some(text) = Self::accept_text(&stabilized.pending_text) {
            out.push(Self::event(text, CaptionPhase::Partial));
        }
        out
    }

    fn reset_segment(&mut self) {
        self.buffer.clear();
        self.in_speech = false;
        self.speech_frames = 0;
        self.silence_frames = 0;
        self.frames_since_partial = 0;
        self.held_final.clear();
        self.agreement.reset();
    }
}

impl SttEngine for WhisperSttEngine {
    fn set_language_policy(&mut self, policy: LanguagePolicy) {
        self.policy = policy;
    }

    fn set_initial_prompt(&mut self, prompt: &str) {
        self.initial_prompt = prompt.to_owned();
    }

    fn push_pcm(&mut self, pcm: &[i16], _sample_rate: u32) -> Vec<CaptionEvent> {
        let mut out = Vec::new();
        let energy = Self::rms(pcm);
        if energy >= ENERGY_THRESHOLD {
            self.in_speech = true;
            self.silence_frames = 0;
            self.speech_frames += pcm.len();
            self.frames_since_partial += pcm.len();
            self.buffer.extend_from_slice(pcm);

            if self.buffer.len() > MAX_BUFFER_FRAMES {
                // Согласия долго нет: режем принудительно, иначе инференс
                // по растущему буферу съедает бюджет латентности.
                let overflow_ms = ((self.buffer.len() - MAX_BUFFER_FRAMES) / 16) as u64;
                self.trim_buffer(overflow_ms + TRIM_GUARD_MS);
            }

            if self.speech_frames >= MIN_SPEECH_FRAMES
                && self.frames_since_partial >= PARTIAL_MIN_FRAMES
            {
                let started = std::time::Instant::now();
                let hypothesis = self.current_hypothesis();
                let inference_ms = started.elapsed().as_millis();
                let stabilized = self.agreement.push(hypothesis);
                Self::log_timing(inference_ms, self.buffer.len(), &stabilized);
                out.extend(self.events(&stabilized));
                if let Some(until_ms) = stabilized.committed_until_ms {
                    self.trim_buffer(until_ms);
                }
                self.frames_since_partial = 0;
            }
        } else if self.in_speech {
            self.silence_frames += pcm.len();
            self.buffer.extend_from_slice(pcm);
            if self.silence_frames >= SILENCE_FRAMES {
                out.extend(self.flush());
            }
        }
        out
    }

    fn take_diagnostics(&mut self) -> Vec<SttDiagnostic> {
        std::mem::take(&mut self.diagnostics)
    }

    fn flush(&mut self) -> Vec<CaptionEvent> {
        if self.buffer.is_empty() && !self.in_speech {
            return Vec::new();
        }
        // Последняя гипотеза плюс принудительная фиксация остатка: контекста
        // больше не будет, ждать согласия не с чем.
        let hypothesis = self.current_hypothesis();
        let mut out = Vec::new();
        let stabilized = self.agreement.push(hypothesis);
        if let Some(text) = self.accept_final(&stabilized.committed_text) {
            out.push(Self::event(text, CaptionPhase::Final));
        }
        let tail = self.agreement.flush();
        if let Some(text) = self.accept_final(&tail.committed_text) {
            out.push(Self::event(text, CaptionPhase::Final));
        }
        // Реплика кончилась: придержанное начало продолжения не дождётся.
        out.extend(self.release_held());
        self.reset_segment();
        out
    }
}
