//! Случай стенда: звук, описание, эталон.
//!
//! Стенд читает **каталог**, а не базу приложения. Это не удобство:
//! данных приложения на Linux-машине нет вовсе, и зависимость от них
//! означала бы прибор, который здесь не запускается — то есть не
//! запускается там, где идёт разработка.

use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::wav;

/// Частота живого пути (ADR-005). Всё остальное — отказ с причиной.
pub const RATE: u32 = 16_000;

/// Как получен эталон.
///
/// Различие несущее, а не описательное. Правленный черновик движка
/// **систематически льстит** тому движку, с которого черновик взят:
/// человек правит ошибки, но не возвращает то, чего движок не услышал
/// вовсе. В числе этого не видно ничем, поэтому пометка едет вместе с
/// эталоном и печатается в отчёте сама.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ReferenceKind {
    /// Напечатан с нуля.
    Typed,
    /// Правленный черновик движка.
    EditedDraft,
    /// Эталона нет.
    None,
}

/// Описание случая — всё, что о записи известно независимо от движков.
#[derive(Debug, Clone, Deserialize)]
pub struct Meta {
    pub case: String,
    /// ADR-003: `ru` | `en` | `es`.
    pub language: String,
    /// Известное число людей. Ноль — неизвестно, и это законный ответ:
    /// выдуманное число здесь молча испортило бы cpWER.
    #[serde(default)]
    pub speakers_expected: u32,
    pub source: String,
    /// Epic 25. У записей до него — `false` навсегда, и метрики,
    /// зависящие от общего времени каналов, на таких не считаются.
    #[serde(default)]
    pub channel_clock_unified: bool,
    pub reference_kind: ReferenceKind,
    /// Какой отрезок покрыт эталоном, мс. WER считается только по нему:
    /// эталон почти всегда покрывает кусок, а не всю встречу.
    #[serde(default)]
    pub reference_covers_ms: Option<[u64; 2]>,
    #[serde(default)]
    pub notes: String,
}

/// Прочитанный случай.
#[derive(Debug)]
pub struct Case {
    pub dir: PathBuf,
    pub meta: Meta,
    pub sample_rate: u32,
    pub mic: Vec<i16>,
    pub system: Option<Vec<i16>>,
    pub reference: Option<String>,
}

impl Case {
    /// Длина записи в миллисекундах — по микрофонному каналу.
    pub fn duration_ms(&self) -> u64 {
        self.mic.len() as u64 * 1000 / u64::from(self.sample_rate.max(1))
    }
}

/// Прочитать случай из каталога.
pub fn load(dir: &Path) -> Result<Case, String> {
    let meta_path = dir.join("meta.toml");
    let text = std::fs::read_to_string(&meta_path)
        .map_err(|error| format!("{}: {error}", meta_path.display()))?;
    let meta: Meta =
        toml::from_str(&text).map_err(|error| format!("{}: {error}", meta_path.display()))?;

    let mic = read_channel(&dir.join("mic.wav"))?;
    let system_path = dir.join("system.wav");
    let system = if system_path.exists() {
        Some(read_channel(&system_path)?)
    } else {
        None
    };

    // Объявленный эталон, которого нет на диске, — отказ. Молчаливое
    // «эталона нет» поставило бы в отчёте прочерк там, где на самом деле
    // потерян файл, и отличить одно от другого было бы нечем.
    let reference = match meta.reference_kind {
        ReferenceKind::None => None,
        _ => {
            let path = dir.join("reference.txt");
            Some(
                std::fs::read_to_string(&path)
                    .map_err(|error| format!("{}: {error}", path.display()))?
                    .trim()
                    .to_string(),
            )
        }
    };

    Ok(Case {
        dir: dir.to_path_buf(),
        meta,
        sample_rate: RATE,
        mic,
        system,
        reference,
    })
}

/// Прочитать канал и убедиться, что это наш формат.
///
/// Чужая частота — отказ, а не молчаливый ресемпл: запись в 48 кГц,
/// принятая без слова, дала бы движку чужой звук и числа, не значащие
/// ничего.
fn read_channel(path: &Path) -> Result<Vec<i16>, String> {
    let wav = wav::read(path)?;
    if wav.sample_rate != RATE {
        return Err(format!(
            "{}: частота {}, а стенд работает только на {RATE}",
            path.display(),
            wav.sample_rate
        ));
    }
    Ok(wav.pcm)
}

#[cfg(test)]
mod tests {
    use super::*;

    const META: &str = r#"
case = "test-case"
language = "ru"
speakers_expected = 2
source = "meetingraft:TEST"
channel_clock_unified = true
reference_kind = "typed"
reference_covers_ms = [0, 1000]
notes = "проверка"
"#;

    /// Каталог случая во временном месте. Имя своё у каждого теста —
    /// иначе они дерутся за один каталог и падают через раз.
    fn write_case(name: &str, meta: &str, with_reference: bool) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("bench-case-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("создать каталог случая");
        std::fs::write(dir.join("meta.toml"), meta).expect("записать meta");
        wav::write(&dir.join("mic.wav"), &vec![0i16; 16_000], 16_000).expect("записать mic.wav");
        if with_reference {
            std::fs::write(dir.join("reference.txt"), "привет мир").expect("записать эталон");
        }
        dir
    }

    #[test]
    fn a_case_directory_loads_with_audio_and_meta() {
        let dir = write_case("full", META, true);
        let case = load(&dir).expect("случай обязан прочитаться");
        assert_eq!(case.meta.language, "ru");
        assert_eq!(case.meta.reference_kind, ReferenceKind::Typed);
        assert_eq!(case.meta.reference_covers_ms, Some([0, 1000]));
        assert_eq!(case.sample_rate, 16_000);
        assert_eq!(case.mic.len(), 16_000, "звук обязан приехать целиком");
        assert_eq!(case.duration_ms(), 1000);
        assert_eq!(case.reference.as_deref(), Some("привет мир"));
        assert!(case.system.is_none(), "системного канала здесь нет");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Эталон объявлен, а файла нет — это отказ, а не случай без эталона.
    #[test]
    fn a_declared_reference_that_is_missing_is_a_refusal() {
        let dir = write_case("no-ref", META, false);
        let error = load(&dir).expect_err("обязан отказать");
        assert!(
            error.contains("reference.txt"),
            "в причине должно быть имя файла: {error}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// А не объявленный и отсутствующий — законный случай без эталона.
    ///
    /// Стоит рядом с предыдущим не для симметрии: без него отказ мог бы
    /// срабатывать всегда, и проверка выше проходила бы по неверной
    /// причине.
    #[test]
    fn a_case_without_a_reference_loads_fine() {
        let meta = META.replace(r#"reference_kind = "typed""#, r#"reference_kind = "none""#);
        let dir = write_case("kind-none", &meta, false);
        let case = load(&dir).expect("случай без эталона законен");
        assert!(case.reference.is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Чужая частота — отказ, а не молчаливый ресемпл.
    #[test]
    fn a_case_at_the_wrong_sample_rate_is_refused() {
        let dir = write_case("wrong-rate", META, true);
        wav::write(&dir.join("mic.wav"), &vec![0i16; 48_000], 48_000).expect("записать 48 кГц");
        let error = load(&dir).expect_err("обязан отказать");
        assert!(
            error.contains("48000"),
            "в причине должна быть частота: {error}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
