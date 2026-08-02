//! Поиск локальной ggml-модели Whisper.

use std::path::{Path, PathBuf};

/// Каталог моделей относительно data root: `…/models/`.
pub fn models_dir(data_root: impl AsRef<Path>) -> PathBuf {
    data_root.as_ref().join("models")
}

/// Первый найденный `ggml-*.bin` (предпочтение: large-v3-turbo → base → любой).
pub fn resolve_whisper_model(data_root: impl AsRef<Path>) -> Option<PathBuf> {
    let dir = models_dir(data_root);
    if !dir.is_dir() {
        return None;
    }
    let preferred = [
        "ggml-large-v3-turbo.bin",
        "ggml-large-v3-turbo-q5_0.bin",
        "ggml-base.bin",
        "ggml-small.bin",
        "ggml-tiny.bin",
    ];
    for name in preferred {
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

    #[test]
    fn resolve_prefers_named_file() {
        let root = std::env::temp_dir().join(format!(
            "mr-models-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let models = models_dir(&root);
        std::fs::create_dir_all(&models).unwrap();
        std::fs::write(models.join("ggml-base.bin"), b"x").unwrap();
        let resolved = resolve_whisper_model(&root).unwrap();
        assert!(resolved.ends_with("ggml-base.bin"));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn resolve_missing_dir_is_none() {
        let root = std::env::temp_dir().join("mr-models-missing-xyz");
        let _ = std::fs::remove_dir_all(&root);
        assert!(resolve_whisper_model(&root).is_none());
    }
}
