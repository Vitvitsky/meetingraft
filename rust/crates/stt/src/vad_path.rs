//! Поиск модели Silero VAD на диске.
//!
//! Кладёт её `scripts/fetch-vad-model.sh` — два мегабайта, в отличие от
//! распознавателей. Приложение её не качает: до чисел прибора никакой
//! настройки VAD не заводится вовсе.

use std::path::{Path, PathBuf};

/// Каталог модели: `<data_root>/models/vad/`.
///
/// Третий сегмент `models/`, написанный явно (после `stt` и `diarize`);
/// связывать ради него крейты дороже, чем повторить. Переедет каталог
/// моделей — править все места.
pub fn vad_models_dir(data_root: impl AsRef<Path>) -> PathBuf {
    data_root.as_ref().join("models").join("vad")
}

/// Имя файла модели. Совпадает с тем, что раздаёт k2-fsa.
pub const SILERO_FILE: &str = "silero_vad.onnx";

/// Найти модель или сказать, чего не хватает.
pub fn resolve_vad_model(data_root: impl AsRef<Path>) -> Result<PathBuf, String> {
    let path = vad_models_dir(data_root).join(SILERO_FILE);
    if path.is_file() {
        return Ok(path);
    }
    Err(format!(
        "нет модели VAD: {} — скачать: scripts/fetch-vad-model.sh <каталог-данных>",
        path.display()
    ))
}

/// Готов ли VAD **на этой сборке и на этой машине**.
///
/// Два условия сразу, как и у русского движка: фича собрана и файл на
/// месте. Без первого движка нет в бинаре, без второго нечего грузить.
pub fn vad_ready(data_root: impl AsRef<Path>) -> bool {
    cfg!(feature = "vad") && resolve_vad_model(data_root).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_root() -> PathBuf {
        std::env::temp_dir().join(format!(
            "mr-vad-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
    }

    #[test]
    fn the_model_is_found_by_its_fixed_name() {
        let root = temp_root();
        let dir = vad_models_dir(&root);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(SILERO_FILE), b"x").unwrap();

        let found = resolve_vad_model(&root).expect("модель");

        assert!(found.ends_with(SILERO_FILE));
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Отказ называет путь и способ это починить: «модель не найдена» не
    /// помогает никому.
    #[test]
    fn the_error_names_the_path_and_the_script() {
        let root = temp_root();

        let error = resolve_vad_model(&root).expect_err("модели нет");

        assert!(error.contains(SILERO_FILE), "{error}");
        assert!(error.contains("fetch-vad-model"), "{error}");
    }

    /// Без фичи готовности нет, даже если файл лежит: движка в сборке
    /// нет вовсе, и «готов» означало бы обещание, которого не исполнить.
    #[test]
    fn readiness_needs_the_feature_too() {
        let root = temp_root();
        let dir = vad_models_dir(&root);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(SILERO_FILE), b"x").unwrap();

        assert_eq!(vad_ready(&root), cfg!(feature = "vad"));
        let _ = std::fs::remove_dir_all(&root);
    }
}
