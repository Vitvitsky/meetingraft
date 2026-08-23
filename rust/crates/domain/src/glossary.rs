//! Термины глоссария и область действия (Phase 5).

use crate::{AudioChannel, SpeechLanguage};

/// Область действия термина глоссария.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GlossaryScope {
    Global,
    Meeting { meeting_id: String },
}

/// Что термин делает с текстом.
///
/// Разделение вынужденное: `normalize_caption` заменяет безусловно и
/// везде, поэтому термин, родившийся из грамматической правки, переписывал
/// бы все будущие тексты. Подсказка такого сделать не может — цена ошибки
/// в `initial_prompt` мизерная и обратимая.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GlossaryKind {
    /// Только подсказка Whisper.
    Hint,
    /// Замена surface → canonical в готовом тексте.
    Replacement,
}

impl GlossaryKind {
    pub fn code(self) -> i64 {
        match self {
            Self::Hint => 0,
            Self::Replacement => 1,
        }
    }

    /// Неизвестный код читается как подсказка: она безопаснее замены.
    pub fn from_code(code: i64) -> Self {
        match code {
            1 => Self::Replacement,
            _ => Self::Hint,
        }
    }
}

/// Термин глоссария: surface → canonical с языком и scope.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GlossaryTerm {
    pub id: String,
    pub surface: String,
    pub canonical: String,
    pub language: SpeechLanguage,
    pub scope: GlossaryScope,
    pub kind: GlossaryKind,
}

/// Чем кандидат обратил на себя внимание.
///
/// Правило хранится вместе с кандидатом, потому что прибор считает
/// числа по каждому отдельно: то, которое не окупается, выбрасывается
/// по числу, а не по ощущению.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CandidateRule {
    /// Латиница внутри русской реплики: `UniFFI`, `backend`.
    Latin,
    /// Две и более заглавных подряд: `API`, `СБП`.
    Acronym,
    /// Повторён достаточно раз и не входит в частотную верхушку самой
    /// встречи. Самое слабое из трёх: словаря, с которым сверяться, нет.
    Repeated,
}

/// Реплика, в которой кандидат встретился.
///
/// Без неё кандидат неодобряем: одна строка вне контекста человеку
/// ничего не говорит через неделю после встречи.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CandidateExample {
    pub start_ms: u64,
    pub text: String,
}

/// Кандидат в термины глоссария.
///
/// `surface` — форма **как распознано**. Верной формы не знает никто, в
/// том числе добыча, поэтому одобренный кандидат становится подсказкой
/// (`GlossaryKind::Hint`) и готовый текст не переписывает.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TermCandidate {
    pub surface: String,
    pub rule: CandidateRule,
    pub occurrences: u32,
    /// Не более двух: больше человек не читает, а меньше не убеждает.
    pub examples: Vec<CandidateExample>,
}

/// Предложение модели: как слово стоит в расшифровке и чем оно, по её
/// мнению, было.
///
/// Отличие от [`TermCandidate`] — в одном поле и в одном праве. Добыча
/// верной формы не знает и потому её не называет; модель называет
/// (`canonical`), но **звука не слышала**: это априорная вероятность, а
/// не свидетельство. Свидетельством пара становится только после того,
/// как человек послушал фрагмент, — ради этого здесь лежит место
/// (`channel`, `start_ms`, `end_ms`), а не один текст.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TermFix {
    pub channel: AudioChannel,
    pub start_ms: u64,
    pub end_ms: u64,
    /// Форма **как в расшифровке**, а не как её написала модель.
    pub surface: String,
    /// Что предложила модель. Догадка.
    pub canonical: String,
    /// На что она опёрлась — соседние слова. Пустая опора сама по себе
    /// сигнал, и видит его человек, а не порог.
    pub reason: String,
    /// Реплика целиком: без неё пара неодобряема глазами.
    pub replica_text: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn glossary_term_holds_meeting_scope() {
        let t = GlossaryTerm {
            id: "1".into(),
            surface: "униффи".into(),
            canonical: "UniFFI".into(),
            language: SpeechLanguage::Ru,
            scope: GlossaryScope::Meeting {
                meeting_id: "s1".into(),
            },
            kind: GlossaryKind::Replacement,
        };
        assert!(matches!(t.scope, GlossaryScope::Meeting { .. }));
    }

    #[test]
    fn a_candidate_carries_its_evidence() {
        let candidate = TermCandidate {
            surface: "UniFFI".into(),
            rule: CandidateRule::Latin,
            occurrences: 4,
            examples: vec![CandidateExample {
                start_ms: 12_000,
                text: "посмотри UniFFI завтра".into(),
            }],
        };

        assert_eq!(candidate.rule, CandidateRule::Latin);
        assert_eq!(candidate.examples[0].start_ms, 12_000);
    }

    /// Предложение неодобряемо без места и без реплики: одно ради ▶,
    /// другое ради глаз. Пара сама по себе — только догадка.
    #[test]
    fn a_fix_carries_the_place_you_can_listen_to() {
        let fix = TermFix {
            channel: AudioChannel::System,
            start_ms: 762_000,
            end_ms: 767_500,
            surface: "кобриаты".into(),
            canonical: "ковариаты".into(),
            reason: "рядом «регрессия» и «выборка»".into(),
            replica_text: "кобриаты в регрессии считаем по той же выборке".into(),
        };

        assert_eq!(fix.start_ms, 762_000);
        assert!(fix.replica_text.contains(&fix.surface));
    }

    /// Правило `Repeated` держится на частоте, а не на форме, и потому
    /// названо сомнительным в спеке. Тест закрепляет только то, что
    /// правила различимы: по ним прибор разносит числа.
    #[test]
    fn rules_are_distinct() {
        assert_ne!(CandidateRule::Latin, CandidateRule::Acronym);
        assert_ne!(CandidateRule::Acronym, CandidateRule::Repeated);
    }
}
