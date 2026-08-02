use domain::{Artifact, ArtifactKind, SpeechLanguage};

const BRIEF_TEMPLATE_ID: &str = "builtin.brief";
const FOLLOW_UP_TEMPLATE_ID: &str = "builtin.follow_up";

/// Рендерит встроенный Brief без обращения к LLM.
pub fn render_brief(final_body: &str, _primary_lang: SpeechLanguage) -> String {
    let paragraphs = paragraphs(final_body);
    let summary = truncate_chars(paragraphs.first().copied().unwrap_or_default(), 280);
    let key_points = bullets(&paragraphs);
    let next_steps = paragraphs
        .iter()
        .copied()
        .filter(|paragraph| is_next_step(paragraph))
        .collect::<Vec<_>>();
    let next_steps = if next_steps.is_empty() {
        "- —".to_owned()
    } else {
        bullets(&next_steps)
    };

    format!(
        "# Brief\n\n## Summary\n{summary}\n\n## Key points\n{key_points}\n\n## Next steps\n{next_steps}"
    )
}

/// Рендерит встроенный follow-up с локализованным приветствием и завершением.
pub fn render_follow_up(
    final_body: &str,
    primary_lang: SpeechLanguage,
    date_label: &str,
) -> String {
    let paragraphs = paragraphs(final_body);
    let summary = truncate_chars(paragraphs.first().copied().unwrap_or_default(), 280);
    let key_points = bullets(&paragraphs);
    let (subject, greeting, closing) = match primary_lang {
        SpeechLanguage::Ru => (
            "Итоги встречи",
            "Здравствуйте,",
            "Пожалуйста, проверьте и дополните, если что-то упущено.",
        ),
        SpeechLanguage::En => (
            "Meeting follow-up",
            "Hello,",
            "Please review and add anything we may have missed.",
        ),
        SpeechLanguage::Es => (
            "Resumen de la reunión",
            "Hola,",
            "Por favor, revise y añada cualquier detalle que falte.",
        ),
    };

    format!(
        "<!-- subject: {subject} {date_label} -->\n\n{greeting}\n\n{summary}\n\n{key_points}\n\n{closing}"
    )
}

/// Создаёт артефакт и фиксирует идентификатор встроенного шаблона.
pub fn make_artifact(meeting_id: &str, kind: ArtifactKind, body: &str, now_ms: u64) -> Artifact {
    let template_id = match kind {
        ArtifactKind::Brief => BRIEF_TEMPLATE_ID,
        ArtifactKind::FollowUp => FOLLOW_UP_TEMPLATE_ID,
    };

    Artifact {
        id: format!("{meeting_id}:{template_id}:{now_ms}"),
        meeting_id: meeting_id.to_owned(),
        kind,
        template_id: template_id.to_owned(),
        body_markdown: body.to_owned(),
        created_at_ms: now_ms,
    }
}

fn paragraphs(body: &str) -> Vec<&str> {
    body.split("\n\n")
        .map(str::trim)
        .filter(|paragraph| !paragraph.is_empty())
        .collect()
}

fn truncate_chars(value: &str, limit: usize) -> String {
    value.chars().take(limit).collect()
}

fn bullets(paragraphs: &[&str]) -> String {
    paragraphs
        .iter()
        .map(|paragraph| format!("- {paragraph}"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn is_next_step(paragraph: &str) -> bool {
    let lowercase = paragraph.to_lowercase();
    ["нужно", "сделать", "todo", "action"]
        .iter()
        .any(|cue| lowercase.contains(cue))
}

#[cfg(test)]
mod tests {
    use domain::{ArtifactKind, SpeechLanguage};

    use super::{make_artifact, render_brief, render_follow_up};

    #[test]
    fn brief_renders_summary_key_points_and_detected_next_steps() {
        let markdown = render_brief(
            "Первый абзац.\n\nНужно сделать X.\n\nСправочная деталь.",
            SpeechLanguage::Ru,
        );

        assert_eq!(
            markdown,
            "# Brief\n\n## Summary\nПервый абзац.\n\n## Key points\n- Первый абзац.\n- Нужно сделать X.\n- Справочная деталь.\n\n## Next steps\n- Нужно сделать X."
        );
    }

    #[test]
    fn brief_limits_summary_to_280_characters() {
        let paragraph = "я".repeat(281);

        let markdown = render_brief(&paragraph, SpeechLanguage::Ru);
        let summary = markdown
            .split("## Summary\n")
            .nth(1)
            .and_then(|tail| tail.split("\n\n## Key points").next())
            .expect("секция Summary должна присутствовать");

        assert_eq!(summary.chars().count(), 280);
    }

    #[test]
    fn brief_uses_placeholder_when_next_steps_are_absent() {
        let markdown = render_brief("Только информационный итог.", SpeechLanguage::En);

        assert!(markdown.ends_with("## Next steps\n- —"));
    }

    #[test]
    fn follow_up_has_subject_comment_and_ru_copy() {
        let markdown =
            render_follow_up("Итог один.\n\nИтог два.", SpeechLanguage::Ru, "2026-08-02");

        assert!(markdown.starts_with("<!-- subject: Итоги встречи 2026-08-02 -->"));
        assert!(markdown.contains("Здравствуйте,"));
        assert!(markdown.contains("- Итог один.\n- Итог два."));
        assert!(markdown.contains("Пожалуйста, проверьте и дополните, если что-то упущено."));
    }

    #[test]
    fn follow_up_uses_english_copy_for_english_primary_language() {
        let markdown = render_follow_up(
            "First outcome.\n\nSecond outcome.",
            SpeechLanguage::En,
            "2026-08-02",
        );

        assert!(markdown.starts_with("<!-- subject: Meeting follow-up 2026-08-02 -->"));
        assert!(markdown.contains("\n\nHello,\n\n"));
        assert!(markdown.contains("Please review and add anything we may have missed."));
    }

    #[test]
    fn artifact_uses_builtin_template_for_kind() {
        let artifact = make_artifact("m1", ArtifactKind::FollowUp, "body", 42);

        assert_eq!(artifact.meeting_id, "m1");
        assert_eq!(artifact.kind, ArtifactKind::FollowUp);
        assert_eq!(artifact.template_id, "builtin.follow_up");
        assert_eq!(artifact.body_markdown, "body");
        assert_eq!(artifact.created_at_ms, 42);
        assert!(!artifact.id.is_empty());
    }
}
