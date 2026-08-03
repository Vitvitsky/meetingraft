use domain::{CaptionEvent, CaptionPhase, FinalTranscript};

/// Собирает отдельный финальный транскрипт только из завершённых caption.
pub fn assemble_final(
    meeting_id: &str,
    captions: &[CaptionEvent],
    normalize: impl Fn(&str) -> String,
    now_ms: u64,
    version: u32,
) -> FinalTranscript {
    let body_markdown = captions
        .iter()
        .filter(|caption| caption.phase == CaptionPhase::Final)
        .map(|caption| normalize(&caption.text))
        .collect::<Vec<_>>()
        .join("\n\n");

    FinalTranscript {
        meeting_id: meeting_id.to_owned(),
        version,
        body_markdown,
        created_at_ms: now_ms,
    }
}

#[cfg(test)]
mod tests {
    use domain::{CaptionEvent, CaptionPhase};

    use super::assemble_final;

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
        assert_eq!(transcript.body_markdown, "привет UniFFI\n\nвторая");
        assert_eq!(transcript.version, 7);
        assert_eq!(transcript.created_at_ms, 100);
    }

    #[test]
    fn assemble_empty_finals_yields_empty_body_without_normalizing() {
        let transcript = assemble_final("m1", &[], |_| unreachable!(), 1, 1);

        assert!(transcript.body_markdown.is_empty());
    }
}
