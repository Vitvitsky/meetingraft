//! Пересчёт подписей по слепкам: что делать с репликами Final (ADR-013).
//!
//! Схема целиком: человек подписал несколько реплик → по ним сложились
//! слепки → слепки прогнаны по всем репликам → непохожее осталось
//! неопознанным → человек подписал ещё → пересчёт. Обучения нет: слепок —
//! среднее по векторам, и добавление примеров стоит ноль.
//!
//! Здесь **только решение**, без звука и без базы. Векторы считает движок
//! за фичей `model`, читает и пишет их хранилище, а этот файл отвечает на
//! два вопроса и собирается везде: из чего складывать слепки и какой
//! реплике какое имя ставить.
//!
//! Разделение не для красоты. Правило «неопознанный остаётся
//! неопознанным» — главное в этой ветке, и проверяться оно должно без
//! модели, на Linux, числами из теста, а не прогоном на записи.

use domain::SpeakerSource;

use crate::voiceprint::{Match, VoicePrint, best_match, build_print};

/// Порог похожести по умолчанию.
///
/// Середина измеренного окна 0.40…0.50 (замер на встрече `6CE19EC5`,
/// 2026-08-12): ниже 0.40 появляются неверные подписи, выше 0.50 быстро
/// растёт неопознанное, которое безобидно. Середина, а не край, — то же
/// правило, что при выборе порога кластеризации: край плато держится
/// ровно до первой другой записи.
///
/// Умолчание, а не константа: числа сняты на одной встрече и **старой**
/// английской моделью.
pub const DEFAULT_ACCEPT: f32 = 0.45;

/// Отрыв от следующего кандидата по умолчанию.
///
/// Без него двое похожих делят реплики монеткой, причём каждый раз
/// уверенно.
pub const DEFAULT_MARGIN: f32 = 0.05;

/// Реплика Final с посчитанным вектором голоса.
#[derive(Debug, Clone, PartialEq)]
pub struct Reply {
    /// Порядковый номер внутри версии Final.
    pub index: u32,
    /// Кому подписана сейчас; пусто — никому.
    pub speaker_id: String,
    /// Откуда взялась текущая подпись.
    pub source: SpeakerSource,
    /// Вектор голоса на отрезке реплики.
    ///
    /// Пустой означает «посчитать не удалось» — реплика короче окна
    /// модели, звук за неё удалён, отказ движка. Такая реплика не
    /// участвует ни в складывании слепков, ни в подписи, и это отличается
    /// от «вектор есть, но не похож ни на кого»: первое — молчание
    /// прибора, второе — его ответ.
    pub vector: Vec<f32>,
    pub seconds: f32,
}

/// Что пересчёт предлагает сделать с одной репликой.
#[derive(Debug, Clone, PartialEq)]
pub struct Assignment {
    pub index: u32,
    /// Кому подписать; пусто — снять подпись, поставленную прошлым
    /// пересчётом.
    pub speaker_id: String,
    pub similarity: f32,
}

/// Итог пересчёта.
#[derive(Debug, Clone, PartialEq)]
pub struct EnrollPlan {
    /// Слепки участников: идентификатор спикера и вектор.
    pub prints: Vec<(String, VoicePrint)>,
    /// Реплики, у которых подпись меняется.
    pub assignments: Vec<Assignment>,
    /// Реплик осталось неопознанными (считая те, что и были).
    pub unknown: usize,
    /// Реплик, у которых нет вектора, — считать было нечего.
    pub without_vector: usize,
}

/// Сложить слепки по подписанному человеком и разнести остальные реплики.
///
/// Слепки складываются **только по ручным подписям**. Это не
/// осторожность, а условие работоспособности: сложи слепок по тому, что
/// сам же и подписал, — и он подтвердит собственную прошлую ошибку, а
/// отчёт выйдет прекрасным и пустым. Тем же способом можно получить
/// красивые числа на замере, и в приборе `--enroll` разметка ради этого
/// делится надвое.
///
/// Подпись по каналу (ADR-012) в слепок тоже не идёт. На встрече через
/// динамики микрофонная дорожка несёт всех участников (ADR-014), и слепок
/// «владельца», сложенный по всему каналу `mic`, оказался бы средним по
/// шестерым.
pub fn plan(replies: &[Reply], accept: f32, margin: f32) -> EnrollPlan {
    /// Куски одного человека: вектор и его длительность в секундах.
    /// Длительность идёт вместе с вектором, потому что `build_print`
    /// считает по ней секунды слепка — то, чем человеку показывают,
    /// насколько слепок надёжен.
    type Samples = Vec<(Vec<f32>, f32)>;

    let mut by_speaker: Vec<(String, Samples)> = Vec::new();
    for reply in replies {
        if reply.source != SpeakerSource::Human
            || reply.speaker_id.is_empty()
            || reply.vector.is_empty()
        {
            continue;
        }
        match by_speaker
            .iter_mut()
            .find(|(id, _)| id == &reply.speaker_id)
        {
            Some((_, samples)) => samples.push((reply.vector.clone(), reply.seconds)),
            None => by_speaker.push((
                reply.speaker_id.clone(),
                vec![(reply.vector.clone(), reply.seconds)],
            )),
        }
    }

    let mut prints: Vec<(String, VoicePrint)> = by_speaker
        .into_iter()
        .filter_map(|(id, samples)| build_print(&samples).map(|print| (id, print)))
        .collect();
    prints.sort_by(|a, b| a.0.cmp(&b.0));

    let mut assignments = Vec::new();
    let mut unknown = 0usize;
    let mut without_vector = 0usize;
    for reply in replies {
        // Порядок источников — единственное место, где он записан
        // (`SpeakerSource::may_overwrite`). Ручное и канальное пересчёт не
        // трогает вовсе, поэтому и в неопознанные они не попадают: они
        // подписаны, просто не нами.
        if !reply.source.may_overwrite(SpeakerSource::VoicePrint) {
            continue;
        }
        if reply.vector.is_empty() {
            without_vector += 1;
            // Прошлый пересчёт мог подписать эту реплику, когда вектор
            // ещё считался. Снимаем: подпись без основания хуже её
            // отсутствия, а основания у нас больше нет.
            if reply.source == SpeakerSource::VoicePrint && !reply.speaker_id.is_empty() {
                assignments.push(Assignment {
                    index: reply.index,
                    speaker_id: String::new(),
                    similarity: 0.0,
                });
            }
            continue;
        }

        let (speaker_id, similarity) = match best_match(&reply.vector, &prints, accept, margin) {
            Match::Named {
                name, similarity, ..
            } => (name, similarity),
            Match::Unknown { best, .. } => {
                unknown += 1;
                (String::new(), best)
            }
        };
        if speaker_id != reply.speaker_id {
            assignments.push(Assignment {
                index: reply.index,
                speaker_id,
                similarity,
            });
        }
    }

    EnrollPlan {
        prints,
        assignments,
        unknown,
        without_vector,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reply(index: u32, speaker: &str, source: SpeakerSource, vector: Vec<f32>) -> Reply {
        Reply {
            index,
            speaker_id: speaker.to_owned(),
            source,
            vector,
            seconds: 3.0,
        }
    }

    /// Заведомо положительный случай: две группы непохожих векторов,
    /// по одной ручной подписи в каждой — остальные обязаны разойтись
    /// по своим.
    ///
    /// Без него все проверки ниже выполнялись бы и на движке, который не
    /// подписывает вообще ничего.
    #[test]
    fn replies_go_to_the_speaker_they_sound_like() {
        let replies = vec![
            reply(0, "anna", SpeakerSource::Human, vec![1.0, 0.0]),
            reply(1, "peter", SpeakerSource::Human, vec![0.0, 1.0]),
            reply(2, "", SpeakerSource::None, vec![0.96, 0.28]),
            reply(3, "", SpeakerSource::None, vec![0.28, 0.96]),
        ];

        let plan = plan(&replies, DEFAULT_ACCEPT, DEFAULT_MARGIN);

        assert_eq!(plan.prints.len(), 2, "слепка два");
        assert_eq!(
            plan.assignments
                .iter()
                .map(|a| (a.index, a.speaker_id.as_str()))
                .collect::<Vec<_>>(),
            vec![(2, "anna"), (3, "peter")]
        );
        assert_eq!(plan.unknown, 0);
    }

    /// Главное правило ветки: непохожее остаётся неопознанным, а не
    /// уходит к ближайшему.
    #[test]
    fn a_stranger_stays_unknown() {
        let replies = vec![
            reply(0, "anna", SpeakerSource::Human, vec![1.0, 0.0, 0.0]),
            reply(1, "peter", SpeakerSource::Human, vec![0.0, 1.0, 0.0]),
            reply(2, "", SpeakerSource::None, vec![0.0, 0.0, 1.0]),
        ];

        let plan = plan(&replies, DEFAULT_ACCEPT, DEFAULT_MARGIN);

        assert_eq!(plan.unknown, 1);
        assert!(
            plan.assignments.is_empty(),
            "чужому поставили имя: {:?}",
            plan.assignments
        );
    }

    /// Двое одинаково похожи — не подписывать никого. Иначе реплики
    /// делятся монеткой, и каждая выглядит уверенной.
    #[test]
    fn a_tie_between_two_speakers_signs_neither() {
        let replies = vec![
            reply(0, "anna", SpeakerSource::Human, vec![1.0, 0.0]),
            reply(1, "peter", SpeakerSource::Human, vec![0.0, 1.0]),
            // Ровно между ними: похожесть к обоим одна и та же.
            reply(2, "", SpeakerSource::None, vec![1.0, 1.0]),
        ];

        let plan = plan(&replies, DEFAULT_ACCEPT, DEFAULT_MARGIN);

        assert_eq!(plan.unknown, 1);
        assert!(plan.assignments.is_empty());
    }

    /// Ручное сильнее автоматического (ADR-013): подписанное человеком
    /// пересчёт не трогает, даже когда слепок другого похож сильнее.
    #[test]
    fn a_human_label_is_never_rewritten() {
        let replies = vec![
            reply(0, "anna", SpeakerSource::Human, vec![1.0, 0.0]),
            reply(1, "peter", SpeakerSource::Human, vec![0.0, 1.0]),
            // Звучит как Анна, подписан человеком как Пётр. Человек прав.
            reply(2, "peter", SpeakerSource::Human, vec![1.0, 0.02]),
        ];

        let plan = plan(&replies, DEFAULT_ACCEPT, DEFAULT_MARGIN);

        assert!(
            !plan.assignments.iter().any(|a| a.index == 2),
            "пересчёт тронул ручную подпись: {:?}",
            plan.assignments
        );
    }

    /// Подписанное каналом слепок тоже не трогает: на звонке один на один
    /// канал точен абсолютно (ADR-012), и уточнять там нечего.
    #[test]
    fn a_channel_label_is_left_alone() {
        let replies = vec![
            reply(0, "anna", SpeakerSource::Human, vec![1.0, 0.0]),
            reply(1, "peter", SpeakerSource::Channel, vec![0.99, 0.1]),
        ];

        let plan = plan(&replies, DEFAULT_ACCEPT, DEFAULT_MARGIN);

        assert!(plan.assignments.is_empty());
    }

    /// Слепок складывается только по ручному. Подпись по каналу в него не
    /// идёт: на встрече через динамики канал `mic` несёт всех участников
    /// (ADR-014), и такой слепок был бы средним по шестерым.
    #[test]
    fn only_human_labels_feed_a_print() {
        let replies = vec![
            reply(0, "anna", SpeakerSource::Channel, vec![1.0, 0.0]),
            reply(1, "peter", SpeakerSource::VoicePrint, vec![0.0, 1.0]),
        ];

        let plan = plan(&replies, DEFAULT_ACCEPT, DEFAULT_MARGIN);

        assert!(plan.prints.is_empty(), "слепок сложен не по ручному");
    }

    /// Пересчёт обязан снимать собственную прошлую подпись, когда
    /// основания для неё не стало: например, звук встречи удалён и вектор
    /// больше не считается.
    #[test]
    fn a_previous_voiceprint_label_is_withdrawn_when_the_vector_is_gone() {
        let replies = vec![
            reply(0, "anna", SpeakerSource::Human, vec![1.0, 0.0]),
            reply(1, "anna", SpeakerSource::VoicePrint, Vec::new()),
        ];

        let plan = plan(&replies, DEFAULT_ACCEPT, DEFAULT_MARGIN);

        assert_eq!(plan.without_vector, 1);
        assert_eq!(
            plan.assignments,
            vec![Assignment {
                index: 1,
                speaker_id: String::new(),
                similarity: 0.0,
            }]
        );
    }

    /// Реплика без вектора — это молчание прибора, а не его ответ. В
    /// неопознанные она не попадает: там лежит то, что померили и не
    /// узнали.
    #[test]
    fn a_reply_without_a_vector_is_not_counted_as_unknown() {
        let replies = vec![
            reply(0, "anna", SpeakerSource::Human, vec![1.0, 0.0]),
            reply(1, "", SpeakerSource::None, Vec::new()),
        ];

        let plan = plan(&replies, DEFAULT_ACCEPT, DEFAULT_MARGIN);

        assert_eq!(plan.without_vector, 1);
        assert_eq!(plan.unknown, 0);
    }

    /// Без разметки пересчёт не делает ничего. Схема со слепками стоит на
    /// том, что число людей называет человек; выдумывать их — работа
    /// кластеризации, которая замер проиграла.
    #[test]
    fn without_any_human_label_nothing_happens() {
        let replies = vec![
            reply(0, "", SpeakerSource::None, vec![1.0, 0.0]),
            reply(1, "", SpeakerSource::None, vec![0.0, 1.0]),
        ];

        let plan = plan(&replies, DEFAULT_ACCEPT, DEFAULT_MARGIN);

        assert!(plan.prints.is_empty());
        assert!(plan.assignments.is_empty());
        assert_eq!(plan.unknown, 2, "обе реплики померены и не узнаны");
    }

    /// Повторный пересчёт на тех же данных ничего не меняет: иначе
    /// кнопка «пересчитать» переписывала бы транскрипт при каждом
    /// нажатии, и человек не мог бы сказать, устоялось ли уже.
    #[test]
    fn a_second_pass_over_settled_labels_changes_nothing() {
        let replies = vec![
            reply(0, "anna", SpeakerSource::Human, vec![1.0, 0.0]),
            reply(1, "peter", SpeakerSource::Human, vec![0.0, 1.0]),
            reply(2, "anna", SpeakerSource::VoicePrint, vec![0.96, 0.28]),
        ];

        let plan = plan(&replies, DEFAULT_ACCEPT, DEFAULT_MARGIN);

        assert!(
            plan.assignments.is_empty(),
            "устоявшееся переписано: {:?}",
            plan.assignments
        );
    }
}
