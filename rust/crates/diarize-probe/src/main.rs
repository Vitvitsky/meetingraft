//! Прибор для разделения голосов внутри дорожки.
//!
//! Третий рядом с `echo-probe` и `gate-probe`, с той же дисциплиной:
//! **сперва случай с известным ответом, потом настоящие данные**. Правило
//! писано кровью — `count-audio-taps.swift` показал ноль tap'ов, ноль
//! прочли как «утечки нет», а скрипт был слеп (`CLAUDE.md`).
//!
//! Здесь оно жёстче, чем у соседей, потому что ошибиться легче. У гейта
//! ноль пропущенных кадров хотя бы выглядит подозрительно; у диаризации
//! **«нашёлся один голос» — законный ответ**: монолог, запись одного
//! человека, встреча, где второй молчал. Отличить его от сломанного
//! движка по самому числу нельзя вовсе.
//!
//! ## Почему контроль — записи, а не синтетика
//!
//! Первая версия проверяла движок двумя синтетическими голосами: тон
//! 110 Гц с гармониками, следом 210 Гц. Логику прибора это проверяет
//! (тестовые двойники на ней и живут), а вот **движок — нет**, и
//! обнаружилось это в первый же прогон настоящей связки: sherpa-onnx
//! нашёл в двух тонах речь, но счёл их одним голосом.
//!
//! Дело было не в движке. На записи с двумя заведомо разными людьми он
//! находит двоих. Тоны просто не речь, и модель голосов на них не
//! работает — а прибор при этом печатал «смены движок не видит» **про
//! работающий движок**. То самое враньё прибора, ради которого вся эта
//! дисциплина и заведена.
//!
//! Поэтому контроль — записи с известным ответом, число людей в имени
//! файла. Кладёт их `scripts/fetch-diarize-models.sh` рядом с моделями.
//! Нет записей — прибор не судит движок вовсе и говорит, чего не хватает:
//! судить по негодному материалу хуже, чем не судить.
//!
//! ## Что вердикт считает поломкой, а что — неточностью
//!
//! Разделение не случайно. Прибор отвечает на один вопрос — **видит ли
//! движок смену голоса вообще**, — и только на него имеет право отвечать
//! отказом:
//!
//! - движок отказался считать, либо нашёл голосов **меньше**, чем в
//!   записи заведомо есть, — прибор слеп, дальше не идём;
//! - нашёл **больше**, либо число растёт от количества материала — это
//!   неточность настройки, а не слепота. Печатается громко, с числами, и
//!   работать не мешает: выбор порога и есть задача 3, а спрятать числа,
//!   по которым он делается, значит сделать её невыполнимой.

mod wav;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use diarize::{DiarizeReport, Diarizer, diarize_backend, diarize_models_dir};
use domain::AudioChannel;
use storage::AudioManifestStore;

/// Частота живого пути; ею же пишутся чанки на диск (ADR-005).
const RATE: u32 = 16_000;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();

    // Каталог данных нужен раньше самопроверки: и модели, и контрольные
    // записи лежат в нём же, рядом с базой.
    let (root, session) = match args.as_slice() {
        [] => {
            println!("\n{USAGE}");
            return ExitCode::SUCCESS;
        }
        [root] => (Path::new(root), None),
        [root, session] => (Path::new(root), Some(session.as_str())),
        _ => {
            eprintln!("{USAGE}");
            return ExitCode::FAILURE;
        }
    };

    let mut engine = diarize_backend(root);
    let controls = match load_controls(root) {
        Ok(controls) => controls,
        Err(error) => {
            eprintln!("контрольные записи не прочлись: {error}");
            return ExitCode::FAILURE;
        }
    };
    if !self_check(engine.as_mut(), &controls).is_empty() {
        eprintln!("\nПрибор слеп: до настоящих данных дело не дошло.");
        return ExitCode::FAILURE;
    }

    let result = match session {
        None => list_sessions(root),
        Some(id) => probe(root, id, engine.as_mut()),
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

Каталог данных — тот, где лежит meetingraft.sqlite3. Модели и контрольные
записи кладёт туда scripts/fetch-diarize-models.sh; движок включается
сборкой с --features model.

Мерить надо две записи, и они отвечают на разные вопросы: очную (двое
говорят в один микрофон ноутбука) и созвон. Первая — тот случай, который
атрибуция по каналам не берёт по построению; вторая показывает, что
диаризация даёт сверх канала там, где канал уже отвечает.";

/// Запись, о которой заранее известно, сколько в ней людей.
#[derive(Debug)]
pub struct Control {
    name: String,
    /// Сколько человек в записи на самом деле — из имени файла.
    speakers: u32,
    pcm: Vec<i16>,
}

/// Каталог контрольных записей: `<data_root>/models/diarize/check/`.
fn controls_dir(data_root: &Path) -> PathBuf {
    diarize_models_dir(data_root).join("check")
}

/// Прочитать контроли. Имя файла начинается с числа людей: `2-...wav`.
///
/// Отсутствие каталога — не ошибка: без движка контроли и не нужны, а
/// решает, что с этим делать, сам вердикт. Ошибка — только испорченный
/// файл: молча пропустить его значило бы судить движок по неполному
/// набору и не сказать об этом.
fn load_controls(data_root: &Path) -> Result<Vec<Control>, String> {
    let dir = controls_dir(data_root);
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Ok(Vec::new());
    };

    let mut out = Vec::new();
    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("wav") {
            continue;
        }
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default()
            .to_string();
        let speakers: u32 = name
            .split('-')
            .next()
            .and_then(|head| head.parse().ok())
            .ok_or_else(|| {
                format!("{name}: имя обязано начинаться с числа людей, например 2-...")
            })?;
        if speakers == 0 {
            return Err(format!("{name}: ноль людей в контроле проверять нечего"));
        }
        let wav = wav::read(&path)?;
        if wav.sample_rate != RATE {
            return Err(format!(
                "{name}: частота {} Гц, а живой путь пишет {RATE} Гц — \
                 контроль в чужой частоте проверял бы не тот звук",
                wav.sample_rate
            ));
        }
        out.push(Control {
            name,
            speakers,
            pcm: wav.pcm,
        });
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(out)
}

/// Проверка движка по записям с известным ответом.
///
/// Пусто — можно верить числам ниже. Иначе — по строке на каждую беду, и
/// строка называет **свою**: вердикт «прибор слеп» без причины под ним
/// нечем ни проверить, ни починить.
fn self_check(engine: &mut dyn Diarizer, controls: &[Control]) -> Vec<String> {
    println!("Проверка движка на записях с известным ответом");

    if controls.is_empty() {
        // Движок мог не подняться вовсе — тогда его отказ и есть ответ, и
        // отсутствие контролей ни при чём.
        let probe = engine.diarize(&vec![0i16; RATE as usize], RATE);
        return match probe.refused {
            Some(reason) => report(vec![format!("движок отказался считать — {reason}")]),
            None => report(vec![
                "движок отвечает, а проверить его нечем: контрольных записей нет \
                 (скачать — scripts/fetch-diarize-models.sh)"
                    .to_string(),
            ]),
        };
    }

    let mut problems = Vec::new();
    for control in controls {
        let seconds = control.pcm.len() as f64 / f64::from(RATE);
        let once = engine.diarize(&control.pcm, RATE);
        if let Some(reason) = once.refused {
            problems.push(format!(
                "{}: движок отказался считать — {reason}",
                control.name
            ));
            continue;
        }

        // Тот же материал дважды подряд. Люди в нём по построению те же,
        // поэтому рост числа голосов означает, что движок делит человека,
        // а не что в записи кто-то появился. Контроль не требует второго
        // файла и не зависит от того, верна ли подпись на первом.
        let doubled: Vec<i16> = control
            .pcm
            .iter()
            .chain(control.pcm.iter())
            .copied()
            .collect();
        let twice = engine.diarize(&doubled, RATE);
        let twice_found = twice.refused.is_none().then_some(twice.speakers_found);

        println!(
            "  {:26} {:.1} с: в записи {} человек, движок нашёл {}{}",
            control.name,
            seconds,
            control.speakers,
            once.speakers_found,
            match twice_found {
                Some(found) => format!(", на удвоенной записи — {found}"),
                None => ", удвоенную посчитать не удалось".to_string(),
            }
        );

        if once.speakers_found < control.speakers {
            problems.push(format!(
                "{}: заведомо разные голоса слились в {} из {} — движок смены не видит",
                control.name, once.speakers_found, control.speakers
            ));
        }
        if once.speakers_found > control.speakers {
            println!(
                "    ! разорвал {} человек на {} — порог кластеризации не настроен под этот\n\
                 \x20     материал. Числа ниже читать с этим в уме; выбор порога — задача 3",
                control.speakers, once.speakers_found
            );
        }
        if let Some(found) = twice_found
            && found > once.speakers_found
        {
            println!(
                "    ! та же запись дважды дала {found} голосов вместо {} — число зависит от\n\
                 \x20     количества материала, а не только от того, кто говорит. Для встречи\n\
                 \x20     на час это значит больше дробления, чем на десяти минутах",
                once.speakers_found
            );
        }
    }
    report(problems)
}

/// Напечатать вердикт и вернуть его же вызывающему.
fn report(problems: Vec<String>) -> Vec<String> {
    if problems.is_empty() {
        println!("  ВЕРДИКТ: движок видит смену голоса, числам ниже можно верить");
    }
    for problem in &problems {
        println!("  ВЕРДИКТ: {problem}");
    }
    problems
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
    // Метки движка идут с пропусками: два голоса могут получить номера 0 и
    // 3. Число выше при этом верное — оно считается по разным меткам, а не
    // по наибольшей, — но читается «два голоса, а номер третий» как потеря.
    // Переименовывать метки прибор не станет: он показывает то, что отдал
    // движок. Списку голосов в интерфейсе (задача 6) плотная нумерация
    // понадобится, и делать её надо там, а не прятать здесь.
    if let Some(highest) = by_cluster.keys().next_back()
        && *highest as usize + 1 != by_cluster.len()
    {
        println!(
            "    (номера меток идут с пропусками — так их раздаёт движок; голосов\n\
             \x20    всё равно {})",
            by_cluster.len()
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

    /// Длительность каждого голоса в тестовой синтетике.
    const CASE_MS: u64 = 3_000;

    fn tmp_root(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "mr-diarize-probe-{name}-{:?}",
            std::thread::current().id()
        ))
    }

    /// Синтетический голос: основной тон с гармониками и слоговой
    /// огибающей.
    ///
    /// **Материал для двойников, а не для движка.** Настоящая модель
    /// голосов на тонах не работает — проверено первым же прогоном
    /// sherpa-onnx, счёвшего два разных тона одним голосом. Здесь тоны
    /// годятся ровно потому, что двойники ниже такие же игрушечные:
    /// проверяется логика вердикта, а не чьё-то умение различать людей.
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

    fn control(name: &str, speakers: u32, pcm: Vec<i16>) -> Control {
        Control {
            name: name.to_string(),
            speakers,
            pcm,
        }
    }

    /// Контроль с двумя заведомо разными тонами.
    fn two_voices() -> Control {
        control(
            "2-tones.wav",
            2,
            [voice(110.0, CASE_MS), voice(210.0, CASE_MS)].concat(),
        )
    }

    /// Контроль с одним тоном.
    fn one_voice() -> Control {
        control("1-tone.wav", 1, voice(110.0, CASE_MS * 2))
    }

    /// Заведомо рабочий двойник: контроль, а не модель.
    ///
    /// Считает частоту переходов через ноль по окнам и делит дорожку
    /// надвое, только если разброс достаточно велик. Существует, чтобы
    /// доказать, что вердикт **бывает зелёным**: вердикт, который красен
    /// всегда, не проверяет ничего.
    #[derive(Default)]
    struct PitchControl;

    impl PitchControl {
        const WINDOW_MS: u64 = 250;
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
                    chunk
                        .windows(2)
                        .filter(|pair| (pair[0] < 0) != (pair[1] < 0))
                        .count() as f32
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

    /// Двойник, для которого всё — один голос.
    struct NeverSplits;

    impl Diarizer for NeverSplits {
        fn diarize(&mut self, pcm: &[i16], sample_rate: u32) -> DiarizeReport {
            let ms = pcm.len() as u64 * 1_000 / u64::from(sample_rate);
            DiarizeReport::from_turns(vec![VoiceTurn::new(0, ms, 0)])
        }
    }

    /// Двойник, который делит всё пополам независимо от материала.
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

    /// Двойник, у которого число голосов растёт от количества материала:
    /// по голосу на каждые три секунды.
    struct MultipliesWithLength;

    impl Diarizer for MultipliesWithLength {
        fn diarize(&mut self, pcm: &[i16], sample_rate: u32) -> DiarizeReport {
            let ms = pcm.len() as u64 * 1_000 / u64::from(sample_rate);
            let count = (ms / 3_000).max(1);
            let turns = (0..count)
                .map(|index| VoiceTurn::new(index * 3_000, (index + 1) * 3_000, index as u32))
                .collect();
            DiarizeReport::from_turns(turns)
        }
    }

    fn problem(engine: &mut dyn Diarizer, controls: &[Control], needle: &str) {
        let problems = self_check(engine, controls);
        assert!(
            problems.iter().any(|line| line.contains(needle)),
            "вердикт не назвал «{needle}»: {problems:?}"
        );
    }

    /// Вердикт бывает зелёным. Без этого «прибор красный» ничего не
    /// значит — он мог бы быть красным всегда.
    #[test]
    fn the_self_check_passes_a_working_diarizer() {
        assert!(self_check(&mut PitchControl, &[two_voices(), one_voice()]).is_empty());
    }

    /// Движок, не видящий смены, — единственная настоящая слепота, и
    /// только она останавливает прибор.
    #[test]
    fn a_diarizer_that_never_splits_is_blind() {
        problem(&mut NeverSplits, &[two_voices()], "слились");
    }

    /// Заглушка отказывает, и её причина доезжает до вердикта.
    #[test]
    fn a_refusing_engine_stops_the_probe() {
        problem(
            &mut MockDiarizer::new(),
            &[two_voices()],
            "отказался считать",
        );
    }

    /// Контролей нет, а движок отвечает — судить его нечем, и прибор
    /// говорит именно это, а не «движок сломан».
    ///
    /// Ровно тот случай, из-за которого прибор переписан: синтетика
    /// объявляла работающий движок слепым.
    #[test]
    fn without_controls_a_working_engine_is_not_judged_blind() {
        let problems = self_check(&mut PitchControl, &[]);

        assert!(
            problems
                .iter()
                .any(|line| line.contains("проверить его нечем")),
            "{problems:?}"
        );
        assert!(
            !problems.iter().any(|line| line.contains("слились")),
            "движок объявлен слепым без материала: {problems:?}"
        );
    }

    /// Контролей нет и движка нет — ответ про движок, а не про контроли.
    #[test]
    fn without_controls_a_missing_engine_is_named_first() {
        problem(&mut MockDiarizer::new(), &[], "отказался считать");
    }

    /// Дробление — неточность настройки, а не слепота: прибор говорит о
    /// нём громко, но работать не мешает.
    ///
    /// Решение осознанное. Порог кластеризации и выбирается замером
    /// (задача 3), а отказ прибора при неверном пороге спрятал бы ровно те
    /// числа, по которым его выбирают.
    #[test]
    fn over_splitting_is_loud_but_not_fatal() {
        assert!(self_check(&mut AlwaysSplits, &[one_voice()]).is_empty());
        assert!(self_check(&mut MultipliesWithLength, &[two_voices()]).is_empty());
    }

    /// Синтетика для двойников обязана быть разной: если бы два «голоса»
    /// звучали одинаково, положительный случай проверял бы сам себя.
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

    /// Собрать WAV 16 кГц моно — материал для проверок загрузки контролей.
    fn wav_bytes(rate: u32, samples: &[i16]) -> Vec<u8> {
        let data: Vec<u8> = samples
            .iter()
            .flat_map(|sample| sample.to_le_bytes())
            .collect();
        let mut out = Vec::new();
        out.extend(b"RIFF");
        out.extend(((36 + data.len()) as u32).to_le_bytes());
        out.extend(b"WAVEfmt ");
        out.extend(16u32.to_le_bytes());
        out.extend(1u16.to_le_bytes());
        out.extend(1u16.to_le_bytes());
        out.extend(rate.to_le_bytes());
        out.extend((rate * 2).to_le_bytes());
        out.extend(2u16.to_le_bytes());
        out.extend(16u16.to_le_bytes());
        out.extend(b"data");
        out.extend((data.len() as u32).to_le_bytes());
        out.extend(data);
        out
    }

    fn put_control(root: &Path, name: &str, rate: u32, samples: &[i16]) {
        let dir = controls_dir(root);
        std::fs::create_dir_all(&dir).expect("каталог");
        std::fs::write(dir.join(name), wav_bytes(rate, samples)).expect("файл");
    }

    /// Число людей читается из имени файла, а не угадывается по звуку.
    #[test]
    fn the_expected_count_comes_from_the_file_name() {
        let root = tmp_root("names");
        let _ = std::fs::remove_dir_all(&root);
        put_control(&root, "2-two-speakers.wav", RATE, &[1, 2, 3, 4]);
        put_control(&root, "4-four-speakers.wav", RATE, &[5, 6]);

        let controls = load_controls(&root).expect("контроли");

        assert_eq!(controls.len(), 2, "прочлись не все");
        assert_eq!(controls[0].speakers, 2);
        assert_eq!(controls[1].speakers, 4);
        assert_eq!(controls[0].pcm, vec![1, 2, 3, 4], "звук прочёлся не весь");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Файл без числа в имени — отказ, а не тихий пропуск: судить движок
    /// по неполному набору и не сказать об этом хуже, чем не судить.
    #[test]
    fn a_control_without_a_count_is_refused() {
        let root = tmp_root("noname");
        let _ = std::fs::remove_dir_all(&root);
        put_control(&root, "какая-то-запись.wav", RATE, &[1, 2]);

        let error = load_controls(&root).expect_err("имя без числа");

        assert!(error.contains("числа людей"), "{error}");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Чужая частота — отказ. Контроль в 48 кГц проверял бы не тот звук,
    /// которым записан живой путь.
    #[test]
    fn a_control_at_the_wrong_rate_is_refused() {
        let root = tmp_root("rate");
        let _ = std::fs::remove_dir_all(&root);
        put_control(&root, "2-wrong-rate.wav", 48_000, &[1, 2]);

        let error = load_controls(&root).expect_err("чужая частота");

        assert!(error.contains("48000"), "{error}");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Каталога нет — пустой список, а не ошибка: без движка контроли и
    /// не нужны, решает это вердикт.
    #[test]
    fn a_missing_control_dir_is_not_an_error() {
        let root = tmp_root("nodir");
        let _ = std::fs::remove_dir_all(&root);

        assert!(load_controls(&root).expect("не ошибка").is_empty());
    }

    fn bytes_of(pcm: &[i16]) -> Vec<u8> {
        pcm.iter().flat_map(|sample| sample.to_le_bytes()).collect()
    }

    /// Сессия с записанной дорожкой: чанки по 100 мс, как в живом пути.
    fn seed(root: &Path, session_id: &str, pcm: &[i16]) {
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
