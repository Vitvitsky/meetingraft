//! Сведение двух распознанных дорожек в одну хронологию (Phase 10, T4).
//!
//! Post-call распознаёт каналы **раздельно**, поэтому канал сегмента
//! известен точно — без эвристики RMS-доминирования, которой пользуется
//! live (ADR-009). Ради этого ADR-004 и держит дорожки раздельными на
//! диске; здесь это окупается.

use domain::{AudioChannel, FinalSegment, Speaker, TranscriptSegment};

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
            // Заполняется назначением по каналу (Phase 11).
            speaker_id: String::new(),
            speaker_pinned: false,
            text: segment.text,
            text_edited: false,
            original_text: String::new(),
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
///
/// Имена подставляются, если участник известен. Без них модель, строящая
/// Brief, вынуждена догадываться, кто что предложил, — а по абзацам это
/// неразрешимо (Phase 11, T6).
pub fn render_segments(segments: &[FinalSegment], speakers: &[Speaker]) -> String {
    let mut out = String::new();
    // Реплика принадлежит участнику, а канал — запасной ключ: до
    // назначения имён абзацы всё равно должны делиться по дорожкам.
    let mut previous: Option<(&str, AudioChannel)> = None;
    for segment in segments {
        let text = segment.text.trim();
        if text.is_empty() {
            continue;
        }
        let key = (segment.speaker_id.as_str(), segment.channel);
        let speaker_changed = previous != Some(key);
        let starts_paragraph = out.is_empty() || speaker_changed || ends_sentence(&out);

        if !out.is_empty() {
            out.push_str(if starts_paragraph { "\n\n" } else { " " });
        }
        // Подпись ставится в начале реплики участника, а не каждого
        // абзаца: имя через предложение читается как заикание.
        if let Some(name) = speaker_name(&segment.speaker_id, speakers).filter(|_| speaker_changed)
        {
            out.push_str("**");
            out.push_str(name);
            out.push_str(":** ");
        }
        out.push_str(text);
        previous = Some(key);
    }
    out
}

/// Имя участника или `None`, если он не назначен или запись удалена.
fn speaker_name<'a>(speaker_id: &str, speakers: &'a [Speaker]) -> Option<&'a str> {
    if speaker_id.is_empty() {
        return None;
    }
    speakers
        .iter()
        .find(|speaker| speaker.id == speaker_id)
        .map(|speaker| speaker.display_name.as_str())
        .filter(|name| !name.is_empty())
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

        assert_eq!(render_segments(&merged, &[]), "привет\n\nздравствуйте");
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

        assert_eq!(render_segments(&merged, &[]), "первая часть вторая часть.");
    }

    #[test]
    fn render_breaks_paragraph_after_sentence_end() {
        let merged = merge_channels(
            vec![segment(0, 500, "первое."), segment(600, 900, "второе")],
            Vec::new(),
        );

        assert_eq!(render_segments(&merged, &[]), "первое.\n\nвторое");
    }

    #[test]
    fn render_skips_blank_segments() {
        let merged = merge_channels(
            vec![segment(0, 500, "   "), segment(600, 900, "текст")],
            Vec::new(),
        );

        assert_eq!(render_segments(&merged, &[]), "текст");
    }

    fn named(id: &str, name: &str) -> Speaker {
        Speaker {
            id: id.to_string(),
            meeting_id: "m1".to_string(),
            display_name: name.to_string(),
            sort_index: 0,
        }
    }

    fn assign(mut segments: Vec<FinalSegment>, ids: &[&str]) -> Vec<FinalSegment> {
        for (segment, id) in segments.iter_mut().zip(ids) {
            segment.speaker_id = (*id).to_string();
        }
        segments
    }

    #[test]
    fn render_prefixes_known_speakers() {
        let merged = assign(
            merge_channels(
                vec![segment(0, 500, "привет")],
                vec![segment(600, 900, "здравствуйте")],
            ),
            &["s1", "s2"],
        );

        assert_eq!(
            render_segments(&merged, &[named("s1", "Вы"), named("s2", "Пётр")]),
            "**Вы:** привет\n\n**Пётр:** здравствуйте"
        );
    }

    /// Имя ставится в начале реплики, а не каждого абзаца: подпись через
    /// предложение читается как заикание.
    #[test]
    fn render_names_the_turn_not_every_paragraph() {
        let merged = assign(
            merge_channels(
                vec![segment(0, 500, "первое."), segment(600, 900, "второе")],
                Vec::new(),
            ),
            &["s1", "s1"],
        );

        assert_eq!(
            render_segments(&merged, &[named("s1", "Вы")]),
            "**Вы:** первое.\n\nвторое"
        );
    }

    /// Переназначенная реплика внутри одной дорожки обязана начать новый
    /// абзац — иначе чужие слова приклеятся к предыдущему говорящему.
    #[test]
    fn render_breaks_paragraph_when_speaker_changes_inside_channel() {
        let merged = assign(
            merge_channels(
                vec![
                    segment(0, 500, "моя мысль"),
                    segment(600, 900, "а это не я"),
                ],
                Vec::new(),
            ),
            &["s1", "s2"],
        );

        assert_eq!(
            render_segments(&merged, &[named("s1", "Вы"), named("s2", "Гость")]),
            "**Вы:** моя мысль\n\n**Гость:** а это не я"
        );
    }

    /// Удалённый участник оставляет текст без подписи, а не с пустой:
    /// `**:**` выглядело бы как сбой рендера.
    #[test]
    fn render_omits_prefix_for_unknown_speaker() {
        let merged = assign(
            merge_channels(vec![segment(0, 500, "текст")], Vec::new()),
            &["ghost"],
        );

        assert_eq!(render_segments(&merged, &[named("s1", "Вы")]), "текст");
    }
}
