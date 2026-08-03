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

/// Формирует инструкции для списка следующих действий.
pub fn follow_up_prompts(final_md: &str, primary_lang: SpeechLanguage) -> (String, String) {
    let system = format!(
        "Extract follow-up actions in language `{}`. \
         Return Markdown with owners and due dates only when explicitly stated. \
         Do not invent missing assignments or deadlines.",
        primary_lang.code()
    );
    let user = format!(
        "Create follow-up actions from this final transcript:\n\n\
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
    fn follow_up_prompts_include_transcript_and_language() {
        let (system, user) =
            follow_up_prompts("# Final transcript\nSchedule the demo.", SpeechLanguage::En);

        assert!(system.contains("Markdown"));
        assert!(system.contains("en"));
        assert!(user.contains("# Final transcript\nSchedule the demo."));
    }
}
