//! Поиск файлов русского распознавателя GigaAM на диске.
//!
//! Кладёт их `scripts/fetch-gigaam-models.sh`; приложение их не качает.
//!
//! Имена фиксированы, а не угадываются по шаблону — по той же причине,
//! что и у `diarize::model_path`: в соседнем экспорте того же семейства
//! (CTC вместо RNNT) лежит `model.int8.onnx` почти того же размера, и
//! подстановка чужого файла не сломала бы ничего видимого. Движок просто
//! распознавал бы хуже, а объяснить расхождение чисел замера было бы
//! нечем.

use std::path::{Path, PathBuf};

/// Каталог модели GigaAM: `<data_root>/models/gigaam/`.
///
/// `models/` повторяет [`crate::models_dir`] третий раз в репозитории
/// (второй — `diarize`), и это осознанно: сегмент пути один, а связывать
/// ради него крейты дороже. Переедет каталог моделей — править все три.
pub fn gigaam_models_dir(data_root: impl AsRef<Path>) -> PathBuf {
    data_root.as_ref().join("models").join("gigaam")
}

/// Энкодер Conformer (int8).
pub const ENCODER_FILE: &str = "encoder.int8.onnx";
/// Предсказатель RNNT.
pub const DECODER_FILE: &str = "decoder.onnx";
/// Объединяющая сеть RNNT.
pub const JOINER_FILE: &str = "joiner.onnx";
/// Алфавит: посимвольный, не subword.
pub const TOKENS_FILE: &str = "tokens.txt";

/// Четыре файла, без которых движок не открыть.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GigaamModels {
    pub encoder: PathBuf,
    pub decoder: PathBuf,
    pub joiner: PathBuf,
    pub tokens: PathBuf,
}

/// Найти все четыре файла — или сказать, какого именно не хватает.
///
/// Ошибка называет **файл и каталог**, а не «модель не найдена»:
/// человеку, у которого недокачался один файл из четырёх, второе
/// сообщение не помогает вовсе (урок `resolve_diarize_models`).
pub fn resolve_gigaam_models(data_root: impl AsRef<Path>) -> Result<GigaamModels, String> {
    let dir = gigaam_models_dir(data_root);
    let models = GigaamModels {
        encoder: dir.join(ENCODER_FILE),
        decoder: dir.join(DECODER_FILE),
        joiner: dir.join(JOINER_FILE),
        tokens: dir.join(TOKENS_FILE),
    };

    let missing: Vec<&str> = [
        (&models.encoder, ENCODER_FILE),
        (&models.decoder, DECODER_FILE),
        (&models.joiner, JOINER_FILE),
        (&models.tokens, TOKENS_FILE),
    ]
    .into_iter()
    .filter(|(path, _)| !path.is_file())
    .map(|(_, name)| name)
    .collect();

    if missing.is_empty() {
        return Ok(models);
    }

    Err(format!(
        "в {} не хватает: {} — скачать: scripts/fetch-gigaam-models.sh <каталог-данных>",
        dir.display(),
        missing.join(", ")
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Каталог с файлами нужных имён — пустыми, содержимое здесь не при
    /// чём: это поиск, а не проверка модели.
    ///
    /// Свой временный каталог, как в `model_path`: `tempfile` в
    /// зависимостях workspace нет, и заводить его ради трёх тестов
    /// значило бы тащить крейт в сборку приложения.
    fn make_root(files: &[&str]) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "mr-gigaam-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let models = gigaam_models_dir(&root);
        std::fs::create_dir_all(&models).unwrap();
        for name in files {
            std::fs::write(models.join(name), b"x").unwrap();
        }
        root
    }

    #[test]
    fn all_four_files_resolve() {
        let root = make_root(&[ENCODER_FILE, DECODER_FILE, JOINER_FILE, TOKENS_FILE]);
        let models = resolve_gigaam_models(&root).expect("resolve");
        assert!(models.encoder.ends_with(ENCODER_FILE));
        assert!(models.tokens.ends_with(TOKENS_FILE));
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Три файла из четырёх — самый вероятный исход недокачки, и он
    /// обязан назвать **недостающий**, а не пожаловаться вообще.
    #[test]
    fn the_error_names_the_missing_file_and_the_directory() {
        let root = make_root(&[ENCODER_FILE, DECODER_FILE, TOKENS_FILE]);
        let error = resolve_gigaam_models(&root).expect_err("joiner отсутствует");
        assert!(error.contains(JOINER_FILE), "не назван файл: {error}");
        assert!(
            error.contains(&gigaam_models_dir(&root).display().to_string()),
            "не назван каталог: {error}"
        );
        // И отдельно: про имеющиеся файлы в жалобе речи нет — иначе
        // человек пойдёт перекачивать всё.
        assert!(!error.contains(ENCODER_FILE), "лишнее в жалобе: {error}");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Пустой каталог назван целиком: тут перекачивать и правда всё.
    #[test]
    fn an_empty_directory_names_every_file() {
        let root = make_root(&[]);
        let error = resolve_gigaam_models(&root).expect_err("файлов нет");
        for name in [ENCODER_FILE, DECODER_FILE, JOINER_FILE, TOKENS_FILE] {
            assert!(error.contains(name), "не назван {name}: {error}");
        }
        let _ = std::fs::remove_dir_all(&root);
    }
}
