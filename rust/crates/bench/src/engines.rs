//! Движки, которые стенд умеет звать.
//!
//! Каждый за своей фичей, по образцу `stt/gigaam` и `diarize/model`:
//! `build.rs` крейта sherpa-onnx ходит в сеть, и сборка без фич не качает
//! ничего.
//!
//! **Отказ движка едет `Err`, а не пустым текстом.** Сломанный проход,
//! выглядящий как молчание, — худшее, что прибор может показать: он
//! неотличим от честной тишины, и разница всплывает уже в выводах.

/// Что услышал движок на одном куске.
#[derive(Debug, Default, Clone)]
pub struct Heard {
    pub text: String,
    /// Время окончания каждого слова, мс от начала **куска**.
    ///
    /// Отдельно от текста, потому что теряется отдельно: когда длины
    /// токенов и тайм-кодов расходятся, движок отдаёт правильный текст
    /// без времени, и на глаз такая потеря неотличима от нормы.
    pub word_end_ms: Vec<u64>,
}

/// Распознаватель одного куска.
pub trait Recognize {
    fn transcribe(&self, pcm: &[i16], sample_rate: u32) -> Result<Heard, String>;
    fn name(&self) -> &'static str;
}

#[cfg(feature = "gigaam")]
pub struct Gigaam(stt::GigaamRecognizer);

#[cfg(feature = "gigaam")]
impl Gigaam {
    pub fn open(
        data_root: &std::path::Path,
        biasing: Option<&stt::Biasing>,
    ) -> Result<Self, String> {
        stt::GigaamRecognizer::open_with(data_root, biasing)
            .map(Self)
            // Подробности про недостающие файлы `ModelMissing` не несёт
            // намеренно — там идентификатор модели для человека.
            // Спрашиваем их у резолвера сами, как это делает `stt-probe`.
            .map_err(|error| match stt::resolve_gigaam_models(data_root) {
                Err(details) => details,
                Ok(_) => error.to_string(),
            })
    }
}

#[cfg(feature = "gigaam")]
impl Recognize for Gigaam {
    fn transcribe(&self, pcm: &[i16], sample_rate: u32) -> Result<Heard, String> {
        let hypothesis = self
            .0
            .transcribe(pcm, sample_rate)
            .map_err(|error| error.to_string())?;
        Ok(Heard {
            text: hypothesis.text,
            word_end_ms: hypothesis.words.iter().map(|word| word.end_ms).collect(),
        })
    }

    fn name(&self) -> &'static str {
        "gigaam"
    }
}

#[cfg(feature = "parakeet")]
pub struct Parakeet(stt::ParakeetRecognizer);

#[cfg(feature = "parakeet")]
impl Parakeet {
    pub fn open(
        data_root: &std::path::Path,
        biasing: Option<&stt::Biasing>,
    ) -> Result<Self, String> {
        stt::ParakeetRecognizer::open_with(data_root, biasing)
            .map(Self)
            .map_err(|error| match stt::resolve_parakeet_models(data_root) {
                Err(details) => details,
                Ok(_) => error.to_string(),
            })
    }
}

#[cfg(feature = "parakeet")]
impl Recognize for Parakeet {
    fn transcribe(&self, pcm: &[i16], sample_rate: u32) -> Result<Heard, String> {
        let hypothesis = self
            .0
            .transcribe(pcm, sample_rate)
            .map_err(|error| error.to_string())?;
        Ok(Heard {
            text: hypothesis.text,
            word_end_ms: hypothesis.words.iter().map(|word| word.end_ms).collect(),
        })
    }

    fn name(&self) -> &'static str {
        "parakeet"
    }
}

/// Распознаватель, который сам ставит границы реплик.
///
/// Отдельный тип, а не флаг у [`Recognize`], и это не педантизм: у
/// потокового движка **другой контракт**. Ему подают запись целиком, а он
/// отдаёт готовые реплики; спросить у него «что в этом куске» можно, но
/// тогда мерился бы не он.
pub trait StreamTranscribe {
    fn transcribe_stream(
        &self,
        pcm: &[i16],
        sample_rate: u32,
    ) -> Result<Vec<domain::TranscriptSegment>, String>;
    fn name(&self) -> &'static str;
}

#[cfg(feature = "tone")]
pub struct Tone(stt::ToneStreamer);

#[cfg(feature = "tone")]
impl Tone {
    pub fn open(data_root: &std::path::Path) -> Result<Self, String> {
        stt::ToneStreamer::open(data_root)
            .map(Self)
            .map_err(|error| match stt::resolve_tone_model(data_root) {
                Err(details) => details,
                Ok(_) => error.to_string(),
            })
    }
}

#[cfg(feature = "tone")]
impl StreamTranscribe for Tone {
    fn transcribe_stream(
        &self,
        pcm: &[i16],
        sample_rate: u32,
    ) -> Result<Vec<domain::TranscriptSegment>, String> {
        self.0
            .transcribe_stream(pcm, sample_rate)
            .map_err(|error| error.to_string())
    }

    fn name(&self) -> &'static str {
        "tone"
    }
}

#[cfg(feature = "whisper")]
pub struct Whisper {
    // `transcribe_all` требует `&mut self`, а `Recognize::transcribe`
    // берёт `&self`: движок в стенде общий для всех кусков, и открывать
    // его заново на каждый — это 1.6 ГБ чтения с диска. Ячейка дешевле
    // смены контракта у трёх соседних движков.
    inner: std::cell::RefCell<stt::WhisperBatchTranscriber>,
}

#[cfg(feature = "whisper")]
impl Whisper {
    /// Модель берётся та, которую предпочитает приложение (`auto`), —
    /// иначе стенд мерил бы не то, что у человека работает.
    pub fn open(data_root: &std::path::Path, terms: &[String]) -> Result<Self, String> {
        let mut inner = stt::WhisperBatchTranscriber::open(
            data_root,
            "auto",
            domain::LanguagePolicy::default_v1(),
        )
        .map_err(|error| error.to_string())?;
        // Смещение у Whisper — **другой механизм**: не автомат по лучам,
        // а текст в декодер. Сравнивать «с глоссарием» у него и у
        // transducer'а можно по результату, но не по устройству.
        if !terms.is_empty() {
            inner.set_initial_prompt(&terms.join(", "));
        }
        Ok(Self {
            inner: std::cell::RefCell::new(inner),
        })
    }

    fn run(&self, pcm: &[i16], sample_rate: u32) -> Result<Vec<domain::TranscriptSegment>, String> {
        use stt::BatchTranscriber;
        let mut progress = |_: f32| true;
        self.inner
            .borrow_mut()
            .transcribe_all(pcm, sample_rate, &mut progress)
            .map_err(|error| error.to_string())
    }
}

#[cfg(feature = "whisper")]
impl Recognize for Whisper {
    /// Кусок отдаётся модели целиком, а её собственные сегменты
    /// склеиваются в один текст.
    ///
    /// Тут теряются **и** её сегментация, **и** тайм-коды слов: этот путь
    /// нужен, чтобы Whisper стоял в одной таблице с transducer'ами при
    /// нашей нарезке. Своя его нарезка меряется значением `native`.
    fn transcribe(&self, pcm: &[i16], sample_rate: u32) -> Result<Heard, String> {
        let segments = self.run(pcm, sample_rate)?;
        Ok(Heard {
            text: segments
                .iter()
                .map(|segment| segment.text.as_str())
                .collect::<Vec<_>>()
                .join(" "),
            word_end_ms: Vec::new(),
        })
    }

    fn name(&self) -> &'static str {
        "whisper"
    }
}

#[cfg(feature = "whisper")]
impl StreamTranscribe for Whisper {
    fn transcribe_stream(
        &self,
        pcm: &[i16],
        sample_rate: u32,
    ) -> Result<Vec<domain::TranscriptSegment>, String> {
        self.run(pcm, sample_rate)
    }

    fn name(&self) -> &'static str {
        "whisper"
    }
}

/// Открыть движок, ставящий границы сам. `Ok(None)` — движок не потоковый.
pub fn open_native(
    name: &str,
    data_root: &std::path::Path,
    terms: &[String],
) -> Result<Option<Box<dyn StreamTranscribe>>, String> {
    let _ = (data_root, terms);
    match name {
        #[cfg(feature = "tone")]
        "tone" => {
            Tone::open(data_root).map(|engine| Some(Box::new(engine) as Box<dyn StreamTranscribe>))
        }
        #[cfg(not(feature = "tone"))]
        "tone" => Err("стенд собран без --features tone".to_string()),
        #[cfg(feature = "whisper")]
        "whisper" => Whisper::open(data_root, terms)
            .map(|engine| Some(Box::new(engine) as Box<dyn StreamTranscribe>)),
        #[cfg(not(feature = "whisper"))]
        "whisper" => Err("стенд собран без --features whisper".to_string()),
        _ => Ok(None),
    }
}

/// Умеет ли движок ставить границы сам.
///
/// Отвечает одинаково с фичей и без неё: иначе правило сочетаемости
/// появлялось бы и исчезало от флагов сборки, то есть зависело бы от
/// того, что скачано на машине.
pub fn supports_native(name: &str) -> bool {
    matches!(name, "tone" | "whisper")
}

/// Работает ли движок **только** своими границами.
///
/// Разница с [`supports_native`] существенная и в одну сторону: T-one
/// принимает звук чанками и без собственного эндпойнтинга не работает
/// вовсе, а Whisper нашу нарезку принимает — просто теряет при ней свою.
pub fn requires_native(name: &str) -> bool {
    name == "tone"
}

/// Открыть движок по имени, при желании со смещением под глоссарий.
///
/// Смещение — не свойство движка, а свойство прогона: один и тот же
/// движок гоняется с ним и без него, и сравниваются два числа. Поэтому
/// оно передаётся сюда, а не хранится где-то рядом.
#[cfg(feature = "biasing")]
pub type BiasingRef<'a> = Option<&'a stt::Biasing>;
/// Без единой фичи движка тип смещения неоткуда взять, а сигнатура
/// должна остаться той же.
#[cfg(not(feature = "biasing"))]
pub type BiasingRef<'a> = Option<&'a ()>;

pub fn open(
    name: &str,
    data_root: &std::path::Path,
    biasing: BiasingRef<'_>,
    whisper_terms: Option<&[String]>,
) -> Result<Box<dyn Recognize>, String> {
    let _ = (biasing, whisper_terms);
    // Без единой фичи движка каталог данных никому не нужен, и компилятор
    // об этом говорит. Он прав, но параметр остаётся: сборка с фичей и
    // без неё должна отличаться только тем, что внутри.
    let _ = data_root;
    match name {
        #[cfg(feature = "gigaam")]
        "gigaam" => {
            Gigaam::open(data_root, biasing).map(|engine| Box::new(engine) as Box<dyn Recognize>)
        }
        #[cfg(not(feature = "gigaam"))]
        "gigaam" => Err("стенд собран без --features gigaam".to_string()),
        #[cfg(feature = "parakeet")]
        "parakeet" => {
            Parakeet::open(data_root, biasing).map(|engine| Box::new(engine) as Box<dyn Recognize>)
        }
        #[cfg(not(feature = "parakeet"))]
        "parakeet" => Err("стенд собран без --features parakeet".to_string()),
        #[cfg(feature = "whisper")]
        "whisper" => {
            // Смещение Whisper приезжает при открытии, а не отдельным
            // файлом: у него это initial_prompt, а не hotwords.
            let terms = whisper_terms.unwrap_or(&[]);
            Whisper::open(data_root, terms).map(|engine| Box::new(engine) as Box<dyn Recognize>)
        }
        #[cfg(not(feature = "whisper"))]
        "whisper" => Err("стенд собран без --features whisper".to_string()),
        other => Err(format!("движка {other} стенд не знает")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Признаки сочетаемости не зависят от фич сборки.
    #[test]
    fn the_boundary_rules_do_not_depend_on_features() {
        assert!(supports_native("tone"));
        assert!(supports_native("whisper"));
        assert!(!supports_native("gigaam"));
        assert!(!supports_native("parakeet"));
    }

    /// «Умеет сам» и «умеет только сам» — разные вещи, и Whisper стоит
    /// ровно между ними: своя нарезка у него есть, но нашу он принимает.
    #[test]
    fn whisper_can_take_our_cut_while_tone_cannot() {
        assert!(requires_native("tone"));
        assert!(!requires_native("whisper"));
    }

    /// Незнакомое имя своих границ не заявляет — иначе опечатка молча
    /// меняла бы правила прогона.
    #[test]
    fn an_unknown_engine_claims_nothing() {
        assert!(!supports_native("нет-такого"));
        assert!(!requires_native("нет-такого"));
    }
}
