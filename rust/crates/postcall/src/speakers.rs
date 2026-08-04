//! Сводка по говорящим (Phase 11, T3).
//!
//! Считается здесь, а не в UI: доля речи — это агрегат по сегментам, и
//! вью не должно заниматься арифметикой над доменными данными
//! (`AGENTS.md`).

use domain::{AudioChannel, FinalSegment, Speaker};

/// Сколько и как долго говорил участник.
#[derive(Debug, Clone, PartialEq)]
pub struct SpeakerStat {
    pub speaker_id: String,
    pub display_name: String,
    /// Канал, к которому спикер привязан большинством реплик.
    pub channel: AudioChannel,
    pub segment_count: u32,
    pub speaking_ms: u64,
    /// Доля от общего времени речи, 0…1.
    pub share: f64,
}

/// Свести сегменты в статистику по спикерам.
///
/// Сегменты без спикера пропускаются: показывать «неизвестный участник»
/// с долей речи означало бы выдавать пробел в данных за факт встречи.
pub fn speaker_stats(segments: &[FinalSegment], speakers: &[Speaker]) -> Vec<SpeakerStat> {
    let mut stats: Vec<SpeakerStat> = Vec::new();
    let mut total_ms: u64 = 0;

    for segment in segments {
        if segment.speaker_id.is_empty() {
            continue;
        }
        let duration = segment.duration_ms();
        total_ms += duration;

        if let Some(existing) = stats
            .iter_mut()
            .find(|stat| stat.speaker_id == segment.speaker_id)
        {
            existing.segment_count += 1;
            existing.speaking_ms += duration;
            continue;
        }

        let display_name = speakers
            .iter()
            .find(|speaker| speaker.id == segment.speaker_id)
            .map(|speaker| speaker.display_name.clone())
            .unwrap_or_default();
        stats.push(SpeakerStat {
            speaker_id: segment.speaker_id.clone(),
            display_name,
            channel: segment.channel,
            segment_count: 1,
            speaking_ms: duration,
            share: 0.0,
        });
    }

    if total_ms > 0 {
        for stat in &mut stats {
            stat.share = stat.speaking_ms as f64 / total_ms as f64;
        }
    }
    // Больше всех говоривший — первым: список читают, чтобы понять, кто
    // вёл встречу.
    stats.sort_by(|left, right| {
        right
            .speaking_ms
            .cmp(&left.speaking_ms)
            .then_with(|| left.display_name.cmp(&right.display_name))
    });
    stats
}

#[cfg(test)]
mod tests {
    use super::*;

    fn segment(index: u32, speaker: &str, channel: AudioChannel, duration_ms: u64) -> FinalSegment {
        let start = u64::from(index) * 10_000;
        FinalSegment {
            index,
            start_ms: start,
            end_ms: start + duration_ms,
            channel,
            speaker_id: speaker.to_string(),
            speaker_pinned: false,
            text: "текст".to_string(),
            text_edited: false,
        }
    }

    fn speaker(id: &str, name: &str) -> Speaker {
        Speaker {
            id: id.to_string(),
            meeting_id: "m1".to_string(),
            display_name: name.to_string(),
            sort_index: 0,
        }
    }

    #[test]
    fn aggregates_time_and_count_per_speaker() {
        let segments = vec![
            segment(0, "a", AudioChannel::Mic, 3_000),
            segment(1, "b", AudioChannel::System, 1_000),
            segment(2, "a", AudioChannel::Mic, 2_000),
        ];
        let speakers = vec![speaker("a", "Вы"), speaker("b", "Пётр")];

        let stats = speaker_stats(&segments, &speakers);

        assert_eq!(stats.len(), 2);
        assert_eq!(stats[0].display_name, "Вы");
        assert_eq!(stats[0].segment_count, 2);
        assert_eq!(stats[0].speaking_ms, 5_000);
        assert!((stats[0].share - 5.0 / 6.0).abs() < 0.001);
        assert!((stats[1].share - 1.0 / 6.0).abs() < 0.001);
    }

    /// Больше всех говоривший идёт первым: список читают, чтобы понять,
    /// кто вёл встречу.
    #[test]
    fn most_talkative_comes_first() {
        let segments = vec![
            segment(0, "quiet", AudioChannel::Mic, 1_000),
            segment(1, "loud", AudioChannel::System, 9_000),
        ];

        let stats = speaker_stats(
            &segments,
            &[speaker("quiet", "Тихий"), speaker("loud", "Громкий")],
        );

        assert_eq!(stats[0].display_name, "Громкий");
    }

    /// Сегмент без спикера — пробел в данных, а не участник встречи.
    #[test]
    fn unassigned_segments_are_skipped() {
        let segments = vec![
            segment(0, "", AudioChannel::Mic, 5_000),
            segment(1, "a", AudioChannel::Mic, 1_000),
        ];

        let stats = speaker_stats(&segments, &[speaker("a", "Вы")]);

        assert_eq!(stats.len(), 1);
        assert_eq!(stats[0].speaking_ms, 1_000);
        assert!(
            (stats[0].share - 1.0).abs() < 0.001,
            "доля считается от учтённого"
        );
    }

    /// Удалённый спикер оставляет реплики без имени, но не без времени:
    /// прятать их значило бы врать про длительность встречи.
    #[test]
    fn missing_speaker_record_keeps_the_time() {
        let stats = speaker_stats(&[segment(0, "ghost", AudioChannel::Mic, 2_000)], &[]);

        assert_eq!(stats.len(), 1);
        assert!(stats[0].display_name.is_empty());
        assert_eq!(stats[0].speaking_ms, 2_000);
    }

    #[test]
    fn empty_input_gives_no_stats() {
        assert!(speaker_stats(&[], &[]).is_empty());
    }
}
