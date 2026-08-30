//! Поиск файлов многоязычного распознавателя parakeet на диске.
//!
//! Кладёт их `scripts/fetch-parakeet-models.sh`; приложение их не качает.
//!
//! Имена фиксированы и **отличаются от GigaAM**: здесь `decoder.int8.onnx`
//! и `joiner.int8.onnx`, у соседа — `decoder.onnx` и `joiner.onnx`.
//! Разница ровно в тех файлах, подстановка которых не сломала бы ничего
//! видимого: движок открылся бы и распознавал бы хуже.

use std::path::{Path, PathBuf};

/// Каталог модели: `<data_root>/models/parakeet/`.
pub fn parakeet_models_dir(data_root: impl AsRef<Path>) -> PathBuf {
    data_root.as_ref().join("models").join("parakeet")
}

/// Как эта модель называется в ошибках и в интерфейсе.
pub const PARAKEET_MODEL_ID: &str = "parakeet-tdt-0.6b-v3";

/// Энкодер FastConformer (int8, 622 МБ).
pub const PARAKEET_ENCODER_FILE: &str = "encoder.int8.onnx";
/// Предсказатель TDT.
pub const PARAKEET_DECODER_FILE: &str = "decoder.int8.onnx";
/// Объединяющая сеть.
pub const PARAKEET_JOINER_FILE: &str = "joiner.int8.onnx";
/// Словарь: BPE sentencepiece на 8193 позиции, не посимвольный.
pub const PARAKEET_TOKENS_FILE: &str = "tokens.txt";

/// Четыре файла, без которых движок не открыть.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParakeetModels {
    pub encoder: PathBuf,
    pub decoder: PathBuf,
    pub joiner: PathBuf,
    pub tokens: PathBuf,
}

/// Найти все четыре файла — или сказать, какого именно не хватает.
///
/// Ошибка называет файл и каталог, а не «модель не найдена»: человеку, у
/// которого недокачался один файл из четырёх, второе сообщение не
/// помогает вовсе.
pub fn resolve_parakeet_models(data_root: impl AsRef<Path>) -> Result<ParakeetModels, String> {
    let dir = parakeet_models_dir(data_root);
    let models = ParakeetModels {
        encoder: dir.join(PARAKEET_ENCODER_FILE),
        decoder: dir.join(PARAKEET_DECODER_FILE),
        joiner: dir.join(PARAKEET_JOINER_FILE),
        tokens: dir.join(PARAKEET_TOKENS_FILE),
    };
    let missing: Vec<&str> = [
        (&models.encoder, PARAKEET_ENCODER_FILE),
        (&models.decoder, PARAKEET_DECODER_FILE),
        (&models.joiner, PARAKEET_JOINER_FILE),
        (&models.tokens, PARAKEET_TOKENS_FILE),
    ]
    .into_iter()
    .filter(|(path, _)| !path.exists())
    .map(|(_, name)| name)
    .collect();

    if missing.is_empty() {
        Ok(models)
    } else {
        Err(format!(
            "в {} не хватает: {}. Скачать: scripts/fetch-parakeet-models.sh <каталог-данных>",
            dir.display(),
            missing.join(", ")
        ))
    }
}

/// Скачана ли модель целиком.
pub fn parakeet_ready(data_root: impl AsRef<Path>) -> bool {
    resolve_parakeet_models(data_root).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Пустой каталог — отказ, называющий все четыре файла.
    #[test]
    fn an_empty_directory_names_every_missing_file() {
        let error = resolve_parakeet_models(std::env::temp_dir().join("нет-такого-каталога"))
            .expect_err("обязан отказать");
        for name in [
            PARAKEET_ENCODER_FILE,
            PARAKEET_DECODER_FILE,
            PARAKEET_JOINER_FILE,
            PARAKEET_TOKENS_FILE,
        ] {
            assert!(error.contains(name), "в причине нет {name}: {error}");
        }
    }

    /// Имена не совпадают с GigaAM — иначе скрипт закачки и резолвер
    /// разошлись бы молча.
    #[test]
    fn the_file_names_differ_from_gigaam() {
        assert_ne!(PARAKEET_DECODER_FILE, crate::DECODER_FILE);
        assert_ne!(PARAKEET_JOINER_FILE, crate::JOINER_FILE);
    }
}
