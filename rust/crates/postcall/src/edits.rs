//! Перенос ручных правок на новую версию Final (Epic 19).
//!
//! Пересбор нарезает сегменты заново: границы и индексы другие, поэтому
//! правку нельзя привязать к номеру. Привязка идёт по перекрытию времени
//! и наличию исходного текста — если модель распознала это место иначе,
//! правка не применяется и человек видит её в отдельном разделе.

use std::collections::HashMap;
use std::collections::hash_map::Entry;

use domain::{EditPosition, FinalSegment, SegmentEdit};

/// Пересадить правки на сегменты версии `version`.
///
/// Правка без подходящего сегмента получает `applied_version = None`:
/// молча терять ручную работу нельзя.
///
/// Две правки могут прийтись на один сегмент — если новая нарезка слила
/// два ранее правленых сегмента в один. Место у них тогда общее, а
/// показать при чтении можно только одну: `edits_by_position` берёт
/// сильнейшую по [`SegmentEdit::precedence`]. Проигравшая **отвязывается**.
///
/// Иначе она осталась бы с номером версии и не нашлась бы нигде: в
/// сегментах стоит победитель, а в разделе неприменившихся ищут `None`.
/// Ручная работа при этом никуда не девается — отвязанная правка видна
/// человеку, и следующий пересбор снова берёт её в работу вместе со
/// всем журналом.
pub fn reattach_edits(
    edits: &[SegmentEdit],
    segments: &[FinalSegment],
    version: u32,
) -> Vec<SegmentEdit> {
    let homes: Vec<Option<&FinalSegment>> =
        edits.iter().map(|edit| home_for(edit, segments)).collect();

    let mut winners: HashMap<EditPosition, usize> = HashMap::new();
    for (index, home) in homes.iter().enumerate() {
        let Some(segment) = home else { continue };
        match winners.entry(segment.position()) {
            Entry::Vacant(slot) => {
                slot.insert(index);
            }
            Entry::Occupied(mut slot) => {
                if edits[index].precedence() > edits[*slot.get()].precedence() {
                    slot.insert(index);
                }
            }
        }
    }

    homes
        .iter()
        .enumerate()
        .map(|(index, home)| {
            let mut moved = edits[index].clone();
            match home {
                Some(segment) if winners.get(&segment.position()) == Some(&index) => {
                    moved.start_ms = segment.start_ms;
                    moved.end_ms = segment.end_ms;
                    moved.applied_version = Some(version);
                }
                // Границы переезжают только у победителя: у отвязанной
                // правки они остаются от места, которое человек правил на
                // самом деле, — по нему её и искать в следующий раз.
                _ => moved.applied_version = None,
            }
            moved
        })
        .collect()
}

/// Сегмент, на который садится правка. `None` — места не нашлось.
fn home_for<'a>(edit: &SegmentEdit, segments: &'a [FinalSegment]) -> Option<&'a FinalSegment> {
    // Пустой исходный текст содержится в любой строке, поэтому такая
    // правка села бы на первый попавшийся перекрывающийся сегмент и молча
    // переписала бы чужую реплику. Опознать место нечем — правка честно
    // остаётся неприменившейся.
    if edit.original_text.trim().is_empty() {
        return None;
    }
    segments
        .iter()
        .filter(|segment| segment.channel == edit.channel)
        .filter(|segment| segment.text.contains(edit.original_text.as_str()))
        .filter_map(|segment| overlap_ms(edit, segment).map(|ms| (ms, segment)))
        // При равном перекрыве выбираем сегмент с меньшим индексом.
        // Reverse инвертирует порядок сравнения, так что меньшие индексы побеждают.
        // Это гарантирует результат, не зависящий от порядка элементов входного среза:
        // голый max_by_key при ничьей выбрал бы последний элемент в итерации,
        // а нам нужна воспроизводимость. При ничьей выбираем более ранний сегмент.
        .max_by_key(|(ms, segment)| (*ms, std::cmp::Reverse(segment.index)))
        .map(|(_, segment)| segment)
}

/// Длина пересечения диапазонов; `None`, если не пересекаются.
fn overlap_ms(edit: &SegmentEdit, segment: &FinalSegment) -> Option<u64> {
    let start = edit.start_ms.max(segment.start_ms);
    let end = edit.end_ms.min(segment.end_ms);
    (end > start).then(|| end - start)
}

#[cfg(test)]
mod tests {
    use domain::{AudioChannel, FinalSegment, SegmentEdit, edits_by_position};

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
            original_text: String::new(),
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

    /// Правка без исходного текста опознать своё место не может: пустая
    /// строка содержится в любой. Без раннего отказа она села бы на
    /// первый попавшийся перекрывающийся сегмент и переписала бы чужую
    /// реплику.
    #[test]
    fn edit_without_original_text_stays_unapplied() {
        // Пустая строка содержится в любой, одиночный пробел — почти в
        // любой: без раннего отказа обе правки сели бы на чужую реплику.
        let edits = vec![edit(1000, 2000, ""), edit(1000, 2000, " ")];
        let segments = vec![segment(0, 900, 2100, "совсем другая реплика")];

        let result = reattach_edits(&edits, &segments, 2);

        assert_eq!(result[0].applied_version, None, "пустой исходный текст");
        assert_eq!(result[1].applied_version, None, "исходный текст из пробела");
        assert_eq!(result[0].start_ms, 1000, "границы остаются прежними");
    }

    /// Правка с собственными id и временем создания: коллизию решает
    /// `precedence`, а он смотрит именно на них.
    fn edit_at(
        id: &str,
        created_at_ms: u64,
        start_ms: u64,
        end_ms: u64,
        original: &str,
    ) -> SegmentEdit {
        SegmentEdit {
            id: id.into(),
            created_at_ms,
            ..edit(start_ms, end_ms, original)
        }
    }

    /// Слияние двух правленых сегментов в один не должно прятать
    /// проигравшую правку: с номером версии она не видна ни в сегментах,
    /// ни в разделе неприменившихся.
    #[test]
    fn merged_segments_leave_the_weaker_edit_visible() {
        let earlier = edit_at("e1", 10, 1000, 2000, "интра ру");
        let later = edit_at("e2", 20, 2000, 3000, "жира");
        let merged = segment(0, 900, 3100, "смотри интра ру и жира");

        // Материал обязан столкнуться: сядь правки на разные места, тест
        // прошёл бы, ничего не проверив.
        assert!(
            merged.text.contains(&earlier.original_text)
                && merged.text.contains(&later.original_text),
            "обе правки должны находить себе один и тот же сегмент"
        );

        let result = reattach_edits(&[earlier, later], &[merged], 2);

        assert_eq!(result[1].applied_version, Some(2), "победила поздняя");
        assert_eq!(
            result[0].applied_version, None,
            "проигравшая отвязана, а не оставлена с номером версии"
        );
        assert_eq!(
            (result[0].start_ms, result[0].end_ms),
            (1000, 2000),
            "границы проигравшей остались от места, которое правил человек"
        );
    }

    /// То самое свойство, ради которого правка и отвязывается: всё, что
    /// после переноса числится за версией, при чтении видно.
    #[test]
    fn every_attached_edit_is_shown_by_its_position() {
        let edits = vec![
            edit_at("e1", 10, 1000, 2000, "интра ру"),
            edit_at("e2", 20, 2000, 3000, "жира"),
            edit_at("e3", 30, 5000, 6000, "постколл"),
        ];
        let segments = vec![
            segment(0, 900, 3100, "смотри интра ру и жира"),
            segment(1, 4900, 6100, "это постколл"),
        ];

        let result = reattach_edits(&edits, &segments, 2);

        let attached: Vec<_> = result
            .iter()
            .filter(|edit| edit.applied_version == Some(2))
            .collect();
        assert_eq!(
            attached.len(),
            2,
            "привязанных должно быть две из трёх: две правки делят один сегмент"
        );

        let shown = edits_by_position(&result, 2);
        for edit in attached {
            assert_eq!(
                shown.get(&edit.position()).map(|shown| shown.id.as_str()),
                Some(edit.id.as_str()),
                "правка {} числится за версией, но при чтении показана не она",
                edit.id
            );
        }
    }

    /// Порядок правок во входе — это порядок выборки из базы, к решению
    /// человека отношения не имеющий. Победитель от него зависеть не может.
    #[test]
    fn the_winner_does_not_depend_on_input_order() {
        let earlier = edit_at("e1", 10, 1000, 2000, "интра ру");
        let later = edit_at("e2", 20, 2000, 3000, "жира");
        let merged = segment(0, 900, 3100, "смотри интра ру и жира");

        let merged = [merged];
        let straight = reattach_edits(&[earlier.clone(), later.clone()], &merged, 2);
        let reversed = reattach_edits(&[later, earlier], &merged, 2);

        let winner = |result: &[SegmentEdit]| {
            result
                .iter()
                .find(|edit| edit.applied_version == Some(2))
                .map(|edit| edit.id.clone())
        };
        assert_eq!(winner(&straight), Some("e2".to_string()));
        assert_eq!(winner(&straight), winner(&reversed));
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
