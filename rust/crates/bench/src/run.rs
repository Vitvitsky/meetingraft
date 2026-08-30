//! Один прогон: случай × движок × нарезка × смещение.
//!
//! Кладёт `run.json` — машиночитаемый результат — и `segments.txt`,
//! который читают глазами. Второе не украшение: числа говорят, что
//! нарезка изменилась, а увидеть, **как** она изменилась, можно только
//! прочитав реплики.

use std::path::Path;
use std::time::Instant;

use domain::TranscriptSegment;
use serde::{Deserialize, Serialize};

use crate::case::Case;
use crate::engines::Recognize;
use crate::metrics::{SegmentStats, segment_stats};
use crate::segmentation::{self, Piece, Strategy};
use crate::wer::{cer, wer};

/// Результат одного прогона.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Run {
    pub case: String,
    pub engine: String,
    pub segmentation: String,
    pub biasing: String,
    /// Отказ отдельным полем: прогон, упавший в середине, не должен
    /// выглядеть как встреча, на которой движок ничего не услышал.
    pub refused: Option<String>,
    pub segments: Vec<Segment>,
    /// Сколько кусков подали движку и сколько вернулось пустыми.
    ///
    /// Без этой пары «ноль сегментов» сливает две разные вещи: резать
    /// было нечего — и всё, что резали, вернулось пустым. Поймано живым
    /// прогоном: русский движок на английской записи честно промолчал на
    /// каждом куске, а отчёт показал ровно то же, что показал бы при
    /// сломанной нарезке.
    pub pieces_fed: usize,
    pub pieces_empty: usize,
    /// Сколько разных голосов дала **нарезка** — считается по кускам, а
    /// не по сегментам.
    ///
    /// Разница несущая: куски, вернувшиеся пустыми, из сегментов
    /// выпадают, и счёт по сегментам показал бы ноль голосов на записи,
    /// где нарезка нашла двоих. У окон и VAD голосов нет вовсе — `None`,
    /// а не ноль.
    pub speakers_found: Option<usize>,
    pub stats: Option<SegmentStats>,
    /// WER и CER считаются **только** когда есть эталон и назван отрезок,
    /// который он покрывает. Иначе `None` — прочерк в отчёте, а не ноль.
    pub wer: Option<f32>,
    pub cer: Option<f32>,
    pub reference_kind: String,
    /// Откуда взят эталон. Два источника дают **разные числа** на одной
    /// записи, и молча выбранный из них — это два значения под одним
    /// именем.
    #[serde(default = "reference_source_none")]
    pub reference_source: crate::history::ReferenceSource,
    /// Версия разметки, если эталон пришёл из неё.
    #[serde(default)]
    pub labels_version: Option<u32>,
    /// Оценка по фразам — она же источник `wer`/`cer`, когда есть
    /// разметка.
    #[serde(default)]
    pub phrase_score: Option<PhraseScore>,
    /// Что дало смещение и чего оно стоило. `None` — смещения не было
    /// или сравнивать не с чем.
    pub biasing_report: Option<crate::hotwords::BiasingReport>,
    pub split_ms: f32,
    pub model_ms_per_audio_second: f32,
    pub audio_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Segment {
    pub start_ms: u64,
    pub end_ms: u64,
    pub speaker: Option<u32>,
    pub text: String,
}

/// Сколько речи движку подали и что он ответил.
pub struct Plan<'a> {
    pub pieces: Vec<Piece>,
    /// Отрезки речи по независимому источнику — для метрик границ.
    /// Пусто, если речь не размечена.
    pub speech: Vec<(u64, u64)>,
    pub strategy: Strategy,
    pub split_ms: f32,
    pub engine: &'a dyn Recognize,
}

/// Прогнать куски через движок и собрать результат.
pub fn execute(case: &Case, plan: Plan<'_>, biasing: &str) -> Run {
    let mut segments = Vec::new();
    let mut spoken_ms = 0u64;
    let mut fed = 0usize;
    let mut empty = 0usize;
    let started = Instant::now();

    for piece in &plan.pieces {
        let samples = segmentation::samples(&case.mic, case.sample_rate, piece);
        if samples.is_empty() {
            continue;
        }
        spoken_ms += piece.end_ms.saturating_sub(piece.start_ms);
        fed += 1;
        match plan.engine.transcribe(samples, case.sample_rate) {
            Ok(heard) if heard.text.trim().is_empty() => empty += 1,
            Ok(heard) => segments.push(Segment {
                start_ms: piece.start_ms,
                end_ms: piece.end_ms,
                speaker: piece.speaker,
                text: heard.text.trim().to_string(),
            }),
            Err(error) => {
                return refused(
                    case,
                    &plan,
                    biasing,
                    format!("кусок {}: {error}", piece.start_ms),
                );
            }
        }
    }

    let model_ms = started.elapsed().as_secs_f32() * 1000.0;
    let as_transcript: Vec<TranscriptSegment> = segments
        .iter()
        .map(|segment| TranscriptSegment::new(segment.start_ms, segment.end_ms, &segment.text))
        .collect();

    let (wer_rate, cer_rate) = score(case, &segments);

    Run {
        case: case.meta.case.clone(),
        engine: plan.engine.name().to_string(),
        segmentation: plan.strategy.name().to_string(),
        biasing: biasing.to_string(),
        refused: None,
        pieces_fed: fed,
        pieces_empty: empty,
        speakers_found: speakers_in(&plan.pieces),
        stats: Some(segment_stats(
            &as_transcript,
            &plan.speech,
            case.duration_ms(),
        )),
        segments,
        wer: wer_rate,
        cer: cer_rate,
        reference_kind: format!("{:?}", case.meta.reference_kind),
        reference_source: if wer_rate.is_some() {
            crate::history::ReferenceSource::ReferenceText
        } else {
            crate::history::ReferenceSource::None
        },
        labels_version: None,
        phrase_score: None,
        biasing_report: None,
        split_ms: plan.split_ms,
        model_ms_per_audio_second: if spoken_ms == 0 {
            0.0
        } else {
            model_ms / (spoken_ms as f32 / 1000.0)
        },
        audio_ms: case.duration_ms(),
    }
}

/// Прогон потокового движка: границы реплик ставит он сам.
///
/// Отдельная функция, а не ветка в [`execute`], потому что и вход другой
/// — запись целиком вместо списка кусков, — и мерить у неё нечего из
/// того, что меряет нарезку: числа `pieces_fed` здесь не бывает, а
/// «сколько кусков вернулось пустыми» не имеет смысла вовсе.
pub fn execute_stream(
    case: &Case,
    engine: &dyn crate::engines::StreamTranscribe,
    speech: Vec<(u64, u64)>,
    biasing: &str,
) -> Run {
    let started = Instant::now();
    let heard = engine.transcribe_stream(&case.mic, case.sample_rate);
    let model_ms = started.elapsed().as_secs_f32() * 1000.0;

    let transcript = match heard {
        Ok(transcript) => transcript,
        Err(error) => {
            return Run {
                case: case.meta.case.clone(),
                engine: engine.name().to_string(),
                segmentation: Strategy::Native.name().to_string(),
                biasing: biasing.to_string(),
                refused: Some(error),
                segments: Vec::new(),
                pieces_fed: 0,
                pieces_empty: 0,
                speakers_found: None,
                stats: None,
                wer: None,
                cer: None,
                reference_kind: format!("{:?}", case.meta.reference_kind),
                reference_source: crate::history::ReferenceSource::None,
                labels_version: None,
                phrase_score: None,
                biasing_report: None,
                split_ms: 0.0,
                model_ms_per_audio_second: 0.0,
                audio_ms: case.duration_ms(),
            };
        }
    };

    let segments: Vec<Segment> = transcript
        .iter()
        .map(|segment| Segment {
            start_ms: segment.start_ms,
            end_ms: segment.end_ms,
            // Голосов потоковый движок не различает: он слышит звук, а не
            // людей. `None` честнее нуля.
            speaker: None,
            text: segment.text.clone(),
        })
        .collect();

    let (wer_rate, cer_rate) = score(case, &segments);
    let audio_ms = case.duration_ms();

    Run {
        case: case.meta.case.clone(),
        engine: engine.name().to_string(),
        segmentation: Strategy::Native.name().to_string(),
        biasing: biasing.to_string(),
        refused: None,
        // Кусков не было: движку подали запись целиком, и он сам решил,
        // где реплики. Ноль здесь означает «не применимо», и читать его
        // надо вместе с `segmentation = stream`.
        pieces_fed: 0,
        pieces_empty: 0,
        speakers_found: None,
        stats: Some(segment_stats(&transcript, &speech, audio_ms)),
        segments,
        wer: wer_rate,
        cer: cer_rate,
        reference_kind: format!("{:?}", case.meta.reference_kind),
        reference_source: if wer_rate.is_some() {
            crate::history::ReferenceSource::ReferenceText
        } else {
            crate::history::ReferenceSource::None
        },
        labels_version: None,
        phrase_score: None,
        biasing_report: None,
        // Нарезка не стоила ничего отдельно: она внутри модели, и её цена
        // сидит в `model_ms_per_audio_second`.
        split_ms: 0.0,
        model_ms_per_audio_second: if audio_ms == 0 {
            0.0
        } else {
            model_ms / (audio_ms as f32 / 1000.0)
        },
        audio_ms,
    }
}

/// Сколько разных голосов в кусках. `None`, если меток нет вовсе.
fn speakers_in(pieces: &[Piece]) -> Option<usize> {
    let mut labels: Vec<u32> = pieces.iter().filter_map(|piece| piece.speaker).collect();
    if labels.is_empty() {
        return None;
    }
    labels.sort_unstable();
    labels.dedup();
    Some(labels.len())
}

/// Оценка по размеченным фразам.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PhraseScore {
    pub wer: f32,
    pub cer: f32,
    /// Сколько проверенных фраз участвовало.
    pub phrases: usize,
    pub reference_words: usize,
    /// Худшие фразы: id и их WER. По ним видно **какие** места движок
    /// валит, а не только сколько.
    pub worst: Vec<(String, f32)>,
}

/// Сколько худших фраз попадает в отчёт.
const WORST_SHOWN: usize = 5;

/// Посчитать WER по разметке: пофразно, а не по одному отрезку.
///
/// Идут **только проверенные** фразы. Непроверенная содержит текст
/// движка, и сравнить движок с ней значит сравнить его с самим собой.
///
/// Гипотеза для фразы — текст сегментов, пересекающих её время. Границы
/// у движка и у разметки свои, и требовать совпадения нельзя: это
/// наказывало бы за нарезку в метрике, которая про слова.
pub fn score_by_phrases(
    labels: &crate::labels::Labels,
    segments: &[Segment],
) -> Option<PhraseScore> {
    let verified: Vec<&crate::labels::Phrase> = labels.verified().collect();
    if verified.is_empty() {
        // Ноль проверенных фраз — не «идеальный WER», а «мерить нечем».
        return None;
    }

    let mut errors = 0.0f32;
    let mut words = 0.0f32;
    let mut characters = 0.0f32;
    let mut character_errors = 0.0f32;
    let mut worst: Vec<(String, f32)> = Vec::new();
    let mut scored = 0usize;

    for phrase in verified {
        let heard: String = segments
            .iter()
            .filter(|segment| segment.start_ms < phrase.end_ms && segment.end_ms > phrase.start_ms)
            .map(|segment| segment.text.as_str())
            .collect::<Vec<_>>()
            .join(" ");
        let report = wer(&phrase.text, &heard);
        let phrase_words = crate::wer::normalize(&phrase.text).len() as f32;
        if phrase_words == 0.0 {
            continue;
        }
        let rate = report.rate();
        errors += rate * phrase_words;
        words += phrase_words;

        let phrase_characters = crate::wer::normalize(&phrase.text)
            .join(" ")
            .chars()
            .count() as f32;
        character_errors += cer(&phrase.text, &heard) * phrase_characters;
        characters += phrase_characters;

        worst.push((phrase.id.clone(), rate));
        scored += 1;
    }

    if words == 0.0 {
        return None;
    }
    worst.sort_by(|left, right| right.1.total_cmp(&left.1));
    worst.truncate(WORST_SHOWN);

    Some(PhraseScore {
        wer: errors / words,
        cer: if characters == 0.0 {
            0.0
        } else {
            character_errors / characters
        },
        phrases: scored,
        reference_words: words as usize,
        worst,
    })
}

/// Сравнить с эталоном — если он есть и если сказано, какой отрезок он
/// покрывает.
///
/// Без границ отрезка сравнивать нельзя вовсе: эталон на три минуты
/// против расшифровки всей встречи дал бы гору «лишних» слов и WER,
/// который читается как провал движка. Молчаливое сравнение здесь хуже
/// прочерка.
fn score(case: &Case, segments: &[Segment]) -> (Option<f32>, Option<f32>) {
    let (Some(reference), Some([from, to])) =
        (case.reference.as_ref(), case.meta.reference_covers_ms)
    else {
        return (None, None);
    };
    let heard: String = segments
        .iter()
        // Сегмент считается попавшим в отрезок, если пересекается с ним:
        // граница почти никогда не совпадает со словом.
        .filter(|segment| segment.start_ms < to && segment.end_ms > from)
        .map(|segment| segment.text.as_str())
        .collect::<Vec<_>>()
        .join(" ");
    (
        Some(wer(reference, &heard).rate()),
        Some(cer(reference, &heard)),
    )
}

fn refused(case: &Case, plan: &Plan<'_>, biasing: &str, reason: String) -> Run {
    Run {
        case: case.meta.case.clone(),
        engine: plan.engine.name().to_string(),
        segmentation: plan.strategy.name().to_string(),
        biasing: biasing.to_string(),
        refused: Some(reason),
        segments: Vec::new(),
        pieces_fed: 0,
        pieces_empty: 0,
        speakers_found: None,
        stats: None,
        wer: None,
        cer: None,
        reference_kind: format!("{:?}", case.meta.reference_kind),
        reference_source: crate::history::ReferenceSource::None,
        labels_version: None,
        phrase_score: None,
        biasing_report: None,
        split_ms: plan.split_ms,
        model_ms_per_audio_second: 0.0,
        audio_ms: case.duration_ms(),
    }
}

/// Досчитать, что дало смещение под глоссарий.
///
/// Отдельным шагом, а не внутри прогона, по одной причине: считать это
/// можно только имея эталон, а прогон бывает и без него. Смешав, мы
/// получили бы `caught = 0` там, где сравнивать было не с чем, — то есть
/// «смещение не сработало» вместо «не мерялось».
pub fn add_biasing_report(run: &mut Run, case: &Case, terms: &[String]) {
    if terms.is_empty() {
        return;
    }
    let (Some(reference), Some([from, to])) =
        (case.reference.as_ref(), case.meta.reference_covers_ms)
    else {
        return;
    };
    let heard: String = run
        .segments
        .iter()
        .filter(|segment| segment.start_ms < to && segment.end_ms > from)
        .map(|segment| segment.text.as_str())
        .collect::<Vec<_>>()
        .join(" ");
    run.biasing_report = Some(crate::hotwords::measure(terms, reference, &heard));
}

/// Прочитать прогон обратно — им живёт подкоманда `judge`.
pub fn load(dir: &Path) -> Result<Run, String> {
    let path = dir.join("run.json");
    let text =
        std::fs::read_to_string(&path).map_err(|error| format!("{}: {error}", path.display()))?;
    serde_json::from_str(&text).map_err(|error| format!("{}: {error}", path.display()))
}

fn reference_source_none() -> crate::history::ReferenceSource {
    crate::history::ReferenceSource::None
}

/// Пересчитать оценку по разметке, если она есть.
///
/// Отдельным шагом после прогона: разметка живёт рядом со случаем, а не
/// внутри него, и читать её при каждом прогоне движка незачем.
///
/// Приоритет у разметки: она точнее — проверенные фразы разбросаны по
/// встрече, а `reference_covers_ms` покрывает один отрезок. Замена
/// источника **печатается**, а не происходит молча.
pub fn apply_labels(run: &mut Run, labels: &crate::labels::Labels) {
    match score_by_phrases(labels, &run.segments) {
        Some(score) => {
            run.wer = Some(score.wer);
            run.cer = Some(score.cer);
            run.reference_source = crate::history::ReferenceSource::Labels;
            run.labels_version = Some(labels.version);
            run.phrase_score = Some(score);
        }
        None => {
            // Разметка есть, но проверенных фраз в ней нет. Это не повод
            // молча вернуться к `reference.txt`: человек думает, что
            // мерит по разметке.
            run.reference_source = crate::history::ReferenceSource::None;
            run.labels_version = Some(labels.version);
            run.wer = None;
            run.cer = None;
            run.phrase_score = None;
        }
    }
}

/// Записать результат прогона на диск.
pub fn save(run: &Run, out_dir: &Path) -> Result<(), String> {
    std::fs::create_dir_all(out_dir).map_err(|error| format!("{}: {error}", out_dir.display()))?;
    let json = serde_json::to_string_pretty(run).map_err(|error| error.to_string())?;
    std::fs::write(out_dir.join("run.json"), json).map_err(|error| format!("run.json: {error}"))?;

    let mut text = String::new();
    for segment in &run.segments {
        let speaker = match segment.speaker {
            Some(cluster) => format!("голос {cluster}"),
            None => "—".to_string(),
        };
        text.push_str(&format!(
            "[{:>7} .. {:>7}] {speaker}: {}\n",
            segment.start_ms, segment.end_ms, segment.text
        ));
    }
    std::fs::write(out_dir.join("segments.txt"), text)
        .map_err(|error| format!("segments.txt: {error}"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::case::{Meta, ReferenceKind};

    struct Parrot;

    impl Recognize for Parrot {
        fn transcribe(
            &self,
            pcm: &[i16],
            _sample_rate: u32,
        ) -> Result<crate::engines::Heard, String> {
            Ok(crate::engines::Heard {
                text: format!("кусок из {} отсчётов", pcm.len()),
                word_end_ms: Vec::new(),
            })
        }

        fn name(&self) -> &'static str {
            "parrot"
        }

        fn model_id(&self) -> String {
            "тестовый".to_string()
        }
    }

    struct Broken;

    impl Recognize for Broken {
        fn transcribe(
            &self,
            _pcm: &[i16],
            _sample_rate: u32,
        ) -> Result<crate::engines::Heard, String> {
            Err("движок отказал".to_string())
        }

        fn name(&self) -> &'static str {
            "broken"
        }

        fn model_id(&self) -> String {
            "тестовый".to_string()
        }
    }

    fn case(reference: Option<&str>, covers: Option<[u64; 2]>) -> Case {
        Case {
            dir: std::env::temp_dir(),
            meta: Meta {
                case: "test".to_string(),
                language: "ru".to_string(),
                speakers_expected: 0,
                source: "test".to_string(),
                channel_clock_unified: true,
                reference_kind: if reference.is_some() {
                    ReferenceKind::Typed
                } else {
                    ReferenceKind::None
                },
                reference_covers_ms: covers,
                notes: String::new(),
            },
            sample_rate: 16_000,
            mic: vec![1i16; 16_000 * 4],
            system: None,
            reference: reference.map(str::to_string),
        }
    }

    fn plan<'a>(engine: &'a dyn Recognize, pieces: Vec<Piece>) -> Plan<'a> {
        Plan {
            pieces,
            speech: Vec::new(),
            strategy: Strategy::Windows30,
            split_ms: 0.0,
            engine,
        }
    }

    /// Пустые ответы движка считаются отдельно от отсутствия кусков.
    ///
    /// Поймано живым прогоном: русский движок на английской записи
    /// промолчал на каждом куске, и отчёт выглядел как при сломанной
    /// нарезке.
    #[test]
    fn silent_answers_are_counted_apart_from_having_nothing_to_cut() {
        struct Mute;
        impl Recognize for Mute {
            fn transcribe(
                &self,
                _pcm: &[i16],
                _sample_rate: u32,
            ) -> Result<crate::engines::Heard, String> {
                Ok(crate::engines::Heard::default())
            }

            fn name(&self) -> &'static str {
                "mute"
            }

            fn model_id(&self) -> String {
                "тестовый".to_string()
            }
        }

        let case = case(None, None);
        let pieces = segmentation::split_windows(4000, 1000);
        let run = execute(&case, plan(&Mute, pieces), "none");
        assert!(run.segments.is_empty());
        assert_eq!(run.pieces_fed, 4, "куски подавались: {run:?}");
        assert_eq!(run.pieces_empty, 4, "и все вернулись пустыми");

        // А когда резать нечего, подано ноль — и это другая картина.
        let nothing = execute(&case, plan(&Mute, Vec::new()), "none");
        assert_eq!(nothing.pieces_fed, 0);
    }

    /// Голоса считаются по кускам, а не по сегментам.
    ///
    /// Молчаливый движок выбрасывает куски из сегментов, и счёт по
    /// сегментам показал бы ноль голосов на записи, где нарезка нашла
    /// двоих, — ровно то, что случилось на английской контрольной записи.
    #[test]
    fn speakers_are_counted_from_the_cut_not_from_the_text() {
        struct Mute;
        impl Recognize for Mute {
            fn transcribe(
                &self,
                _pcm: &[i16],
                _sample_rate: u32,
            ) -> Result<crate::engines::Heard, String> {
                Ok(crate::engines::Heard::default())
            }

            fn name(&self) -> &'static str {
                "mute"
            }

            fn model_id(&self) -> String {
                "тестовый".to_string()
            }
        }

        let case = case(None, None);
        let pieces = segmentation::from_turns(&[(0, 2000, 0), (2000, 4000, 1)]);
        let run = execute(&case, plan(&Mute, pieces), "none");
        assert!(run.segments.is_empty(), "движок промолчал");
        assert_eq!(run.speakers_found, Some(2), "а голосов нарезка нашла двое");
    }

    /// У нарезки без меток голосов нет вовсе — это `None`, а не ноль.
    #[test]
    fn a_cut_without_labels_claims_no_speakers() {
        let case = case(None, None);
        let pieces = segmentation::split_windows(4000, 30_000);
        let run = execute(&case, plan(&Parrot, pieces), "none");
        assert_eq!(run.speakers_found, None);
    }

    /// Непроверенные фразы в счёт не идут.
    ///
    /// Тот случай, ради которого заведено состояние: в непроверенной
    /// фразе лежит текст движка, и сравнить движок с ней значит сравнить
    /// его с самим собой — WER вышел бы нулевым у кого угодно.
    #[test]
    fn unchecked_phrases_are_not_scored() {
        use crate::labels::{Labels, Phrase, State};

        let phrase = |id: &str, start, end, text: &str, state| Phrase {
            id: id.to_string(),
            start_ms: start,
            end_ms: end,
            hypotheses: Default::default(),
            text: text.to_string(),
            state,
            kinds: Vec::new(),
            speaker: None,
            note: String::new(),
        };
        let labels = Labels {
            case: "test".to_string(),
            version: 2,
            boundaries: "vad".to_string(),
            engines: vec!["a".to_string()],
            updated_ms: 0,
            phrases: vec![
                phrase("1", 0, 1000, "привет всем", State::Correct),
                // Эта не проверена, и текст в ней движковый: попади она в
                // счёт, WER бы упал.
                phrase("2", 1000, 2000, "совершенно другое", State::Unchecked),
            ],
        };
        let segments = vec![
            Segment {
                start_ms: 0,
                end_ms: 1000,
                speaker: None,
                text: "привет всем".to_string(),
            },
            Segment {
                start_ms: 1000,
                end_ms: 2000,
                speaker: None,
                text: "совершенно другое".to_string(),
            },
        ];

        let score = score_by_phrases(&labels, &segments).expect("есть что мерить");
        assert_eq!(score.phrases, 1, "считается только проверенная");
        assert_eq!(score.wer, 0.0);
        assert_eq!(score.reference_words, 2);
    }

    /// Ноль проверенных фраз — отказ, а не идеальный ноль.
    #[test]
    fn nothing_verified_means_nothing_measured() {
        use crate::labels::{Labels, Phrase, State};
        let labels = Labels {
            case: "test".to_string(),
            version: 1,
            boundaries: "vad".to_string(),
            engines: Vec::new(),
            updated_ms: 0,
            phrases: vec![Phrase {
                id: "1".to_string(),
                start_ms: 0,
                end_ms: 1000,
                hypotheses: Default::default(),
                text: "что-то".to_string(),
                state: State::Unchecked,
                kinds: Vec::new(),
                speaker: None,
                note: String::new(),
            }],
        };
        assert!(score_by_phrases(&labels, &[]).is_none());
    }

    /// Ошибка в одной фразе видна в общем числе и в списке худших.
    ///
    /// Заведомо отрицательный случай к первому тесту: считалка, всегда
    /// отдающая ноль, проходит его и валится здесь.
    #[test]
    fn a_wrong_phrase_shows_up_in_the_total_and_by_name() {
        use crate::labels::{Labels, Phrase, State};
        let phrase = |id: &str, start, end, text: &str| Phrase {
            id: id.to_string(),
            start_ms: start,
            end_ms: end,
            hypotheses: Default::default(),
            text: text.to_string(),
            state: State::Correct,
            kinds: Vec::new(),
            speaker: None,
            note: String::new(),
        };
        let labels = Labels {
            case: "test".to_string(),
            version: 1,
            boundaries: "vad".to_string(),
            engines: Vec::new(),
            updated_ms: 0,
            phrases: vec![
                phrase("1", 0, 1000, "привет всем"),
                phrase("2", 1000, 2000, "добрый день"),
            ],
        };
        let segments = vec![
            Segment {
                start_ms: 0,
                end_ms: 1000,
                speaker: None,
                text: "привет всем".to_string(),
            },
            Segment {
                start_ms: 1000,
                end_ms: 2000,
                speaker: None,
                text: "совсем другое".to_string(),
            },
        ];

        let score = score_by_phrases(&labels, &segments).expect("есть что мерить");
        assert_eq!(score.phrases, 2);
        // Половина слов эталона не угадана.
        assert!((score.wer - 0.5).abs() < 1e-6, "{score:?}");
        assert_eq!(score.worst.first().map(|(id, _)| id.as_str()), Some("2"));
    }

    /// Гипотеза берётся по пересечению времени, а не по совпадению
    /// границ: у движка нарезка своя, и требовать совпадения значило бы
    /// наказывать за неё в метрике про слова.
    #[test]
    fn a_hypothesis_is_gathered_by_overlap_not_by_exact_bounds() {
        use crate::labels::{Labels, Phrase, State};
        let labels = Labels {
            case: "test".to_string(),
            version: 1,
            boundaries: "vad".to_string(),
            engines: Vec::new(),
            updated_ms: 0,
            phrases: vec![Phrase {
                id: "1".to_string(),
                start_ms: 1000,
                end_ms: 2000,
                hypotheses: Default::default(),
                text: "привет всем".to_string(),
                state: State::Correct,
                kinds: Vec::new(),
                speaker: None,
                note: String::new(),
            }],
        };
        // Сегмент движка шире фразы и начинается раньше.
        let segments = vec![Segment {
            start_ms: 500,
            end_ms: 2500,
            speaker: None,
            text: "привет всем".to_string(),
        }];
        let score = score_by_phrases(&labels, &segments).expect("есть что мерить");
        assert_eq!(score.wer, 0.0, "перекрывающий сегмент обязан найтись");
    }

    /// Отказ движка — отказ прогона, а не пустая расшифровка.
    #[test]
    fn an_engine_failure_is_a_refusal_not_an_empty_transcript() {
        let case = case(None, None);
        let pieces = segmentation::split_windows(4000, 30_000);
        let run = execute(&case, plan(&Broken, pieces), "none");
        assert!(run.refused.is_some(), "{run:?}");
        assert!(run.segments.is_empty());
        assert!(run.stats.is_none(), "у отказа не бывает метрик нарезки");
    }

    /// Без эталона WER не считается вовсе — это прочерк, а не ноль.
    #[test]
    fn without_a_reference_there_is_no_score() {
        let case = case(None, None);
        let pieces = segmentation::split_windows(4000, 30_000);
        let run = execute(&case, plan(&Parrot, pieces), "none");
        assert!(run.refused.is_none());
        assert_eq!(run.wer, None);
        assert_eq!(run.cer, None);
    }

    /// И с эталоном, но без названного отрезка — тоже.
    ///
    /// Эталон на три минуты против расшифровки всей встречи дал бы WER,
    /// читающийся как провал движка.
    #[test]
    fn a_reference_without_bounds_is_not_scored_either() {
        let case = case(Some("кусок из 64000 отсчётов"), None);
        let pieces = segmentation::split_windows(4000, 30_000);
        let run = execute(&case, plan(&Parrot, pieces), "none");
        assert_eq!(run.wer, None, "отрезок не назван — сравнивать нечего");
    }

    /// А когда есть и то и другое — считается, и совпадение даёт ноль.
    #[test]
    fn a_reference_with_bounds_is_scored() {
        let case = case(Some("кусок из 64000 отсчётов"), Some([0, 4000]));
        let pieces = segmentation::split_windows(4000, 30_000);
        let run = execute(&case, plan(&Parrot, pieces), "none");
        assert_eq!(run.wer, Some(0.0), "{run:?}");
    }

    /// Заведомо отрицательный случай к предыдущему: считалка, всегда
    /// отдающая ноль, проходит тот тест и валится здесь.
    #[test]
    fn a_wrong_transcript_scores_badly() {
        let case = case(Some("совершенно другие слова"), Some([0, 4000]));
        let pieces = segmentation::split_windows(4000, 30_000);
        let run = execute(&case, plan(&Parrot, pieces), "none");
        assert!(run.wer.unwrap_or(0.0) >= 1.0, "{:?}", run.wer);
    }
}
