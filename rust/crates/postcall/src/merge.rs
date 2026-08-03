//! Сведение двух распознанных дорожек в одну хронологию (Phase 10, T4).
//!
//! Post-call распознаёт каналы **раздельно**, поэтому канал сегмента
//! известен точно — без эвристики RMS-доминирования, которой пользуется
//! live (ADR-009). Ради этого ADR-004 и держит дорожки раздельными на
//! диске; здесь это окупается.

use domain::{AudioChannel, FinalSegment, TranscriptSegment};

/// Свести дорожки микрофона и системного звука в один список по времени.
///
/// Перекрытия **сохраняются оба**: если участники говорили одновременно,
/// это факт встречи, а не ошибка распознавания. В live такой сегмент
/// достался бы одному каналу — тому, кто громче.
pub fn merge_channels(
    mic: Vec<TranscriptSegment>,
    system: Vec<TranscriptSegment>,
) -> Vec<FinalSegment> {
    let mut tagged: Vec<(AudioChannel, TranscriptSegment)> =
        Vec::with_capacity(mic.len() + system.len());
    tagged.extend(mic.into_iter().map(|segment| (AudioChannel::Mic, segment)));
    tagged.extend(
        system
            .into_iter()
            .map(|segment| (AudioChannel::System, segment)),
    );

    // Порядок сортировки задан полностью, чтобы результат не зависел от
    // реализации sort: при равном начале сперва идёт микрофон, затем более
    // короткий сегмент.
    tagged.sort_by(|left, right| {
        left.1
            .start_ms
            .cmp(&right.1.start_ms)
            .then_with(|| channel_rank(left.0).cmp(&channel_rank(right.0)))
            .then_with(|| left.1.end_ms.cmp(&right.1.end_ms))
    });

    tagged
        .into_iter()
        .enumerate()
        .map(|(index, (channel, segment))| FinalSegment {
            index: index as u32,
            start_ms: segment.start_ms,
            end_ms: segment.end_ms,
            channel,
            // Заполнит диаризация (Phase 11).
            speaker_id: String::new(),
            text: segment.text,
        })
        .collect()
}

fn channel_rank(channel: AudioChannel) -> u8 {
    match channel {
        AudioChannel::Mic => 0,
        AudioChannel::System => 1,
    }
}

/// Рендер сегментов в markdown для экспорта и артефактов.
///
/// `FinalTranscript.body_markdown` остаётся производным: истина живёт в
/// таблице сегментов (Phase 10, T3).
pub fn render_segments(segments: &[FinalSegment]) -> String {
    let mut out = String::new();
    let mut previous_channel: Option<AudioChannel> = None;
    for segment in segments {
        if segment.text.trim().is_empty() {
            continue;
        }
        if !out.is_empty() {
            // Смена говорящего — всегда новый абзац; внутри одного канала
            // абзац начинается после конца предложения.
            if previous_channel != Some(segment.channel) || ends_sentence(&out) {
                out.push_str("\n\n");
            } else {
                out.push(' ');
            }
        }
        out.push_str(segment.text.trim());
        previous_channel = Some(segment.channel);
    }
    out
}

fn ends_sentence(text: &str) -> bool {
    matches!(
        text.trim_end().chars().last(),
        Some('.') | Some('!') | Some('?') | Some('…')
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn segment(start_ms: u64, end_ms: u64, text: &str) -> TranscriptSegment {
        TranscriptSegment::new(start_ms, end_ms, text)
    }

    #[test]
    fn non_overlapping_tracks_alternate_by_time() {
        let mic = vec![segment(0, 500, "я"), segment(2000, 2500, "и снова я")];
        let system = vec![segment(1000, 1500, "они")];

        let merged = merge_channels(mic, system);

        assert_eq!(
            merged.iter().map(|s| s.text.as_str()).collect::<Vec<_>>(),
            vec!["я", "они", "и снова я"]
        );
        assert_eq!(merged[1].channel, AudioChannel::System);
    }

    /// Одновременная речь — факт встречи; оба сегмента остаются.
    #[test]
    fn overlapping_speech_keeps_both_segments() {
        let mic = vec![segment(0, 3000, "я говорю долго")];
        let system = vec![segment(1000, 2000, "перебивают")];

        let merged = merge_channels(mic, system);

        assert_eq!(merged.len(), 2);
        assert_eq!(merged[0].channel, AudioChannel::Mic);
        assert_eq!(merged[1].channel, AudioChannel::System);
    }

    #[test]
    fn indices_are_sequential_after_merge() {
        let mic = vec![segment(100, 200, "a"), segment(300, 400, "b")];
        let system = vec![segment(150, 250, "c")];

        let merged = merge_channels(mic, system);

        assert_eq!(
            merged.iter().map(|s| s.index).collect::<Vec<_>>(),
            vec![0, 1, 2]
        );
    }

    /// При одинаковом начале порядок задан, а не зависит от реализации.
    #[test]
    fn equal_start_puts_microphone_first() {
        let mic = vec![segment(0, 900, "мой")];
        let system = vec![segment(0, 500, "их")];

        let merged = merge_channels(mic, system);

        assert_eq!(merged[0].channel, AudioChannel::Mic);
        assert_eq!(merged[1].channel, AudioChannel::System);
    }

    #[test]
    fn empty_system_track_gives_monologue() {
        let mic = vec![segment(0, 500, "один")];

        let merged = merge_channels(mic, Vec::new());

        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].channel, AudioChannel::Mic);
    }

    #[test]
    fn both_tracks_empty_gives_nothing() {
        assert!(merge_channels(Vec::new(), Vec::new()).is_empty());
    }

    /// Смена говорящего разрывает абзац даже без точки.
    #[test]
    fn render_starts_new_paragraph_on_channel_change() {
        let merged = merge_channels(
            vec![segment(0, 500, "привет")],
            vec![segment(600, 900, "здравствуйте")],
        );

        assert_eq!(render_segments(&merged), "привет\n\nздравствуйте");
    }

    #[test]
    fn render_joins_same_channel_until_sentence_end() {
        let merged = merge_channels(
            vec![
                segment(0, 500, "первая часть"),
                segment(600, 900, "вторая часть."),
            ],
            Vec::new(),
        );

        assert_eq!(render_segments(&merged), "первая часть вторая часть.");
    }

    #[test]
    fn render_breaks_paragraph_after_sentence_end() {
        let merged = merge_channels(
            vec![segment(0, 500, "первое."), segment(600, 900, "второе")],
            Vec::new(),
        );

        assert_eq!(render_segments(&merged), "первое.\n\nвторое");
    }

    #[test]
    fn render_skips_blank_segments() {
        let merged = merge_channels(
            vec![segment(0, 500, "   "), segment(600, 900, "текст")],
            Vec::new(),
        );

        assert_eq!(render_segments(&merged), "текст");
    }
}
