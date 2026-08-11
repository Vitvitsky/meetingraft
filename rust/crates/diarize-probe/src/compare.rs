//! Сравнение найденных голосов с разметкой человека.
//!
//! Лучший контроль из возможных: настоящая встреча, наша речь, и ответ
//! проставлен человеком. Чужие записи с известным числом людей отвечают
//! только на вопрос «видит ли движок смену вообще»; здесь видно, **тех
//! ли** он разделил.
//!
//! Считается всё в миллисекундах пересечения, а не в отрезках: отрезки у
//! человека и у движка нарезаны по-разному и по числу не сравнимы вовсе.
//!
//! ## Чего это сравнение не доказывает
//!
//! Расхождение — не обязательно ошибка движка. Человек размечал
//! **реплики в транскрипте**, а не голоса на слух: если внутри одной
//! реплики заговорил второй, разметка этого не знает, а движок мог
//! услышать правильно. Поэтому здесь считаются доли и называются обе
//! стороны расхождения, а вердикта «движок неправ» не выносится.

use std::collections::BTreeMap;

use diarize::VoiceTurn;

/// Отрезок, о котором человек сказал, кто говорит.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Labelled {
    pub speaker: String,
    pub start_ms: u64,
    pub end_ms: u64,
}

impl Labelled {
    pub fn duration_ms(&self) -> u64 {
        self.end_ms.saturating_sub(self.start_ms)
    }
}

/// Насколько разметка и найденные голоса сошлись.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Agreement {
    /// Сколько времени размечено человеком.
    pub labelled_ms: u64,
    /// Сколько из него движок вообще накрыл отрезками.
    pub covered_ms: u64,
    /// Пересечение по парам: (спикер, кластер) -> миллисекунды.
    pub overlap: BTreeMap<(String, u32), u64>,
    /// Кто на кого лёг при лучшем взаимно-однозначном соответствии.
    pub mapping: Vec<(String, u32, u64)>,
    /// Сколько накрытого времени объясняет это соответствие.
    pub matched_ms: u64,
}

impl Agreement {
    /// Доля размеченного времени, которую движок вообще накрыл.
    ///
    /// Считается первой и печатается первой: при низком покрытии все
    /// проценты ниже описывают крохотный кусок встречи и читаются как
    /// оценка целого.
    pub fn coverage(&self) -> f64 {
        ratio(self.covered_ms, self.labelled_ms)
    }

    /// Доля накрытого времени, попавшая в «свой» кластер.
    pub fn accuracy(&self) -> f64 {
        ratio(self.matched_ms, self.covered_ms)
    }

    /// Насколько цельно каждый человек лёг в один кластер.
    ///
    /// Ниже единицы — движок **разорвал** человека на несколько голосов.
    pub fn per_speaker_wholeness(&self) -> Vec<(String, f64, u64)> {
        let mut total: BTreeMap<&String, u64> = BTreeMap::new();
        let mut best: BTreeMap<&String, u64> = BTreeMap::new();
        for ((speaker, _), ms) in &self.overlap {
            *total.entry(speaker).or_default() += ms;
            let slot = best.entry(speaker).or_default();
            *slot = (*slot).max(*ms);
        }
        total
            .into_iter()
            .map(|(speaker, all)| {
                (
                    speaker.clone(),
                    ratio(best.get(speaker).copied().unwrap_or(0), all),
                    all,
                )
            })
            .collect()
    }

    /// Насколько чист каждый кластер.
    ///
    /// Ниже единицы — движок **слил** нескольких людей в один голос.
    pub fn per_cluster_purity(&self) -> Vec<(u32, f64, u64)> {
        let mut total: BTreeMap<u32, u64> = BTreeMap::new();
        let mut best: BTreeMap<u32, u64> = BTreeMap::new();
        for ((_, cluster), ms) in &self.overlap {
            *total.entry(*cluster).or_default() += ms;
            let slot = best.entry(*cluster).or_default();
            *slot = (*slot).max(*ms);
        }
        total
            .into_iter()
            .map(|(cluster, all)| {
                (
                    cluster,
                    ratio(best.get(&cluster).copied().unwrap_or(0), all),
                    all,
                )
            })
            .collect()
    }
}

/// Сопоставить разметку и найденные голоса.
///
/// Соответствие ищется жадно — самая большая клетка первой, — а не
/// оптимально. Разница с оптимальным возможна, и потому число называется
/// долей объяснённого времени, а не «точностью движка».
pub fn compare(labels: &[Labelled], turns: &[VoiceTurn]) -> Agreement {
    let mut out = Agreement {
        labelled_ms: labels.iter().map(Labelled::duration_ms).sum(),
        ..Agreement::default()
    };

    for label in labels {
        for turn in turns {
            let ms = overlap_ms(label.start_ms, label.end_ms, turn.start_ms, turn.end_ms);
            if ms == 0 {
                continue;
            }
            out.covered_ms += ms;
            *out.overlap
                .entry((label.speaker.clone(), turn.cluster))
                .or_default() += ms;
        }
    }

    // Жадное соответствие: пока есть непристроенные пары, берём самую
    // весомую и вычёркиваем её строку и столбец.
    let mut cells: Vec<(&String, u32, u64)> = out
        .overlap
        .iter()
        .map(|((speaker, cluster), ms)| (speaker, *cluster, *ms))
        .collect();
    // Порядок при равных весах фиксируем именами: иначе одна и та же база
    // давала бы разные отчёты от запуска к запуску.
    cells.sort_by(|a, b| b.2.cmp(&a.2).then(a.0.cmp(b.0)).then(a.1.cmp(&b.1)));

    let mut used_speakers: Vec<&String> = Vec::new();
    let mut used_clusters: Vec<u32> = Vec::new();
    for (speaker, cluster, ms) in cells {
        if used_speakers.contains(&speaker) || used_clusters.contains(&cluster) {
            continue;
        }
        used_speakers.push(speaker);
        used_clusters.push(cluster);
        out.matched_ms += ms;
        out.mapping.push((speaker.clone(), cluster, ms));
    }
    out.mapping.sort_by_key(|(_, _, ms)| std::cmp::Reverse(*ms));
    out
}

/// Пересечение двух полуинтервалов в миллисекундах.
fn overlap_ms(a_start: u64, a_end: u64, b_start: u64, b_end: u64) -> u64 {
    a_end.min(b_end).saturating_sub(a_start.max(b_start))
}

fn ratio(part: u64, whole: u64) -> f64 {
    if whole == 0 {
        return 0.0;
    }
    part as f64 / whole as f64
}

#[cfg(test)]
mod tests {
    use super::*;

    fn label(speaker: &str, start_ms: u64, end_ms: u64) -> Labelled {
        Labelled {
            speaker: speaker.to_string(),
            start_ms,
            end_ms,
        }
    }

    /// Заведомо совпавший случай: движок нашёл ровно то, что разметил
    /// человек, только под своими номерами.
    #[test]
    fn a_perfect_split_explains_all_of_it() {
        let labels = vec![label("аня", 0, 10_000), label("боря", 10_000, 20_000)];
        let turns = vec![
            VoiceTurn::new(0, 10_000, 7),
            VoiceTurn::new(10_000, 20_000, 2),
        ];

        let seen = compare(&labels, &turns);

        assert_eq!(seen.labelled_ms, 20_000, "разметки нет — считать нечего");
        assert_eq!(seen.covered_ms, 20_000);
        assert!((seen.coverage() - 1.0).abs() < 1e-9);
        assert!((seen.accuracy() - 1.0).abs() < 1e-9);
        assert_eq!(
            seen.mapping,
            vec![
                ("аня".to_string(), 7, 10_000),
                ("боря".to_string(), 2, 10_000)
            ]
        );
    }

    /// Заведомо провальный случай: движок нашёл один голос там, где двое.
    /// Соответствие обязано объяснить только половину.
    #[test]
    fn a_merge_of_two_people_explains_half() {
        let labels = vec![label("аня", 0, 10_000), label("боря", 10_000, 20_000)];
        let turns = vec![VoiceTurn::new(0, 20_000, 0)];

        let seen = compare(&labels, &turns);

        assert_eq!(seen.covered_ms, 20_000, "накрыто должно быть всё");
        assert!(
            (seen.accuracy() - 0.5).abs() < 1e-9,
            "объяснено {:.2}",
            seen.accuracy()
        );
        let purity = seen.per_cluster_purity();
        assert_eq!(purity.len(), 1);
        assert!(
            (purity[0].1 - 0.5).abs() < 1e-9,
            "кластер из двух людей чист на {:.2}",
            purity[0].1
        );
    }

    /// Человека разорвало надвое: соответствие объясняет столько, сколько
    /// в большей половине, и цельность это называет.
    #[test]
    fn a_split_person_is_named_by_wholeness() {
        let labels = vec![label("аня", 0, 10_000)];
        let turns = vec![
            VoiceTurn::new(0, 7_000, 0),
            VoiceTurn::new(7_000, 10_000, 1),
        ];

        let seen = compare(&labels, &turns);

        let wholeness = seen.per_speaker_wholeness();
        assert_eq!(wholeness.len(), 1);
        assert!(
            (wholeness[0].1 - 0.7).abs() < 1e-9,
            "цельность {:.2}",
            wholeness[0].1
        );
        assert!((seen.accuracy() - 0.7).abs() < 1e-9);
    }

    /// Движок молчал там, где человек размечал: покрытие мало, и это
    /// обязано быть видно **до** любых процентов совпадения.
    ///
    /// Иначе «совпало 100%» на одной секунде из часа прочтётся как ответ
    /// про встречу.
    #[test]
    fn low_coverage_is_visible_before_accuracy() {
        let labels = vec![label("аня", 0, 100_000)];
        let turns = vec![VoiceTurn::new(0, 1_000, 0)];

        let seen = compare(&labels, &turns);

        assert!((seen.accuracy() - 1.0).abs() < 1e-9, "совпало всё накрытое");
        assert!(
            (seen.coverage() - 0.01).abs() < 1e-9,
            "накрыто {:.3}",
            seen.coverage()
        );
    }

    /// Пустая разметка — нули, а не паника и не деление на ноль.
    #[test]
    fn nothing_labelled_is_not_a_hundred_percent() {
        let seen = compare(&[], &[VoiceTurn::new(0, 1_000, 0)]);

        assert_eq!(seen.labelled_ms, 0);
        assert_eq!(seen.coverage(), 0.0);
        assert_eq!(seen.accuracy(), 0.0, "ноль из нуля — не совпадение");
    }

    /// Одному человеку — один кластер, даже если он лучший сразу для
    /// двоих: иначе соответствие объясняло бы одно и то же дважды.
    #[test]
    fn one_cluster_cannot_serve_two_people() {
        let labels = vec![label("аня", 0, 10_000), label("боря", 10_000, 16_000)];
        let turns = vec![
            VoiceTurn::new(0, 10_000, 0),
            VoiceTurn::new(10_000, 16_000, 0),
        ];

        let seen = compare(&labels, &turns);

        assert_eq!(seen.mapping.len(), 1, "кластер роздан дважды");
        assert_eq!(seen.matched_ms, 10_000);
    }

    /// Отрезки, не пересекающиеся вовсе, в покрытие не идут.
    #[test]
    fn turns_beside_the_labels_cover_nothing() {
        let labels = vec![label("аня", 0, 5_000)];
        let turns = vec![VoiceTurn::new(10_000, 20_000, 0)];

        let seen = compare(&labels, &turns);

        assert_eq!(seen.covered_ms, 0);
        assert!(seen.overlap.is_empty());
        assert_eq!(seen.labelled_ms, 5_000, "разметка обязана считаться");
    }

    /// Порядок отчёта не зависит от порядка входа: одна и та же база
    /// обязана давать один и тот же отчёт.
    #[test]
    fn equal_weights_do_not_shuffle_the_report() {
        let labels = vec![label("аня", 0, 10_000), label("боря", 10_000, 20_000)];
        let turns = vec![
            VoiceTurn::new(0, 10_000, 1),
            VoiceTurn::new(10_000, 20_000, 0),
        ];

        let straight = compare(&labels, &turns);
        let reversed = compare(
            &labels.iter().rev().cloned().collect::<Vec<_>>(),
            &turns.iter().rev().copied().collect::<Vec<_>>(),
        );

        assert_eq!(straight.mapping, reversed.mapping);
    }
}
