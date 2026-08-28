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
        other => Err(format!("движка {other} стенд не знает")),
    }
}
