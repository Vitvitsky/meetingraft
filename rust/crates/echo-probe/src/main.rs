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

use domain::{AudioChannel, SpeakerSource};
use storage::AudioManifestStore;
use stt::{EchoReport, detect_echo};

/// Частота живого пути; ею же пишутся чанки на диск (ADR-005).
/// Ниже этого RMS окно считается тихим: решение по нему предопределено, и
/// в зазоре оно только размывает обе группы.
const LOUD_RMS: f32 = 120.0;

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

    // Зазор — отдельным разделом и после отчёта: он требует разметки, и
    // его отказ не должен выглядеть отказом всего прибора.
    match labelled_speech(&store, session_id) {
        Ok(speech) => {
            let loud: Vec<&stt::EchoWindow> = analysis
                .report
                .windows
                .iter()
                .filter(|window| window.mic_rms >= LOUD_RMS)
                .collect();
            print_gap(&loud, &speech);
        }
        Err(reason) => {
            println!("\n  Зазор между эхом и своей речью не построен");
            println!("    {reason}");
        }
    }
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
        .filter(|window| window.mic_rms >= LOUD_RMS)
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

    println!("\n  время, с      mic_rms  sys_rms  explained  эхо");
    for window in &loud {
        println!(
            "  {:>6.1}–{:<6.1} {:>7.0}  {:>7.0}  {:>9.2}  {}",
            window.start_ms as f64 / 1_000.0,
            window.end_ms as f64 / 1_000.0,
            window.mic_rms,
            window.system_rms,
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

/// Отрезки речи, размеченные человеком: владельца отдельно от остальных.
///
/// Ради этого разделения раздел и существует. Медиана `explained` по всем
/// громким окнам смешивает две разные вещи — эхо чужой речи и настоящую
/// речь владельца, — и потому не значит ничего. Настоящий порог живёт в
/// **зазоре** между ними, а зазор без разметки не построить.
#[derive(Debug, Default, PartialEq)]
struct LabelledSpeech {
    /// Имя владельца машины.
    owner: String,
    /// Куски, где говорил владелец, — в них `mic` несёт его речь.
    owner_spans: Vec<(u64, u64)>,
    /// Куски, где говорили остальные, — в них `mic` несёт эхо динамиков.
    others_spans: Vec<(u64, u64)>,
}

/// Кто из размеченных — владелец машины.
///
/// Выводится, а не спрашивается: в системный tap попадает **только**
/// устройство вывода (`SystemAudioCapture.createAggregate`), микрофона в
/// нём нет по построению. Значит человек, размеченный на `mic` и ни разу
/// на `system`, физически не может быть участником созвона — он в комнате.
///
/// Кандидатов не ровно один — **отказ, а не выбор большинством**. Двое
/// означают, что кого-то из удалённых просто не разметили на `system`, и
/// тогда любой ответ здесь будет угадыванием: прибор, угадавший владельца,
/// померит зазор между не теми группами и напечатает уверенное число.
fn derive_owner(on_mic: &[String], on_system: &[String]) -> Result<String, String> {
    let mut candidates: Vec<&String> = on_mic
        .iter()
        .filter(|name| !on_system.contains(name))
        .collect();
    candidates.sort();
    candidates.dedup();

    match candidates.as_slice() {
        [owner] => Ok((*owner).clone()),
        [] => Err(
            "владельца не видно: все размеченные на mic встречаются и на system.\n\
             \x20   Так выглядит встреча, где владелец не сказал ни слова, либо\n\
             \x20   разметка на mic не покрывает его реплик"
                .to_string(),
        ),
        many => Err(format!(
            "кандидатов в владельцы {}: {}. Их не может быть двое — микрофона в\n\
             \x20   системном tap'е нет по построению, и каждый удалённый участник\n\
             \x20   обязан быть размечен на system. Значит кого-то там не разметили,\n\
             \x20   и делить окна пока не на что",
            many.len(),
            many.iter()
                .map(|name| name.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        )),
    }
}

/// Разложить громкие окна по двум группам.
///
/// Окно, попавшее и туда и сюда, **не относится ни к одной**: там говорили
/// оба, и `explained` в нём смешан ровно так же, как в общей медиане.
/// Отнести его к большей доле значило бы вернуть ту же болезнь под другим
/// именем.
fn split_windows<'a>(
    windows: &[&'a stt::EchoWindow],
    speech: &LabelledSpeech,
) -> (Vec<&'a stt::EchoWindow>, Vec<&'a stt::EchoWindow>, usize) {
    let overlaps = |spans: &[(u64, u64)], window: &stt::EchoWindow| {
        spans
            .iter()
            .any(|(from, to)| window.start_ms < *to && *from < window.end_ms)
    };

    let mut owner = Vec::new();
    let mut others = Vec::new();
    let mut both = 0usize;
    for window in windows {
        match (
            overlaps(&speech.owner_spans, window),
            overlaps(&speech.others_spans, window),
        ) {
            (true, true) => both += 1,
            (true, false) => owner.push(*window),
            (false, true) => others.push(*window),
            (false, false) => {}
        }
    }
    (owner, others, both)
}

/// Размеченные человеком реплики последней версии Final.
///
/// Берётся только `SpeakerSource::Human`: подпись по каналу повторяет то,
/// что и так известно из дорожки, а подпись слепком — догадка модели.
/// Строить по ним зазор значило бы мерить прибор прибором.
fn labelled_speech(store: &AudioManifestStore, session_id: &str) -> Result<LabelledSpeech, String> {
    let versions = store
        .list_final_transcripts(session_id)
        .map_err(|error| error.to_string())?;
    let latest = versions
        .first()
        .ok_or_else(|| "у встречи нет собранного Final — размечать было негде".to_string())?;
    let names: std::collections::HashMap<String, String> = store
        .list_speakers(session_id)
        .map_err(|error| error.to_string())?
        .into_iter()
        .map(|speaker| (speaker.id, speaker.display_name))
        .collect();
    let segments = store
        .list_final_segments(session_id, latest.version)
        .map_err(|error| error.to_string())?;

    let named = |segment: &domain::FinalSegment| -> Option<String> {
        if segment.speaker_source != SpeakerSource::Human {
            return None;
        }
        names
            .get(&segment.speaker_id)
            .map(|name| name.trim().to_owned())
            .filter(|name| !name.is_empty())
    };

    let on_mic: Vec<String> = segments
        .iter()
        .filter(|segment| segment.channel == AudioChannel::Mic)
        .filter_map(named)
        .collect();
    if on_mic.is_empty() {
        return Err(
            "на дорожке mic нет ни одной реплики, подписанной человеком — делить\n\
             \x20   окна нечем"
                .to_string(),
        );
    }
    let on_system: Vec<String> = segments
        .iter()
        .filter(|segment| segment.channel == AudioChannel::System)
        .filter_map(named)
        .collect();

    let owner = derive_owner(&on_mic, &on_system)?;
    let mut speech = LabelledSpeech {
        owner: owner.clone(),
        ..LabelledSpeech::default()
    };
    for segment in segments
        .iter()
        .filter(|segment| segment.channel == AudioChannel::Mic)
    {
        let Some(name) = named(segment) else { continue };
        let span = (segment.start_ms, segment.end_ms);
        if name == owner {
            speech.owner_spans.push(span);
        } else {
            speech.others_spans.push(span);
        }
    }
    Ok(speech)
}

/// Напечатать зазор — то, ради чего прибор и заводился.
fn print_gap(windows: &[&stt::EchoWindow], speech: &LabelledSpeech) {
    let (owner, others, both) = split_windows(windows, speech);
    println!("\n  Зазор между эхом и своей речью (по разметке человека)");
    println!(
        "    владелец: «{}» — размечен на mic и ни разу на system",
        speech.owner
    );
    if owner.is_empty() || others.is_empty() {
        // Одна группа пуста — зазора нет по построению, и печатать вместо
        // него медиану второй группы значило бы выдать половину за целое.
        println!(
            "    делить нечего: окон со речью владельца {}, окон с речью остальных {}.\n\
             \x20   Зазор строится между двумя группами, и одной из них нет",
            owner.len(),
            others.len()
        );
        return;
    }

    // Опора отдельной строкой у каждой группы, и это не украшение.
    // `explained` при молчащем системном канале не значит «эха нет» — он
    // значит «отражать было нечего», и сравнивать такую долю с долей,
    // посчитанной при звучащей опоре, нельзя вовсе. Пока этих двух чисел
    // не было рядом, зазор читался как свойство эха, а мог быть свойством
    // тишины.
    let owner_median = median(owner.iter().map(|w| w.explained));
    let others_median = median(others.iter().map(|w| w.explained));
    println!(
        "    речь владельца:   {:>4} окон, explained медиана {:.2}, опора медиана {:.0}",
        owner.len(),
        owner_median,
        median(owner.iter().map(|w| w.system_rms))
    );
    println!(
        "    речь остальных:   {:>4} окон, explained медиана {:.2}, опора медиана {:.0}",
        others.len(),
        others_median,
        median(others.iter().map(|w| w.system_rms))
    );
    println!("    смешанных окон:   {both:>4} — говорили оба, в счёт не идут");
    println!(
        "    зазор {:+.2} (остальные минус владелец)",
        others_median - owner_median
    );

    // Порог решает не медиана, а то, чем платят за ошибку. Цена
    // несимметрична (ADR-014): лишний текст человек видит и стирает, а не
    // распознанную реплику владельца он не увидит никогда. Поэтому таблица
    // называет обе ошибки раздельно, а не одну «точность».
    println!("\n     порог  речи владельца съедено  эха осталось");
    for threshold in [0.10_f32, 0.20, 0.30, 0.40, 0.50, 0.60, 0.70, 0.80] {
        let eaten = owner.iter().filter(|w| w.explained >= threshold).count();
        let missed = others.iter().filter(|w| w.explained < threshold).count();
        println!(
            "      {threshold:.2} {:>21} {:>13}",
            format!("{:.0}%", 100.0 * eaten as f64 / owner.len() as f64),
            format!("{:.0}%", 100.0 * missed as f64 / others.len() as f64),
        );
    }
    println!(
        "    Левый столбец — дорогая ошибка: речь владельца, выброшенная как\n\
         \x20   эхо, не восстановится ничем. Правый — дешёвая: лишний текст видно\n\
         \x20   и его стирают. Порог берётся там, где левый ещё ноль."
    );

    // Сверка групп по опоре: она объясняет зазор либо опровергает его.
    let quiet_owner = owner.iter().filter(|w| w.system_rms < LOUD_RMS).count();
    let quiet_others = others.iter().filter(|w| w.system_rms < LOUD_RMS).count();
    println!(
        "\n    С молчащей опорой: у владельца {quiet_owner} окон из {}, у остальных\n\
         \x20   {quiet_others} из {}. Там, где опора молчит, `explained` отвечает не на\n\
         \x20   тот вопрос: отражать было нечего. Если такие окна собрались в одной\n\
         \x20   группе, зазор описывает тишину, а не эхо.",
        owner.len(),
        others.len()
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

    fn window(start_ms: u64, end_ms: u64, explained: f32) -> stt::EchoWindow {
        stt::EchoWindow {
            start_ms,
            end_ms,
            explained,
            mic_rms: 500.0,
            system_rms: 500.0,
            is_echo: false,
        }
    }

    /// Заведомо положительный случай: окна внутри реплик владельца идут в
    /// одну группу, внутри чужих — в другую.
    ///
    /// Без него все проверки ниже выполнялись бы и на функции, которая не
    /// раскладывает ничего.
    #[test]
    fn windows_go_to_the_group_of_the_reply_they_fall_into() {
        let speech = LabelledSpeech {
            owner: "Я".to_string(),
            owner_spans: vec![(0, 1_000)],
            others_spans: vec![(2_000, 3_000)],
        };
        let windows = [window(100, 400, 0.01), window(2_100, 2_400, 0.80)];
        let refs: Vec<&stt::EchoWindow> = windows.iter().collect();

        let (owner, others, both) = split_windows(&refs, &speech);

        assert_eq!(owner.len(), 1);
        assert_eq!(others.len(), 1);
        assert_eq!(both, 0);
        assert!((owner[0].explained - 0.01).abs() < f32::EPSILON);
    }

    /// Окно, накрывающее реплики обоих, не идёт никуда. Отнести его к
    /// большей доле значило бы вернуть в зазор ту самую смесь, ради
    /// избавления от которой он и считается.
    #[test]
    fn a_window_covering_both_belongs_to_neither() {
        let speech = LabelledSpeech {
            owner: "Я".to_string(),
            owner_spans: vec![(0, 1_000)],
            others_spans: vec![(900, 2_000)],
        };
        let windows = [window(800, 1_100, 0.5)];
        let refs: Vec<&stt::EchoWindow> = windows.iter().collect();

        let (owner, others, both) = split_windows(&refs, &speech);

        assert!(owner.is_empty());
        assert!(others.is_empty());
        assert_eq!(both, 1);
    }

    /// Окно, не попавшее ни в одну размеченную реплику, тоже не в счёт:
    /// разметка покрывает часть встречи, и остальное про зазор молчит.
    #[test]
    fn a_window_outside_every_labelled_reply_is_ignored() {
        let speech = LabelledSpeech {
            owner: "Я".to_string(),
            owner_spans: vec![(0, 1_000)],
            others_spans: vec![(2_000, 3_000)],
        };
        let windows = [window(5_000, 5_300, 0.9)];
        let refs: Vec<&stt::EchoWindow> = windows.iter().collect();

        let (owner, others, both) = split_windows(&refs, &speech);

        assert!(owner.is_empty() && others.is_empty());
        assert_eq!(both, 0);
    }

    /// Владелец — тот, кого нет на системной дорожке: микрофона в
    /// системном tap'е нет по построению.
    #[test]
    fn the_owner_is_the_one_absent_from_the_system_track() {
        let owner = derive_owner(
            &["Я".into(), "Дима".into(), "Румия".into()],
            &["Дима".into(), "Румия".into()],
        );

        assert_eq!(owner.as_deref(), Ok("Я"));
    }

    /// Двое кандидатов — отказ, а не выбор большинством. Второй кандидат
    /// означает, что кого-то из удалённых не разметили на `system`, и
    /// зазор построился бы между не теми группами — уверенно и неверно.
    #[test]
    fn two_candidates_are_a_refusal_not_a_guess() {
        let error = derive_owner(
            &["Я".into(), "Гость".into(), "Дима".into()],
            &["Дима".into()],
        )
        .expect_err("двое кандидатов обязаны быть отказом");

        assert!(error.contains("Я") && error.contains("Гость"), "{error}");
    }

    /// Владельца не видно вовсе — тоже отказ: встреча, где он не сказал ни
    /// слова, зазора не даёт.
    #[test]
    fn no_candidate_is_a_refusal_too() {
        assert!(derive_owner(&["Дима".into()], &["Дима".into()]).is_err());
    }

    /// Заведомо положительный случай для самого вердикта прибора.
    #[test]
    fn the_self_check_passes_on_a_healthy_detector() {
        assert!(self_check());
    }
}
