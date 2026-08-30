//! Что получается из разметки, кроме эталона: материал для дообучения и
//! пары для глоссария.
//!
//! Спека — `docs/superpowers/specs/2026-08-30-labelled-datasets-design.md`,
//! раздел «Что из разметки получается».
//!
//! Главное правило здесь одно, и оно про отказ. Манифест, собранный из
//! непроверенных фраз, учил бы модель на её собственных ошибках, поэтому
//! непроверенное не «пропускается молча», а **останавливает экспорт с
//! причиной**: пустой манифест выглядел бы как законченная работа.

use std::path::Path;

use crate::labels::{Kind, Labels, Phrase, State};
use crate::wer::{Op, align, normalize};

/// Куда ложится нарезка внутри каталога экспорта.
const CLIPS: &str = "clips";
/// Имя манифеста. Расширение `.jsonl`, потому что это строки JSON, а не
/// один документ: так его читают и NeMo, и всё остальное.
pub const MANIFEST: &str = "manifest.jsonl";

/// Строка манифеста NeMo — того самого, который едят и GigaAM, и
/// parakeet.
#[derive(Debug, Clone, PartialEq)]
pub struct Clip {
    /// Путь относительно каталога экспорта.
    pub audio_filepath: String,
    pub text: String,
    /// Секунды. Дробное — так требует формат.
    ///
    /// `f64`, а не `f32`, и причина видна только в файле: у `f32` 1.6
    /// секунды печатаются как `1.600000023841858`. Числа это не портит,
    /// а манифест — портит, и первым, кто его откроет, будет человек.
    pub duration: f64,
}

/// Чем кончился экспорт.
#[derive(Debug, Clone, PartialEq)]
pub struct Report {
    pub clips: Vec<Clip>,
    /// Сколько фраз осталось непроверенными — они в датасет не идут.
    pub left_unchecked: usize,
    /// Сколько отброшено человеком (`skip`).
    pub left_skipped: usize,
    /// Сколько звука попало в датасет.
    pub total_ms: u64,
}

impl Report {
    /// Доля фраз разметки, дошедшая до датасета.
    ///
    /// Число печатается рядом с манифестом не для красоты: датасет из
    /// одной десятой размеченного — законный результат утра работы и
    /// незаконный повод считать разметку сделанной.
    pub fn share(&self) -> f32 {
        let total = self.clips.len() + self.left_unchecked + self.left_skipped;
        if total == 0 {
            return 0.0;
        }
        self.clips.len() as f32 / total as f32
    }
}

/// Пара для глоссария: распознанное против введённого.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pair {
    pub surface: String,
    pub canonical: String,
    /// Чья гипотеза дала эту пару.
    pub engine: String,
    /// Фраза, из которой пара взята, — чтобы было куда посмотреть.
    pub phrase: String,
}

/// Собрать манифест, ничего не записывая.
///
/// Отказы перечислены здесь целиком и наступают **до** любой записи:
/// каталог с половиной датасета на диске выглядит как датасет.
pub fn build(labels: &Labels, samples: usize, sample_rate: u32) -> Result<Report, String> {
    if sample_rate != crate::case::RATE {
        return Err(format!(
            "частота {sample_rate} — не {}: ресемпл здесь не делается, иначе в дообучение \
             уехал бы звук, которого в записи не было",
            crate::case::RATE
        ));
    }

    let mut clips = Vec::new();
    let mut total_ms = 0;
    let mut left_unchecked = 0;
    let mut left_skipped = 0;

    for phrase in &labels.phrases {
        match phrase.state {
            State::Unchecked => {
                left_unchecked += 1;
                continue;
            }
            State::Skip => {
                left_skipped += 1;
                continue;
            }
            State::Correct | State::Corrected => {}
        }

        if phrase.text.trim().is_empty() {
            // `correct` при пустом тексте — промах разметки, а не
            // молчание в записи: для молчания есть `skip`. Взять такое
            // в датасет значит учить модель отвечать пустотой на речь.
            return Err(format!(
                "фраза {} помечена проверенной, но текста в ней нет",
                phrase.id
            ));
        }
        if phrase.end_ms <= phrase.start_ms {
            return Err(format!(
                "у фразы {} конец не позже начала: {}..{}",
                phrase.id, phrase.start_ms, phrase.end_ms
            ));
        }
        let end = span(phrase, sample_rate).1;
        if end > samples {
            // Обрезать молча значит подать в дообучение звук короче
            // своего текста; заметить это можно будет только на слух.
            return Err(format!(
                "фраза {} кончается на {} мс, а записи всего {} мс",
                phrase.id,
                phrase.end_ms,
                samples as u64 * 1000 / u64::from(sample_rate.max(1))
            ));
        }

        total_ms += phrase.duration_ms();
        clips.push(Clip {
            audio_filepath: format!("{CLIPS}/{}.wav", phrase.id),
            text: phrase.text.trim().to_owned(),
            duration: phrase.duration_ms() as f64 / 1000.0,
        });
    }

    if clips.is_empty() {
        // Тот самый отказ, ради которого модуль написан: пустой манифест
        // на диске неотличим от законченной работы.
        return Err(format!(
            "проверенных фраз нет: непроверенных {left_unchecked}, отброшенных {left_skipped}; \
             манифест из непроверенного учил бы модель на её же ошибках"
        ));
    }

    Ok(Report {
        clips,
        left_unchecked,
        left_skipped,
        total_ms,
    })
}

/// Границы фразы в отсчётах.
fn span(phrase: &Phrase, sample_rate: u32) -> (usize, usize) {
    let per_second = sample_rate as usize;
    let start = phrase.start_ms as usize * per_second / 1000;
    let end = phrase.end_ms as usize * per_second / 1000;
    (start, end)
}

/// Записать нарезку и манифест.
pub fn write(
    labels: &Labels,
    pcm: &[i16],
    sample_rate: u32,
    out_dir: &Path,
) -> Result<Report, String> {
    // Сборка первой, запись второй — и это порядок, а не стиль: отказ
    // обязан случиться до того, как на диск ляжет первый файл.
    let report = build(labels, pcm.len(), sample_rate)?;

    let clips_dir = out_dir.join(CLIPS);
    std::fs::create_dir_all(&clips_dir)
        .map_err(|error| format!("{}: {error}", clips_dir.display()))?;

    for (clip, phrase) in report.clips.iter().zip(labels.verified()) {
        let (start, end) = span(phrase, sample_rate);
        crate::wav::write(
            &out_dir.join(&clip.audio_filepath),
            &pcm[start..end],
            sample_rate,
        )?;
    }

    let manifest = out_dir.join(MANIFEST);
    std::fs::write(&manifest, manifest_text(&report))
        .map_err(|error| format!("{}: {error}", manifest.display()))?;
    Ok(report)
}

/// Манифест строками JSONL.
pub fn manifest_text(report: &Report) -> String {
    let mut out = String::new();
    for clip in &report.clips {
        let line = serde_json::json!({
            "audio_filepath": clip.audio_filepath,
            "text": clip.text,
            "duration": clip.duration,
        });
        out.push_str(&line.to_string());
        out.push('\n');
    }
    out
}

/// Пары для глоссария из правок вида `term`.
///
/// Пара берётся не из фразы целиком, а из **того места, где гипотеза
/// разошлась с ответом человека**: глоссарий нормализует термины, и
/// правило на целую реплику не сработает никогда.
///
/// Соседние расхождения склеиваются в одно: термин, услышанный двумя
/// словами, иначе дал бы пару на первое слово и потерю второго — то
/// есть правило, срабатывающее не там.
pub fn glossary_pairs(labels: &Labels) -> Vec<Pair> {
    let mut pairs = Vec::new();
    for phrase in &labels.phrases {
        // Только `corrected`: `correct` означает, что движок не ошибся,
        // и паре взяться неоткуда.
        if phrase.state != State::Corrected || !phrase.kinds.contains(&Kind::Term) {
            continue;
        }
        let reference = tokens(&phrase.text);
        for (engine, guess) in &phrase.hypotheses {
            let hypothesis = tokens(guess);
            for (surface, canonical) in runs(&reference, &hypothesis) {
                pairs.push(Pair {
                    surface,
                    canonical,
                    engine: engine.clone(),
                    phrase: phrase.id.clone(),
                });
            }
        }
    }
    pairs
}

/// Слово в двух видах: как его показывать и как его сравнивать.
///
/// Сравнивается нормализованное — `wer::normalize`, та же нормализация,
/// что у метрик; второй её быть не должно. А в пару едет исходное: в
/// глоссарии canonical — это «UniFFI», а не «униффи».
fn tokens(text: &str) -> Vec<(String, String)> {
    text.split_whitespace()
        .filter_map(|word| {
            let normalized = normalize(word).pop()?;
            let display = word
                .trim_matches(|symbol: char| !symbol.is_alphanumeric())
                .to_owned();
            if display.is_empty() {
                return None;
            }
            Some((display, normalized))
        })
        .collect()
}

/// Слитные участки расхождения: `(распознанное, введённое)`.
fn runs(reference: &[(String, String)], hypothesis: &[(String, String)]) -> Vec<(String, String)> {
    let left: Vec<&str> = reference.iter().map(|(_, key)| key.as_str()).collect();
    let right: Vec<&str> = hypothesis.iter().map(|(_, key)| key.as_str()).collect();

    let mut out = Vec::new();
    let mut canonical: Vec<&str> = Vec::new();
    let mut surface: Vec<&str> = Vec::new();

    for op in align(&left, &right) {
        match op {
            Op::Match(..) => flush(&mut surface, &mut canonical, &mut out),
            Op::Substitute(reference_at, hypothesis_at) => {
                canonical.push(&reference[reference_at].0);
                surface.push(&hypothesis[hypothesis_at].0);
            }
            Op::Insert(hypothesis_at) => surface.push(&hypothesis[hypothesis_at].0),
            Op::Delete(reference_at) => canonical.push(&reference[reference_at].0),
        }
    }
    flush(&mut surface, &mut canonical, &mut out);
    out
}

/// Закрыть участок расхождения.
///
/// Участок, где пуста одна из сторон, парой не становится: «ничего →
/// слово» и «слово → ничего» в глоссарии не выражаются вовсе, и
/// записать такое значит завести правило, которое не сработает.
fn flush(surface: &mut Vec<&str>, canonical: &mut Vec<&str>, out: &mut Vec<(String, String)>) {
    if !canonical.is_empty() && !surface.is_empty() {
        out.push((surface.join(" "), canonical.join(" ")));
    }
    canonical.clear();
    surface.clear();
}

/// Пары в CSV-контракте глоссария (`surface,canonical,language,scope`).
pub fn pairs_csv(pairs: &[Pair], language: &str) -> String {
    let mut out = String::from("surface,canonical,language,scope\n");
    for pair in pairs {
        out.push_str(&format!(
            "{},{},{language},global\n",
            quote(&pair.surface),
            quote(&pair.canonical)
        ));
    }
    out
}

/// Кавычки по правилам CSV.
///
/// Поле с запятой, оставленное без них, разъезжается по столбцам, и
/// разбор пропустит строку — молча, потому что для него это просто
/// строка с лишними полями.
fn quote(value: &str) -> String {
    if value.contains([',', '"', '\n']) {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value.to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::labels::Phrase;
    use std::collections::BTreeMap;

    const RATE: u32 = 16_000;

    fn phrase(id: &str, start_ms: u64, end_ms: u64, text: &str, state: State) -> Phrase {
        Phrase {
            id: id.into(),
            start_ms,
            end_ms,
            hypotheses: BTreeMap::new(),
            text: text.into(),
            state,
            kinds: Vec::new(),
            speaker: None,
            note: String::new(),
        }
    }

    fn labels(phrases: Vec<Phrase>) -> Labels {
        Labels {
            case: "проба".into(),
            version: 3,
            boundaries: "vad".into(),
            engines: vec!["gigaam".into(), "parakeet".into()],
            updated_ms: 0,
            phrases,
        }
    }

    /// Сколько отсчётов нужно, чтобы покрыть разметку целиком.
    fn samples_for(labels: &Labels) -> usize {
        let end = labels
            .phrases
            .iter()
            .map(|phrase| phrase.end_ms)
            .max()
            .unwrap_or(0);
        (end as usize) * RATE as usize / 1000
    }

    /// Заведомо положительный случай: проверенные фразы доезжают до
    /// манифеста, и длительность в нём — из их же границ, а не вписана
    /// числом.
    #[test]
    fn verified_phrases_reach_the_manifest_with_their_own_duration() {
        let set = labels(vec![
            phrase("0001", 0, 1600, "первая фраза", State::Correct),
            phrase("0002", 2000, 5100, "вторая фраза", State::Corrected),
        ]);
        let report = build(&set, samples_for(&set), RATE).expect("экспорт");

        assert_eq!(report.clips.len(), 2, "{report:?}");
        assert_eq!(report.clips[0].text, "первая фраза");
        assert_eq!(report.clips[1].text, "вторая фраза");
        // Ожидаемое выводится из фикстуры: 1600 мс и 3100 мс. Границы
        // взяты **не** круглыми в двоичном виде намеренно: на 1500 и
        // 3000 мс любая потеря точности совпадает сама собой, и первая
        // версия этого теста прошла при `f32`, печатавшем в манифест
        // `1.600000023841858`.
        assert!((report.clips[0].duration - 1.6).abs() < 1e-9, "{report:?}");
        assert!((report.clips[1].duration - 3.1).abs() < 1e-9, "{report:?}");
        assert_eq!(report.total_ms, 1600 + 3100, "{report:?}");
    }

    /// Непроверенное и отброшенное в датасет не идут, и обе величины
    /// названы вслух: молча потерянная половина разметки выглядит как
    /// маленький датасет, а не как несделанная работа.
    #[test]
    fn unchecked_and_skipped_stay_out_and_are_counted_aloud() {
        let set = labels(vec![
            phrase("0001", 0, 1000, "проверенная", State::Correct),
            phrase("0002", 1000, 2000, "черновик движка", State::Unchecked),
            phrase("0003", 2000, 3000, "неразборчиво", State::Skip),
            phrase("0004", 3000, 4000, "ещё черновик", State::Unchecked),
        ]);
        let report = build(&set, samples_for(&set), RATE).expect("экспорт");

        assert_eq!(report.clips.len(), 1, "{report:?}");
        assert_eq!(report.left_unchecked, 2, "{report:?}");
        assert_eq!(report.left_skipped, 1, "{report:?}");
        // Доля выводится из фикстуры: одна фраза из четырёх.
        assert!((report.share() - 0.25).abs() < 1e-6, "{report:?}");
    }

    /// Заведомо отрицательный случай, ради которого модуль и написан:
    /// разметка без единой проверенной фразы — **отказ**, а не пустой
    /// манифест. Такой манифест учил бы модель на её собственных
    /// ошибках, и на диске он неотличим от законченной работы.
    #[test]
    fn a_labelling_nobody_checked_is_refused_not_exported_empty() {
        let set = labels(vec![
            phrase("0001", 0, 1000, "черновик движка", State::Unchecked),
            phrase("0002", 1000, 2000, "и второй", State::Unchecked),
        ]);
        let error = build(&set, samples_for(&set), RATE).expect_err("обязан отказать");
        assert!(
            error.contains("проверен"),
            "в причине должно быть сказано, чего не хватает: {error}"
        );
        assert!(
            error.contains('2'),
            "в причине должно быть число непроверенных: {error}"
        );
    }

    /// Проверенная фраза без текста — тоже отказ, и по своей причине:
    /// это обучение на тишине под видом речи. Состояние `correct` при
    /// пустом тексте означает промах разметки, а не молчание в записи;
    /// для молчания есть `skip`.
    #[test]
    fn a_verified_phrase_without_text_is_refused_by_name() {
        let set = labels(vec![
            phrase("0001", 0, 1000, "хорошая", State::Correct),
            phrase("0002", 1000, 2000, "   ", State::Corrected),
        ]);
        let error = build(&set, samples_for(&set), RATE).expect_err("обязан отказать");
        assert!(
            error.contains("0002"),
            "отказ обязан называть фразу: {error}"
        );
    }

    /// Фраза, выходящая за конец записи, — отказ, а не обрезка. Обрезать
    /// молча значит подать в дообучение звук, который короче своего
    /// текста, и заметить это можно будет только на слух.
    #[test]
    fn a_phrase_past_the_end_of_the_recording_is_refused_not_trimmed() {
        let set = labels(vec![phrase("0007", 0, 4000, "длинная", State::Correct)]);
        // Записи ровно вдвое меньше, чем требует фраза.
        let error = build(&set, 2000 * RATE as usize / 1000, RATE).expect_err("обязан отказать");
        assert!(
            error.contains("0007"),
            "отказ обязан называть фразу: {error}"
        );
    }

    /// Частота, отличная от живого пути (ADR-005), — отказ с причиной, а
    /// не молчаливый ресемпл.
    #[test]
    fn a_foreign_sample_rate_is_refused_with_a_reason() {
        let set = labels(vec![phrase("0001", 0, 1000, "фраза", State::Correct)]);
        let error = build(&set, 48_000, 48_000).expect_err("обязан отказать");
        assert!(
            error.contains("48000") || error.contains("16000"),
            "{error}"
        );
    }

    /// Пустая разметка — отказ, а не «ноль клипов, всё в порядке».
    #[test]
    fn an_empty_labelling_is_refused_too() {
        let set = labels(Vec::new());
        build(&set, 16_000, RATE).expect_err("обязан отказать");
    }

    // --- пары для глоссария ---

    fn term_phrase(id: &str, text: &str, hypotheses: &[(&str, &str)]) -> Phrase {
        let mut phrase = phrase(id, 0, 2000, text, State::Corrected);
        phrase.kinds = vec![Kind::Term];
        phrase.hypotheses = hypotheses
            .iter()
            .map(|(engine, guess)| ((*engine).to_owned(), (*guess).to_owned()))
            .collect();
        phrase
    }

    /// Заведомо положительный случай: правка термина даёт пару, где
    /// surface — распознанное, canonical — введённое. Оба поля берутся
    /// из действия, а не заполняются руками (та же схема, что в Epic 19).
    #[test]
    fn a_term_correction_yields_the_word_that_changed_not_the_whole_phrase() {
        let set = labels(vec![term_phrase(
            "0007",
            "вынесли это в UniFFI",
            &[("gigaam", "вынесли это в юнифай")],
        )]);
        let pairs = glossary_pairs(&set);

        assert_eq!(pairs.len(), 1, "{pairs:?}");
        assert_eq!(pairs[0].surface, "юнифай", "{pairs:?}");
        assert_eq!(pairs[0].canonical, "UniFFI", "{pairs:?}");
        assert_eq!(pairs[0].engine, "gigaam", "{pairs:?}");
    }

    /// Термин, услышанный двумя словами, склеивается в одну пару: иначе
    /// в глоссарий уехало бы «юни» → «UniFFI» и отдельно потерянное
    /// «фай», то есть правило, которое сработает не там.
    #[test]
    fn a_term_heard_as_two_words_becomes_one_pair() {
        let set = labels(vec![term_phrase(
            "0007",
            "вынесли это в UniFFI",
            &[("whisper", "вынесли это в юни фай")],
        )]);
        let pairs = glossary_pairs(&set);

        assert_eq!(pairs.len(), 1, "{pairs:?}");
        assert_eq!(pairs[0].surface, "юни фай", "{pairs:?}");
        assert_eq!(pairs[0].canonical, "UniFFI", "{pairs:?}");
    }

    /// Заведомо отрицательный случай: правка не про термин пар не даёт.
    /// Иначе в глоссарий уехала бы каждая ошибка слуха, и он перестал бы
    /// быть глоссарием.
    #[test]
    fn corrections_that_are_not_about_terms_yield_nothing() {
        let mut misheard = term_phrase("0001", "у лукоморья дуб", &[("gigaam", "у лукоморья дут")]);
        misheard.kinds = vec![Kind::Misheard];
        let unchecked = {
            let mut phrase =
                term_phrase("0002", "вынесли в UniFFI", &[("gigaam", "вынесли в юни")]);
            phrase.state = State::Unchecked;
            phrase
        };
        let set = labels(vec![misheard, unchecked]);

        assert!(
            glossary_pairs(&set).is_empty(),
            "{:?}",
            glossary_pairs(&set)
        );
    }

    /// Движок, услышавший термин верно, пары не даёт — но его сосед даёт.
    /// Проверяется вместе, потому что порознь это два теста, каждый из
    /// которых проходит на заглушке, возвращающей пустоту.
    #[test]
    fn only_the_engines_that_got_it_wrong_produce_pairs() {
        let set = labels(vec![term_phrase(
            "0007",
            "вынесли это в UniFFI",
            &[
                ("gigaam", "вынесли это в юнифай"),
                ("parakeet", "вынесли это в UniFFI"),
            ],
        )]);
        let pairs = glossary_pairs(&set);

        assert_eq!(pairs.len(), 1, "{pairs:?}");
        assert_eq!(pairs[0].engine, "gigaam", "{pairs:?}");
    }

    /// CSV — тот самый контракт, который читает `glossary::parse_csv`.
    /// Проверяется его же разбором, а не сравнением со строкой: своя
    /// сборка CSV, разошедшаяся с чужим разбором, — это файл, который
    /// импортируется в ноль строк без единой жалобы.
    #[test]
    fn the_csv_we_write_is_the_csv_the_glossary_reads() {
        let pairs = vec![
            Pair {
                surface: "юни фай".into(),
                canonical: "UniFFI".into(),
                engine: "gigaam".into(),
                phrase: "0007".into(),
            },
            Pair {
                // Запятая внутри поля — то место, где наивная сборка
                // ломается молча.
                surface: "рафт, митинг".into(),
                canonical: "MeetingRaft".into(),
                engine: "whisper".into(),
                phrase: "0009".into(),
            },
        ];
        let csv = pairs_csv(&pairs, "ru");
        let (terms, skipped) = glossary::parse_csv(&csv).expect("разбор");

        assert_eq!(skipped, 0, "ни одна строка не должна быть пропущена: {csv}");
        assert_eq!(terms.len(), 2, "{csv}");
        assert_eq!(terms[0].surface, "юни фай", "{csv}");
        assert_eq!(terms[0].canonical, "UniFFI", "{csv}");
        assert_eq!(terms[1].surface, "рафт, митинг", "{csv}");
    }

    /// Манифест — по строке JSON на клип, и читается он обратно как
    /// JSON, а не как текст: формат едят чужие обучалки, и «почти JSON»
    /// они не едят.
    #[test]
    fn the_manifest_is_one_json_object_per_line() {
        let report = Report {
            clips: vec![Clip {
                audio_filepath: "clips/0001.wav".into(),
                text: "первая фраза".into(),
                duration: 1.6,
            }],
            left_unchecked: 0,
            left_skipped: 0,
            total_ms: 1600,
        };
        let text = manifest_text(&report);
        let lines: Vec<&str> = text.lines().collect();

        assert_eq!(lines.len(), 1, "{text}");
        let parsed: serde_json::Value = serde_json::from_str(lines[0]).expect("JSON");
        assert_eq!(parsed["audio_filepath"], "clips/0001.wav", "{text}");
        assert_eq!(parsed["text"], "первая фраза", "{text}");
        assert_eq!(parsed["duration"], 1.6, "{text}");
        // И отдельно — как оно **напечатано**: манифест читает человек,
        // а `1.600000023841858` в нём означает, что где-то по дороге
        // стоит `f32`.
        assert!(lines[0].contains("\"duration\":1.6"), "{text}");
    }

    /// Запись целиком: нарезка кладётся файлами, манифест ссылается на
    /// них, и каждый файл **читается обратно** — своим же разбором, с
    /// длиной из границ фразы и с теми самыми отсчётами, что лежат в
    /// записи на этом месте.
    ///
    /// Сверять длину мало, и это не придирка: клип, вырезанный не с того
    /// места, длину имеет верную. Он попал бы в дообучение звуком одной
    /// фразы под текстом другой, и увидеть это можно было бы только на
    /// слух. Наполнитель поэтому не постоянный — у постоянного любое
    /// смещение совпадает само собой.
    #[test]
    fn written_clips_are_readable_and_cut_from_the_right_place() {
        let set = labels(vec![
            phrase("0001", 0, 1000, "первая", State::Correct),
            phrase("0002", 2000, 3500, "вторая", State::Corrected),
        ]);
        // Наполнитель с простым периодом, и это не украшение. Первым
        // здесь стоял `index % 1000`, отчего отрезки с 0 мс и с 2000 мс
        // совпадали **побайтно**: период наполнителя делил обе границы.
        // Тест на вырезание не с того места проходил при заведомо
        // сломанном вырезании.
        let pcm: Vec<i16> = (0..samples_for(&set))
            .map(|index| (index % 30_011) as i16)
            .collect();
        let out = std::env::temp_dir().join(format!("bench-dataset-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&out);

        let report = write(&set, &pcm, RATE, &out).expect("запись");
        assert_eq!(report.clips.len(), 2, "{report:?}");

        for (clip, phrase) in report.clips.iter().zip(set.verified()) {
            let path = out.join(&clip.audio_filepath);
            let back = crate::wav::read(&path).expect("клип читается обратно");
            assert_eq!(back.sample_rate, RATE, "{clip:?}");

            // Ожидаемое выводится из фикстуры и границ фразы, а не
            // вписано числом.
            let from = phrase.start_ms as usize * RATE as usize / 1000;
            let till = phrase.end_ms as usize * RATE as usize / 1000;
            assert_eq!(back.pcm.len(), till - from, "{clip:?}");
            assert_eq!(
                back.pcm,
                pcm[from..till],
                "клип вырезан не с того места: {clip:?}"
            );
        }

        let manifest = std::fs::read_to_string(out.join(MANIFEST)).expect("манифест");
        assert_eq!(manifest.lines().count(), 2, "{manifest}");
        let _ = std::fs::remove_dir_all(&out);
    }

    /// И заведомо отрицательный случай для записи: отказ обязан
    /// случиться **до** того, как на диск ляжет первый файл. Каталог с
    /// половиной датасета выглядит как датасет.
    #[test]
    fn a_refused_export_leaves_nothing_on_disk() {
        let set = labels(vec![
            phrase("0001", 0, 1000, "хорошая", State::Correct),
            phrase("0002", 1000, 2000, "", State::Correct),
        ]);
        let pcm: Vec<i16> = vec![0; samples_for(&set)];
        let out = std::env::temp_dir().join(format!("bench-dataset-bad-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&out);

        write(&set, &pcm, RATE, &out).expect_err("обязан отказать");
        assert!(
            !out.join("manifest.jsonl").exists(),
            "манифеста быть не должно"
        );
        let clips = out.join("clips");
        let written = std::fs::read_dir(&clips)
            .map(|entries| entries.count())
            .unwrap_or(0);
        assert_eq!(written, 0, "клипов на диске быть не должно");
        let _ = std::fs::remove_dir_all(&out);
    }
}
