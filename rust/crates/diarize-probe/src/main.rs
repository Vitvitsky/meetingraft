//! Прибор для разделения голосов внутри дорожки.
//!
//! Третий рядом с `echo-probe` и `gate-probe`, с той же дисциплиной:
//! **сперва заведомо положительный и заведомо отрицательный случай, потом
//! настоящие данные**. Правило писано кровью — `count-audio-taps.swift`
//! показал ноль tap'ов, ноль прочли как «утечки нет», а скрипт был слеп
//! (`CLAUDE.md`).
//!
//! Здесь оно жёстче, чем у соседей, потому что ошибиться легче. У гейта
//! ноль пропущенных кадров хотя бы выглядит подозрительно; у диаризации
//! **«нашёлся один голос» — законный ответ**: монолог, запись одного
//! человека, встреча, где второй молчал. Отличить его от сломанного
//! движка по самому числу нельзя вовсе. Отсюда два случая:
//!
//! - **заведомо положительный** — два разных синтетических голоса,
//!   склеенных подряд: смену обязан найти;
//! - **заведомо отрицательный** — один голос той же длины: смены быть не
//!   должно.
//!
//! Не разошлись — до настоящих данных дело не доходит.
//!
//! Сегодня прибор до них и не доходит: модель не выбрана, и
//! `diarize_backend()` отдаёт заглушку, которая честно отказывает. Это не
//! поломка прибора, а его первый настоящий ответ: измерять нечем. Выбор
//! модели — задача 3 плана `2026-08-11-voice-clustering`, и делается он
//! замером на Маке.

use std::collections::BTreeMap;
use std::path::Path;
use std::process::ExitCode;

use diarize::{DiarizeReport, Diarizer, diarize_backend};
use domain::AudioChannel;
use storage::AudioManifestStore;

/// Частота живого пути; ею же пишутся чанки на диск (ADR-005).
const RATE: u32 = 16_000;
/// Длительность каждого голоса в синтетике.
const CASE_MS: u64 = 3_000;
/// Насколько граница, найденная прибором, может разойтись со склейкой.
///
/// Полсекунды — не подгонка под выход, а то, чем такой промах обходится:
/// граница внутри реплики отдаёт чужой голос на пол-фразы, и на
/// прослушивании фрагмента (задача 6 плана) это слышно сразу.
const BORDER_TOLERANCE_MS: u64 = 500;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();

    let mut engine = diarize_backend();
    if !self_check(engine.as_mut()).is_empty() {
        eprintln!("\nПрибор слеп: до настоящих данных дело не дошло.");
        return ExitCode::FAILURE;
    }

    let result = match args.as_slice() {
        [] => {
            println!("\n{USAGE}");
            return ExitCode::SUCCESS;
        }
        [root] => list_sessions(Path::new(root)),
        [root, session] => probe(Path::new(root), session, engine.as_mut()),
        _ => {
            eprintln!("{USAGE}");
            return ExitCode::FAILURE;
        }
    };

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}

const USAGE: &str = "\
Использование:
  diarize-probe <каталог-данных>             — список сессий
  diarize-probe <каталог-данных> <сессия>    — разделить голоса по сессии

Каталог данных — тот, где лежит meetingraft.sqlite3.

Мерить надо две записи, и они отвечают на разные вопросы: очную (двое
говорят в один микрофон ноутбука) и созвон. Первая — тот случай, который
атрибуция по каналам не берёт по построению; вторая показывает, что
диаризация даёт сверх канала там, где канал уже отвечает.";

/// Заведомо положительный и заведомо отрицательный случай.
///
/// Пусто — прибору можно верить. Иначе — по строке на каждое расхождение,
/// и строка называет **своё**: вердикт «прибор слеп» без причины под ним
/// нечем ни проверить, ни починить. Числа печатаются всегда, до вердикта.
fn self_check(engine: &mut dyn Diarizer) -> Vec<String> {
    println!("Проверка прибора на синтетике");

    let two_voices = [voice(110.0, CASE_MS), voice(210.0, CASE_MS)].concat();
    let one_voice = voice(110.0, CASE_MS * 2);
    // Материал обязан быть, и это утверждается до утверждений о
    // результате: «ноль голосов» на пустом входе выполняется само собой
    // (Epic 16, детектор эха).
    let expected_len = (RATE as u64 * CASE_MS * 2 / 1_000) as usize;
    if two_voices.len() != expected_len || one_voice.len() != expected_len {
        return report(vec![
            "синтетика не сгенерировалась — проверять было нечего".to_string(),
        ]);
    }

    let positive = engine.diarize(&two_voices, RATE);
    let negative = engine.diarize(&one_voice, RATE);

    describe("  два голоса подряд", &positive);
    describe("  один голос      ", &negative);

    if let Some(reason) = positive.refused.as_ref().or(negative.refused.as_ref()) {
        // Отказ — не то же самое, что неверный ответ, и разбирать его
        // дальше нечего: отрезков нет ни у одного случая, и все проверки
        // ниже сработали бы на пустоте, назвав движок сломанным вместо
        // отсутствующего.
        return report(vec![format!("движок отказался считать — {reason}")]);
    }

    let mut problems = Vec::new();
    if positive.turns.is_empty() {
        problems.push("на двух голосах не нашлось ни одного отрезка речи".to_string());
    }
    if positive.speakers_found < 2 {
        problems.push(format!(
            "два заведомо разных голоса слились в {} — смены движок не видит",
            positive.speakers_found
        ));
    }
    if negative.speakers_found > 1 {
        problems.push(format!(
            "один голос разорван на {} — движок делит на пустом месте",
            negative.speakers_found
        ));
    }
    // Число голосов может сойтись при границе, поставленной мимо: два
    // кластера, оба вперемешку. Тогда фрагмент на прослушивании окажется
    // чужим, а число в отчёте — верным.
    if problems.is_empty() {
        match border_error_ms(&positive, CASE_MS) {
            Some(error) if error > BORDER_TOLERANCE_MS => problems.push(format!(
                "смена найдена, но не там — граница разошлась со склейкой на {error} мс"
            )),
            Some(error) => println!("  граница разошлась со склейкой на {error} мс"),
            None => {
                problems.push("смены метки внутри отрезков нет — делить было нечем".to_string())
            }
        }
    }
    report(problems)
}

/// Напечатать вердикт и вернуть его же вызывающему.
fn report(problems: Vec<String>) -> Vec<String> {
    if problems.is_empty() {
        println!(
            "  ВЕРДИКТ: прибор различает два голоса и не делит один, числам ниже можно верить"
        );
    }
    for problem in &problems {
        println!("  ВЕРДИКТ: {problem}");
    }
    problems
}

fn describe(label: &str, report: &DiarizeReport) {
    match report.refused.as_ref() {
        Some(reason) => println!("{label}: отказ — {reason}"),
        None => println!(
            "{label}: голосов {}, отрезков {}, речи {:.1} с",
            report.speakers_found,
            report.turns.len(),
            report.speech_ms() as f64 / 1_000.0
        ),
    }
}

/// Насколько ближайшая смена метки разошлась с ожидаемым временем.
///
/// `None` — смены нет вовсе: отрезки есть, но метка на всех одна.
fn border_error_ms(report: &DiarizeReport, expected_ms: u64) -> Option<u64> {
    report
        .turns
        .windows(2)
        .filter(|pair| pair[0].cluster != pair[1].cluster)
        .map(|pair| pair[1].start_ms.abs_diff(expected_ms))
        .min()
}

fn list_sessions(root: &Path) -> Result<(), String> {
    let store = AudioManifestStore::open(root).map_err(|error| error.to_string())?;
    let meetings = store
        .list_meeting_summaries()
        .map_err(|error| error.to_string())?;
    if meetings.is_empty() {
        return Err("встреч в базе нет".to_string());
    }

    println!("\nСессии (кадров на дорожку)");
    for meeting in meetings {
        let chunks = store
            .list_chunks(&meeting.id)
            .map_err(|error| error.to_string())?;
        let frames = |channel: AudioChannel| -> u64 {
            chunks
                .iter()
                .filter(|chunk| chunk.channel == channel)
                .map(|chunk| u64::from(chunk.frame_count))
                .sum()
        };
        let (mic, system) = (frames(AudioChannel::Mic), frames(AudioChannel::System));
        let mark = if mic > 0 || system > 0 { "+" } else { " " };
        println!("  {mark} {} — mic {mic}, system {system}", meeting.id);
    }
    println!("\nСтрока с «+» годится для прогона: diarize-probe <каталог> <сессия>");
    Ok(())
}

fn probe(root: &Path, session_id: &str, engine: &mut dyn Diarizer) -> Result<(), String> {
    let store = AudioManifestStore::open(root).map_err(|error| error.to_string())?;
    let tracks = tracks(&store, session_id)?;

    println!("\nСессия {session_id}, {RATE} Гц");
    // Проход идёт **внутри канала**, а не по миксу, и это не оптимизация.
    // Канал остаётся источником истины там, где он есть (ADR-012):
    // диаризация отвечает на другой вопрос — кто из нескольких людей
    // говорит внутри одной дорожки. Смешать каналы значило бы заново
    // спрашивать то, на что запись уже ответила, и получить в ответ эхо
    // динамиков, помеченное как третий голос.
    for (channel, pcm) in tracks {
        let seconds = pcm.len() as f64 / f64::from(RATE);
        println!("\n  Дорожка {} — {seconds:.1} с", channel.code());

        let report = engine.diarize(&pcm, RATE);
        if let Some(reason) = report.refused {
            println!("    отказ: {reason}");
            continue;
        }
        print_report(&report, pcm.len() as u64 * 1_000 / u64::from(RATE));
    }
    Ok(())
}

fn print_report(report: &DiarizeReport, track_ms: u64) {
    println!("    голосов: {}", report.speakers_found);
    if report.turns.is_empty() {
        println!("    отрезков нет — речи на дорожке не нашлось");
        return;
    }

    let mut by_cluster: BTreeMap<u32, (usize, u64)> = BTreeMap::new();
    for turn in &report.turns {
        let entry = by_cluster.entry(turn.cluster).or_default();
        entry.0 += 1;
        entry.1 += turn.duration_ms();
    }
    for (cluster, (count, ms)) in &by_cluster {
        println!(
            "    голос {cluster}: {count} отрезков, {:.1} с ({:.1}%)",
            *ms as f64 / 1_000.0,
            percent(*ms, track_ms)
        );
    }

    // Доля тишины считается вычитанием из длины дорожки, а не суммой
    // промежутков: отрезки могут перекрываться, и сумма промежутков
    // тогда врала бы в меньшую сторону.
    let silence = track_ms.saturating_sub(report.speech_ms());
    println!(
        "    не покрыто отрезками: {:.1} с ({:.1}%)",
        silence as f64 / 1_000.0,
        percent(silence, track_ms)
    );

    println!("\n    начало, с   конец, с   голос");
    for turn in &report.turns {
        println!(
            "    {:>9.1} {:>10.1}   {}",
            turn.start_ms as f64 / 1_000.0,
            turn.end_ms as f64 / 1_000.0,
            turn.cluster
        );
    }
}

/// Дорожки сессии целиком, по одной на канал (ADR-006).
fn tracks(
    store: &AudioManifestStore,
    session_id: &str,
) -> Result<Vec<(AudioChannel, Vec<i16>)>, String> {
    let chunks = store
        .list_chunks(session_id)
        .map_err(|error| error.to_string())?;
    if chunks.is_empty() {
        return Err(format!("у сессии {session_id} нет чанков"));
    }
    let mut rates: Vec<u32> = chunks.iter().map(|chunk| chunk.sample_rate).collect();
    rates.sort_unstable();
    rates.dedup();
    if rates != [RATE] {
        return Err(format!(
            "частоты чанков не те, что у живого пути: {rates:?}"
        ));
    }

    let mut out = Vec::new();
    for channel in [AudioChannel::Mic, AudioChannel::System] {
        let track = store
            .read_session_pcm(session_id, channel)
            .map_err(|error| error.to_string())?;
        if !track.is_empty() {
            out.push((channel, track));
        }
    }
    if out.is_empty() {
        return Err("обе дорожки пусты — делить нечего".to_string());
    }
    Ok(out)
}

/// Синтетический голос: основной тон с затухающими гармониками и
/// слоговой огибающей.
///
/// Гармоники нужны, чтобы дорожка не была чистым тоном — на чистом тоне
/// «разделяет» что угодно, включая прибор, сравнивающий частоты. Огибающая
/// не доходит до нуля: провал в тишину дал бы движку паузу там, где её в
/// речи нет, и смена нашлась бы по паузе, а не по голосу.
fn voice(f0: f32, ms: u64) -> Vec<i16> {
    let samples = (u64::from(RATE) * ms / 1_000) as usize;
    (0..samples)
        .map(|index| {
            let t = index as f32 / RATE as f32;
            let envelope = 0.7 + 0.3 * (2.0 * std::f32::consts::PI * 4.0 * t).sin();
            let tone: f32 = (1..=12)
                .map(|harmonic| {
                    let amplitude = 1.0 / harmonic as f32;
                    amplitude * (2.0 * std::f32::consts::PI * f0 * harmonic as f32 * t).sin()
                })
                .sum();
            (tone * envelope * 3_000.0).clamp(-32_000.0, 32_000.0) as i16
        })
        .collect()
}

fn percent(part: u64, whole: u64) -> f64 {
    if whole == 0 {
        return 0.0;
    }
    part as f64 * 100.0 / whole as f64
}

#[cfg(test)]
mod tests {
    use super::*;

    use diarize::{MockDiarizer, VoiceTurn};

    fn tmp_root(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "mr-diarize-probe-{name}-{:?}",
            std::thread::current().id()
        ))
    }

    /// Заведомо рабочий движок: контроль, а не модель.
    ///
    /// Считает частоту переходов через ноль по окнам и делит дорожку
    /// надвое, только если разброс достаточно велик. На настоящем звуке
    /// это не работает и работать не должно — вход у контроля один:
    /// синтетика из `self_check`. Он существует, чтобы доказать, что
    /// прибор **видит** разделение, когда оно есть. Без такого контроля
    /// красный вердикт прибора неотличим от вердикта прибора сломанного.
    #[derive(Default)]
    struct PitchControl;

    impl PitchControl {
        const WINDOW_MS: u64 = 250;
        /// Во сколько раз самое высокое окно должно превосходить самое
        /// низкое, чтобы считать голоса разными.
        const SPREAD: f32 = 1.5;
    }

    impl Diarizer for PitchControl {
        fn diarize(&mut self, pcm: &[i16], sample_rate: u32) -> DiarizeReport {
            let window = (u64::from(sample_rate) * Self::WINDOW_MS / 1_000) as usize;
            if pcm.len() < window * 2 {
                return DiarizeReport::refused("дорожка короче двух окон");
            }
            let rates: Vec<f32> = pcm
                .chunks(window)
                .filter(|chunk| chunk.len() == window)
                .map(|chunk| {
                    let crossings = chunk
                        .windows(2)
                        .filter(|pair| (pair[0] < 0) != (pair[1] < 0))
                        .count();
                    crossings as f32
                })
                .collect();
            let low = rates.iter().copied().fold(f32::MAX, f32::min);
            let high = rates.iter().copied().fold(0.0, f32::max);

            let label = |rate: f32| -> u32 {
                if high <= low * Self::SPREAD || rate * 2.0 <= low + high {
                    0
                } else {
                    1
                }
            };

            let mut turns: Vec<VoiceTurn> = Vec::new();
            for (index, rate) in rates.iter().enumerate() {
                let start = index as u64 * Self::WINDOW_MS;
                let cluster = label(*rate);
                match turns.last_mut() {
                    Some(last) if last.cluster == cluster => last.end_ms = start + Self::WINDOW_MS,
                    _ => turns.push(VoiceTurn::new(start, start + Self::WINDOW_MS, cluster)),
                }
            }
            DiarizeReport::from_turns(turns)
        }
    }

    /// Движок, для которого всё — один голос. Ловится положительным
    /// случаем.
    struct NeverSplits;

    impl Diarizer for NeverSplits {
        fn diarize(&mut self, pcm: &[i16], sample_rate: u32) -> DiarizeReport {
            let ms = pcm.len() as u64 * 1_000 / u64::from(sample_rate);
            DiarizeReport::from_turns(vec![VoiceTurn::new(0, ms, 0)])
        }
    }

    /// Движок, который делит всё пополам. Ловится отрицательным случаем —
    /// и это самая опасная поломка: «нашлось двое» звучит как результат.
    struct AlwaysSplits;

    impl Diarizer for AlwaysSplits {
        fn diarize(&mut self, pcm: &[i16], sample_rate: u32) -> DiarizeReport {
            let ms = pcm.len() as u64 * 1_000 / u64::from(sample_rate);
            DiarizeReport::from_turns(vec![
                VoiceTurn::new(0, ms / 2, 0),
                VoiceTurn::new(ms / 2, ms, 1),
            ])
        }
    }

    /// Движок, находящий смену не там, где она есть.
    ///
    /// Число голосов у него верное **в обоих случаях**: один голос он не
    /// делит, два разделяет. Ошибается он только местом склейки — и это
    /// единственное, чем он отличается от рабочего. Иначе тест на границу
    /// проходил бы по ветке отрицательного случая и о самой границе не
    /// говорил бы ничего (проверено снятием ветки).
    struct SplitsInTheWrongPlace;

    impl Diarizer for SplitsInTheWrongPlace {
        fn diarize(&mut self, pcm: &[i16], sample_rate: u32) -> DiarizeReport {
            let honest = PitchControl.diarize(pcm, sample_rate);
            if honest.speakers_found < 2 {
                return honest;
            }
            let ms = pcm.len() as u64 * 1_000 / u64::from(sample_rate);
            DiarizeReport::from_turns(vec![
                VoiceTurn::new(0, ms / 6, 0),
                VoiceTurn::new(ms / 6, ms, 1),
            ])
        }
    }

    /// Каждый тест ниже утверждает **свою** строку вердикта, а не просто
    /// «прибор красный». Красным он бывает по нескольким причинам сразу, и
    /// проверка одного лишь цвета проходила бы по чужой ветке — так и
    /// вышло на первой версии: отказ движка ловился проверкой «отрезков
    /// нет», и своя ветка снималась незамеченной.
    fn problem(engine: &mut dyn Diarizer, needle: &str) {
        let problems = self_check(engine);
        assert!(
            problems.iter().any(|line| line.contains(needle)),
            "вердикт не назвал «{needle}»: {problems:?}"
        );
    }

    /// Прибор обязан пропустить движок, который действительно разделяет.
    ///
    /// Заведомо положительный случай для самого вердикта: без него
    /// «прибор красный» ничего не значит — он мог бы быть красным всегда.
    #[test]
    fn the_self_check_passes_a_working_diarizer() {
        assert!(self_check(&mut PitchControl).is_empty());
    }

    /// Сегодняшнее состояние: модели нет, заглушка отказывает, прибор до
    /// настоящих данных не доходит. Это ответ, а не поломка.
    #[test]
    fn the_stub_does_not_reach_real_data() {
        problem(&mut MockDiarizer::new(), "отказался считать");
    }

    #[test]
    fn a_diarizer_that_never_splits_fails_the_positive_case() {
        problem(&mut NeverSplits, "слились");
    }

    #[test]
    fn a_diarizer_that_always_splits_fails_the_negative_case() {
        problem(&mut AlwaysSplits, "делит на пустом месте");
    }

    /// Число голосов сходится, а граница — нет. Проверка на границу
    /// заводилась ровно против этого случая.
    #[test]
    fn a_border_in_the_wrong_place_is_caught() {
        problem(&mut SplitsInTheWrongPlace, "граница разошлась");
    }

    /// Синтетика обязана быть разной: если бы два «голоса» звучали
    /// одинаково, положительный случай проверял бы сам себя.
    #[test]
    fn the_two_synthetic_voices_are_actually_different() {
        let crossings = |pcm: &[i16]| {
            pcm.windows(2)
                .filter(|pair| (pair[0] < 0) != (pair[1] < 0))
                .count()
        };
        let low = voice(110.0, CASE_MS);
        let high = voice(210.0, CASE_MS);

        assert!(!low.is_empty() && low.len() == high.len(), "материала нет");
        assert!(
            crossings(&high) > crossings(&low) * 3 / 2,
            "голоса неразличимы по частоте: {} против {}",
            crossings(&low),
            crossings(&high)
        );
    }

    fn bytes_of(pcm: &[i16]) -> Vec<u8> {
        pcm.iter().flat_map(|sample| sample.to_le_bytes()).collect()
    }

    /// Сессия с записанной дорожкой: чанки по 100 мс, как в живом пути.
    fn seed(root: &std::path::Path, session_id: &str, pcm: &[i16]) {
        let mut store = AudioManifestStore::open(root).expect("store");
        store
            .begin_session(session_id, 0, "проба")
            .expect("session");
        let frame = RATE as usize / 10;
        for (index, chunk) in pcm.chunks(frame).enumerate() {
            store
                .append_chunk(
                    AudioChannel::Mic,
                    &bytes_of(chunk),
                    RATE,
                    (index * 100) as u64,
                )
                .expect("chunk");
        }
        store.end_session(1_000).expect("end");
    }

    /// Половина прибора, ходящая в базу, проверяется тем же заведомым
    /// случаем, что и половина, считающая голоса.
    ///
    /// Иначе «на записи один голос» ничего не значило бы: дорожка могла не
    /// прочитаться вовсе, и выглядело бы это точно так же, как монолог.
    #[test]
    fn reads_stored_tracks_and_finds_the_known_change() {
        let root = tmp_root("known-voices");
        let _ = std::fs::remove_dir_all(&root);
        let pcm = [voice(110.0, CASE_MS), voice(210.0, CASE_MS)].concat();
        seed(&root, "s1", &pcm);

        let store = AudioManifestStore::open(&root).expect("store");
        let tracks = tracks(&store, "s1").expect("дорожки");

        assert_eq!(tracks.len(), 1, "дорожка одна — микрофонная");
        assert_eq!(tracks[0].1.len(), pcm.len(), "дорожка прочлась не вся");

        let report = PitchControl.diarize(&tracks[0].1, RATE);

        assert!(!report.is_refused(), "контроль отказался считать");
        assert_eq!(
            report.speakers_found, 2,
            "два заведомо разных голоса с диска не разделились"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Пустая сессия — отказ, а не отчёт с нулями.
    #[test]
    fn an_empty_session_is_refused_out_loud() {
        let root = tmp_root("empty");
        let _ = std::fs::remove_dir_all(&root);
        {
            let mut store = AudioManifestStore::open(&root).expect("store");
            store.begin_session("s1", 0, "проба").expect("session");
            store.end_session(1_000).expect("end");
        }

        let store = AudioManifestStore::open(&root).expect("store");
        let error = tracks(&store, "s1").expect_err("пустая сессия — отказ");

        assert!(error.contains("нет чанков"), "{error}");
        let _ = std::fs::remove_dir_all(&root);
    }
}
