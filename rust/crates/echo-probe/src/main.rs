//! Прибор для детектора эха (Epic 16).
//!
//! `detect_echo` живёт в `stt` и вызывается только из своих тестов, то
//! есть на синтетике. Порог `ECHO_EXPLAINED` при этом взят по ней же:
//! на синтетике зазор между эхом и своей речью — 0.60 против ~0.00, а
//! вживую он будет уже. Насколько уже — неизвестно, потому что прогнать
//! детектор по настоящей записи было **неоткуда**.
//!
//! Прибор эту дыру и закрывает: берёт сохранённые дорожки сессии
//! (ADR-006), гоняет по ним детектор и печатает решения по окнам так,
//! чтобы человек мог сверить их с тем, что на записи слышит.
//!
//! Каждый запуск начинается с заведомо положительного случая и только
//! потом трогает настоящие данные. Правило про это писано кровью:
//! `scripts/count-audio-taps.swift` показал ноль tap'ов, ноль прочли как
//! «утечки нет», а скрипт был слеп (`CLAUDE.md`). Ноль помеченных окон
//! от слепого прибора выглядит точно так же, как ноль от чистой записи.

use std::path::Path;
use std::process::ExitCode;

use domain::AudioChannel;
use storage::AudioManifestStore;
use stt::{EchoReport, detect_echo};

/// Частота живого пути; ею же пишутся чанки на диск (ADR-005).
const SELF_CHECK_RATE: u32 = 16_000;
/// Задержка синтетического эха: 75 мс, как в тестах `stt`.
const SELF_CHECK_DELAY: usize = 1_200;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();

    // Сперва прибор, потом данные. Обратный порядок позволил бы прочитать
    // чистый ноль там, где детектор просто ничего не умеет.
    if !self_check() {
        eprintln!("\nПрибор слеп: до настоящих данных дело не дошло.");
        return ExitCode::FAILURE;
    }

    match args.as_slice() {
        [] => {
            println!("\n{USAGE}");
            ExitCode::SUCCESS
        }
        [root] => match list_sessions(Path::new(root)) {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("{error}");
                ExitCode::FAILURE
            }
        },
        [root, session] => match probe(Path::new(root), session) {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("{error}");
                ExitCode::FAILURE
            }
        },
        _ => {
            eprintln!("{USAGE}");
            ExitCode::FAILURE
        }
    }
}

const USAGE: &str = "\
Использование:
  echo-probe <каталог-данных>             — список сессий с обоими каналами
  echo-probe <каталог-данных> <сессия>    — прогнать детектор по сессии

Каталог данных — тот, где лежит meetingraft.sqlite3.

Гонять сборкой --release: фильтр отклика — 1024 умножения на отсчёт, и
в debug минута записи считается минутами же.
  cargo run --release -p meetingraft-echo-probe -- <каталог> <сессия>";

/// Заведомо положительный и заведомо отрицательный случаи.
///
/// Возвращает `false`, если детектор не увидел эха там, где оно заведомо
/// есть, или увидел там, где его заведомо нет. Печатает оба числа: важен
/// не только вердикт, но и **зазор** между случаями — он же и есть та
/// величина, ради которой прибор заводился.
fn self_check() -> bool {
    let seconds = SELF_CHECK_RATE as usize;
    let system = speechlike(seconds * 8, 1);

    // Заведомо есть: микрофон слышит только динамики.
    let echo = echo_of(&system, SELF_CHECK_DELAY, 0.4);
    let positive = detect_echo(&echo, &system, SELF_CHECK_RATE);

    // Заведомо нет: владелец говорит сам, собеседник молчит. Первая
    // половина всё же эхо — иначе задержка не найдётся, детектор выйдет
    // раньше окон, и «ноль помеченных» получится из пустоты.
    let mut own = echo.clone();
    own[seconds * 4..].copy_from_slice(&speechlike(seconds * 4, 7));
    let negative = detect_echo(&own, &system, SELF_CHECK_RATE);
    let late: Vec<_> = negative
        .windows
        .iter()
        .filter(|window| window.start_ms >= 4_000)
        .collect();

    println!("Проверка прибора на синтетике");
    println!(
        "  эхо есть:  задержка {} мс (заложено {} мс), корреляция {:.2}, помечено {} окон из {}, explained медиана {:.2}",
        positive.delay_ms,
        SELF_CHECK_DELAY * 1_000 / SELF_CHECK_RATE as usize,
        positive.delay_correlation,
        positive.echo_windows(),
        positive.windows.len(),
        median(positive.windows.iter().map(|w| w.explained)),
    );
    println!(
        "  эха нет:   помечено {} окон из {}, explained медиана {:.2}",
        late.iter().filter(|window| window.is_echo).count(),
        late.len(),
        median(late.iter().map(|w| w.explained)),
    );

    let mut ok = true;
    if positive.windows.is_empty() || late.is_empty() {
        println!("  ВЕРДИКТ: окон нет вовсе — проверять было нечего");
        ok = false;
    }
    if positive.echo_windows() * 2 < positive.windows.len() {
        println!("  ВЕРДИКТ: эхо не опознано там, где оно заведомо есть");
        ok = false;
    }
    if late.iter().any(|window| window.is_echo) {
        println!("  ВЕРДИКТ: своя речь принята за эхо — ложные пометки стоят реплик");
        ok = false;
    }
    if ok {
        println!("  ВЕРДИКТ: прибор различает оба случая, числам ниже можно верить");
    }
    ok
}

fn list_sessions(root: &Path) -> Result<(), String> {
    let store = AudioManifestStore::open(root).map_err(|error| error.to_string())?;
    let meetings = store
        .list_meeting_summaries()
        .map_err(|error| error.to_string())?;
    if meetings.is_empty() {
        return Err("встреч в базе нет".to_string());
    }

    println!("\nСессии (каналы: кадров на дорожку)");
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
        // Обе дорожки обязательны: детектору нечего сравнивать с одной. И
        // общее время обязательно: без него сравнение смещено на
        // неизвестную величину (Epic 25).
        let unified = store
            .channel_clock_unified(&meeting.id)
            .map_err(|error| error.to_string())?
            .unwrap_or(false);
        let mark = if mic > 0 && system > 0 && unified {
            "+"
        } else {
            " "
        };
        let clock = if unified {
            ""
        } else {
            ", метки каналов не сведены"
        };
        println!(
            "  {mark} {} — mic {mic}, system {system}{clock}",
            meeting.id
        );
    }
    println!("\nСтрока с «+» годится для прогона: echo-probe <каталог> <сессия>");
    println!(
        "Записи без общего времени каналов прибор не судит вовсе: сдвиг их\n\
         старта неизвестен, а искать эхо он умеет только в пределах 250 мс."
    );
    Ok(())
}

fn probe(root: &Path, session_id: &str) -> Result<(), String> {
    let store = AudioManifestStore::open(root).map_err(|error| error.to_string())?;
    let analysis = analyze(&store, session_id)?;

    println!("\nСессия {session_id}, {} Гц", analysis.rate);
    println!(
        "  mic {:.1} с, system {:.1} с",
        seconds(analysis.mic_frames, analysis.rate),
        seconds(analysis.system_frames, analysis.rate)
    );
    if analysis.offset_ms != 0 {
        println!(
            "  системная дорожка началась на {} мс позже микрофонной — выровнено\n\
             \x20 перед поиском сдвига: детектор ищет в пределах 250 мс, а старт\n\
             \x20 каналов расходится на секунды",
            analysis.offset_ms
        );
    }

    print_report(&analysis.report);
    Ok(())
}

/// Что вышло по сессии: сам отчёт и то, из чего он посчитан.
#[derive(Debug)]
struct Analysis {
    rate: u32,
    mic_frames: usize,
    system_frames: usize,
    /// На сколько системная дорожка стартовала позже микрофонной.
    offset_ms: i64,
    report: EchoReport,
}

/// Чтение дорожек и прогон детектора, без печати.
///
/// Отдельно от вывода, чтобы половину прибора, которая ходит в базу,
/// можно было проверить тестом. Непроверенная половина прибора — то же
/// самое, что непроверенный прибор целиком.
fn analyze(store: &AudioManifestStore, session_id: &str) -> Result<Analysis, String> {
    // Отказ до чтения дорожек: на записи с несведёнными метками у прибора
    // нет ответа, и печатать вместо ответа число — хуже, чем не печатать
    // ничего. Именно так и вышло на `6CE19EC5`: 0.09 приняли за «эха нет»,
    // а выравнивать было нечем (Epic 25).
    require_common_clock(
        store
            .channel_clock_unified(session_id)
            .map_err(|error| error.to_string())?,
        session_id,
    )?;

    let rate = session_sample_rate(store, session_id)?;
    let mut mic = store
        .read_session_pcm(session_id, AudioChannel::Mic)
        .map_err(|error| error.to_string())?;
    let mut system = store
        .read_session_pcm(session_id, AudioChannel::System)
        .map_err(|error| error.to_string())?;

    // Непустота входа — отдельным утверждением до любых выводов о
    // результате: детектор на пустой дорожке молча отдаёт пустой отчёт.
    if mic.is_empty() || system.is_empty() {
        return Err(format!(
            "дорожки пусты (mic {}, system {}) — мерить нечего",
            mic.len(),
            system.len()
        ));
    }

    // Дорожки начинаются в разное время: системный tap поднимается позже
    // микрофона. `read_session_pcm` склеивает чанки канала подряд, и
    // нулевой отсчёт двух дорожек — **не** один и тот же момент.
    //
    // Детектор ищет сдвиг в пределах 250 мс, а старт расходится на
    // секунды. Без выравнивания он честно не находит ничего и печатает
    // отказ — то есть прибор молчит там, где эхо может быть.
    let offset_ms = start_offset_ms(store, session_id)?;
    let offset = (offset_ms.unsigned_abs() as usize) * rate as usize / 1_000;
    if offset_ms > 0 {
        system.splice(0..0, std::iter::repeat_n(0i16, offset));
    } else if offset_ms < 0 {
        mic.splice(0..0, std::iter::repeat_n(0i16, offset));
    }

    Ok(Analysis {
        rate,
        mic_frames: mic.len(),
        system_frames: system.len(),
        offset_ms,
        report: detect_echo(&mic, &system, rate),
    })
}

/// Годится ли запись для сравнения дорожек между собой (Epic 25).
///
/// Отдельной функцией от чтения базы, потому что решение здесь и есть
/// предмет проверки: до Epic 25 оба канала помечали своё начало нулём, и
/// выравнивание по манифесту было выравниванием по неправде.
fn require_common_clock(unified: Option<bool>, session_id: &str) -> Result<(), String> {
    match unified {
        Some(true) => Ok(()),
        // «Сессии нет» и «метки не сведены» — разные отказы. Первый значит
        // «сравнивать нечего», второй — «сравнивать можно, но со сдвигом
        // на неизвестную величину».
        None => Err(format!(
            "сессии {session_id} нет в базе — сравнивать нечего"
        )),
        Some(false) => Err(format!(
            "метки каналов сессии {session_id} не сведены к общему времени (Epic 25).\n\
             Прибор не судит: детектор ищет сдвиг в пределах 250 мс, а старт\n\
             каналов у таких записей расходится на секунды. Величина сдвига\n\
             нигде не записана, восстановить её нечем, и подогнать дорожки\n\
             значило бы придумать ответ. Нужна новая запись."
        )),
    }
}

/// На сколько системная дорожка стартовала позже микрофонной, мс.
///
/// Положительное — system позже; отрицательное — раньше. Считается по
/// меткам первых чанков манифеста, потому что другого общего времени у
/// дорожек нет.
fn start_offset_ms(store: &AudioManifestStore, session_id: &str) -> Result<i64, String> {
    let chunks = store
        .list_chunks(session_id)
        .map_err(|error| error.to_string())?;
    let first = |channel: AudioChannel| -> Option<u64> {
        chunks
            .iter()
            .filter(|chunk| chunk.channel == channel)
            .map(|chunk| chunk.timestamp_ms)
            .min()
    };
    match (first(AudioChannel::Mic), first(AudioChannel::System)) {
        (Some(mic), Some(system)) => Ok(system as i64 - mic as i64),
        _ => Err("у одной из дорожек нет чанков — выравнивать нечем".to_string()),
    }
}

fn print_report(report: &EchoReport) {
    if report.windows.is_empty() {
        // Разница между «искали и не нашли» и «искать было нечем» —
        // это разница между ответом и отказом, и она в самом числе.
        if report.delay_correlation > 0.0 {
            println!(
                "  сдвиг не опознан: лучшая корреляция {:.2} при пороге 0.30",
                report.delay_correlation
            );
            println!(
                "  сравнивать было с чем — совпадения между дорожками нет. Так\n\
                 \x20 выглядит разговор в наушниках: динамики микрофон не слышал.\n\
                 \x20 Эхо дало бы корреляцию заметно выше порога."
            );
        } else {
            println!("  сдвиг не искался: корреляции нет вовсе");
            println!(
                "  это не «эха нет», а отказ: на одной из дорожек нечего было\n\
                 \x20 сопоставлять — тишина или слишком короткий кусок."
            );
        }
        return;
    }

    println!(
        "  задержка {} мс, корреляция {:.2}",
        report.delay_ms, report.delay_correlation
    );

    // Тихие окна не показываем: решение по ним предопределено порогом
    // громкости, и в таблице они только прячут остальное.
    let loud: Vec<_> = report
        .windows
        .iter()
        .filter(|window| window.mic_rms >= 120.0)
        .collect();
    println!(
        "  окон {}, из них громких {}, помечено эхом {}",
        report.windows.len(),
        loud.len(),
        report.echo_windows()
    );
    if loud.is_empty() {
        println!("  громких окон нет — на записи тишина, зазор мерить не на чем");
        return;
    }

    println!("\n  время, с      mic_rms  explained  эхо");
    for window in &loud {
        println!(
            "  {:>6.1}–{:<6.1} {:>7.0}  {:>9.2}  {}",
            window.start_ms as f64 / 1_000.0,
            window.end_ms as f64 / 1_000.0,
            window.mic_rms,
            window.explained,
            if window.is_echo { "да" } else { "" }
        );
    }

    println!(
        "\n  explained по громким окнам: медиана {:.2}, минимум {:.2}, максимум {:.2}",
        median(loud.iter().map(|w| w.explained)),
        loud.iter().map(|w| w.explained).fold(f32::MAX, f32::min),
        loud.iter().map(|w| w.explained).fold(f32::MIN, f32::max)
    );
    println!(
        "  Смотреть надо не на пометки, а на **зазор**: где говорил только\n\
         \x20 собеседник, explained обязан быть высоким, где владелец — низким.\n\
         \x20 Граница между этими группами и есть настоящий порог; сейчас в коде\n\
         \x20 стоит 0.50, взятое по синтетике."
    );
}

/// Частота дорожек сессии. Разнобой — отказ: детектор берёт одну.
fn session_sample_rate(store: &AudioManifestStore, session_id: &str) -> Result<u32, String> {
    let chunks = store
        .list_chunks(session_id)
        .map_err(|error| error.to_string())?;
    let mut rates: Vec<u32> = chunks.iter().map(|chunk| chunk.sample_rate).collect();
    rates.sort_unstable();
    rates.dedup();
    match rates.as_slice() {
        [] => Err(format!("у сессии {session_id} нет чанков")),
        [rate] => Ok(*rate),
        many => Err(format!("частоты чанков разошлись: {many:?}")),
    }
}

fn seconds(frames: usize, rate: u32) -> f64 {
    frames as f64 / f64::from(rate)
}

fn median(values: impl Iterator<Item = f32>) -> f32 {
    let mut values: Vec<f32> = values.collect();
    if values.is_empty() {
        return 0.0;
    }
    values.sort_by(f32::total_cmp);
    values[values.len() / 2]
}

/// Речеподобный сигнал: шум под огибающей слогов.
///
/// Шум, а не синус: два независимых синуса на близких частотах
/// коррелируют, и отрицательный случай проходил бы по случайности.
///
/// Повторяет генератор из тестов `stt::echo` — прибор обязан стоять на
/// своих ногах, а тестовые вспомогательные функции наружу не выведены.
fn speechlike(frames: usize, seed: u32) -> Vec<i16> {
    let mut noise = seed | 1;
    (0..frames)
        .map(|n| {
            noise = noise.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let sample = ((noise >> 16) as i32 - 32_768) as f32 / 32_768.0;
            let envelope = {
                let t = n as f32 / SELF_CHECK_RATE as f32;
                0.4 + 0.6 * (2.0 * std::f32::consts::PI * 2.5 * t).sin().abs()
            };
            (sample * envelope * 6_000.0) as i16
        })
        .collect()
}

/// Эхо так, как его слышит микрофон: отражения, сглаживание и свой шум.
///
/// Идеальная задержанная копия дала бы корреляцию ровно 1.0, и проверка
/// прибора прошла бы при любом пороге вплоть до единицы.
fn echo_of(source: &[i16], delay: usize, attenuation: f32) -> Vec<i16> {
    let taps = [(0usize, 1.0f32), (137, 0.45), (289, 0.2), (523, 0.1)];
    let mut wide = vec![0f32; source.len()];
    for (offset, gain) in taps {
        let shift = delay + offset;
        for index in shift..source.len() {
            wide[index] += f32::from(source[index - shift]) * gain * attenuation;
        }
    }
    let mut noise = 0x9E37_79B9u32;
    (0..wide.len())
        .map(|index| {
            let smoothed = if index >= 2 {
                (wide[index] + wide[index - 1] + wide[index - 2]) / 3.0
            } else {
                wide[index]
            };
            noise = noise.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let hiss = ((noise >> 16) as i32 - 32_768) as f32 / 32_768.0 * 200.0;
            (smoothed + hiss).clamp(-32_768.0, 32_767.0) as i16
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_root(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "mr-echo-probe-{name}-{:?}",
            std::thread::current().id()
        ))
    }

    fn bytes_of(pcm: &[i16]) -> Vec<u8> {
        pcm.iter().flat_map(|sample| sample.to_le_bytes()).collect()
    }

    /// Сессия с записанными дорожками: ровно то, что даёт живой путь.
    fn seed(root: &std::path::Path, session_id: &str, mic: &[i16], system: &[i16]) {
        let mut store = AudioManifestStore::open(root).expect("store");
        store
            .begin_session(session_id, 0, "проба")
            .expect("session");
        if !mic.is_empty() {
            store
                .append_chunk(AudioChannel::Mic, &bytes_of(mic), SELF_CHECK_RATE, 0)
                .expect("mic");
        }
        if !system.is_empty() {
            store
                .append_chunk(AudioChannel::System, &bytes_of(system), SELF_CHECK_RATE, 0)
                .expect("system");
        }
        store.end_session(1_000).expect("end");
    }

    /// Половина прибора, ходящая в базу, проверяется тем же заведомо
    /// положительным случаем, что и половина, считающая сигнал.
    ///
    /// Иначе «ноль помеченных окон» на настоящей записи ничего не значил
    /// бы: дорожки могли не прочитаться вовсе, и выглядело бы это точно
    /// так же, как чистая запись.
    #[test]
    fn reads_stored_tracks_and_finds_a_known_echo() {
        let root = tmp_root("known-echo");
        let _ = std::fs::remove_dir_all(&root);
        let seconds = SELF_CHECK_RATE as usize;
        let system = speechlike(seconds * 8, 1);
        let mic = echo_of(&system, SELF_CHECK_DELAY, 0.4);
        seed(&root, "s1", &mic, &system);

        let store = AudioManifestStore::open(&root).expect("store");
        let analysis = analyze(&store, "s1").expect("разбор");

        assert_eq!(analysis.rate, SELF_CHECK_RATE);
        assert_eq!(analysis.mic_frames, mic.len(), "дорожка прочлась не вся");
        assert_eq!(analysis.system_frames, system.len());
        assert!(
            !analysis.report.windows.is_empty(),
            "окон нет — проверять нечего"
        );
        assert!(
            analysis.report.echo_windows() * 2 >= analysis.report.windows.len(),
            "заведомое эхо не найдено на сохранённых дорожках: {} из {}",
            analysis.report.echo_windows(),
            analysis.report.windows.len()
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Запись без общего времени каналов прибор не судит вовсе.
    ///
    /// Три случая вместе: без положительного утверждение «отказывает»
    /// выполнялось бы и функцией, которая отказывает всегда.
    #[test]
    fn a_recording_without_a_common_clock_gets_no_verdict() {
        assert!(require_common_clock(Some(true), "s1").is_ok());

        let old = require_common_clock(Some(false), "s1").expect_err("отказ");
        assert!(old.contains("не сведены"), "{old}");
        assert!(old.contains("не судит"), "{old}");

        let missing = require_common_clock(None, "s1").expect_err("отказ");
        assert!(
            missing.contains("нет в базе"),
            "«сессии нет» смешано с «не сведены»: {missing}"
        );
    }

    /// Прогон настоящей сессии через guard проходит: `begin_session`
    /// помечает новую запись сведённой сам.
    #[test]
    fn a_fresh_session_passes_the_clock_check() {
        let root = tmp_root("fresh-clock");
        let _ = std::fs::remove_dir_all(&root);
        let seconds = SELF_CHECK_RATE as usize;
        let system = speechlike(seconds * 8, 1);
        let mic = echo_of(&system, SELF_CHECK_DELAY, 0.4);
        seed(&root, "s1", &mic, &system);

        let store = AudioManifestStore::open(&root).expect("store");
        assert_eq!(
            store.channel_clock_unified("s1").expect("флаг"),
            Some(true),
            "новая запись обязана быть помечена сведённой"
        );
        assert!(
            analyze(&store, "s1").is_ok(),
            "guard не пустил свежую запись"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Одна дорожка — отказ, а не пустой отчёт.
    ///
    /// `detect_echo` на пустом системном канале молча отдаёт ноль окон, и
    /// прочитать это как «эха нет» было бы ровно той ошибкой, ради
    /// которой прибор и заводился.
    #[test]
    fn a_missing_channel_is_refused_out_loud() {
        let root = tmp_root("one-channel");
        let _ = std::fs::remove_dir_all(&root);
        seed(&root, "s1", &speechlike(SELF_CHECK_RATE as usize, 1), &[]);

        let store = AudioManifestStore::open(&root).expect("store");
        let error = analyze(&store, "s1").expect_err("одна дорожка — отказ");

        assert!(error.contains("пусты"), "{error}");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Системный tap поднимается позже микрофона, и без выравнивания
    /// заведомое эхо не находится вовсе: детектор ищет сдвиг в пределах
    /// 250 мс, а старт расходится на секунду.
    ///
    /// Это и был дефект, из-за которого прибор отказался считать
    /// настоящую встречу.
    #[test]
    fn a_late_system_tap_does_not_hide_the_echo() {
        let root = tmp_root("late-tap");
        let _ = std::fs::remove_dir_all(&root);
        let seconds = SELF_CHECK_RATE as usize;
        let system = speechlike(seconds * 8, 1);
        let mic = echo_of(&system, SELF_CHECK_DELAY, 0.4);

        // Tap поднялся на секунду позже: первой секунды системного звука
        // на диске нет вовсе, а в микрофоне её эхо есть — так и выглядит
        // настоящая запись.
        let late_by_ms = 1_000u64;
        let late_frames = SELF_CHECK_RATE as usize * late_by_ms as usize / 1_000;
        {
            let mut store = AudioManifestStore::open(&root).expect("store");
            store.begin_session("s1", 0, "проба").expect("session");
            store
                .append_chunk(AudioChannel::Mic, &bytes_of(&mic), SELF_CHECK_RATE, 0)
                .expect("mic");
            store
                .append_chunk(
                    AudioChannel::System,
                    &bytes_of(&system[late_frames..]),
                    SELF_CHECK_RATE,
                    late_by_ms,
                )
                .expect("system");
            store.end_session(1_000).expect("end");
        }

        let store = AudioManifestStore::open(&root).expect("store");
        let analysis = analyze(&store, "s1").expect("разбор");

        assert_eq!(
            analysis.offset_ms, late_by_ms as i64,
            "сдвиг старта не замечен"
        );
        assert!(
            !analysis.report.windows.is_empty(),
            "заведомое эхо не найдено после выравнивания"
        );
        assert!(
            analysis.report.echo_windows() * 2 >= analysis.report.windows.len(),
            "помечено {} окон из {}",
            analysis.report.echo_windows(),
            analysis.report.windows.len()
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Заведомо положительный случай для самого вердикта прибора.
    #[test]
    fn the_self_check_passes_on_a_healthy_detector() {
        assert!(self_check());
    }
}
