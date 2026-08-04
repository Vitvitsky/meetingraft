//! Перенос ручных правок на новую версию Final (Epic 19).
//!
//! Пересбор нарезает сегменты заново: границы и индексы другие, поэтому
//! правку нельзя привязать к номеру. Привязка идёт по перекрытию времени
//! и наличию исходного текста — если модель распознала это место иначе,
//! правка не применяется и человек видит её в отдельном разделе.

use domain::{FinalSegment, SegmentEdit};

/// Пересадить правки на сегменты версии `version`.
///
/// Правка без подходящего сегмента получает `applied_version = None`:
/// молча терять ручную работу нельзя.
///
/// Две правки могут сесть на один сегмент — если новая нарезка слила
/// два ранее правленых сегмента в один. Это разрешено: побеждать при
/// чтении будет более поздняя по `created_at_ms`, как и при обычной
/// повторной правке одного места.
pub fn reattach_edits(
    edits: &[SegmentEdit],
    segments: &[FinalSegment],
    version: u32,
) -> Vec<SegmentEdit> {
    edits
        .iter()
        .map(|edit| {
            let best = segments
                .iter()
                .filter(|segment| segment.channel == edit.channel)
                .filter(|segment| segment.text.contains(edit.original_text.as_str()))
                .filter_map(|segment| overlap_ms(edit, segment).map(|ms| (ms, segment)))
                // При равном перекрыве выбираем сегмент с меньшим индексом.
                // Reverse инвертирует порядок сравнения, так что меньшие индексы побеждают.
                // Это гарантирует результат, не зависящий от порядка элементов входного среза:
                // голый max_by_key при ничьей выбрал бы последний элемент в итерации,
                // а нам нужна воспроизводимость. При ничьей выбираем более ранний сегмент.
                .max_by_key(|(ms, segment)| (*ms, std::cmp::Reverse(segment.index)));

            let mut moved = edit.clone();
            match best {
                Some((_, segment)) => {
                    moved.start_ms = segment.start_ms;
                    moved.end_ms = segment.end_ms;
                    moved.applied_version = Some(version);
                }
                None => moved.applied_version = None,
            }
            moved
        })
        .collect()
}

/// Длина пересечения диапазонов; `None`, если не пересекаются.
fn overlap_ms(edit: &SegmentEdit, segment: &FinalSegment) -> Option<u64> {
    let start = edit.start_ms.max(segment.start_ms);
    let end = edit.end_ms.min(segment.end_ms);
    (end > start).then(|| end - start)
}

#[cfg(test)]
mod tests {
    use domain::{AudioChannel, FinalSegment, SegmentEdit};

    use super::reattach_edits;

    fn segment(index: u32, start_ms: u64, end_ms: u64, text: &str) -> FinalSegment {
        FinalSegment {
            index,
            start_ms,
            end_ms,
            channel: AudioChannel::Mic,
            speaker_id: String::new(),
            speaker_pinned: false,
            text: text.into(),
            text_edited: false,
        }
    }

    fn edit(start_ms: u64, end_ms: u64, original: &str) -> SegmentEdit {
        SegmentEdit {
            id: "e1".into(),
            meeting_id: "m1".into(),
            channel: AudioChannel::Mic,
            start_ms,
            end_ms,
            original_text: original.into(),
            edited_text: "intra.ru".into(),
            created_at_ms: 0,
            applied_version: Some(1),
        }
    }

    #[test]
    fn attaches_to_overlapping_segment_containing_original_text() {
        let edits = vec![edit(1000, 2000, "интра ру")];
        let segments = vec![segment(0, 900, 2100, "смотри интра ру там")];

        let result = reattach_edits(&edits, &segments, 2);

        assert_eq!(result[0].applied_version, Some(2));
        assert_eq!(
            result[0].start_ms, 900,
            "диапазон переезжает на новый сегмент"
        );
        assert_eq!(result[0].end_ms, 2100);
    }

    #[test]
    fn drops_when_original_text_is_gone() {
        let edits = vec![edit(1000, 2000, "интра ру")];
        let segments = vec![segment(0, 900, 2100, "совсем другое распознавание")];

        let result = reattach_edits(&edits, &segments, 2);

        assert_eq!(result[0].applied_version, None);
    }

    #[test]
    fn picks_candidate_with_largest_overlap() {
        let edits = vec![edit(1000, 2000, "интра ру")];
        let segments = vec![
            segment(0, 900, 1200, "интра ру"),
            segment(1, 1100, 2100, "интра ру ещё раз"),
        ];

        let result = reattach_edits(&edits, &segments, 2);

        assert_eq!(result[0].start_ms, 1100, "победил больший перекрыв");
    }

    #[test]
    fn ignores_segments_of_other_channel() {
        let edits = vec![edit(1000, 2000, "интра ру")];
        let mut other = segment(0, 900, 2100, "интра ру");
        other.channel = AudioChannel::System;

        let result = reattach_edits(&edits, &[other], 2);

        assert_eq!(result[0].applied_version, None);
    }

    #[test]
    fn picks_earlier_segment_on_equal_overlap() {
        let edits = vec![edit(1000, 2000, "интра ру")];
        let segments = vec![
            segment(0, 1000, 1500, "интра ру"),
            segment(1, 1500, 2000, "интра ру ещё раз"),
        ];

        let result = reattach_edits(&edits, &segments, 2);

        assert_eq!(
            result[0].start_ms, 1000,
            "при равном перекрыве (500мс) победил более ранний сегмент"
        );
    }
}
