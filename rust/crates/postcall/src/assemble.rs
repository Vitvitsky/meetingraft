use domain::{CaptionEvent, CaptionPhase, FinalTranscript};

/// Собирает отдельный финальный транскрипт только из завершённых caption.
pub fn assemble_final(
    meeting_id: &str,
    captions: &[CaptionEvent],
    normalize: impl Fn(&str) -> String,
    now_ms: u64,
    version: u32,
) -> FinalTranscript {
    // Стабилизация live-субтитров (ADR-010) отдаёт Final кусками по мере
    // согласия гипотез, а не по одному на реплику. Склеивать их пустой
    // строкой значило бы превращать связную речь в рванину, поэтому новый
    // абзац начинается только после конца предложения.
    let fragments: Vec<String> = captions
        .iter()
        .filter(|caption| caption.phase == CaptionPhase::Final)
        .map(|caption| normalize(&caption.text))
        .filter(|text| !text.trim().is_empty())
        .collect();
    let body_markdown = join_fragments(&fragments);

    FinalTranscript {
        meeting_id: meeting_id.to_owned(),
        version,
        body_markdown,
        created_at_ms: now_ms,
    }
}

/// Конец предложения — разрыв абзаца; иначе продолжаем строку.
fn join_fragments(fragments: &[String]) -> String {
    let mut out = String::new();
    for fragment in fragments {
        let fragment = fragment.trim();
        if fragment.is_empty() {
            continue;
        }
        if out.is_empty() {
            out.push_str(fragment);
            continue;
        }
        if ends_sentence(&out) {
            out.push_str("\n\n");
        } else {
            out.push(' ');
        }
        out.push_str(fragment);
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
    use domain::{CaptionEvent, CaptionPhase};

    use super::assemble_final;

    /// Куски одной реплики склеиваются пробелом, а не пустой строкой.
    #[test]
    fn fragments_without_sentence_end_join_with_space() {
        let captions = vec![
            CaptionEvent::new("1".into(), "обсудим биллинг".into(), CaptionPhase::Final),
            CaptionEvent::new("2".into(), "и сроки".into(), CaptionPhase::Final),
        ];

        let transcript = assemble_final("m1", &captions, |text| text.to_string(), 100, 1);

        assert_eq!(transcript.body_markdown, "обсудим биллинг и сроки");
    }

    #[test]
    fn sentence_end_starts_new_paragraph() {
        let captions = vec![
            CaptionEvent::new(
                "1".into(),
                "решили по биллингу.".into(),
                CaptionPhase::Final,
            ),
            CaptionEvent::new("2".into(), "Теперь сроки".into(), CaptionPhase::Final),
        ];

        let transcript = assemble_final("m1", &captions, |text| text.to_string(), 100, 1);

        assert_eq!(
            transcript.body_markdown,
            "решили по биллингу.\n\nТеперь сроки"
        );
    }

    #[test]
    fn blank_fragments_are_dropped() {
        let captions = vec![
            CaptionEvent::new("1".into(), "   ".into(), CaptionPhase::Final),
            CaptionEvent::new("2".into(), "текст".into(), CaptionPhase::Final),
        ];

        let transcript = assemble_final("m1", &captions, |text| text.to_string(), 100, 1);

        assert_eq!(transcript.body_markdown, "текст");
    }

    #[test]
    fn assemble_keeps_finals_only_and_normalizes() {
        let captions = vec![
            CaptionEvent::new("1".into(), "частичный".into(), CaptionPhase::Partial),
            CaptionEvent::new("2".into(), "привет униффи".into(), CaptionPhase::Final),
            CaptionEvent::new("3".into(), "вторая".into(), CaptionPhase::Final),
        ];

        let transcript = assemble_final(
            "m1",
            &captions,
            |text| text.replace("униффи", "UniFFI"),
            100,
            7,
        );

        assert_eq!(transcript.meeting_id, "m1");
        // Ни один фрагмент не заканчивает предложение — склейка пробелом.
        assert_eq!(transcript.body_markdown, "привет UniFFI вторая");
        assert_eq!(transcript.version, 7);
        assert_eq!(transcript.created_at_ms, 100);
    }

    #[test]
    fn assemble_empty_finals_yields_empty_body_without_normalizing() {
        let transcript = assemble_final("m1", &[], |_| unreachable!(), 1, 1);

        assert!(transcript.body_markdown.is_empty());
    }
}
