//! Поиск локальной ggml-модели Whisper.

use std::path::{Path, PathBuf};

/// Каталог моделей относительно data root: `…/models/`.
pub fn models_dir(data_root: impl AsRef<Path>) -> PathBuf {
    data_root.as_ref().join("models")
}

/// Имя файла ggml для известного id модели; `auto` и неизвестные id → `None`.
pub fn whisper_filename_for_id(model_id: &str) -> Option<&'static str> {
    match model_id {
        "base" => Some("ggml-base.bin"),
        "small" => Some("ggml-small.bin"),
        "large-v3-turbo" => Some("ggml-large-v3-turbo.bin"),
        _ => None,
    }
}

/// Первый найденный `ggml-*.bin` с учётом предпочтения пользователя.
///
/// `preferred` = `None` или `"auto"` — приоритет: large-v3-turbo → base → small → любой.
/// Явный id (`base` / `small` / `large-v3-turbo`) — только этот файл или `None`.
pub fn resolve_whisper_model(
    data_root: impl AsRef<Path>,
    preferred: Option<&str>,
) -> Option<PathBuf> {
    let dir = models_dir(data_root);
    if !dir.is_dir() {
        return None;
    }

    if let Some(id) = preferred
        && id != "auto"
        && let Some(filename) = whisper_filename_for_id(id)
    {
        let path = dir.join(filename);
        return path.is_file().then_some(path);
    }

    let priority = [
        "ggml-large-v3-turbo.bin",
        "ggml-large-v3-turbo-q5_0.bin",
        "ggml-base.bin",
        "ggml-small.bin",
        "ggml-tiny.bin",
    ];
    for name in priority {
        let path = dir.join(name);
        if path.is_file() {
            return Some(path);
        }
    }
    let mut found: Vec<PathBuf> = std::fs::read_dir(&dir)
        .ok()?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| {
            p.extension().and_then(|e| e.to_str()) == Some("bin")
                && p.file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.starts_with("ggml-"))
        })
        .collect();
    found.sort();
    found.into_iter().next()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tempfile() -> PathBuf {
        std::env::temp_dir().join(format!(
            "mr-models-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
    }

    #[test]
    fn resolve_prefers_named_file() {
        let root = tempfile();
        let models = models_dir(&root);
        std::fs::create_dir_all(&models).unwrap();
        std::fs::write(models.join("ggml-base.bin"), b"x").unwrap();
        let resolved = resolve_whisper_model(&root, None).unwrap();
        assert!(resolved.ends_with("ggml-base.bin"));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn resolve_missing_dir_is_none() {
        let root = std::env::temp_dir().join("mr-models-missing-xyz");
        let _ = std::fs::remove_dir_all(&root);
        assert!(resolve_whisper_model(&root, None).is_none());
    }

    #[test]
    fn preferred_base_selects_base_even_if_turbo_present() {
        let root = tempfile();
        let models = models_dir(&root);
        std::fs::create_dir_all(&models).unwrap();
        std::fs::write(models.join("ggml-large-v3-turbo.bin"), b"t").unwrap();
        std::fs::write(models.join("ggml-base.bin"), b"b").unwrap();
        let path = resolve_whisper_model(&root, Some("base")).unwrap();
        assert!(path.ends_with("ggml-base.bin"));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn preferred_missing_returns_none() {
        let root = tempfile();
        std::fs::create_dir_all(models_dir(&root)).unwrap();
        assert!(resolve_whisper_model(&root, Some("small")).is_none());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn auto_prefers_turbo_over_base() {
        let root = tempfile();
        let models = models_dir(&root);
        std::fs::create_dir_all(&models).unwrap();
        std::fs::write(models.join("ggml-base.bin"), b"b").unwrap();
        std::fs::write(models.join("ggml-large-v3-turbo.bin"), b"t").unwrap();
        let path = resolve_whisper_model(&root, Some("auto")).unwrap();
        assert!(path.ends_with("ggml-large-v3-turbo.bin"));
        let _ = std::fs::remove_dir_all(&root);
    }
}
