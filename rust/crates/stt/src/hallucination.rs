//! Отсев типовых галлюцинаций Whisper (особенно на тишине / шуме).

/// Нормализованный текст — кандидат на отбрасывание.
pub fn is_whisper_hallucination(text: &str) -> bool {
    let normalized = normalize(text);
    if normalized.is_empty() {
        return true;
    }
    // Слишком короткие «обрывки» после нормализации часто мусор.
    if normalized.chars().count() < 2 {
        return true;
    }
    HALLUCINATION_MARKERS
        .iter()
        .any(|marker| normalized.contains(marker))
}

fn normalize(text: &str) -> String {
    text.chars()
        .map(|c| {
            if c.is_alphanumeric() || c.is_whitespace() {
                c.to_lowercase().next().unwrap_or(c)
            } else {
                ' '
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Подстроки после `normalize` (нижний регистр, без пунктуации).
const HALLUCINATION_MARKERS: &[&str] = &[
    // Классика RU YouTube / softsub
    "авторы субтитров",
    "автор субтитров",
    "субтитры создал",
    "субтитры сделал",
    "редактор субтитров",
    "переводчик субтитров",
    "продолжение следует",
    "подписывайтесь на канал",
    "ставьте лайк",
    "пара имен",
    "корректор а",
    "амara org",
    "amara org",
    // EN / mixed
    "thanks for watching",
    "thank you for watching",
    "subscribe to",
    "subtitles by",
    "字幕",
    "www ",
    "http",
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drops_russian_credits_hallucination() {
        assert!(is_whisper_hallucination(
            "Авторы субтитров и пара имен"
        ));
        assert!(is_whisper_hallucination(
            "авторы субтитров А. Иванов"
        ));
    }

    #[test]
    fn keeps_real_meeting_speech() {
        assert!(!is_whisper_hallucination(
            "Давайте обсудим roadmap на следующий спринт"
        ));
        assert!(!is_whisper_hallucination("Welcome to MeetingRaft"));
    }
}
