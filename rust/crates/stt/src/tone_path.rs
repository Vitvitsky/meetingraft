//! Поиск файлов потокового распознавателя T-one на диске.
//!
//! Кладёт их `scripts/fetch-tone-model.sh`; приложение их не качает.
//!
//! Файлов здесь два, а не четыре: у CTC нет отдельных декодера и
//! объединяющей сети — вся модель одним графом.

use std::path::{Path, PathBuf};

/// Каталог модели: `<data_root>/models/tone/`.
pub fn tone_models_dir(data_root: impl AsRef<Path>) -> PathBuf {
    data_root.as_ref().join("models").join("tone")
}

/// Как эта модель называется в ошибках и в интерфейсе.
pub const TONE_MODEL_ID: &str = "t-one-russian";

/// Потоковый Conformer-CTC одним графом.
pub const TONE_MODEL_FILE: &str = "model.onnx";
/// Алфавит: посимвольный, 35 позиций.
pub const TONE_TOKENS_FILE: &str = "tokens.txt";

/// Два файла, без которых движок не открыть.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToneModel {
    pub model: PathBuf,
    pub tokens: PathBuf,
}

/// Найти оба файла — или сказать, какого именно не хватает.
pub fn resolve_tone_model(data_root: impl AsRef<Path>) -> Result<ToneModel, String> {
    let dir = tone_models_dir(data_root);
    let model = ToneModel {
        model: dir.join(TONE_MODEL_FILE),
        tokens: dir.join(TONE_TOKENS_FILE),
    };
    let missing: Vec<&str> = [
        (&model.model, TONE_MODEL_FILE),
        (&model.tokens, TONE_TOKENS_FILE),
    ]
    .into_iter()
    .filter(|(path, _)| !path.exists())
    .map(|(_, name)| name)
    .collect();

    if missing.is_empty() {
        Ok(model)
    } else {
        Err(format!(
            "в {} не хватает: {}. Скачать: scripts/fetch-tone-model.sh <каталог-данных>",
            dir.display(),
            missing.join(", ")
        ))
    }
}

/// Скачана ли модель целиком.
pub fn tone_ready(data_root: impl AsRef<Path>) -> bool {
    resolve_tone_model(data_root).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Пустой каталог — отказ, называющий оба файла.
    #[test]
    fn an_empty_directory_names_every_missing_file() {
        let error = resolve_tone_model(std::env::temp_dir().join("нет-такого-каталога"))
            .expect_err("обязан отказать");
        assert!(error.contains(TONE_MODEL_FILE), "{error}");
        assert!(error.contains(TONE_TOKENS_FILE), "{error}");
    }
}
