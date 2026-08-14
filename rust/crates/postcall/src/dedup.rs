//! Удвоенные реплики: измерение и свёртка (Epic 8, задачи 1–2).
//!
//! Микрофон слышит созвон (ADR-014): реплика удалённого участника
//! попадает и в системную дорожку, и в микрофонную, и Whisper
//! распознаёт обе — **разными словами**. Дословных повторов среди них
//! 12–15%, так что дословное сравнение меряет не то.
//!
//! Две половины, и порядок между ними жёсткий. [`scan_twins`] мерит и
//! печатается прибором `dup-probe`; [`collapse_doubles`] сворачивает и
//! **берёт порог параметром**. Умолчания у порога нет вовсе: его берут
//! из распределения, которое напечатал прибор, а не из головы.
//!
//! **Final не меняется ни на шаг.** Свёртка отдаёт отдельный список для
//! входа артефакта, а исходные сегменты не трогает: обе копии реплики в
//! Final уместны — по ним и видно, что микрофон слышал созвон.
//!
//! Мер похожести две, и обе нормированы на длину. Пословное
//! редакционное расстояние ([`word_similarity`]) учитывает порядок слов,
//! доля общих слов ([`word_overlap`]) — нет. Какая из них делит пары
//! лучше, решается по числам с настоящей записи, а не здесь.

use domain::{AudioChannel, FinalSegment, SpeakerSource, SpeechLanguage};

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

    let (paired, lonely_mic) = pair_up(&mic, &system, &mic_words, &system_words);

    // Контроль ищется только для тех реплик, у которых близнец нашёлся.
    let mut control = Vec::new();
    for &(mic_position, _) in &paired {
        if let Some(far) = farthest_match(
            mic[mic_position],
            &mic_words[mic_position],
            &system,
            &system_words,
        ) {
            control.push(far);
        }
    }

    TwinScan {
        mic_total: mic.len(),
        system_total: system.len(),
        overlapping: paired.into_iter().map(|(_, pair)| pair).collect(),
        lonely_mic,
        control,
    }
}

/// Лучший системный близнец для каждой микрофонной реплики и число тех,
/// у кого его нет вовсе.
///
/// Отдельно от [`scan_twins`], потому что контроль стоит дорого —
/// сравнение каждой микрофонной реплики со **всеми** далёкими
/// системными, — а свёртке ([`collapse_doubles`]) он не нужен: она идёт
/// по живому пути сборки артефакта, где лишний проход платится временем
/// человека.
///
/// Пара едет вместе с местом микрофонной реплики в списке: искать его
/// заново по номеру значило бы завести второе правило соответствия там,
/// где хватает одного.
fn pair_up(
    mic: &[&FinalSegment],
    system: &[&FinalSegment],
    mic_words: &[Vec<String>],
    system_words: &[Vec<String>],
) -> (Vec<(usize, TwinPair)>, usize) {
    let mut pairs = Vec::new();
    let mut lonely = 0;

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
        match best {
            Some(pair) => pairs.push((mic_position, pair)),
            None => lonely += 1,
        }
    }

    (pairs, lonely)
}

/// Отчёт о свёртке.
///
/// Едет вместе с сегментами и дальше — в артефакт. Право на
/// преобразование даёт отчёт о нём, а не его качество: человек обязан
/// видеть, сколько реплик исчезло с его глаз, даже если исчезли они
/// правильно.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CollapseReport {
    /// Реплик осталось.
    pub kept: usize,
    /// Микрофонных копий свёрнуто в системные.
    pub collapsed: usize,
    /// Реплик оставлено потому, что их касался человек.
    ///
    /// Отдельным числом от [`CollapseReport::kept`]: это не «столько
    /// было непохожих», а «столько раз свёртка отступила», и молчать об
    /// этом нельзя — иначе правка выглядит дублем, а дубль правкой.
    pub kept_by_hand: usize,
    /// Порог, по которому свернули. Записан в отчёт, потому что без него
    /// числа выше не значат ничего.
    pub threshold: f32,
}

/// Строка отчёта для человека, читающего артефакт.
///
/// Свёртка обязана быть **видимой**: право на преобразование даёт отчёт
/// о нём, а не его качество. Без этой строки реплики исчезают из брифа
/// молча, и человеку неоткуда узнать, что их вообще было больше.
///
/// Язык — тот же, на котором собран артефакт: строка едет в его тело, а
/// не в интерфейс, и локализовать её нечем, кроме языка встречи.
///
/// Числа стоят после двоеточий не для красоты: «свёрнуто 2 дубля» и
/// «свёрнуто 5 дублей» — разные формы, и склонять их в трёх языках
/// значит завести грамматику там, где нужен отчёт.
pub fn collapse_note(report: &CollapseReport, language: SpeechLanguage) -> String {
    let (built, folded, threshold, by_hand) = match language {
        SpeechLanguage::Ru => (
            "Собран из реплик",
            "свёрнуто удвоенных",
            "порог",
            "оставлено правленых рукой",
        ),
        SpeechLanguage::En => (
            "Built from replies",
            "doubles collapsed",
            "threshold",
            "kept as hand-edited",
        ),
        SpeechLanguage::Es => (
            "Compuesto de intervenciones",
            "duplicados plegados",
            "umbral",
            "conservadas por edición manual",
        ),
    };
    let mut note = format!(
        "{built}: {}; {folded}: {}; {threshold}: {:.2}",
        report.kept, report.collapsed, report.threshold
    );
    if report.kept_by_hand > 0 {
        note.push_str(&format!("; {by_hand}: {}", report.kept_by_hand));
    }
    note.push('.');
    note
}

/// Строка о том, что свёртку сделать было не над чем.
///
/// Отдельным сообщением от [`collapse_note`], потому что «свёрнуто 0
/// дублей» и «сворачивать было нечего» — разные ответы, и первый из них
/// на версии без реплик был бы неправдой: пар не искали вовсе.
///
/// Такие версии Final собраны из live-субтитров (ADR-011), реплик у них
/// нет, и свёртке не за что взяться.
pub fn collapse_skipped_note(language: SpeechLanguage) -> String {
    match language {
        SpeechLanguage::Ru => {
            "Свёртка удвоенных реплик не выполнена: у этой версии Final нет реплик.".to_string()
        }
        SpeechLanguage::En => {
            "Doubles were not collapsed: this Final version has no replies.".to_string()
        }
        SpeechLanguage::Es => {
            "No se plegaron duplicados: esta versión de Final no tiene intervenciones.".to_string()
        }
    }
}

/// Результат свёртки: сегменты и отчёт о том, что с ними сделали.
#[derive(Debug, Clone, PartialEq)]
pub struct Collapsed {
    pub segments: Vec<FinalSegment>,
    pub report: CollapseReport,
}

/// Почему свернуть не вышло.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CollapseError {
    /// Порог вне (0.0, 1.0] либо не число.
    ///
    /// Отказом, а не умолчанием: порог здесь — единственное, что стоит
    /// между «свернуть дубли» и «выбросить половину встречи», и
    /// подставить вместо него своё значение значит решить за человека
    /// то, ради чего затевался прибор.
    Threshold(f32),
}

impl std::fmt::Display for CollapseError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Threshold(value) => write!(
                formatter,
                "порог похожести {value} вне (0.0, 1.0]: свёртка без порога не бывает"
            ),
        }
    }
}

impl std::error::Error for CollapseError {}

/// Свернуть удвоенные реплики: механически, обратимо, с отчётом.
///
/// **Final не меняется.** Вход — его сегменты, выход — отдельный список
/// для входа артефакта; порядок и нумерация сохраняются, чтобы по
/// оставшейся реплике можно было найти её место в Final.
///
/// Свёртка требует всех условий сразу:
///
/// 1. реплики с **разных** дорожек — удвоение живёт между ними;
/// 2. они пересекаются во времени;
/// 3. похожесть текста не ниже `threshold`.
///
/// Из пары остаётся **системная** копия: она прямой цифровой сигнал, а
/// микрофонная прошла динамики, комнату и АРУ. На скрине 2026-08-14
/// системная и распознана лучше.
///
/// Порог параметром, и умолчания у него нет: его берут из распределения,
/// которое печатает `dup-probe`, а не из головы.
pub fn collapse_doubles(
    segments: &[FinalSegment],
    threshold: f32,
) -> Result<Collapsed, CollapseError> {
    if !threshold.is_finite() || threshold <= 0.0 || threshold > 1.0 {
        return Err(CollapseError::Threshold(threshold));
    }

    let mic: Vec<&FinalSegment> = channel_segments(segments, AudioChannel::Mic);
    let system: Vec<&FinalSegment> = channel_segments(segments, AudioChannel::System);
    let mic_words: Vec<Vec<String>> = mic.iter().map(|segment| words(&segment.text)).collect();
    let system_words: Vec<Vec<String>> =
        system.iter().map(|segment| words(&segment.text)).collect();
    let (paired, _) = pair_up(&mic, &system, &mic_words, &system_words);

    let mut drop_indices: Vec<u32> = Vec::new();
    let mut kept_by_hand = 0;
    for (mic_position, pair) in paired {
        if pair.similarity < threshold {
            continue;
        }
        // То, чего человек касался, не исчезает. Ручная правка (Epic 19)
        // и ручная подпись (ADR-013) — свидетельства о реплике, и
        // механическое правило их не отменяет.
        if touched_by_hand(mic[mic_position]) {
            kept_by_hand += 1;
            continue;
        }
        drop_indices.push(pair.mic_index);
    }

    let kept: Vec<FinalSegment> = segments
        .iter()
        .filter(|segment| {
            segment.channel != AudioChannel::Mic || !drop_indices.contains(&segment.index)
        })
        .cloned()
        .collect();

    Ok(Collapsed {
        report: CollapseReport {
            kept: kept.len(),
            collapsed: drop_indices.len(),
            kept_by_hand,
            threshold,
        },
        segments: kept,
    })
}

/// Касался ли реплики человек.
fn touched_by_hand(segment: &FinalSegment) -> bool {
    segment.text_edited || segment.speaker_source == SpeakerSource::Human
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

    /// Порог для тестов свёртки.
    ///
    /// Не «правильный» и не измеренный: живых чисел ещё нет. Он взят так,
    /// чтобы известная пара со скрина оказалась выше него, а чужая речь —
    /// ниже, и годится ровно на то, чтобы проверить правила свёртки.
    const TEST_THRESHOLD: f32 = 0.5;

    fn doubled_meeting() -> Vec<FinalSegment> {
        vec![
            segment(0, AudioChannel::Mic, 1_000, 5_000, MIC_LINE),
            segment(1, AudioChannel::System, 1_200, 5_100, SYSTEM_LINE),
            segment(
                2,
                AudioChannel::Mic,
                10_000,
                14_000,
                "Тогда я беру на себя выгрузку",
            ),
        ]
    }

    #[test]
    fn the_note_names_the_numbers_and_the_threshold() {
        let report = CollapseReport {
            kept: 128,
            collapsed: 96,
            kept_by_hand: 0,
            threshold: 0.6,
        };
        let note = collapse_note(&report, SpeechLanguage::Ru);
        assert!(note.contains("128"), "{note}");
        assert!(note.contains("96"), "{note}");
        assert!(note.contains("0.60"), "порог обязан быть в строке: {note}");
        assert!(
            !note.contains("рукой"),
            "нечего сообщать: свёртка ни разу не отступила — {note}"
        );
    }

    #[test]
    fn the_note_says_when_the_fold_stepped_back() {
        // Отступление свёртки — не то же, что её отсутствие, и молчать о
        // нём нельзя: иначе правка выглядит дублем, а дубль правкой.
        let report = CollapseReport {
            kept: 10,
            collapsed: 2,
            kept_by_hand: 3,
            threshold: 0.5,
        };
        let note = collapse_note(&report, SpeechLanguage::Ru);
        assert!(note.contains("рукой"), "{note}");
        assert!(note.contains('3'), "{note}");
    }

    #[test]
    fn a_double_collapses_into_the_system_copy() {
        let collapsed = collapse_doubles(&doubled_meeting(), TEST_THRESHOLD).expect("свёртка");
        assert_eq!(collapsed.report.collapsed, 1);
        assert_eq!(collapsed.report.kept, 2);
        assert_eq!(collapsed.report.kept_by_hand, 0);
        let texts: Vec<&str> = collapsed
            .segments
            .iter()
            .map(|segment| segment.text.as_str())
            .collect();
        assert_eq!(
            texts,
            vec![SYSTEM_LINE, "Тогда я беру на себя выгрузку"],
            "остаётся системная копия: она прямой сигнал, а не запись через комнату"
        );
    }

    #[test]
    fn the_final_itself_is_left_alone() {
        // Обе копии в Final уместны: по ним и видно, что микрофон слышал
        // созвон. Свёртка отдаёт отдельный список, а не правит вход.
        let segments = doubled_meeting();
        let before = segments.clone();
        let collapsed = collapse_doubles(&segments, TEST_THRESHOLD).expect("свёртка");
        assert_eq!(segments, before, "вход изменён");
        assert!(collapsed.segments.len() < segments.len());
    }

    #[test]
    fn a_reply_the_hand_touched_survives_the_fold() {
        // Правка Epic 19 на микрофонной копии: то, чего человек касался,
        // не исчезает молча.
        let mut segments = doubled_meeting();
        segments[0].text_edited = true;
        let collapsed = collapse_doubles(&segments, TEST_THRESHOLD).expect("свёртка");
        assert_eq!(collapsed.report.collapsed, 0);
        assert_eq!(collapsed.report.kept_by_hand, 1);
        assert_eq!(collapsed.segments.len(), 3);
    }

    #[test]
    fn a_reply_signed_by_hand_survives_the_fold() {
        let mut segments = doubled_meeting();
        segments[0].speaker_source = SpeakerSource::Human;
        let collapsed = collapse_doubles(&segments, TEST_THRESHOLD).expect("свёртка");
        assert_eq!(collapsed.report.collapsed, 0);
        assert_eq!(collapsed.report.kept_by_hand, 1);
    }

    #[test]
    fn a_reply_signed_by_channel_or_print_folds_as_usual() {
        // Свидетельство — только рука. Канал проставляется оптом
        // пересбором, слепок — автоматикой (ADR-013), и защитой от
        // свёртки ни то, ни другое быть не может.
        for source in [SpeakerSource::Channel, SpeakerSource::VoicePrint] {
            let mut segments = doubled_meeting();
            segments[0].speaker_source = source;
            let collapsed = collapse_doubles(&segments, TEST_THRESHOLD).expect("свёртка");
            assert_eq!(collapsed.report.collapsed, 1, "{source:?}");
            assert_eq!(collapsed.report.kept_by_hand, 0, "{source:?}");
        }
    }

    #[test]
    fn unlike_neighbours_do_not_collapse() {
        let segments = vec![
            segment(
                0,
                AudioChannel::Mic,
                1_000,
                5_000,
                "Тогда я беру на себя выгрузку",
            ),
            // Пересекается по времени, но говорит о другом: одновременная
            // речь — не дубль.
            segment(
                1,
                AudioChannel::System,
                1_200,
                5_100,
                "Давай тогда созвонимся после обеда",
            ),
        ];
        let collapsed = collapse_doubles(&segments, TEST_THRESHOLD).expect("свёртка");
        assert_eq!(collapsed.report.collapsed, 0);
        assert_eq!(collapsed.segments.len(), 2);
    }

    #[test]
    fn an_owner_reply_without_a_twin_is_untouched() {
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
        let collapsed = collapse_doubles(&segments, TEST_THRESHOLD).expect("свёртка");
        assert_eq!(collapsed.report.collapsed, 0);
        assert_eq!(collapsed.segments.len(), 2);
    }

    #[test]
    fn the_system_copy_is_never_the_one_that_goes() {
        // Дословный дубль: похожесть 1.0, и решает уже не она, а правило
        // «остаётся системная». Без него из пары ушла бы любая из двух.
        let segments = vec![
            segment(0, AudioChannel::Mic, 1_000, 5_000, "Значит, сделаем так"),
            segment(1, AudioChannel::System, 1_000, 5_000, "Значит, сделаем так"),
        ];
        let collapsed = collapse_doubles(&segments, TEST_THRESHOLD).expect("свёртка");
        assert_eq!(collapsed.segments.len(), 1);
        assert_eq!(collapsed.segments[0].channel, AudioChannel::System);
    }

    #[test]
    fn the_order_and_numbering_survive() {
        // По оставшейся реплике надо уметь найти её место в Final:
        // перенумерация оборвала бы эту связь.
        let collapsed = collapse_doubles(&doubled_meeting(), TEST_THRESHOLD).expect("свёртка");
        let indices: Vec<u32> = collapsed
            .segments
            .iter()
            .map(|segment| segment.index)
            .collect();
        assert_eq!(indices, vec![1, 2]);
    }

    #[test]
    fn a_threshold_out_of_range_is_refused() {
        // Умолчание вместо порога решило бы за человека то, ради чего
        // заводился прибор, — и решило бы молча.
        for bad in [0.0, -0.1, 1.5, f32::NAN] {
            assert!(
                matches!(
                    collapse_doubles(&doubled_meeting(), bad),
                    Err(CollapseError::Threshold(_))
                ),
                "порог {bad} принят"
            );
        }
        assert!(collapse_doubles(&doubled_meeting(), 1.0).is_ok());
    }

    #[test]
    fn the_report_carries_the_threshold_it_used() {
        // Числа отчёта без порога не значат ничего.
        let collapsed = collapse_doubles(&doubled_meeting(), 0.9).expect("свёртка");
        assert_eq!(collapsed.report.threshold, 0.9);
        assert_eq!(
            collapsed.report.collapsed, 0,
            "порог 0.9 паре со скрина не по росту: она похожа на 0.64"
        );
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
