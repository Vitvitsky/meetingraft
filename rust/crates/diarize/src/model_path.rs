//! Поиск моделей разделения голосов на диске.
//!
//! Имена фиксированы, а не угадываются по шаблону. Соблазн взять «любой
//! `.onnx` в каталоге» здесь дорого стоит: в архиве сегментации рядом с
//! `model.onnx` лежит `model.int8.onnx`, и подстановка квантованной модели
//! вместо обычной не сломала бы ничего видимого — она просто разделяла бы
//! хуже, и объяснить потом, почему числа замера разошлись, было бы нечем.

use std::path::{Path, PathBuf};

/// Каталог моделей диаризации: `<data_root>/models/diarize/`.
///
/// `models/` повторяет `stt::models_dir`, а не заимствует его: `diarize`
/// от `stt` не зависит и зависеть не должен. Сегмент пути один, разойтись
/// ему почти негде, но если каталог моделей когда-нибудь переедет —
/// править надо оба места.
pub fn diarize_models_dir(data_root: impl AsRef<Path>) -> PathBuf {
    data_root.as_ref().join("models").join("diarize")
}

/// Имя файла сегментации (кто когда говорит).
pub const SEGMENTATION_FILE: &str = "segmentation.onnx";
/// Имя файла эмбеддинга (насколько голоса похожи).
pub const EMBEDDING_FILE: &str = "embedding.onnx";

/// Пара моделей, без которой диаризация невозможна.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiarizeModels {
    pub segmentation: PathBuf,
    pub embedding: PathBuf,
}

/// Найти обе модели или сказать, чего именно не хватает.
///
/// Ошибка называет **файл и каталог**, а не «модели не найдены»: человеку,
/// у которого скачалась одна из двух, второе сообщение не помогает вовсе.
pub fn resolve_diarize_models(data_root: impl AsRef<Path>) -> Result<DiarizeModels, String> {
    let dir = diarize_models_dir(data_root);
    let segmentation = dir.join(SEGMENTATION_FILE);
    let embedding = dir.join(EMBEDDING_FILE);

    let missing: Vec<&str> = [
        (SEGMENTATION_FILE, &segmentation),
        (EMBEDDING_FILE, &embedding),
    ]
    .iter()
    .filter(|(_, path)| !path.is_file())
    .map(|(name, _)| *name)
    .collect();

    if missing.is_empty() {
        return Ok(DiarizeModels {
            segmentation,
            embedding,
        });
    }
    Err(format!(
        "в {} нет файлов: {} (скачать — scripts/fetch-diarize-models.sh)",
        dir.display(),
        missing.join(", ")
    ))
}

/// Отпечаток модели эмбеддинга: по чему судить, сравнимы ли слепки.
///
/// Считается **по содержимому файла**, а не по его имени. Имя здесь врёт
/// по построению: `embedding.onnx` называется так и до, и после замены
/// модели, и ровно на этом обожглись однажды — скрипт пропускал уже
/// лежащий файл, английская модель осталась вместо многоязычной, и замер
/// шёл прежней, ничего об этом не сказав. «Файл уже есть» и «файл тот,
/// который нужен» — разные утверждения.
///
/// Метка `.source`, заведённая тем же разбором, читается рядом и идёт в
/// ответ для человека, но решает не она: метку можно потерять, а
/// содержимое — нет.
///
/// Ошибка — когда файла нет вовсе. Пустой строки этот вызов не отдаёт:
/// пустой идентификатор сравнялся бы с пустым у другой модели, и слепки
/// разных моделей молча признались бы сравнимыми.
pub fn embedding_model_id(data_root: impl AsRef<Path>) -> Result<String, String> {
    let path = diarize_models_dir(data_root).join(EMBEDDING_FILE);
    let bytes = std::fs::read(&path)
        .map_err(|error| format!("модель {} не читается: {error}", path.display()))?;

    // FNV-1a: нужен различающий отпечаток, а не криптография.
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in &bytes {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }

    let marker = std::fs::read_to_string(path.with_extension("onnx.source"))
        .ok()
        .map(|name| name.trim().to_owned())
        .filter(|name| !name.is_empty());
    Ok(match marker {
        Some(name) => format!("{name}:{hash:016x}"),
        None => format!("{hash:016x}"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Отпечаток обязан меняться вместе с содержимым файла: на этом
    /// стоит всё правило «векторы разных моделей несравнимы».
    ///
    /// Заведомо положительный случай идёт первым: тот же файл — тот же
    /// отпечаток. Без него проверка «отличается» выполнялась бы и у
    /// функции, отдающей каждый раз новое число.
    #[test]
    fn the_model_id_follows_the_file_contents() {
        let root = tmp_root("model-id");
        let dir = diarize_models_dir(&root);
        std::fs::create_dir_all(&dir).expect("каталог");
        let embedding = dir.join(EMBEDDING_FILE);
        std::fs::write(&embedding, b"english model").expect("файл");

        let first = embedding_model_id(&root).expect("отпечаток");
        assert_eq!(first, embedding_model_id(&root).expect("повтор"));

        std::fs::write(&embedding, b"multilingual model").expect("замена");
        let second = embedding_model_id(&root).expect("отпечаток после замены");

        assert_ne!(
            first, second,
            "замена модели прошла незамеченной — тот самый дефект скрипта"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Имя файла не меняется при замене модели, и метка `.source` тоже
    /// может остаться прежней — она пишется скриптом, а модель мог
    /// положить и человек. Отпечаток обязан разойтись всё равно.
    #[test]
    fn the_model_id_does_not_trust_the_source_marker_alone() {
        let root = tmp_root("model-id-marker");
        let dir = diarize_models_dir(&root);
        std::fs::create_dir_all(&dir).expect("каталог");
        std::fs::write(dir.join(EMBEDDING_FILE), b"one").expect("файл");
        std::fs::write(dir.join("embedding.onnx.source"), "cam++").expect("метка");
        let first = embedding_model_id(&root).expect("отпечаток");

        std::fs::write(dir.join(EMBEDDING_FILE), b"another").expect("замена");
        let second = embedding_model_id(&root).expect("отпечаток");

        assert!(
            first.starts_with("cam++:"),
            "метка не попала в ответ: {first}"
        );
        assert_ne!(first, second);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Модели нет — отказ, а не пустая строка. Пустой идентификатор
    /// сравнялся бы с пустым у другой модели.
    #[test]
    fn a_missing_model_has_no_id_at_all() {
        let root = tmp_root("model-id-missing");
        assert!(embedding_model_id(&root).is_err());
    }

    fn tmp_root(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "mr-diarize-models-{name}-{:?}",
            std::thread::current().id()
        ))
    }

    fn touch(path: &Path) {
        std::fs::create_dir_all(path.parent().expect("родитель")).expect("каталог");
        std::fs::write(path, "не модель, но файл").expect("файл");
    }

    #[test]
    fn both_files_present_resolve() {
        let root = tmp_root("both");
        let _ = std::fs::remove_dir_all(&root);
        let dir = diarize_models_dir(&root);
        touch(&dir.join(SEGMENTATION_FILE));
        touch(&dir.join(EMBEDDING_FILE));

        let models = resolve_diarize_models(&root).expect("обе модели на месте");

        assert_eq!(models.segmentation, dir.join(SEGMENTATION_FILE));
        assert_eq!(models.embedding, dir.join(EMBEDDING_FILE));
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Одна из двух — тоже отказ, и отказ называет **недостающую**.
    ///
    /// Половина моделей хуже, чем их отсутствие: движок бы не поднялся, а
    /// человек искал бы причину в том файле, который скачал.
    #[test]
    fn a_half_download_names_the_missing_file() {
        let root = tmp_root("half");
        let _ = std::fs::remove_dir_all(&root);
        touch(&diarize_models_dir(&root).join(SEGMENTATION_FILE));

        let error = resolve_diarize_models(&root).expect_err("эмбеддинга нет");

        assert!(error.contains(EMBEDDING_FILE), "{error}");
        assert!(
            !error.contains(SEGMENTATION_FILE),
            "названо лишнее — этот файл на месте: {error}"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Пустой каталог — обе в списке.
    #[test]
    fn an_empty_dir_names_both() {
        let root = tmp_root("empty");
        let _ = std::fs::remove_dir_all(&root);

        let error = resolve_diarize_models(&root).expect_err("моделей нет");

        assert!(error.contains(SEGMENTATION_FILE), "{error}");
        assert!(error.contains(EMBEDDING_FILE), "{error}");
    }

    /// Каталог — это каталог, а не файл с таким именем.
    #[test]
    fn a_directory_is_not_a_model() {
        let root = tmp_root("dir-not-file");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(diarize_models_dir(&root).join(SEGMENTATION_FILE))
            .expect("каталог вместо файла");
        touch(&diarize_models_dir(&root).join(EMBEDDING_FILE));

        let error = resolve_diarize_models(&root).expect_err("сегментация — каталог");

        assert!(error.contains(SEGMENTATION_FILE), "{error}");
        let _ = std::fs::remove_dir_all(&root);
    }
}
