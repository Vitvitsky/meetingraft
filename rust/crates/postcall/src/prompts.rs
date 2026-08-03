use domain::SpeechLanguage;

/// Формирует инструкции для краткого итога встречи.
pub fn brief_prompts(final_md: &str, primary_lang: SpeechLanguage) -> (String, String) {
    let system = format!(
        "Create a concise meeting brief in language `{}`. \
         Return Markdown with a summary, decisions, and key discussion points. \
         Do not invent facts absent from the transcript.",
        primary_lang.code()
    );
    let user = format!(
        "Create the meeting brief from this final transcript:\n\n\
         <transcript>\n{final_md}\n</transcript>"
    );

    (system, user)
}

/// Формирует инструкции для follow-up письма по итогам встречи.
pub fn follow_up_prompts(final_md: &str, primary_lang: SpeechLanguage) -> (String, String) {
    let system = format!(
        "You are a meeting assistant. Draft a follow-up email in language `{}` as Markdown. \
         Start with the subject line in an HTML comment, then include a greeting, \
         a concise meeting summary, explicitly stated next steps, and a closing. \
         Do not invent facts, assignments, or deadlines absent from the transcript.",
        primary_lang.code()
    );
    let user = format!(
        "Draft a follow-up email from this final transcript:\n\n\
         <transcript>\n{final_md}\n</transcript>"
    );

    (system, user)
}

#[cfg(test)]
mod tests {
    use domain::SpeechLanguage;

    use super::{brief_prompts, follow_up_prompts};

    #[test]
    fn brief_prompts_request_markdown_in_primary_language() {
        let (system, user) =
            brief_prompts("# Итоговый транскрипт\nОбсудили релиз.", SpeechLanguage::Ru);

        assert!(system.contains("Markdown"));
        assert!(system.contains("ru"));
        assert!(user.contains("# Итоговый транскрипт\nОбсудили релиз."));
    }

    #[test]
    fn follow_up_prompts_request_email_with_expected_structure() {
        let (system, user) =
            follow_up_prompts("# Final transcript\nSchedule the demo.", SpeechLanguage::En);

        assert!(system.contains("Markdown"));
        assert!(system.contains("en"));
        assert!(system.contains("follow-up email"));
        assert!(system.contains("subject"));
        assert!(system.contains("greeting"));
        assert!(system.contains("summary"));
        assert!(system.contains("closing"));
        assert!(!system.contains("follow-up actions"));
        assert!(user.contains("Draft a follow-up email"));
        assert!(user.contains("# Final transcript\nSchedule the demo."));
    }
}
