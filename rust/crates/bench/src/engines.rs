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
    pub fn open(data_root: &std::path::Path) -> Result<Self, String> {
        stt::GigaamRecognizer::open(data_root)
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
    pub fn open(data_root: &std::path::Path) -> Result<Self, String> {
        stt::ParakeetRecognizer::open(data_root)
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

/// Открыть потоковый движок по имени. `Ok(None)` — движок не потоковый.
pub fn open_streaming(
    name: &str,
    data_root: &std::path::Path,
) -> Result<Option<Box<dyn StreamTranscribe>>, String> {
    let _ = data_root;
    match name {
        #[cfg(feature = "tone")]
        "tone" => {
            Tone::open(data_root).map(|engine| Some(Box::new(engine) as Box<dyn StreamTranscribe>))
        }
        #[cfg(not(feature = "tone"))]
        "tone" => Err("стенд собран без --features tone".to_string()),
        _ => Ok(None),
    }
}

/// Потоковый ли это движок — вопрос к имени, а не к сборке.
///
/// Отвечает одинаково с фичей и без неё: иначе отказ «нарезка потоковому
/// не задаётся» появлялся бы и исчезал от флагов сборки.
pub fn is_streaming(name: &str) -> bool {
    name == "tone"
}

/// Открыть движок по имени.
pub fn open(name: &str, data_root: &std::path::Path) -> Result<Box<dyn Recognize>, String> {
    // Без единой фичи движка каталог данных никому не нужен, и компилятор
    // об этом говорит. Он прав, но параметр остаётся: сборка с фичей и
    // без неё должна отличаться только тем, что внутри.
    let _ = data_root;
    match name {
        #[cfg(feature = "gigaam")]
        "gigaam" => Gigaam::open(data_root).map(|engine| Box::new(engine) as Box<dyn Recognize>),
        #[cfg(not(feature = "gigaam"))]
        "gigaam" => Err("стенд собран без --features gigaam".to_string()),
        #[cfg(feature = "parakeet")]
        "parakeet" => {
            Parakeet::open(data_root).map(|engine| Box::new(engine) as Box<dyn Recognize>)
        }
        #[cfg(not(feature = "parakeet"))]
        "parakeet" => Err("стенд собран без --features parakeet".to_string()),
        other => Err(format!("движка {other} стенд не знает")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Признак «потоковый» отвечает одинаково с фичей и без неё.
    ///
    /// Иначе отказ «нарезка потоковому не задаётся» появлялся бы и
    /// исчезал от флагов сборки — то есть правило продукта зависело бы от
    /// того, что скачано на машине.
    #[test]
    fn the_streaming_flag_does_not_depend_on_features() {
        assert!(is_streaming("tone"));
        assert!(!is_streaming("gigaam"));
        assert!(!is_streaming("parakeet"));
    }

    /// Незнакомое имя потоковым не считается — иначе опечатка в имени
    /// движка молча меняла бы правила прогона.
    #[test]
    fn an_unknown_engine_is_not_streaming() {
        assert!(!is_streaming("нет-такого"));
    }
}
