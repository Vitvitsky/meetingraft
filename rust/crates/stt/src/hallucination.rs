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

/// Текст — **начало** известной галлюцинации, но ещё не она целиком.
///
/// Нужен потому, что LocalAgreement фиксирует текст порциями по мере
/// согласия (ADR-010): «Субтитры сделал DimaTorzok» приходит по кускам,
/// и каждый кусок по отдельности выглядит безобидно. Проверка целой
/// фразы срабатывает слишком поздно — начало уже отдано.
///
/// Выбросить такой кусок нельзя: «субтитры» бывает и настоящим словом.
/// Его придерживают до следующего шага, где станет ясно, чем он был.
pub fn is_hallucination_prefix(text: &str) -> bool {
    let normalized = normalize(text);
    // Порог против ложных задержек: без него «па» задерживалось бы как
    // начало «пара имен», а «ко» — как начало «корректор а». Реальная
    // речь тормозилась бы на пустом месте.
    if normalized.chars().count() < MIN_PREFIX_CHARS {
        return false;
    }
    HALLUCINATION_MARKERS
        .iter()
        .any(|marker| marker.len() > normalized.len() && marker.starts_with(&normalized))
}

/// Короче этого начало фразы не опознаётся: слишком много совпадений.
const MIN_PREFIX_CHARS: usize = 5;

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
    // Классика RU YouTube / softsub. Формы глагола перечислены полно:
    // модель выдаёт «сделал», «создал», «создавал», «делал» вперемешку,
    // и каждая незакрытая форма — отдельный видимый пользователю дефект.
    "авторы субтитров",
    "автор субтитров",
    "субтитры создал",
    "субтитры создавал",
    "субтитры сделал",
    "субтитры делал",
    "субтитры добавил",
    "субтитры подготовил",
    "субтитры и перевод",
    "субтитры предоставлены",
    "редактор субтитров",
    "переводчик субтитров",
    // Самый частый «автор» русских титров в обучающем корпусе. Как
    // слово в речи не встречается, поэтому ловится и без контекста.
    "dimatorzok",
    "дима торжок",
    "димасторжок",
    "продолжение следует",
    "подписывайтесь на канал",
    "спасибо за просмотр",
    "спасибо за внимание всем",
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
        assert!(is_whisper_hallucination("Авторы субтитров и пара имен"));
        assert!(is_whisper_hallucination("авторы субтитров А. Иванов"));
    }

    #[test]
    fn keeps_real_meeting_speech() {
        assert!(!is_whisper_hallucination(
            "Давайте обсудим roadmap на следующий спринт"
        ));
        assert!(!is_whisper_hallucination("Welcome to MeetingRaft"));
    }

    /// Формы глагола перечислены полно: каждая незакрытая — отдельный
    /// видимый пользователю дефект.
    #[test]
    fn drops_verb_variants_of_the_credits_line() {
        for text in [
            "Субтитры сделал DimaTorzok",
            "Субтитры создавал DimaTorzok",
            "Субтитры делал Дима Торжок",
            "Субтитры и перевод: студия",
            "Спасибо за просмотр!",
        ] {
            assert!(is_whisper_hallucination(text), "пропущено: {text}");
        }
    }

    /// Имя «автора» титров ловится и без вводной фразы: LocalAgreement
    /// мог отдать её раньше отдельным куском.
    #[test]
    fn drops_the_credited_name_alone() {
        assert!(is_whisper_hallucination("DimaTorzok"));
        assert!(is_whisper_hallucination("Дима Торжок"));
    }

    #[test]
    fn holds_back_the_beginning_of_a_known_phrase() {
        assert!(is_hallucination_prefix("Субтитры"));
        assert!(is_hallucination_prefix("субтитры сдел"));
    }

    /// Целая фраза уже не «начало»: её надо выбрасывать, а не держать.
    #[test]
    fn complete_phrase_is_not_a_prefix() {
        assert!(!is_hallucination_prefix("Субтитры сделал DimaTorzok"));
    }

    /// Обычная речь не должна задерживаться ни на шаг.
    #[test]
    fn real_speech_is_not_held_back() {
        assert!(!is_hallucination_prefix("Давайте начнём"));
        assert!(!is_hallucination_prefix("суббота"));
        assert!(!is_hallucination_prefix(""));
    }

    /// Короткие обрывки совпадают с началом слишком многого: «па» — это
    /// «пара имен», «ко» — «корректор». Задерживать их нельзя.
    #[test]
    fn short_fragments_are_not_treated_as_a_beginning() {
        for text in ["па", "ко", "суб", "ht"] {
            assert!(!is_hallucination_prefix(text), "задержано: {text}");
        }
    }
}
