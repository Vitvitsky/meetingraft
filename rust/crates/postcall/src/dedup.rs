//! Сколько в записи удвоенных реплик (Epic 8, задача 1).
//!
//! Микрофон слышит созвон (ADR-014): реплика удалённого участника
//! попадает и в системную дорожку, и в микрофонную, и Whisper
//! распознаёт обе — **разными словами**. Дословных повторов среди них
//! 12–15%, так что дословное сравнение меряет не то.
//!
//! Здесь только измерение: пары находятся, свёртка не делается и Final
//! не трогается. Порог похожести тут не назначается вовсе — он берётся
//! из распределения, которое печатает `dup-probe`.
//!
//! Мер похожести две, и обе нормированы на длину. Пословное
//! редакционное расстояние ([`word_similarity`]) учитывает порядок слов,
//! доля общих слов ([`word_overlap`]) — нет. Какая из них делит пары
//! лучше, решается по числам с настоящей записи, а не здесь.

use domain::{AudioChannel, FinalSegment};

/// Потолок длины реплики в словах.
///
/// Редакционное расстояние считается таблицей в произведение длин, а
/// прибор гоняет его по всем парам «микрофонная × системная». Реплики
/// Whisper — единицы и десятки слов; потолок нужен на случай сегмента,
/// склеенного из целого монолога, чтобы прибор не встал.
const MAX_WORDS: usize = 400;

/// Насколько далеко должна отстоять реплика, чтобы годиться в контроль.
///
/// Заведомо отрицательный случай: та же микрофонная реплика против
/// системной, которая ей близнецом быть не может. Полминуты берётся с
/// запасом — сегменты Whisper короче десятка секунд.
pub const CONTROL_GAP_MS: u64 = 30_000;

/// Пара «микрофонная реплика и системная».
///
/// Одним типом описаны и пары по перекрытию времени, и контрольные:
/// сравнивать их можно только одной меркой, и разные типы позволили бы
/// незаметно померить их по-разному.
#[derive(Debug, Clone, PartialEq)]
pub struct TwinPair {
    /// Порядковый номер микрофонной реплики в версии Final.
    pub mic_index: u32,
    /// Порядковый номер системной реплики.
    pub system_index: u32,
    /// Пересечение по времени; у контрольных пар — 0.
    pub overlap_ms: u64,
    /// Пословное редакционное расстояние, нормированное на длину.
    pub similarity: f32,
    /// Доля общих слов (мера Дайса по мультимножествам).
    pub overlap_share: f32,
    /// Слов в короткой из двух реплик.
    ///
    /// Короткие («да», «ага», «понятно») совпадают по случайности, и
    /// распределение по ним надо смотреть отдельно, иначе порог
    /// подбирается под шум.
    pub words: usize,
    /// Слов в микрофонной реплике.
    ///
    /// Ею и придётся платить за свёртку: из пары остаётся системная
    /// копия. Отсюда же ответ на «стоит ли овчинка выделки» — доля
    /// текста, которая уйдёт из входа артефакта.
    pub mic_words: usize,
}

/// Что нашлось в записи.
#[derive(Debug, Clone, PartialEq)]
pub struct TwinScan {
    pub mic_total: usize,
    pub system_total: usize,
    /// Микрофонные реплики, у которых есть системная, пересекающаяся по
    /// времени: для каждой — лучшая по похожести.
    pub overlapping: Vec<TwinPair>,
    /// Микрофонные реплики, у которых системного соседа нет вовсе.
    ///
    /// Это кандидаты в речь владельца: в системную дорожку она не
    /// попадает. Близнецов у них нет по построению, и выдавать это за
    /// проверку меры нельзя — проверка живёт в [`TwinScan::control`].
    pub lonely_mic: usize,
    /// Заведомо отрицательный случай: те же микрофонные реплики против
    /// самой похожей **далёкой** системной.
    ///
    /// Берутся ровно те реплики, что попали в [`TwinScan::overlapping`],
    /// иначе распределения окажутся посчитаны по разному набору и
    /// разойдутся из-за состава, а не из-за родства реплик.
    pub control: Vec<TwinPair>,
}

/// Найти пары и контроль к ним.
///
/// Сегменты берутся как есть, порядок и нумерация не меняются: функция
/// ничего не решает про Final.
pub fn scan_twins(segments: &[FinalSegment]) -> TwinScan {
    let mic: Vec<&FinalSegment> = channel_segments(segments, AudioChannel::Mic);
    let system: Vec<&FinalSegment> = channel_segments(segments, AudioChannel::System);

    // Слова считаются по разу на реплику: пар — произведение дорожек, и
    // нормализация в каждой обошлась бы дороже самого сравнения.
    let mic_words: Vec<Vec<String>> = mic.iter().map(|segment| words(&segment.text)).collect();
    let system_words: Vec<Vec<String>> =
        system.iter().map(|segment| words(&segment.text)).collect();

    let mut overlapping = Vec::new();
    let mut control = Vec::new();
    let mut lonely_mic = 0;

    for (mic_position, mic_segment) in mic.iter().enumerate() {
        let mut best: Option<TwinPair> = None;
        for (system_position, system_segment) in system.iter().enumerate() {
            let overlap = overlap_ms(mic_segment, system_segment);
            if overlap == 0 {
                continue;
            }
            let pair = pair_of(
                mic_segment,
                system_segment,
                overlap,
                &mic_words[mic_position],
                &system_words[system_position],
            );
            if best.as_ref().is_none_or(|current| better(&pair, current)) {
                best = Some(pair);
            }
        }

        let Some(best) = best else {
            lonely_mic += 1;
            continue;
        };

        // Контроль ищется только для тех реплик, у которых близнец нашёлся.
        if let Some(far) = farthest_match(
            mic_segment,
            &mic_words[mic_position],
            &system,
            &system_words,
        ) {
            control.push(far);
        }
        overlapping.push(best);
    }

    TwinScan {
        mic_total: mic.len(),
        system_total: system.len(),
        overlapping,
        lonely_mic,
        control,
    }
}

/// Самая похожая системная реплика среди заведомо далёких.
///
/// Берётся именно максимум, а не случайная: вопрос к контролю — не
/// «сколько бывает в среднем», а «как высоко мера забирается по
/// случайности». Ответ на него и есть та граница, ниже которой порог
/// ставить нельзя.
fn farthest_match(
    mic_segment: &FinalSegment,
    mic_words: &[String],
    system: &[&FinalSegment],
    system_words: &[Vec<String>],
) -> Option<TwinPair> {
    let mut best: Option<TwinPair> = None;
    for (position, system_segment) in system.iter().enumerate() {
        if gap_ms(mic_segment, system_segment) < CONTROL_GAP_MS {
            continue;
        }
        let pair = pair_of(
            mic_segment,
            system_segment,
            0,
            mic_words,
            &system_words[position],
        );
        if best.as_ref().is_none_or(|current| better(&pair, current)) {
            best = Some(pair);
        }
    }
    best
}

/// Кто из двух пар лучше подходит на роль близнеца.
///
/// Сначала похожесть, при равной — перекрытие, при равном — меньший
/// номер: без последнего порядок выборки из базы менял бы ответ.
fn better(candidate: &TwinPair, current: &TwinPair) -> bool {
    (
        candidate.similarity,
        candidate.overlap_ms,
        std::cmp::Reverse(candidate.system_index),
    )
        .partial_cmp(&(
            current.similarity,
            current.overlap_ms,
            std::cmp::Reverse(current.system_index),
        ))
        .is_some_and(std::cmp::Ordering::is_gt)
}

fn pair_of(
    mic_segment: &FinalSegment,
    system_segment: &FinalSegment,
    overlap: u64,
    mic_words: &[String],
    system_words: &[String],
) -> TwinPair {
    TwinPair {
        mic_index: mic_segment.index,
        system_index: system_segment.index,
        overlap_ms: overlap,
        similarity: tokens_similarity(mic_words, system_words),
        overlap_share: tokens_overlap(mic_words, system_words),
        words: mic_words.len().min(system_words.len()),
        mic_words: mic_words.len(),
    }
}

fn channel_segments(segments: &[FinalSegment], channel: AudioChannel) -> Vec<&FinalSegment> {
    segments
        .iter()
        .filter(|segment| segment.channel == channel)
        .collect()
}

/// Пересечение двух реплик по времени, 0 — не пересекаются.
pub fn overlap_ms(left: &FinalSegment, right: &FinalSegment) -> u64 {
    let start = left.start_ms.max(right.start_ms);
    let end = left.end_ms.min(right.end_ms);
    end.saturating_sub(start)
}

/// Расстояние между репликами во времени, 0 — пересекаются.
fn gap_ms(left: &FinalSegment, right: &FinalSegment) -> u64 {
    let forward = right.start_ms.saturating_sub(left.end_ms);
    let backward = left.start_ms.saturating_sub(right.end_ms);
    forward.max(backward)
}

/// Пословное редакционное расстояние, нормированное на длину: 1.0 —
/// тексты совпадают, 0.0 — общего нет.
pub fn word_similarity(left: &str, right: &str) -> f32 {
    tokens_similarity(&words(left), &words(right))
}

/// Доля общих слов: мера Дайса по мультимножествам, 1.0 — те же слова в
/// тех же количествах, 0.0 — ни одного общего.
///
/// Порядок слов не учитывается вовсе. Против [`word_similarity`] это и
/// достоинство, и недостаток: перестановка слов родство не рвёт, но и
/// «я тебе говорил» от «ты мне говорил» такая мера не отличает.
pub fn word_overlap(left: &str, right: &str) -> f32 {
    tokens_overlap(&words(left), &words(right))
}

/// Сколько слов в реплике после нормализации.
pub fn word_count(text: &str) -> usize {
    words(text).len()
}

fn tokens_similarity(left: &[String], right: &[String]) -> f32 {
    // Пустая реплика ни на что не похожа, в том числе на другую пустую.
    // Единица здесь означала бы «дубль», и все реплики без слов
    // свернулись бы друг в друга.
    if left.is_empty() || right.is_empty() {
        return 0.0;
    }
    let left = &left[..left.len().min(MAX_WORDS)];
    let right = &right[..right.len().min(MAX_WORDS)];
    let distance = edit_distance(left, right) as f32;
    let longest = left.len().max(right.len()) as f32;
    (1.0 - distance / longest).clamp(0.0, 1.0)
}

fn tokens_overlap(left: &[String], right: &[String]) -> f32 {
    if left.is_empty() || right.is_empty() {
        return 0.0;
    }
    let mut rest: Vec<&String> = right.iter().collect();
    let mut common = 0usize;
    for word in left {
        if let Some(position) = rest.iter().position(|candidate| *candidate == word) {
            rest.swap_remove(position);
            common += 1;
        }
    }
    2.0 * common as f32 / (left.len() + right.len()) as f32
}

/// Расстояние Левенштейна по словам, две строки таблицы.
fn edit_distance(left: &[String], right: &[String]) -> usize {
    let mut previous: Vec<usize> = (0..=right.len()).collect();
    let mut current = vec![0usize; right.len() + 1];
    for (row, left_word) in left.iter().enumerate() {
        current[0] = row + 1;
        for (column, right_word) in right.iter().enumerate() {
            let substitution = previous[column] + usize::from(left_word != right_word);
            current[column + 1] = substitution
                .min(previous[column + 1] + 1)
                .min(current[column] + 1);
        }
        std::mem::swap(&mut previous, &mut current);
    }
    previous[right.len()]
}

/// Слова реплики без пунктуации и регистра.
///
/// «ё» приводится к «е»: Whisper пишет её то так, то так, и разница в
/// одну букву стоила бы целого слова.
fn words(text: &str) -> Vec<String> {
    text.split_whitespace()
        .map(|word| {
            word.chars()
                .filter(|symbol| symbol.is_alphanumeric())
                .flat_map(char::to_lowercase)
                .map(|symbol| if symbol == 'ё' { 'е' } else { symbol })
                .collect::<String>()
        })
        .filter(|word| !word.is_empty())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use domain::SpeakerSource;

    /// Реальная пара со скрина 2026-08-14: одна фраза, распознанная
    /// дважды разными словами.
    const MIC_LINE: &str = "Нет, у тебя вчера какие-то задачки накидывал, а у них нет";
    const SYSTEM_LINE: &str = "Нет, я вчера какие-то задачки накидывал, а там их нет.";

    fn segment(
        index: u32,
        channel: AudioChannel,
        start_ms: u64,
        end_ms: u64,
        text: &str,
    ) -> FinalSegment {
        FinalSegment {
            index,
            start_ms,
            end_ms,
            channel,
            speaker_id: String::new(),
            speaker_source: SpeakerSource::None,
            text: text.to_string(),
            text_edited: false,
            original_text: String::new(),
        }
    }

    #[test]
    fn paraphrase_scores_far_above_unrelated_speech() {
        let paraphrase = word_similarity(MIC_LINE, SYSTEM_LINE);
        let unrelated = word_similarity(MIC_LINE, "Давай тогда созвонимся после обеда");
        assert!(
            paraphrase > 0.5,
            "пересказ той же фразы должен читаться похожим: {paraphrase}"
        );
        assert!(
            paraphrase - unrelated > 0.3,
            "мера обязана делить пересказ и чужую речь: {paraphrase} против {unrelated}"
        );
    }

    #[test]
    fn punctuation_and_case_do_not_break_a_match() {
        assert_eq!(word_similarity("Да, всё верно!", "да все верно"), 1.0);
    }

    #[test]
    fn empty_text_is_not_a_duplicate_of_anything() {
        // Иначе всякая реплика без слов свернулась бы в любую другую.
        assert_eq!(word_similarity("", ""), 0.0);
        assert_eq!(word_overlap("", "   "), 0.0);
        assert_eq!(word_similarity("", "привет"), 0.0);
    }

    #[test]
    fn word_order_separates_the_two_measures() {
        let left = "я тебе это говорил";
        let right = "говорил это тебе я";
        assert_eq!(word_overlap(left, right), 1.0, "слова те же");
        assert!(
            word_similarity(left, right) < 0.6,
            "порядок другой, и пословное расстояние обязано это видеть"
        );
    }

    #[test]
    fn twin_is_found_across_channels_by_time_overlap() {
        let segments = vec![
            segment(0, AudioChannel::Mic, 1_000, 5_000, MIC_LINE),
            segment(1, AudioChannel::System, 1_200, 5_100, SYSTEM_LINE),
        ];
        let scan = scan_twins(&segments);
        assert_eq!(scan.mic_total, 1);
        assert_eq!(scan.system_total, 1);
        assert_eq!(scan.overlapping.len(), 1, "близнец обязан найтись");
        assert_eq!(scan.lonely_mic, 0);
        let pair = &scan.overlapping[0];
        assert_eq!((pair.mic_index, pair.system_index), (0, 1));
        assert!(pair.overlap_ms > 0);
        assert!(pair.similarity > 0.5, "{}", pair.similarity);
    }

    #[test]
    fn the_best_of_several_overlapping_is_taken() {
        let segments = vec![
            segment(0, AudioChannel::Mic, 1_000, 5_000, MIC_LINE),
            // Обе пересекаются по времени, но родная только вторая.
            segment(
                1,
                AudioChannel::System,
                900,
                2_000,
                "Секунду, я отключу микрофон",
            ),
            segment(2, AudioChannel::System, 2_000, 5_100, SYSTEM_LINE),
        ];
        let scan = scan_twins(&segments);
        assert_eq!(scan.overlapping.len(), 1);
        assert_eq!(
            scan.overlapping[0].system_index, 2,
            "выбирается похожая, а не первая попавшаяся"
        );
    }

    #[test]
    fn a_mic_reply_without_system_speech_stays_lonely() {
        let segments = vec![
            segment(
                0,
                AudioChannel::Mic,
                1_000,
                5_000,
                "Тогда я беру на себя выгрузку",
            ),
            segment(1, AudioChannel::System, 60_000, 64_000, SYSTEM_LINE),
        ];
        let scan = scan_twins(&segments);
        assert_eq!(
            scan.overlapping.len(),
            0,
            "пересечения по времени нет вовсе"
        );
        assert_eq!(scan.lonely_mic, 1);
        assert!(
            scan.control.is_empty(),
            "контроль строится только к найденным парам"
        );
    }

    #[test]
    fn same_channel_neighbours_are_never_twins() {
        // Две микрофонные реплики подряд — не дубль, даже дословный:
        // удвоение живёт между дорожками.
        let segments = vec![
            segment(0, AudioChannel::Mic, 1_000, 5_000, MIC_LINE),
            segment(1, AudioChannel::Mic, 1_000, 5_000, MIC_LINE),
        ];
        let scan = scan_twins(&segments);
        assert_eq!(scan.system_total, 0);
        assert_eq!(scan.overlapping.len(), 0);
        assert_eq!(scan.lonely_mic, 2);
    }

    #[test]
    fn control_takes_a_distant_reply_not_the_twin() {
        let segments = vec![
            segment(0, AudioChannel::Mic, 1_000, 5_000, MIC_LINE),
            segment(1, AudioChannel::System, 1_200, 5_100, SYSTEM_LINE),
            segment(
                2,
                AudioChannel::System,
                90_000,
                94_000,
                "Давай тогда созвонимся после обеда",
            ),
        ];
        let scan = scan_twins(&segments);
        assert_eq!(scan.control.len(), 1);
        let control = &scan.control[0];
        assert_eq!(
            control.system_index, 2,
            "в контроль обязана попасть далёкая реплика, а не близнец"
        );
        assert_eq!(control.overlap_ms, 0);
        assert!(
            scan.overlapping[0].similarity - control.similarity > 0.3,
            "зазор между парой и контролем и есть то, ради чего прибор заводился: {} против {}",
            scan.overlapping[0].similarity,
            control.similarity
        );
    }

    #[test]
    fn a_near_but_not_overlapping_reply_is_no_control() {
        // Соседняя реплика — не заведомо отрицательный случай: та же
        // мысль теми же словами продолжается в соседнем сегменте.
        let segments = vec![
            segment(0, AudioChannel::Mic, 1_000, 5_000, MIC_LINE),
            segment(1, AudioChannel::System, 1_200, 5_100, SYSTEM_LINE),
            segment(
                2,
                AudioChannel::System,
                6_000,
                9_000,
                "Ну да, ровно об этом и речь",
            ),
        ];
        let scan = scan_twins(&segments);
        assert!(
            scan.control.is_empty(),
            "до полминуты от реплики контроля нет: {:?}",
            scan.control
        );
    }

    #[test]
    fn pair_reports_the_shorter_length_in_words() {
        let segments = vec![
            segment(0, AudioChannel::Mic, 1_000, 5_000, "Да"),
            segment(
                1,
                AudioChannel::System,
                1_000,
                5_000,
                "Да, конечно, давай так и сделаем",
            ),
        ];
        let scan = scan_twins(&segments);
        assert_eq!(
            scan.overlapping[0].words, 1,
            "короткая сторона и решает, случайно ли совпадение"
        );
        assert_eq!(
            scan.overlapping[0].mic_words, 1,
            "платит за свёртку микрофонная сторона, и её длина считается своя"
        );
    }
}
