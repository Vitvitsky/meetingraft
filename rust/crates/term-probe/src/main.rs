//! Прибор для кандидатов в глоссарий (Phase 13).
//!
//! Отвечает на вопрос, от которого зависит, строить ли очередь одобрения
//! вообще: **сколько кандидатов даёт каждое правило и сколько из них
//! мусор**. Если из полусотни сорок пять — шум, очередь станет работой
//! для человека вместо помощи, и делать её не надо.
//!
//! Долю мусора прибор не знает и знать не может: он не различает термин
//! и случайное слово, ради того человек и нужен. Прибор даёт список с
//! доказательствами; отметить, сколько строк настоящие, — работа глазами.
//!
//! Каждый запуск начинается с заведомо положительного и заведомо
//! отрицательного случая. Правило писано кровью: `count-audio-taps.swift`
//! показал ноль tap'ов при заведомо идущей записи, ноль прочли как
//! «утечки нет», а скрипт был слеп (`CLAUDE.md`). Ноль кандидатов от
//! слепого прибора выглядит ровно так же, как встреча без терминов.
//!
//! Три исхода различаются и не сливаются в один: **прибор слеп**,
//! **сравнивать нечего** (Final ни у одной встречи нет), **смотрели и
//! нашли столько-то**.

use std::collections::HashSet;
use std::path::Path;
use std::process::ExitCode;

use domain::{CandidateRule, TermCandidate, utc_date_label};
use glossary::{MineInput, mine_candidates};
use storage::AudioManifestStore;

/// Сколько раз слово должно прозвучать для правила `Repeated`.
const MIN_OCCURRENCES: u32 = 3;
/// Частотная верхушка встречи, не рассматриваемая правилом `Repeated`.
///
/// Величина не измерена — её и предстоит выбрать по выводу прибора,
/// поэтому она задаётся ключом `--head`, а это лишь отправная точка.
const DEFAULT_FREQUENT_HEAD: usize = 40;

const USAGE: &str = "\
Прибор для кандидатов в термины глоссария.

    term-probe <путь-к-данным> [id-встречи] [--head N]

Без id считает по всем встречам, у которых собран Final.
`--head` — размер частотной верхушки, не рассматриваемой правилом
Repeated. Значение не измерено: его и подбирают по этому выводу.";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();

    // Сперва прибор, потом данные: обратный порядок позволил бы
    // прочитать чистый ноль там, где отбор просто ничего не считает.
    if !self_check() {
        eprintln!("\nПрибор слеп: до настоящих данных дело не дошло.");
        return ExitCode::FAILURE;
    }

    let mut head = DEFAULT_FREQUENT_HEAD;
    let mut positional: Vec<String> = Vec::new();
    let mut index = 0;
    while index < args.len() {
        if args[index] == "--head" {
            let Some(value) = args.get(index + 1).and_then(|v| v.parse::<usize>().ok()) else {
                eprintln!("--head без числа");
                return ExitCode::FAILURE;
            };
            head = value;
            index += 2;
            continue;
        }
        positional.push(args[index].clone());
        index += 1;
    }

    match positional.as_slice() {
        [] => {
            println!("\n{USAGE}");
            ExitCode::SUCCESS
        }
        [root] => run(Path::new(root), None, head),
        [root, meeting] => run(Path::new(root), Some(meeting.as_str()), head),
        _ => {
            eprintln!("{USAGE}");
            ExitCode::FAILURE
        }
    }
}

/// Проверка прибора на синтетике, до всякой базы.
fn self_check() -> bool {
    println!("=== Самопроверка ===");

    // Заведомо положительный: по подлогу на каждое правило. Наполнитель
    // нужен, чтобы у текста была частотная структура настоящей речи —
    // служебные слова десятками, термин единицами. Без него термин сам
    // оказывается самым частым словом и попадает в верхушку.
    let filler = "чтобы этого было тогда потому что этого хотелось";
    let mut replicas: Vec<(u64, &str)> = (0..20).map(|i| (i * 1_000, filler)).collect();
    replicas.push((100_000, "давай посмотрим UniFFI на неделе"));
    replicas.push((101_000, "оплата пойдёт через СБП"));
    replicas.push((102_000, "прескоринг заявки не проходит"));
    replicas.push((103_000, "прескоринг снова упал"));
    replicas.push((104_000, "и прескоринг опять"));

    let filler_vocabulary = filler
        .split_whitespace()
        .map(str::to_lowercase)
        .collect::<HashSet<_>>()
        .len();

    let found = mine_candidates(MineInput {
        replicas: &replicas,
        known: &[],
        dismissed: &[],
        min_occurrences: MIN_OCCURRENCES,
        frequent_head: filler_vocabulary,
    });

    let has = |rule: CandidateRule| found.iter().any(|c| c.rule == rule);
    let (latin, acronym, repeated) = (
        has(CandidateRule::Latin),
        has(CandidateRule::Acronym),
        has(CandidateRule::Repeated),
    );
    println!("  положительный контроль: Latin {latin}, Acronym {acronym}, Repeated {repeated}");
    if !(latin && acronym && repeated) {
        println!("\n  Подложенный термин не найден. Числа ниже были бы числами прибора.");
        return false;
    }

    // Заведомо отрицательный: тот же наполнитель без подлогов. Его слова
    // длиннее четырёх букв и повторены двадцать раз, то есть проходят и
    // порог длины, и порог повторов: отсечь их может только верхушка.
    let noise: Vec<(u64, &str)> = (0..20).map(|i| (i * 1_000, filler)).collect();
    let junk = mine_candidates(MineInput {
        replicas: &noise,
        known: &[],
        dismissed: &[],
        min_occurrences: MIN_OCCURRENCES,
        frequent_head: filler_vocabulary,
    });
    println!(
        "  отрицательный контроль: кандидатов из служебных слов {}",
        junk.len()
    );
    if !junk.is_empty() {
        println!("\n  Отбор принимает служебные слова. Прибору верить нельзя.");
        return false;
    }

    println!("  прибор годен\n");
    true
}

/// Что прибор насчитал по одной встрече.
struct MeetingReport {
    meeting_id: String,
    started_at_ms: u64,
    version: u32,
    replicas: usize,
    candidates: Vec<TermCandidate>,
}

/// Что вышло по всей базе.
struct Report {
    known_terms: usize,
    dismissed: usize,
    meetings_total: usize,
    without_final: usize,
    per_meeting: Vec<MeetingReport>,
}

impl Report {
    fn totals(&self) -> [usize; 3] {
        let mut totals = [0usize; 3];
        for meeting in &self.per_meeting {
            for candidate in &meeting.candidates {
                totals[rule_index(candidate.rule)] += 1;
            }
        }
        totals
    }
}

/// Собрать числа, ничего не печатая.
///
/// Отделено от печати ради теста: путь чтения из базы иначе не проверить
/// ничем, а самопроверка выше говорит только про отбор. Прибор, у
/// которого проверена половина, — прибор непроверенный.
fn collect(
    store: &AudioManifestStore,
    meeting_filter: Option<&str>,
    head: usize,
) -> Result<Report, String> {
    let known = store
        .list_glossary_terms()
        .map_err(|error| format!("глоссарий не прочитался: {error}"))?;
    let dismissed = store
        .list_dismissed_candidates()
        .map_err(|error| format!("список отклонённых не прочитался: {error}"))?;
    let meetings = store
        .list_meeting_summaries()
        .map_err(|error| format!("встречи не прочитались: {error}"))?;

    let mut report = Report {
        known_terms: known.len(),
        dismissed: dismissed.len(),
        meetings_total: meetings.len(),
        without_final: 0,
        per_meeting: Vec::new(),
    };

    for meeting in &meetings {
        if let Some(filter) = meeting_filter
            && meeting.id != filter
        {
            continue;
        }

        let Some(final_transcript) = store
            .get_final_transcript(&meeting.id)
            .map_err(|error| format!("{} — Final не прочитался: {error}", meeting.id))?
        else {
            report.without_final += 1;
            continue;
        };
        let segments = store
            .list_final_segments(&meeting.id, final_transcript.version)
            .map_err(|error| format!("{} — сегменты не прочитались: {error}", meeting.id))?;
        if segments.is_empty() {
            report.without_final += 1;
            continue;
        }

        let replicas: Vec<(u64, &str)> = segments
            .iter()
            .map(|segment| (segment.start_ms, segment.text.as_str()))
            .collect();
        report.per_meeting.push(MeetingReport {
            meeting_id: meeting.id.clone(),
            started_at_ms: meeting.started_at_ms,
            version: final_transcript.version,
            replicas: segments.len(),
            candidates: mine_candidates(MineInput {
                replicas: &replicas,
                known: &known,
                dismissed: &dismissed,
                min_occurrences: MIN_OCCURRENCES,
                frequent_head: head,
            }),
        });
    }

    Ok(report)
}

fn run(root: &Path, meeting_filter: Option<&str>, head: usize) -> ExitCode {
    let store = match AudioManifestStore::open(root) {
        Ok(store) => store,
        Err(error) => {
            eprintln!("База не открылась: {error}");
            return ExitCode::FAILURE;
        }
    };

    let report = match collect(&store, meeting_filter, head) {
        Ok(report) => report,
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::FAILURE;
        }
    };

    println!(
        "Глоссарий: {} терминов · отклонено: {} · верхушка: {head}",
        report.known_terms, report.dismissed
    );

    if report.per_meeting.is_empty() {
        println!(
            "\nСравнивать нечего: встреч с собранным Final не найдено{}.",
            meeting_filter
                .map(|f| format!(" (фильтр {f})"))
                .unwrap_or_default()
        );
        println!(
            "Встреч всего {}, из них без Final {}.",
            report.meetings_total, report.without_final
        );
        println!("Это не «кандидатов нет» — это отсутствие материала.");
        return ExitCode::FAILURE;
    }

    let mut unique: HashSet<String> = HashSet::new();
    for meeting in &report.per_meeting {
        println!(
            "\n=== {} · {} · реплик {} · Final v{} ===",
            utc_date_label(meeting.started_at_ms),
            meeting.meeting_id,
            meeting.replicas,
            meeting.version
        );
        for rule in [
            CandidateRule::Latin,
            CandidateRule::Acronym,
            CandidateRule::Repeated,
        ] {
            let group: Vec<&TermCandidate> = meeting
                .candidates
                .iter()
                .filter(|c| c.rule == rule)
                .collect();
            println!("  {rule:?}: {}", group.len());
            for candidate in group {
                unique.insert(candidate.surface.to_lowercase());
                let example = candidate
                    .examples
                    .first()
                    .map(|e| e.text.as_str())
                    .unwrap_or("—");
                println!(
                    "    {:>3}×  {:<24} «{}»",
                    candidate.occurrences,
                    candidate.surface,
                    shorten(example, 60)
                );
            }
        }
    }

    let totals = report.totals();
    println!("\n=== Итог по {} встречам ===", report.per_meeting.len());
    println!(
        "  Latin {} · Acronym {} · Repeated {} · всего строк {}",
        totals[0],
        totals[1],
        totals[2],
        totals[0] + totals[1] + totals[2]
    );
    println!("  разных слов среди них: {}", unique.len());
    if report.without_final > 0 {
        println!("  пропущено встреч без Final: {}", report.without_final);
    }
    println!(
        "\nДальше глазами, и без этого числа выше ничего не значат:\n\
         пройти по списку и отметить, сколько строк — настоящие термины.\n\
         Долю мусора прибор не знает, он не отличает термин от случайного\n\
         слова. Правило, у которого доля мусора велика, выбрасывается;\n\
         очередь одобрения строится только если числа её оправдали."
    );
    ExitCode::SUCCESS
}

fn rule_index(rule: CandidateRule) -> usize {
    match rule {
        CandidateRule::Latin => 0,
        CandidateRule::Acronym => 1,
        CandidateRule::Repeated => 2,
    }
}

fn shorten(text: &str, limit: usize) -> String {
    if text.chars().count() <= limit {
        return text.to_string();
    }
    text.chars().take(limit).collect::<String>() + "…"
}

#[cfg(test)]
mod tests {
    use domain::{AudioChannel, FinalSegment, FinalTranscript, SpeakerSource};

    use super::*;

    /// Путь чтения из базы: самопроверка выше говорит только про отбор,
    /// а сегменты Final прибор берёт тремя вызовами хранилища, и любой
    /// из них мог бы молча отдать пустоту.
    ///
    /// Тест поэтому заводит настоящую базу и кладёт в неё заведомо
    /// известный термин: если он не нашёлся — читается не то.
    #[test]
    fn a_planted_term_survives_the_round_trip_through_the_database() {
        let root = std::env::temp_dir().join(format!(
            "term-probe-test-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let mut store = AudioManifestStore::open(&root).expect("база");
        store
            .begin_session("m-1", 1_700_000_000_000, "тест")
            .expect("сессия");
        store
            .upsert_final_transcript(&FinalTranscript {
                meeting_id: "m-1".into(),
                version: 1,
                body_markdown: String::new(),
                created_at_ms: 1_700_000_000_000,
            })
            .expect("Final");
        let segments = vec![segment(0, "давай посмотрим UniFFI на неделе")];
        store
            .replace_final_segments("m-1", 1, &segments)
            .expect("сегменты");

        let report = collect(&store, None, 40).expect("сбор");

        assert_eq!(
            report.per_meeting.len(),
            1,
            "встреча с Final не дошла до отбора"
        );
        assert_eq!(report.per_meeting[0].replicas, 1);
        assert_eq!(
            report.per_meeting[0]
                .candidates
                .iter()
                .map(|c| c.surface.as_str())
                .collect::<Vec<_>>(),
            vec!["UniFFI"]
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Заведомо отрицательный случай к тому же пути: встреча без Final
    /// не попадает в отбор и считается отдельно. Слить её с «кандидатов
    /// нет» значило бы повторить дефект `EchoReport::empty()`.
    #[test]
    fn a_meeting_without_a_final_is_counted_apart() {
        let root = std::env::temp_dir().join(format!(
            "term-probe-test-nofinal-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let mut store = AudioManifestStore::open(&root).expect("база");
        store
            .begin_session("m-2", 1_700_000_000_000, "без финала")
            .expect("сессия");

        let report = collect(&store, None, 40).expect("сбор");

        assert!(report.per_meeting.is_empty());
        assert_eq!(report.meetings_total, 1);
        assert_eq!(report.without_final, 1);
        let _ = std::fs::remove_dir_all(&root);
    }

    fn segment(index: u32, text: &str) -> FinalSegment {
        FinalSegment {
            index,
            start_ms: u64::from(index) * 1_000,
            end_ms: u64::from(index) * 1_000 + 900,
            channel: AudioChannel::Mic,
            speaker_id: String::new(),
            speaker_source: SpeakerSource::None,
            text: text.to_string(),
            text_edited: false,
            original_text: text.to_string(),
        }
    }
}
