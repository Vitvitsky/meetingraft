//! Журнал замеров: по строке на прогон, дописывается всегда.
//!
//! Спека — `docs/superpowers/specs/2026-08-30-labelled-datasets-design.md`.
//!
//! ## Почему версия разметки обязательна
//!
//! Без неё история врёт молча. WER упал с 0.21 до 0.18 — от того, что
//! движок стал лучше, или от того, что вчера поправили эталон? Числа без
//! привязки к тому, **с чем** их сравнивали, показывают прогресс одинаково
//! убедительно в обоих случаях.
//!
//! По той же причине пишется коммит: код стенда меняется чаще движков, и
//! правка нарезки сдвигает WER не хуже смены модели.
//!
//! ## И откуда взят эталон
//!
//! Их два: разметка по фразам и `reference.txt` с одним отрезком. Они
//! дают **разные числа** на одной записи — считают по разному материалу.
//! Молча смешать их в одном журнале значит построить график из двух
//! величин под одним именем.
//!
//! ## Формат — JSONL
//!
//! Строка на прогон, дописывание в конец. Не JSON-массив: тот пришлось бы
//! перечитывать и переписывать целиком, и прерванная запись теряла бы
//! всю историю разом.

use serde::{Deserialize, Serialize};

/// Откуда взят эталон прогона.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ReferenceSource {
    /// Размеченные фразы: `labels.json`, только проверенные.
    Labels,
    /// Старый путь: `reference.txt` плюс `reference_covers_ms`.
    ReferenceText,
    /// Эталона не было — WER не считался.
    None,
}

impl ReferenceSource {
    pub fn name(self) -> &'static str {
        match self {
            Self::Labels => "разметка",
            Self::ReferenceText => "reference.txt",
            Self::None => "нет",
        }
    }
}

/// Одна запись журнала.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Entry {
    pub at_ms: u64,
    pub case: String,
    pub engine: String,
    pub segmentation: String,
    pub biasing: String,
    /// Какая модель стояла за движком. У whisper это меняется от файла к
    /// файлу, поэтому имя, а не константа движка.
    pub model_id: String,
    pub reference_source: ReferenceSource,
    /// Версия разметки. `None`, когда эталон взят не из неё — тогда
    /// привязывать не к чему, и выдумывать ноль нельзя.
    pub labels_version: Option<u32>,
    /// Сколько проверенных фраз участвовало. `None` для старого пути.
    pub phrases: Option<usize>,
    pub wer: Option<f32>,
    pub cer: Option<f32>,
    pub segments: usize,
    pub ms_per_second: f32,
    /// Коммит стенда. Пустая строка — если репозиторий не опрошен.
    #[serde(default)]
    pub commit: String,
}

/// Имя файла журнала внутри каталога стенда.
pub const FILE: &str = "history.jsonl";

/// Дописать запись.
///
/// Дописывание, а не перезапись: журнал переживает прерванный прогон, и
/// это единственная причина формата.
pub fn append(path: &std::path::Path, entry: &Entry) -> Result<(), String> {
    use std::io::Write;

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|error| format!("{}: {error}", parent.display()))?;
    }
    let line = serde_json::to_string(entry).map_err(|error| error.to_string())?;
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|error| format!("{}: {error}", path.display()))?;
    writeln!(file, "{line}").map_err(|error| format!("{}: {error}", path.display()))
}

/// Прочитать журнал.
///
/// Битая строка **не молчит**: она пропускается, а её номер уезжает
/// вызывающему. Журнал, который тихо теряет записи, показывает историю
/// без провалов — то есть врёт ровно там, где что-то пошло не так.
pub fn load(path: &std::path::Path) -> Result<(Vec<Entry>, Vec<usize>), String> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        // Журнала ещё нет — это пустая история, а не ошибка.
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok((Vec::new(), Vec::new()));
        }
        Err(error) => return Err(format!("{}: {error}", path.display())),
    };

    let mut entries = Vec::new();
    let mut broken = Vec::new();
    for (number, line) in text.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        match serde_json::from_str::<Entry>(line) {
            Ok(entry) => entries.push(entry),
            Err(_) => broken.push(number + 1),
        }
    }
    Ok((entries, broken))
}

/// Коммит, на котором собран стенд.
///
/// Пустая строка, если спросить не у кого: журнал без коммита хуже
/// журнала с пустым полем ровно настолько, насколько выдуманный коммит
/// хуже отсутствующего.
pub fn current_commit() -> String {
    std::process::Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .map(|text| text.trim().to_string())
        .unwrap_or_default()
}

/// Сравнимы ли две записи между собой.
///
/// Не сравнимы — разный случай, разный источник эталона либо разная
/// версия разметки. Это и есть весь смысл журнала: показать, что число
/// изменилось **при прочих равных**, а где равных не было — сказать об
/// этом, а не поставить их рядом на график.
pub fn comparable(left: &Entry, right: &Entry) -> bool {
    left.case == right.case
        && left.reference_source == right.reference_source
        && left.labels_version == right.labels_version
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(case: &str, engine: &str, version: Option<u32>, wer: f32) -> Entry {
        Entry {
            at_ms: 1,
            case: case.to_string(),
            engine: engine.to_string(),
            segmentation: "vad".to_string(),
            biasing: "none".to_string(),
            model_id: "тест".to_string(),
            reference_source: ReferenceSource::Labels,
            labels_version: version,
            phrases: Some(10),
            wer: Some(wer),
            cer: Some(wer / 3.0),
            segments: 12,
            ms_per_second: 100.0,
            commit: "abc1234".to_string(),
        }
    }

    fn temp(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("bench-history-{}-{name}.jsonl", std::process::id()))
    }

    /// Записи дописываются и читаются обратно.
    #[test]
    fn entries_survive_a_round_trip() {
        let path = temp("round");
        let _ = std::fs::remove_file(&path);

        append(&path, &entry("встреча", "gigaam", Some(3), 0.21)).expect("записалось");
        append(&path, &entry("встреча", "parakeet", Some(3), 0.18)).expect("записалось");

        let (entries, broken) = load(&path).expect("прочиталось");
        assert_eq!(entries.len(), 2);
        assert!(broken.is_empty());
        assert_eq!(entries[1].engine, "parakeet");
        assert_eq!(entries[1].labels_version, Some(3));
        let _ = std::fs::remove_file(&path);
    }

    /// Журнала ещё нет — это пустая история, а не ошибка.
    #[test]
    fn a_missing_journal_is_an_empty_history() {
        let (entries, broken) = load(&temp("missing")).expect("не ошибка");
        assert!(entries.is_empty());
        assert!(broken.is_empty());
    }

    /// Битая строка пропускается, но её номер называется.
    ///
    /// Журнал, который молча теряет записи, показывает историю без
    /// провалов — то есть врёт ровно там, где что-то пошло не так.
    #[test]
    fn a_broken_line_is_named_not_swallowed() {
        let path = temp("broken");
        let _ = std::fs::remove_file(&path);
        append(&path, &entry("встреча", "gigaam", Some(1), 0.2)).expect("записалось");
        {
            use std::io::Write;
            let mut file = std::fs::OpenOptions::new()
                .append(true)
                .open(&path)
                .unwrap();
            writeln!(file, "{{это не запись").unwrap();
        }
        append(&path, &entry("встреча", "tone", Some(1), 0.3)).expect("записалось");

        let (entries, broken) = load(&path).expect("прочиталось");
        assert_eq!(entries.len(), 2, "целые записи на месте");
        assert_eq!(broken, vec![2], "битая названа по номеру");
        let _ = std::fs::remove_file(&path);
    }

    /// Записи с разной версией разметки несравнимы.
    ///
    /// Это и есть то, ради чего версия в журнале: иначе падение WER от
    /// правки эталона выглядело бы как улучшение движка.
    #[test]
    fn a_different_labels_version_makes_entries_incomparable() {
        let old = entry("встреча", "gigaam", Some(2), 0.21);
        let new = entry("встреча", "gigaam", Some(3), 0.18);
        assert!(!comparable(&old, &new));

        let same = entry("встреча", "parakeet", Some(2), 0.18);
        assert!(comparable(&old, &same), "тот же эталон — сравнимы");
    }

    /// И записи с разным источником эталона — тоже.
    #[test]
    fn a_different_reference_source_makes_entries_incomparable() {
        let mut by_text = entry("встреча", "gigaam", None, 0.21);
        by_text.reference_source = ReferenceSource::ReferenceText;
        let by_labels = entry("встреча", "gigaam", Some(1), 0.21);
        assert!(!comparable(&by_text, &by_labels));
    }

    /// Разные записи — тоже несравнимы, даже при одинаковых числах.
    #[test]
    fn different_cases_are_never_comparable() {
        assert!(!comparable(
            &entry("одна", "gigaam", Some(1), 0.2),
            &entry("другая", "gigaam", Some(1), 0.2)
        ));
    }
}
