//! Метрики нарезки: сколько реплик, какой длины, где стоят границы.
//!
//! Отвечают на вопрос, ради которого затевался стенд: режет ли проход по
//! речи или по часам. Нарезка окнами даёт ровные тридцать секунд и
//! границы в случайных местах разговора; нарезка по речи — разброс длин
//! и границы в паузах.
//!
//! **Само по себе число сегментов ничего не значит.** Движок, режущий на
//! каждом слове, даст их много и будет хуже. Поэтому рядом стоят длины и
//! доля границ, попавших в паузу: первое показывает, на что похожи
//! реплики, второе — не выдуманы ли границы вовсе.

use domain::TranscriptSegment;
use serde::{Deserialize, Serialize};

/// Насколько близко к краю реплики граница ещё считается «на краю», мс.
///
/// Двести миллисекунд — это межсловная пауза в быстрой речи: ближе к
/// краю движки расходятся между собой, а не с правдой.
const EDGE_TOLERANCE_MS: u64 = 200;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SegmentStats {
    pub count: usize,
    pub median_ms: u64,
    pub p10_ms: u64,
    pub p90_ms: u64,
    /// Доля записи, покрытая сегментами.
    pub coverage: f32,
    /// Доля внутренних границ, не разрезающих речь.
    ///
    /// Граница считается хорошей, если она **не лежит внутри** реплики:
    /// в паузе между репликами или на самом их краю. Первая версия
    /// требовала тишины в окне вокруг границы и объявляла плохой границу
    /// ровно в начале реплики — то есть идеальную. Тест это поймал.
    ///
    /// При одном сегменте границ нет вовсе, и здесь ноль. Это не «плохо»
    /// — это «мерить было нечего», и читать такую клетку надо вместе с
    /// `count`.
    pub boundaries_in_pause: f32,
}

/// Посчитать метрики нарезки.
///
/// `speech` — отрезки речи по независимому источнику (VAD). Пустой
/// список означает, что речь не размечена, и тогда доля границ в паузе
/// не считается: доля, посчитанная по отсутствующей разметке, вышла бы
/// единицей у любой нарезки — «речи нигде нет, значит все границы в
/// паузе». Это ровно та проверка, которая проходит на пустом входе.
pub fn segment_stats(
    segments: &[TranscriptSegment],
    speech: &[(u64, u64)],
    total_ms: u64,
) -> SegmentStats {
    let mut lengths: Vec<u64> = segments
        .iter()
        .map(|segment| segment.end_ms.saturating_sub(segment.start_ms))
        .collect();
    lengths.sort_unstable();
    let covered: u64 = lengths.iter().sum();

    let mut in_pause = 0usize;
    let mut boundaries = 0usize;
    if !speech.is_empty() {
        for pair in segments.windows(2) {
            boundaries += 1;
            if !cuts_speech(speech, pair[1].start_ms) {
                in_pause += 1;
            }
        }
    }

    SegmentStats {
        count: segments.len(),
        median_ms: quantile(&lengths, 0.5),
        p10_ms: quantile(&lengths, 0.1),
        p90_ms: quantile(&lengths, 0.9),
        coverage: if total_ms == 0 {
            0.0
        } else {
            covered as f32 / total_ms as f32
        },
        boundaries_in_pause: if boundaries == 0 {
            0.0
        } else {
            in_pause as f32 / boundaries as f32
        },
    }
}

/// Разрезает ли граница реплику посередине.
///
/// «Посередине» — строго внутри, с допуском у краёв: граница ровно в
/// начале или в конце реплики речь не режет, а именно её и должна давать
/// хорошая нарезка.
fn cuts_speech(speech: &[(u64, u64)], at_ms: u64) -> bool {
    speech.iter().any(|(start, end)| {
        let inner_start = start + EDGE_TOLERANCE_MS;
        let inner_end = end.saturating_sub(EDGE_TOLERANCE_MS);
        inner_start < at_ms && at_ms < inner_end
    })
}

/// Квантиль по отсортированному списку. Пустой список — ноль.
fn quantile(sorted: &[u64], q: f32) -> u64 {
    if sorted.is_empty() {
        return 0;
    }
    let index = ((sorted.len() - 1) as f32 * q).round() as usize;
    sorted[index.min(sorted.len() - 1)]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn segments(bounds: &[(u64, u64)]) -> Vec<TranscriptSegment> {
        bounds
            .iter()
            .map(|(start, end)| TranscriptSegment::new(*start, *end, "текст"))
            .collect()
    }

    /// Ожидаемое выводится из входа, а не вписывается числом: вписанное
    /// пришлось бы подбирать под ответ и молча ломалось бы от правки
    /// фикстуры.
    #[test]
    fn segment_lengths_are_derived_from_the_input() {
        let input = [(0, 1000), (2000, 5000), (6000, 6500)];
        let stats = segment_stats(&segments(&input), &[], 10_000);
        assert_eq!(stats.count, 3);
        assert_eq!(stats.median_ms, 1000, "длины 500, 1000, 3000");

        let covered: u64 = input.iter().map(|(start, end)| end - start).sum();
        let expected = covered as f32 / 10_000.0;
        assert!((stats.coverage - expected).abs() < 1e-6, "{stats:?}");
    }

    /// Граница посреди сплошной речи в паузу не попала.
    #[test]
    fn a_boundary_inside_speech_does_not_count_as_a_pause() {
        let stats = segment_stats(&segments(&[(0, 3000), (3000, 6000)]), &[(0, 6000)], 6000);
        assert_eq!(stats.boundaries_in_pause, 0.0, "{stats:?}");
    }

    /// А та же граница при паузе ровно в том же месте — попала.
    ///
    /// Пара к предыдущему тесту: поодиночке любой из них проходит и на
    /// считалке, всегда возвращающей своё число.
    #[test]
    fn the_same_boundary_inside_a_pause_counts() {
        let stats = segment_stats(
            &segments(&[(0, 3000), (3000, 6000)]),
            &[(0, 2500), (3500, 6000)],
            6000,
        );
        assert_eq!(stats.boundaries_in_pause, 1.0, "{stats:?}");
    }

    /// Без разметки речи доля границ в паузе не считается вовсе.
    ///
    /// Иначе она выходила бы единицей у любой нарезки — речи нигде нет,
    /// значит все границы «в паузе», — и это тот самый тест, который
    /// проходит на пустом входе.
    #[test]
    fn without_speech_marks_the_pause_share_is_not_claimed() {
        let stats = segment_stats(&segments(&[(0, 3000), (3000, 6000)]), &[], 6000);
        assert_eq!(stats.boundaries_in_pause, 0.0, "{stats:?}");
    }

    /// Пустой вход не должен выглядеть как удачный прогон.
    #[test]
    fn no_segments_is_not_full_coverage() {
        let stats = segment_stats(&[], &[], 10_000);
        assert_eq!(stats.count, 0);
        assert_eq!(stats.coverage, 0.0);
        assert_eq!(stats.median_ms, 0);
    }

    /// Нарезка окнами и нарезка по речи обязаны различаться **числом**, а
    /// не только на глаз: ради этого метрики и заведены.
    #[test]
    fn windows_and_speech_cuts_are_told_apart_by_the_numbers() {
        // Речь: две реплики с большой паузой между ними.
        let speech = [(0u64, 4000u64), (26_000, 30_000)];

        let by_windows = segment_stats(&segments(&[(0, 30_000)]), &speech, 30_000);
        let by_speech = segment_stats(&segments(&[(0, 4000), (26_000, 30_000)]), &speech, 30_000);

        assert_eq!(by_windows.count, 1);
        assert_eq!(by_speech.count, 2);
        assert_eq!(
            by_speech.boundaries_in_pause, 1.0,
            "граница по речи обязана попасть в паузу: {by_speech:?}"
        );
        assert!(
            by_speech.coverage < by_windows.coverage,
            "окно покрывает и тишину тоже: {by_windows:?} против {by_speech:?}"
        );
    }
}
