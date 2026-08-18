//! Прибор: сколько в записи удвоенных реплик (Epic 8, задача 1).
//!
//! Микрофон слышит созвон (ADR-014), и реплика удалённого участника
//! распознаётся дважды. Известно про это пока одно число — дословных
//! повторов 12–15%, — а дословное сравнение как раз и меряет не то:
//! глухую копию Whisper пишет **другими словами**. Сколько дублей на
//! самом деле, неизвестно, и от этого зависит, стоит ли овчинка
//! выделки.
//!
//! Прибор отвечает на этот вопрос и **не назначает порог**. Он печатает
//! распределение похожести и цену каждого порога в ложных срабатываниях;
//! порог из этих чисел берёт человек.
//!
//! Четвёртый рядом с `echo-probe`, `gate-probe` и `diarize-probe`, с той
//! же дисциплиной: сперва случай с известным ответом, потом настоящие
//! данные. Правило писано кровью — `count-audio-taps.swift` показал ноль
//! tap'ов, ноль прочли как «утечки нет», а скрипт был слеп (`CLAUDE.md`).
//!
//! Здесь слепота выглядела бы особенно убедительно: «дублей мало» — это
//! ровно то, что прибор напечатает и при сломанной мере похожести, и на
//! записи без второй дорожки. Поэтому заведомо отрицательный случай
//! идёт не только в самопроверке, но и в каждом прогоне: те же
//! микрофонные реплики сравниваются с системными, отстоящими на
//! полминуты и больше. Родства между ними быть не может, и всё, что мера
//! на них показывает, — её собственный шум.

use std::path::Path;
use std::process::ExitCode;

use domain::{AudioChannel, FinalSegment, SpeakerSource, utc_date_label};
use postcall::{CONTROL_GAP_MS, TwinPair, TwinScan, scan_twins, word_count};
use storage::AudioManifestStore;

/// Пороги, по которым печатается цена свёртки.
const THRESHOLDS: [f32; 6] = [0.3, 0.4, 0.5, 0.6, 0.7, 0.8];

/// Реплики короче считаются короткими и смотрятся отдельно.
///
/// «Да», «ага», «понятно» совпадают по случайности, и порог, подобранный
/// вместе с ними, подобран под шум.
const SHORT_WORDS: usize = 4;

/// Зазор между медианами, ниже которого распределения не разделены.
///
/// Взят до прогона, а не по его числам: порог, подобранный к тому же
/// распределению, которое он должен судить, не судит ничего.
const MIN_MEDIAN_GAP: f32 = 0.15;

/// Доля ложных, которую прибор считает приемлемой, называя порог.
const FALSE_SHARE: f32 = 0.05;

/// Пара со скрина 2026-08-14: одна фраза, распознанная дважды разными
/// словами. Синтетику здесь брать нельзя — придуманная пара похожа ровно
/// настолько, насколько её придумали.
const MIC_LINE: &str = "Нет, у тебя вчера какие-то задачки накидывал, а у них нет";
const SYSTEM_LINE: &str = "Нет, я вчера какие-то задачки накидывал, а там их нет.";
/// Речь владельца: в системную дорожку она не попадает.
const OWN_LINE: &str = "Тогда я беру на себя выгрузку и вечером покажу";
/// Далёкая системная реплика — материал для контроля.
const FAR_LINE: &str = "Давай тогда созвонимся после обеда";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();

    // Сперва прибор, потом данные. Обратный порядок позволил бы
    // прочитать «дублей нет» там, где мера просто ничего не умеет.
    if !self_check() {
        eprintln!("\nПрибор слеп: до настоящих данных дело не дошло.");
        return ExitCode::FAILURE;
    }

    let outcome = match args.as_slice() {
        [] => {
            println!("\n{USAGE}");
            return ExitCode::SUCCESS;
        }
        [root] => list_meetings(Path::new(root)),
        [root, meeting] => probe(Path::new(root), meeting),
        _ => {
            eprintln!("{USAGE}");
            return ExitCode::FAILURE;
        }
    };

    match outcome {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}

const USAGE: &str = "\
Использование:
  dup-probe <каталог-данных>            — встречи, у которых есть Final с обеими дорожками
  dup-probe <каталог-данных> <встреча>  — сколько в ней удвоенных реплик

Каталог данных — тот, где лежит meetingraft.sqlite3.

Гонять сборкой --release: контроль сравнивает каждую микрофонную реплику
со всеми далёкими системными, и в debug часовая встреча считается долго.
  cargo run --release -p meetingraft-dup-probe -- <каталог> <встреча>";

/// Заведомо положительный и заведомо отрицательный случаи.
///
/// Печатает оба числа и зазор между ними: вердикт без зазора ничего не
/// стоит, потому что «дублей нет» слепой прибор напечатает так же
/// уверенно, как зрячий.
fn self_check() -> bool {
    let segments = vec![
        segment(0, AudioChannel::Mic, 1_000, 5_000, MIC_LINE),
        segment(1, AudioChannel::System, 1_200, 5_100, SYSTEM_LINE),
        segment(2, AudioChannel::Mic, 10_000, 14_000, OWN_LINE),
        segment(3, AudioChannel::System, 90_000, 94_000, FAR_LINE),
    ];
    let scan = scan_twins(&segments);

    let twin = scan.overlapping.first().map(|pair| pair.similarity);
    let control = scan.control.first().map(|pair| pair.similarity);

    println!("Проверка прибора на паре с известным ответом");
    println!(
        "  дубль:    похожесть {}, найдено пар {}",
        show(twin),
        scan.overlapping.len()
    );
    println!(
        "  контроль: похожесть {}, реплик владельца без близнеца {}",
        show(control),
        scan.lonely_mic
    );

    let mut ok = true;
    if scan.overlapping.len() != 1 {
        println!("  ВЕРДИКТ: дубль не найден там, где он заведомо есть");
        ok = false;
    }
    if scan.lonely_mic != 1 {
        println!("  ВЕРДИКТ: речь владельца обзавелась близнецом — мера ловит не то");
        ok = false;
    }
    match (twin, control) {
        (Some(twin), Some(control)) if twin - control > 0.3 => {}
        (Some(twin), Some(control)) => {
            println!(
                "  ВЕРДИКТ: зазор {:.2} — мера не делит пересказ и чужую речь",
                twin - control
            );
            ok = false;
        }
        _ => {
            println!("  ВЕРДИКТ: сравнивать было нечего");
            ok = false;
        }
    }
    if ok {
        println!("  ВЕРДИКТ: прибор различает оба случая, числам ниже можно верить");
    }
    ok
}

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

fn list_meetings(root: &Path) -> Result<(), String> {
    let store = AudioManifestStore::open(root).map_err(|error| error.to_string())?;
    let meetings = store
        .list_meeting_summaries()
        .map_err(|error| error.to_string())?;
    if meetings.is_empty() {
        return Err("встреч в базе нет".to_string());
    }

    println!("\nВстречи (реплики Final по дорожкам)");
    for meeting in meetings {
        let Some(final_transcript) = store
            .get_final_transcript(&meeting.id)
            .map_err(|error| error.to_string())?
        else {
            println!(
                "    {} {} — Final не собран",
                utc_date_label(meeting.started_at_ms),
                meeting_label(&meeting)
            );
            continue;
        };
        let segments = store
            .list_final_segments(&meeting.id, final_transcript.version)
            .map_err(|error| error.to_string())?;
        let count = |channel: AudioChannel| {
            segments
                .iter()
                .filter(|segment| segment.channel == channel)
                .count()
        };
        let (mic, system) = (count(AudioChannel::Mic), count(AudioChannel::System));
        // Обе дорожки обязательны: удвоение живёт между ними, и на одной
        // дорожке прибору сравнивать нечего.
        let mark = if mic > 0 && system > 0 { "+" } else { " " };
        println!(
            "  {mark} {} {} — Final v{}, mic {mic}, system {system}",
            utc_date_label(meeting.started_at_ms),
            meeting_label(&meeting),
            final_transcript.version
        );
    }
    println!("\nСтрока с «+» годится для прогона: dup-probe <каталог> <встреча>");
    Ok(())
}

/// Как встреча называется в списке: id и название, если оно есть.
///
/// Id обязателен — им прибор и запускают. Название рядом, потому что по
/// одному id человек не помнит, что это была за встреча, и выбирает
/// наугад.
fn meeting_label(meeting: &domain::MeetingSummary) -> String {
    let title = meeting.title.trim();
    if title.is_empty() {
        return meeting.id.clone();
    }
    format!("{} «{title}»", meeting.id)
}

fn probe(root: &Path, meeting_id: &str) -> Result<(), String> {
    let store = AudioManifestStore::open(root).map_err(|error| error.to_string())?;
    let reading = read(&store, meeting_id)?;

    println!(
        "\nВстреча {} {}, Final v{}",
        utc_date_label(reading.started_at_ms),
        reading.label,
        reading.version
    );
    println!(
        "  реплик: mic {}, system {}",
        reading.scan.mic_total, reading.scan.system_total
    );
    if let Some(note) = channel_clock_note(reading.unified) {
        println!("{note}");
    }

    report(&reading.scan, &reading.segments);
    Ok(())
}

/// Что вышло по встрече: разбор и то, из чего он посчитан.
#[derive(Debug)]
struct Reading {
    version: u32,
    /// Как встреча зовётся в списке: id и название.
    label: String,
    started_at_ms: u64,
    /// Сведены ли метки каналов к общему времени (Epic 25).
    unified: bool,
    segments: Vec<FinalSegment>,
    scan: TwinScan,
}

/// Чтение встречи и разбор, без печати.
///
/// Отдельно от вывода, чтобы половину прибора, которая ходит в базу,
/// можно было проверить тестом. Непроверенная половина прибора — то же
/// самое, что непроверенный прибор целиком: «дублей нет» одинаково
/// выглядит и на чистой записи, и когда сегменты не прочитались вовсе.
fn read(store: &AudioManifestStore, meeting_id: &str) -> Result<Reading, String> {
    let final_transcript = store
        .get_final_transcript(meeting_id)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| format!("у встречи {meeting_id} нет собранного Final"))?;
    let segments = store
        .list_final_segments(meeting_id, final_transcript.version)
        .map_err(|error| error.to_string())?;
    // «Сессии нет» и отсутствие признака читаются как «не сведены»:
    // счёт с предупреждением лучше счёта без него.
    let unified = store
        .channel_clock_unified(meeting_id)
        .map_err(|error| error.to_string())?
        .unwrap_or(false);

    let scan = scan_twins(&segments);
    if scan.mic_total == 0 || scan.system_total == 0 {
        return Err(format!(
            "во встрече {meeting_id} реплики только на одной дорожке \
             (mic {}, system {}) — удвоению взяться неоткуда",
            scan.mic_total, scan.system_total
        ));
    }

    let summary = store
        .list_meeting_summaries()
        .map_err(|error| error.to_string())?
        .into_iter()
        .find(|summary| summary.id == meeting_id);

    Ok(Reading {
        version: final_transcript.version,
        label: summary
            .as_ref()
            .map_or_else(|| meeting_id.to_owned(), meeting_label),
        started_at_ms: summary.map_or(0, |summary| summary.started_at_ms),
        unified,
        segments,
        scan,
    })
}

/// Предупреждение о записи без общего времени каналов (Epic 25).
///
/// Отдельной функцией от печати, чтобы проверялось само решение
/// говорить или молчать, а не текст на экране.
fn channel_clock_note(unified: bool) -> Option<String> {
    if unified {
        return None;
    }
    Some(
        "  ! метки каналов этой записи не сведены к общему времени (Epic 25).\n\
         \x20   Дорожки разъезжались на 1150 мс, и пересечение реплик по времени\n\
         \x20   ниже посчитано с этой ошибкой: пара может не пересечься вовсе.\n\
         \x20   Числа годятся как нижняя оценка, порог по ним не ставить."
            .to_string(),
    )
}

fn report(scan: &TwinScan, segments: &[FinalSegment]) {
    let mic_words: usize = segments
        .iter()
        .filter(|segment| segment.channel == AudioChannel::Mic)
        .map(|segment| word_count(&segment.text))
        .sum();

    println!("\nБлизнецы по времени");
    println!(
        "  микрофонных реплик с системным пересечением: {} из {} ({})",
        scan.overlapping.len(),
        scan.mic_total,
        share(scan.overlapping.len(), scan.mic_total)
    );
    println!(
        "  без пересечения вовсе: {} — кандидаты в речь владельца",
        scan.lonely_mic
    );
    println!(
        "  контрольных пар: {} (системная реплика дальше {} с)",
        scan.control.len(),
        CONTROL_GAP_MS / 1_000
    );

    print_section("Все пары", &scan.overlapping, &scan.control, mic_words);

    let long_pairs = longer_than(&scan.overlapping, SHORT_WORDS);
    let long_control = longer_than(&scan.control, SHORT_WORDS);
    print_section(
        &format!("Только реплики от {SHORT_WORDS} слов"),
        &long_pairs,
        &long_control,
        mic_words,
    );
}

fn print_section(title: &str, pairs: &[TwinPair], control: &[TwinPair], mic_words: usize) {
    println!("\n{title}");
    if pairs.is_empty() {
        println!("  пар нет — мерить нечего");
        return;
    }

    println!("  распределение похожести (пословное расстояние)");
    println!("    похожесть   пары  контроль");
    let pair_bins = histogram(pairs.iter().map(|pair| pair.similarity));
    let control_bins = histogram(control.iter().map(|pair| pair.similarity));
    for bucket in (0..BUCKETS).rev() {
        println!(
            "    {:.1}…{:.1}  {:>6}  {:>8}",
            bucket as f32 / 10.0,
            (bucket + 1) as f32 / 10.0,
            pair_bins[bucket],
            control_bins[bucket]
        );
    }

    let pairs_median = median(pairs.iter().map(|pair| pair.similarity));
    let control_median = median(control.iter().map(|pair| pair.similarity));
    println!(
        "    медиана   {}  {}",
        show(pairs_median),
        show(control_median)
    );

    println!("\n  цена порога");
    println!("    порог   свернётся  ложных  слов mic");
    for row in threshold_rows(pairs, control) {
        println!(
            "    {:.1}     {:>9}  {:>6}  {:>7}",
            row.threshold,
            row.pairs,
            row.control,
            share(row.mic_words, mic_words)
        );
    }

    match judge(pairs_median, control_median) {
        Gap::Nothing => println!("\n  ВЕРДИКТ: сравнивать было нечего"),
        Gap::Blurred => println!(
            "\n  ВЕРДИКТ: контроль забирается не ниже пар — мера ловит не то,\n\
             \x20          и порог по этим числам ставить нельзя"
        ),
        Gap::Clear => {
            println!("\n  ВЕРДИКТ: пары и контроль разошлись, распределению можно верить");
            match clean_threshold(pairs, control) {
                Some(row) => println!(
                    "           берите {:.1} — ложных ноль: свернётся {}, слов mic {}",
                    row.threshold,
                    row.pairs,
                    share(row.mic_words, mic_words)
                ),
                None => println!(
                    "           порога без ложных в сетке нет — сворачивать\n\
                     \x20          придётся с потерями либо не сворачивать вовсе"
                ),
            }
            match honest_threshold(pairs, control) {
                Some(row) => println!(
                    "           ниже {:.1} не опускаться: там уже {} ложных из {}",
                    row.threshold,
                    row.control,
                    control.len()
                ),
                None => println!(
                    "           порога с долей ложных ниже {}% в сетке нет",
                    (FALSE_SHARE * 100.0) as u32
                ),
            }
        }
    }
}

/// Строка таблицы порогов.
#[derive(Debug, Clone, Copy, PartialEq)]
struct ThresholdRow {
    threshold: f32,
    /// Пар, которые при этом пороге свернутся.
    pairs: usize,
    /// Контрольных пар выше порога — заведомо ложные срабатывания.
    control: usize,
    /// Слов микрофонной дорожки, которые уйдут из входа артефакта.
    mic_words: usize,
}

fn threshold_rows(pairs: &[TwinPair], control: &[TwinPair]) -> Vec<ThresholdRow> {
    THRESHOLDS
        .iter()
        .map(|&threshold| ThresholdRow {
            threshold,
            pairs: pairs
                .iter()
                .filter(|pair| pair.similarity >= threshold)
                .count(),
            control: control
                .iter()
                .filter(|pair| pair.similarity >= threshold)
                .count(),
            mic_words: pairs
                .iter()
                .filter(|pair| pair.similarity >= threshold)
                .map(|pair| pair.mic_words)
                .sum(),
        })
        .collect()
}

/// Наименьший порог из сетки, на котором доля ложных не выше [`FALSE_SHARE`].
///
/// Наименьший — потому что чем ниже порог, тем больше дублей свернётся;
/// ограничение сверху ставит контроль, а не вкус.
///
/// Это **нижняя граница**, а не рекомендация: цену ошибок она считает
/// равной, а они не равны. Рекомендацию печатает [`clean_threshold`].
fn honest_threshold(pairs: &[TwinPair], control: &[TwinPair]) -> Option<ThresholdRow> {
    if control.is_empty() {
        return None;
    }
    threshold_rows(pairs, control)
        .into_iter()
        .find(|row| row.control as f32 <= control.len() as f32 * FALSE_SHARE)
}

/// Наименьший порог, на котором ложных **ноль**.
///
/// Тот же выбор, что и у зазора эха, и по той же причине: цена ошибок не
/// равна. Ложная свёртка **стирает реплику** из входа артефакта, и
/// человек об этом не узнает; пропущенный дубль всего лишь оставляет
/// повтор, который видно глазами.
///
/// Печатается рядом с [`honest_threshold`], потому что без него читают
/// его: на `1BF7AEAB` (2026-08-16) прибор назвал 0.4 при шести ложных,
/// тогда как чистым был 0.6. Названное число становится ответом, даже
/// когда оно названо как нижняя граница.
fn clean_threshold(pairs: &[TwinPair], control: &[TwinPair]) -> Option<ThresholdRow> {
    if control.is_empty() {
        return None;
    }
    threshold_rows(pairs, control)
        .into_iter()
        .find(|row| row.control == 0)
}

/// Разошлись ли распределения пар и контроля.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Gap {
    /// Мерить было нечего.
    Nothing,
    /// Контроль не ниже пар: мера ловит не родство, а язык.
    Blurred,
    /// Разошлись.
    Clear,
}

fn judge(pairs_median: Option<f32>, control_median: Option<f32>) -> Gap {
    let (Some(pairs), Some(control)) = (pairs_median, control_median) else {
        return Gap::Nothing;
    };
    if pairs - control < MIN_MEDIAN_GAP {
        return Gap::Blurred;
    }
    Gap::Clear
}

const BUCKETS: usize = 10;

/// Распределение по десятым долям: `[0.0…0.1)`, …, `[0.9…1.0]`.
fn histogram(values: impl Iterator<Item = f32>) -> [usize; BUCKETS] {
    let mut bins = [0usize; BUCKETS];
    for value in values {
        let bucket = ((value * 10.0) as usize).min(BUCKETS - 1);
        bins[bucket] += 1;
    }
    bins
}

fn median(values: impl Iterator<Item = f32>) -> Option<f32> {
    let mut sorted: Vec<f32> = values.collect();
    if sorted.is_empty() {
        return None;
    }
    sorted.sort_by(|left, right| left.partial_cmp(right).unwrap_or(std::cmp::Ordering::Equal));
    Some(sorted[sorted.len() / 2])
}

fn longer_than(pairs: &[TwinPair], words: usize) -> Vec<TwinPair> {
    pairs
        .iter()
        .filter(|pair| pair.words >= words)
        .cloned()
        .collect()
}

fn share(part: usize, whole: usize) -> String {
    if whole == 0 {
        return "—".to_string();
    }
    format!("{:.0}%", 100.0 * part as f32 / whole as f32)
}

fn show(value: Option<f32>) -> String {
    value.map_or_else(|| "—".to_string(), |value| format!("{value:.2}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use domain::FinalTranscript;

    fn tmp_root(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "mr-dup-probe-{name}-{:?}",
            std::thread::current().id()
        ))
    }

    /// Встреча с собранным Final: ровно то, что даёт пересбор.
    fn seed(root: &std::path::Path, meeting_id: &str, segments: &[FinalSegment]) {
        let _ = std::fs::remove_dir_all(root);
        let mut store = AudioManifestStore::open(root).expect("store");
        store
            .begin_session(meeting_id, 0, "проба")
            .expect("session");
        store.end_session(100_000).expect("end");
        store
            .upsert_final_transcript(&FinalTranscript {
                meeting_id: meeting_id.to_string(),
                version: 1,
                body_markdown: String::new(),
                created_at_ms: 1,
            })
            .expect("final");
        store
            .replace_final_segments(meeting_id, 1, segments)
            .expect("segments");
    }

    /// Половина прибора, ходящая в базу, проверяется тем же заведомо
    /// положительным случаем, что и половина, считающая похожесть.
    ///
    /// Иначе «дублей нет» на настоящей записи ничего не значило бы:
    /// сегменты могли не прочитаться вовсе, и выглядело бы это точно так
    /// же, как чистая запись.
    #[test]
    fn reads_stored_segments_and_finds_a_known_double() {
        let root = tmp_root("known-double");
        seed(
            &root,
            "M1",
            &[
                segment(0, AudioChannel::Mic, 1_000, 5_000, MIC_LINE),
                segment(1, AudioChannel::System, 1_200, 5_100, SYSTEM_LINE),
                segment(2, AudioChannel::Mic, 10_000, 14_000, OWN_LINE),
                segment(3, AudioChannel::System, 90_000, 94_000, FAR_LINE),
            ],
        );
        let store = AudioManifestStore::open(&root).expect("store");

        let reading = read(&store, "M1").expect("разбор");
        assert_eq!(reading.version, 1);
        assert!(
            reading.label.contains("M1"),
            "по одному номеру версии встречу не узнать: {}",
            reading.label
        );
        assert_eq!(reading.scan.mic_total, 2, "сегменты обязаны прочитаться");
        assert_eq!(reading.scan.system_total, 2);
        assert_eq!(reading.scan.overlapping.len(), 1);
        assert_eq!(reading.scan.lonely_mic, 1);
        assert!(reading.scan.overlapping[0].similarity > 0.5);
        assert!(
            reading.unified,
            "запись, заведённая сегодняшним кодом, идёт с общим временем каналов"
        );
    }

    #[test]
    fn a_meeting_with_one_track_is_refused() {
        // Печатать «дублей 0» там, где второй дорожки нет вовсе, — то же
        // самое враньё, что и ноль от слепого прибора.
        let root = tmp_root("one-track");
        seed(
            &root,
            "M2",
            &[
                segment(0, AudioChannel::Mic, 1_000, 5_000, MIC_LINE),
                segment(1, AudioChannel::Mic, 10_000, 14_000, OWN_LINE),
            ],
        );
        let store = AudioManifestStore::open(&root).expect("store");

        let error = read(&store, "M2").expect_err("прибор обязан отказать");
        assert!(error.contains("одной дорожке"), "{error}");
    }

    #[test]
    fn a_meeting_without_a_final_is_refused() {
        let root = tmp_root("no-final");
        let _ = std::fs::remove_dir_all(&root);
        let mut store = AudioManifestStore::open(&root).expect("store");
        store.begin_session("M3", 0, "проба").expect("session");
        store.end_session(100_000).expect("end");

        let error = read(&store, "M3").expect_err("прибор обязан отказать");
        assert!(error.contains("Final"), "{error}");
    }

    fn pair(similarity: f32, words: usize, mic_words: usize) -> TwinPair {
        TwinPair {
            mic_index: 0,
            system_index: 1,
            overlap_ms: 100,
            similarity,
            overlap_share: similarity,
            words,
            mic_words,
        }
    }

    #[test]
    fn the_probe_passes_its_own_check() {
        // Самопроверка — единственное, что стоит между прибором и
        // числом, которому поверят.
        assert!(self_check());
    }

    #[test]
    fn a_meeting_shows_its_name_next_to_the_id() {
        // По одному id человек не помнит, что это была за встреча, и
        // выбирает наугад — а прогон идёт минуты.
        let summary = domain::MeetingSummary {
            id: "1BF7AEAB".into(),
            title: "Синк команды".into(),
            started_at_ms: 1_785_628_800_000,
            ended_at_ms: None,
            has_final: true,
            artifact_count: 0,
            audio_deleted_at_ms: None,
        };
        assert_eq!(meeting_label(&summary), "1BF7AEAB «Синк команды»");
    }

    #[test]
    fn a_nameless_meeting_still_shows_its_id() {
        // Пустое название законно (`MeetingSummary`), и подставлять
        // вместо него что-нибудь — дело презентационного слоя, не прибора.
        let summary = domain::MeetingSummary {
            id: "1BF7AEAB".into(),
            title: "   ".into(),
            started_at_ms: 0,
            ended_at_ms: None,
            has_final: false,
            artifact_count: 0,
            audio_deleted_at_ms: None,
        };
        assert_eq!(meeting_label(&summary), "1BF7AEAB");
    }

    #[test]
    fn histogram_puts_a_perfect_match_in_the_top_bucket() {
        // 1.0 * 10 = 10, и без потолка индекс вышел бы за массив.
        let bins = histogram([1.0, 0.0, 0.55].into_iter());
        assert_eq!(bins[9], 1);
        assert_eq!(bins[0], 1);
        assert_eq!(bins[5], 1);
    }

    #[test]
    fn median_of_nothing_is_nothing() {
        // Ноль вместо «нечего мерить» прочитался бы как «дублей нет».
        assert_eq!(median(std::iter::empty()), None);
        assert_eq!(median([0.2, 0.9, 0.4].into_iter()), Some(0.4));
    }

    #[test]
    fn a_blurred_gap_is_named_blurred() {
        assert_eq!(judge(Some(0.62), Some(0.11)), Gap::Clear);
        assert_eq!(judge(Some(0.42), Some(0.40)), Gap::Blurred);
        assert_eq!(judge(Some(0.42), None), Gap::Nothing);
    }

    #[test]
    fn threshold_row_counts_both_sides() {
        let pairs = vec![pair(0.9, 10, 12), pair(0.45, 6, 7)];
        let control = vec![pair(0.35, 8, 9)];
        let rows = threshold_rows(&pairs, &control);
        let row = |threshold: f32| {
            *rows
                .iter()
                .find(|row| (row.threshold - threshold).abs() < 0.01)
                .unwrap()
        };
        assert_eq!(row(0.3).pairs, 2);
        assert_eq!(row(0.3).control, 1, "ложные обязаны считаться тоже");
        assert_eq!(row(0.3).mic_words, 19);
        assert_eq!(row(0.5).pairs, 1);
        assert_eq!(row(0.5).control, 0);
        assert_eq!(row(0.5).mic_words, 12);
    }

    #[test]
    fn the_named_threshold_is_the_lowest_clean_one() {
        // Двадцать пар и двадцать контрольных: 5% — это одна ложная.
        let pairs: Vec<TwinPair> = (0..20).map(|_| pair(0.75, 10, 10)).collect();
        let mut control: Vec<TwinPair> = (0..18).map(|_| pair(0.2, 10, 10)).collect();
        control.push(pair(0.55, 10, 10));
        control.push(pair(0.65, 10, 10));

        let row = honest_threshold(&pairs, &control).expect("порог обязан найтись");
        assert_eq!(
            row.threshold, 0.6,
            "0.5 стоил бы двух ложных из двадцати — это больше 5%"
        );
        assert_eq!(row.pairs, 20);
    }

    #[test]
    fn the_named_threshold_is_the_cheapest_one_without_false_folds() {
        // Цена ошибок не равна: ложная свёртка стирает реплику молча,
        // пропущенный дубль оставляет видимый повтор. Поэтому прибор
        // называет первый порог с нулём ложных, а не первый терпимый.
        let pairs: Vec<TwinPair> = (0..20).map(|_| pair(0.75, 10, 10)).collect();
        let mut control: Vec<TwinPair> = (0..18).map(|_| pair(0.2, 10, 10)).collect();
        control.push(pair(0.45, 10, 10));
        control.push(pair(0.55, 10, 10));

        let clean = clean_threshold(&pairs, &control).expect("чистый порог обязан найтись");
        assert_eq!(clean.threshold, 0.6, "0.5 стоил бы одной ложной");
        assert_eq!(clean.control, 0);

        let lowest = honest_threshold(&pairs, &control).expect("нижняя граница");
        assert!(
            lowest.threshold < clean.threshold,
            "нижняя граница обязана быть ниже чистого порога: {} против {}",
            lowest.threshold,
            clean.threshold
        );
    }

    #[test]
    fn without_a_clean_threshold_the_probe_says_so() {
        // Ложные на каждом пороге — законный ответ, и выдавать за него
        // самый терпимый нельзя.
        let pairs = vec![pair(0.9, 10, 10)];
        let control = vec![pair(0.95, 10, 10)];
        assert_eq!(clean_threshold(&pairs, &control), None);
    }

    #[test]
    fn without_control_no_threshold_is_named() {
        // Порог, названный без заведомо отрицательного случая, — это
        // порог, взятый из головы.
        let pairs = vec![pair(0.9, 10, 10)];
        assert_eq!(honest_threshold(&pairs, &[]), None);
    }

    #[test]
    fn short_replies_are_split_off() {
        let pairs = vec![pair(0.9, 1, 1), pair(0.7, 4, 5)];
        let long = longer_than(&pairs, SHORT_WORDS);
        assert_eq!(long.len(), 1);
        assert_eq!(long[0].words, 4);
    }

    #[test]
    fn a_record_without_a_common_clock_is_warned_about() {
        assert_eq!(channel_clock_note(true), None, "лишнее предупреждение");
        let note = channel_clock_note(false).expect("предупреждение обязано быть");
        assert!(note.contains("Epic 25"), "{note}");
    }

    #[test]
    fn share_of_nothing_is_not_zero_percent() {
        // Ноль процентов от пустоты читается как измеренный ноль.
        assert_eq!(share(0, 0), "—");
        assert_eq!(share(1, 4), "25%");
    }
}
