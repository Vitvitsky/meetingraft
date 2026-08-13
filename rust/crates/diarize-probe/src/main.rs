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
//! - нашёл **больше**, либо число едет от перекладывания того же
//!   материала — это неточность настройки, а не слепота. Печатается
//!   громко, с числами, и работать не мешает: выбор порога и есть
//!   задача 3, а спрятать числа, по которым он делается, значит сделать
//!   её невыполнимой.
//!
//! ## Устойчивость числа проверяется двумя перекладываниями
//!
//! Первое — та же запись дважды подряд. Второе — переставленные местами
//! половины. Люди в обоих те же по построению, поэтому другое число
//! означает, что движок делит человека.
//!
//! Второе заведено после замера и заведено не для симметрии. По одному
//! удвоению было решено, что число **растёт от количества материала** —
//! удвоение ведь меняет и длину. Замер это опроверг: запись с четырьмя
//! людьми даёт 4 в исходном виде, 6 при удвоении, **4 при утроении** и 7
//! при учетверении. Роста нет, есть неустойчивость, и перестановка
//! половин показывает её при неизменной длине (4 → 5).
//!
//! Отдельно проверено, что дело не в случайности: тот же вход трижды
//! подряд даёт одно и то же число. И отдельно — что это свойство не
//! движка вообще, а трудного материала: запись с двумя далёкими голосами
//! даёт 2 при любом расположении.
//!
//! **Числа выше сняты с моделью, которой здесь больше нет.** Английская
//! VoxCeleb заменена на многоязычную CAM++, и на ней та же трудная запись
//! устойчива при любом расположении. То есть неустойчивость оказалась
//! свойством **соответствия модели материалу**, а не кластеризации, — но
//! контроли из-за этого не убраны, а наоборот: они и есть то, чем такую
//! замену можно сравнить. По-русски не обучена ни одна из двух моделей,
//! так что на наших встречах вопрос открыт (задача 3).

mod compare;
mod wav;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use diarize::{
    DiarizeReport, Diarizer, Match, VoiceEmbedder, VoicePrint, best_match, build_print,
    diarize_backend, diarize_models_dir,
};
use domain::AudioChannel;
use storage::AudioManifestStore;

/// Частота живого пути; ею же пишутся чанки на диск (ADR-005).
const RATE: u32 = 16_000;

/// Что прибор делает с сессией.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    /// Разделить голоса и сверить с разметкой.
    Plain,
    /// Перебрать пороги кластеризации.
    Sweep,
    /// Сложить слепки по разметке и проверить их на отложенной части.
    Enroll,
    /// Сложить слепки по разметке и прогнать по всем репликам Final.
    Apply,
}

/// Движок векторов — или отказ, если собрано без него.
#[cfg(feature = "model")]
fn open_embedder(root: &Path) -> Result<Box<dyn VoiceEmbedder>, String> {
    diarize::voice_embedder(root).map(|embedder| Box::new(embedder) as Box<dyn VoiceEmbedder>)
}

#[cfg(not(feature = "model"))]
fn open_embedder(_root: &Path) -> Result<Box<dyn VoiceEmbedder>, String> {
    Err("собрано без --features model: векторов голоса считать нечем".to_string())
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();

    // Каталог данных нужен раньше самопроверки: и модели, и контрольные
    // записи лежат в нём же, рядом с базой.
    let (root, session, mode) = match args.as_slice() {
        [] => {
            println!("\n{USAGE}");
            return ExitCode::SUCCESS;
        }
        [root] => (Path::new(root), None, Mode::Plain),
        [root, session] => (Path::new(root), Some(session.as_str()), Mode::Plain),
        [root, session, flag] if flag == "--sweep" => {
            (Path::new(root), Some(session.as_str()), Mode::Sweep)
        }
        [root, session, flag] if flag == "--enroll" => {
            (Path::new(root), Some(session.as_str()), Mode::Enroll)
        }
        [root, session, flag] if flag == "--apply" => {
            (Path::new(root), Some(session.as_str()), Mode::Apply)
        }
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
    let checked = self_check(engine.as_mut(), &controls);
    if !checked.problems.is_empty() {
        eprintln!("\nПрибор слеп: до настоящих данных дело не дошло.");
        return ExitCode::FAILURE;
    }
    if !checked.notes.is_empty() {
        // Напоминание идёт **после** вердикта и перед числами, а не вместе
        // с предупреждением: между ними уезжает таблица, и «числам можно
        // верить» без этой строки прочитывается как «числа точны».
        println!(
            "\nЧисла ниже — оценка, а не факт: на контроле выше движок показал\n\
             неустойчивость, и на этой записи она тоже возможна."
        );
    }

    let result = match (session, mode) {
        (None, _) => list_sessions(root),
        (Some(id), Mode::Plain) => probe(root, id, engine.as_mut()),
        (Some(id), Mode::Sweep) => sweep_thresholds(root, id, engine.as_mut()),
        (Some(id), Mode::Enroll) => match open_embedder(root) {
            Ok(mut embedder) => enroll(root, id, embedder.as_mut()),
            Err(error) => Err(error),
        },
        (Some(id), Mode::Apply) => match open_embedder(root) {
            Ok(mut embedder) => apply_prints(root, id, embedder.as_mut()),
            Err(error) => Err(error),
        },
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
  diarize-probe <каталог-данных> <сессия> --sweep
                                            — перебрать пороги и сверить
                                              каждый с разметкой человека
  diarize-probe <каталог-данных> <сессия> --enroll
                                            — сложить слепки голосов по
                                              разметке и проверить их на
                                              отложенной её части
  diarize-probe <каталог-данных> <сессия> --apply
                                            — сложить слепки по разметке и
                                              прогнать по всем репликам
                                              Final: сколько встречи
                                              получит имя

Каталог данных — тот, где лежит meetingraft.sqlite3. Модели и контрольные
записи кладёт туда scripts/fetch-diarize-models.sh; движок включается
сборкой с --features model.

Прогон идёт минутами и печатает много; сохранять вывод стоит целиком, а
не переписывать глазами:

  ... -- \"$ROOT\" <сессия> --sweep 2>&1 | tee ~/diarize-sweep.txt

`2>&1` обязателен: отказы движка и его собственные сообщения идут в
stderr, и без него в файл попадёт только половина разговора.

Чем считать, выбирает MEETINGRAFT_DIARIZE_PROVIDER: cpu (умолчание) или
coreml на Маке; число потоков — MEETINGRAFT_DIARIZE_THREADS, по умолчанию
по ядрам, не больше восьми.

Прибор печатает **запрошенное**, а не использованное: sherpa при неудаче
откатывается на cpu и пишет об этом в stderr строкой «Fallback to cpu!».
Узнать это снаружи иначе нельзя, поэтому смотреть надо туда.

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

/// Что прибор увидел, проверяя движок.
///
/// Две корзины, а не одна, и это то же разделение, что в вердикте:
/// `problems` — слепота, прогон дальше не идёт; `notes` — неточность,
/// сказать о ней надо громко, а мешать работе она не должна.
///
/// Обе возвращаются, а не только печатаются: ветка, о которой известно
/// лишь из вывода на экран, тестом не проверяется вовсе — и снимается
/// потом незамеченной.
#[derive(Debug, Default)]
pub struct SelfCheck {
    problems: Vec<String>,
    notes: Vec<String>,
}

/// Проверка движка по записям с известным ответом.
///
/// Пусто — можно верить числам ниже. Иначе — по строке на каждую беду, и
/// строка называет **свою**: вердикт «прибор слеп» без причины под ним
/// нечем ни проверить, ни починить.
fn self_check(engine: &mut dyn Diarizer, controls: &[Control]) -> SelfCheck {
    println!("Проверка движка на записях с известным ответом");

    if controls.is_empty() {
        // Движок мог не подняться вовсе — тогда его отказ и есть ответ, и
        // отсутствие контролей ни при чём.
        let probe = engine.diarize(&vec![0i16; RATE as usize], RATE);
        return match probe.refused {
            Some(reason) => report(
                vec![format!("движок отказался считать — {reason}")],
                Vec::new(),
            ),
            None => report(
                vec![
                    "движок отвечает, а проверить его нечем: контрольных записей нет \
                     (скачать — scripts/fetch-diarize-models.sh)"
                        .to_string(),
                ],
                Vec::new(),
            ),
        };
    }

    let mut problems = Vec::new();
    let mut notes = Vec::new();
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

        // Тот же материал, переложенный двумя способами. Люди в нём по
        // построению те же, поэтому другое число голосов означает, что
        // движок делит человека, а не что в записи кто-то появился.
        // Контроль не требует второго файла и не зависит от того, верна ли
        // подпись на первом.
        //
        // Способа два, и второй заведён после замера. Удвоение меняет и
        // расположение, и **длину**, поэтому по нему одному было решено,
        // что число растёт от количества материала. Замер это опроверг:
        // та же запись трижды даёт верное число, а дважды — завышенное.
        // Перестановка половин держит длину неизменной и разделяет два
        // объяснения: если число едет и здесь, дело не в объёме.
        let doubled: Vec<i16> = control
            .pcm
            .iter()
            .chain(control.pcm.iter())
            .copied()
            .collect();
        let middle = control.pcm.len() / 2;
        let swapped: Vec<i16> = control.pcm[middle..]
            .iter()
            .chain(control.pcm[..middle].iter())
            .copied()
            .collect();
        let count = |engine: &mut dyn Diarizer, pcm: &[i16]| -> Option<u32> {
            let report = engine.diarize(pcm, RATE);
            report.refused.is_none().then_some(report.speakers_found)
        };
        let twice_found = count(engine, &doubled);
        let swapped_found = count(engine, &swapped);

        let say = |found: Option<u32>| match found {
            Some(found) => found.to_string(),
            None => "?".to_string(),
        };
        println!(
            "  {:26} {:.1} с: в записи {} человек, движок нашёл {} \
             (дважды — {}, половины переставлены — {})",
            control.name,
            seconds,
            control.speakers,
            once.speakers_found,
            say(twice_found),
            say(swapped_found),
        );

        if once.speakers_found < control.speakers {
            problems.push(format!(
                "{}: заведомо разные голоса слились в {} из {} — движок смены не видит",
                control.name, once.speakers_found, control.speakers
            ));
        }
        if once.speakers_found > control.speakers {
            notes.push(format!(
                "{}: разорвал {} человек на {} — порог кластеризации не настроен под этот \
                 материал. Числа ниже читать с этим в уме; выбор порога — задача 3",
                control.name, control.speakers, once.speakers_found
            ));
        }
        // Неустойчивость считается по обоим перекладываниям сразу: расходится
        // хоть одно — число голосов свойством записи не является.
        let unstable: Vec<String> = [
            ("дважды", twice_found),
            ("с переставленными половинами", swapped_found),
        ]
        .into_iter()
        .filter_map(|(how, found)| {
            let found = found?;
            (found != once.speakers_found).then(|| format!("{how} — {found}"))
        })
        .collect();
        if !unstable.is_empty() {
            notes.push(format!(
                "{}: те же люди, переложенные иначе, дали другое число ({}, было {}) — число \
                 голосов на этом материале неустойчиво, и верное значение из него не следует. \
                 Движок при этом не случаен: тот же вход даёт то же число",
                control.name,
                unstable.join(", "),
                once.speakers_found
            ));
        }
    }
    report(problems, notes)
}

/// Напечатать вердикт и вернуть его же вызывающему.
fn report(problems: Vec<String>, notes: Vec<String>) -> SelfCheck {
    for note in &notes {
        println!("    ! {note}");
    }
    if problems.is_empty() {
        println!("  ВЕРДИКТ: движок видит смену голоса, числам ниже можно верить");
    }
    for problem in &problems {
        println!("  ВЕРДИКТ: {problem}");
    }
    SelfCheck { problems, notes }
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

        let started = std::time::Instant::now();
        let report = engine.diarize(&pcm, RATE);
        let took = started.elapsed().as_secs_f64();
        if let Some(reason) = report.refused {
            println!("    отказ: {reason}");
            continue;
        }
        // Время прохода рядом с длиной записи, а не отдельно: «9 секунд»
        // ничего не значит, пока не сказано, по какому куску. Отношение
        // и есть та величина, которой замер сравнивается между машинами
        // и между провайдерами.
        println!(
            "    проход {took:.1} с — {:.2} от реального времени, {}",
            took / seconds.max(0.001),
            compute_provider()
        );
        print_report(&report, pcm.len() as u64 * 1_000 / u64::from(RATE));
        print_against_labels(&store, session_id, channel, &report.turns);
    }
    Ok(())
}

/// Чем считает движок — печатается рядом с временем прохода.
///
/// Sherpa, не найдя запрошенного провайдера, печатает «Fallback to cpu!» в
/// stderr и считает дальше. Без этой строки замер «на CoreML» мог бы
/// оказаться замером на CPU, и отличить их было бы нечем — ровно тот
/// молчаливый подлог, из-за которого в проекте и заведено правило про
/// приборы.
#[cfg(feature = "model")]
fn compute_provider() -> String {
    format!(
        "запрошен {} в {} потоков",
        diarize::requested_provider(),
        diarize::threads_in_use()
    )
}

#[cfg(not(feature = "model"))]
fn compute_provider() -> String {
    "движка нет".to_string()
}

/// Порог похожести для прогона по встрече.
///
/// 0.45 — середина окна, снятого замером на настоящей встрече: при
/// 0.40…0.50 подписывалось 84…94% отложенного времени **без единой
/// неверной подписи**, ниже начинали появляться ошибки, выше росло
/// неопознанное. Переопределяется `MEETINGRAFT_DIARIZE_ACCEPT`.
fn accept_threshold() -> f32 {
    std::env::var("MEETINGRAFT_DIARIZE_ACCEPT")
        .ok()
        .and_then(|value| value.trim().parse::<f32>().ok())
        .filter(|value| *value > 0.0)
        .unwrap_or(0.45)
}

/// Реплика Final: то, что прогоняется через слепки.
struct Reply {
    ms: u64,
    /// Кого назвал человек; пусто — не размечена.
    labelled: String,
    /// Разметку ставил человек поимённо, а не массовое назначение.
    pinned: bool,
    vector: Vec<f32>,
}

/// Сложить слепки по разметке и прогнать по **всем** репликам Final.
///
/// То, ради чего всё и затевалось: человек размечает несколько реплик,
/// остальное подписывается само, неопознанное остаётся неопознанным.
///
/// Границы берутся из Final (ADR-011), а не у сегментации: они уже есть,
/// они точнее и совпадают с тем, что человек видит в транскрипте. Побочно
/// это снимает вопрос покрытия — оно становится полным по построению, а у
/// сегментации на этой встрече было 67%.
fn apply_prints(
    root: &Path,
    session_id: &str,
    embedder: &mut dyn VoiceEmbedder,
) -> Result<(), String> {
    let store = AudioManifestStore::open(root).map_err(|error| error.to_string())?;
    let tracks = tracks(&store, session_id)?;
    let accept = accept_threshold();

    let mut worked = 0usize;
    let mut spans: Vec<(AudioChannel, Vec<Span>)> = Vec::new();
    let mut raw: Vec<(AudioChannel, Vec<i16>)> = Vec::new();
    for (channel, pcm) in tracks {
        raw.push((channel, pcm.clone()));
        println!("\n  Дорожка {}", channel.code());
        spans.push((
            channel,
            segment_spans(&store, session_id, channel).unwrap_or_default(),
        ));
        let replies = match read_replies(&store, session_id, channel, embedder, &pcm) {
            Ok(replies) => replies,
            Err(error) => {
                println!("    {error}");
                continue;
            }
        };
        let fit: Vec<(Vec<f32>, f32)> = replies
            .iter()
            .filter(|reply| reply.pinned && !reply.labelled.is_empty())
            .map(|reply| (reply.vector.clone(), reply.ms as f32 / 1_000.0))
            .collect();
        let mut by_name: BTreeMap<&str, Vec<(Vec<f32>, f32)>> = BTreeMap::new();
        for reply in replies
            .iter()
            .filter(|r| r.pinned && !r.labelled.is_empty())
        {
            by_name
                .entry(&reply.labelled)
                .or_default()
                .push((reply.vector.clone(), reply.ms as f32 / 1_000.0));
        }
        let prints: Vec<(String, VoicePrint)> = by_name
            .iter()
            .filter_map(|(name, vectors)| {
                build_print(vectors).map(|print| ((*name).to_string(), print))
            })
            .collect();
        if prints.len() < 2 {
            println!(
                "    слепков вышло {} — размечено слишком мало, подписывать нечем",
                prints.len()
            );
            continue;
        }
        worked += 1;

        let total_ms: u64 = replies.iter().map(|reply| reply.ms).sum();
        println!(
            "    реплик {}, речи {:.1} с; из них размечено человеком {} на {:.1} с",
            replies.len(),
            total_ms as f64 / 1_000.0,
            fit.len(),
            fit.iter().map(|(_, seconds)| seconds).sum::<f32>()
        );
        println!(
            "    слепки: {}",
            prints
                .iter()
                .map(|(name, print)| format!("{name} ({} кусков)", print.samples))
                .collect::<Vec<_>>()
                .join(", ")
        );

        println!("\n     порог  подписано  не опознано");
        for value in ACCEPT {
            let (named, _) = run_prints(&replies, &prints, *value);
            println!(
                "      {value:.2} {:>9.0}% {:>12.0}%",
                percent(named, total_ms),
                percent(total_ms - named, total_ms)
            );
        }

        let (named, per_person) = run_prints(&replies, &prints, accept);
        println!("\n    При пороге {accept:.2} встреча выглядит так:");
        for (name, ms) in &per_person {
            println!(
                "      {name:<24} {:>6.1} с ({:.0}%)",
                *ms as f64 / 1_000.0,
                percent(*ms, total_ms)
            );
        }
        println!(
            "      {:<24} {:>6.1} с ({:.0}%)",
            "не опознано",
            (total_ms - named) as f64 / 1_000.0,
            percent(total_ms - named, total_ms)
        );

        // Размеченные реплики вошли в слепки, поэтому их совпадение ничего
        // не измеряет: слепок похож на своё же. Числа выше — про то, какую
        // часть встречи схема **накрывает**, а не про то, права ли она.
        // Права ли — отвечает `--enroll` на отложенной части.
        println!(
            "\n    Это покрытие, а не точность: размеченные реплики вошли в слепки и\n\
             \x20   на себя же и похожи. Насколько подписи верны, отвечает --enroll,\n\
             \x20   где проверка идёт по кускам, которых слепок не видел."
        );
    }

    report_channel_overlap(&spans);
    report_envelope_match(&raw);

    if worked == 0 {
        return Err("подписывать нечем ни на одной дорожке".to_string());
    }
    Ok(())
}

/// Насколько сдвиг может уехать при поиске совпадения.
///
/// Полсекунды: сетевая задержка Zoom до динамиков и обратно в микрофон
/// укладывается в неё с запасом, а дальше начинают случайно совпадать
/// разные события речи.
const MAX_ENVELOPE_LAG_MS: u64 = 500;

/// Порог, выше которого совпадение громкости считается одним событием.
///
/// 0.5 — не подгонка: у независимых разговоров корреляция громкости
/// болтается около нуля, а у одного звука, попавшего в оба канала, она
/// высока даже при сильном приглушении, потому что тихое и громкое
/// чередуются одинаково.
const SAME_SOUND: f32 = 0.5;

/// Слышит ли микрофон то же, что системный канал.
///
/// Заведён по разбору 2026-08-13. На `mic` должен был попадать только
/// владелец машины и его комната — коллеги были в Zoom, — а разметка
/// человека назвала на этой дорожке всех шестерых. Значит звук созвона в
/// микрофонный канал попадает.
///
/// Повторов текстом при этом всего 12%, и это не возражение: копия в
/// микрофоне глухая, и Whisper распознаёт её **другими словами**. Текст
/// расходится, звук — нет, и мерить надо звук.
fn report_envelope_match(raw: &[(AudioChannel, Vec<i16>)]) {
    if raw.len() < 2 {
        return;
    }
    let (first, second) = (&raw[0], &raw[1]);
    if first.1.is_empty() || second.1.is_empty() {
        return;
    }

    let (value, lag) = envelope_match(&first.1, &second.1, RATE, MAX_ENVELOPE_LAG_MS);
    println!("\n  Звучит ли в одном канале то же, что в другом");
    println!(
        "    громкость {} и {} совпадает на {value:.2} при сдвиге {lag} мс",
        first.0.code(),
        second.0.code(),
    );

    if value >= SAME_SOUND {
        println!(
            "    ! это один и тот же звук в двух дорожках. Если на {} должен был\n\
             \x20     попадать только владелец машины, то микрофон слышит созвон —\n\
             \x20     дефект захвата, который удваивает и распознавание, и место на\n\
             \x20     диске, и проходы Whisper при пересборе",
            first.0.code()
        );
        println!(
            "      `echo-probe` этого не ловит по построению: он ищет **задержанную**\n\
             \x20     копию в пределах 250 мс и меряет, сколько её снимает фильтр\n\
             \x20     отклика. Совпадение на малом сдвиге для него не улика"
        );
    } else {
        println!(
            "    каналы несут разный звук: совпадения громкости нет. Тогда всё,\n\
             \x20   что выше про дублирование, объясняется распознаванием, а не\n\
             \x20   захватом"
        );
    }
}

/// Реплика для сверки каналов: границы и текст.
#[derive(Debug, Clone)]
pub struct Span {
    pub start_ms: u64,
    pub end_ms: u64,
    pub text: String,
}

/// Границы и текст реплик Final по каналу — для сверки каналов.
fn segment_spans(
    store: &AudioManifestStore,
    meeting_id: &str,
    channel: AudioChannel,
) -> Result<Vec<Span>, String> {
    let versions = store
        .list_final_transcripts(meeting_id)
        .map_err(|error| error.to_string())?;
    let latest = versions
        .first()
        .ok_or_else(|| "у встречи нет собранного Final".to_string())?;
    Ok(store
        .list_final_segments(meeting_id, latest.version)
        .map_err(|error| error.to_string())?
        .into_iter()
        .filter(|segment| segment.channel == channel)
        .map(|segment| Span {
            start_ms: segment.start_ms,
            end_ms: segment.end_ms,
            text: segment.text,
        })
        .collect())
}

/// Шаг огибающей: на нём считается громкость каждого канала.
///
/// 50 мс — короче слога и длиннее периода основного тона. Речь на таком
/// шаге видна как чередование громкого и тихого, а сам тон уже не мешает.
const ENVELOPE_MS: u64 = 50;

/// Корреляция громкости двух дорожек и сдвиг, на котором она наибольшая.
///
/// Отвечает на вопрос, которого не задавал ни один прежний прибор:
/// **звучит ли в микрофоне то же, что в системном канале**. Не «есть ли
/// эхо с задержкой» — на это `echo-probe` отвечал и сказал «нет», — а
/// просто одно ли это событие во времени.
///
/// Разница существенная. `echo-probe` ищет задержанную копию и меряет,
/// сколько её вычитается фильтром отклика; если звук попадает в оба
/// канала внутри программы, задержки нет вовсе, и та машинерия проходит
/// мимо. Корреляция громкости слепа к этому не бывает: одно и то же
/// событие громко в обоих каналах одновременно, каким бы путём оно туда
/// ни попало.
///
/// Возвращает `(корреляция, сдвиг в мс)`.
fn envelope_match(mic: &[i16], system: &[i16], sample_rate: u32, max_lag_ms: u64) -> (f32, i64) {
    let step = (sample_rate as u64 * ENVELOPE_MS / 1_000) as usize;
    if step == 0 {
        return (0.0, 0);
    }
    let envelope = |pcm: &[i16]| -> Vec<f32> {
        pcm.chunks(step)
            .map(|frame| {
                let sum: f64 = frame.iter().map(|s| f64::from(*s) * f64::from(*s)).sum();
                ((sum / frame.len().max(1) as f64).sqrt()) as f32
            })
            .collect()
    };
    let (mine, theirs) = (envelope(mic), envelope(system));
    let max_lag = (max_lag_ms / ENVELOPE_MS) as i64;

    let mut best = (0.0f32, 0i64);
    for lag in -max_lag..=max_lag {
        let value = correlation(&mine, &theirs, lag);
        if value > best.0 {
            best = (value, lag * ENVELOPE_MS as i64);
        }
    }
    best
}

/// Корреляция Пирсона двух рядов при сдвиге второго на `lag` шагов.
fn correlation(first: &[f32], second: &[f32], lag: i64) -> f32 {
    let mut pairs: Vec<(f32, f32)> = Vec::new();
    for (index, value) in first.iter().enumerate() {
        let other = index as i64 + lag;
        if other < 0 {
            continue;
        }
        if let Some(theirs) = second.get(other as usize) {
            pairs.push((*value, *theirs));
        }
    }
    if pairs.len() < 2 {
        return 0.0;
    }
    let count = pairs.len() as f32;
    let mean_a = pairs.iter().map(|(a, _)| a).sum::<f32>() / count;
    let mean_b = pairs.iter().map(|(_, b)| b).sum::<f32>() / count;
    let mut top = 0.0f32;
    let (mut left, mut right) = (0.0f32, 0.0f32);
    for (a, b) in &pairs {
        let (da, db) = (a - mean_a, b - mean_b);
        top += da * db;
        left += da * da;
        right += db * db;
    }
    if left <= f32::EPSILON || right <= f32::EPSILON {
        return 0.0;
    }
    (top / (left.sqrt() * right.sqrt())).clamp(-1.0, 1.0)
}

/// Сколько времени каналы звучали бы вместе от одной лишь плотности речи.
///
/// Вынесено ради теста, и тест здесь важнее обычного: без этой величины
/// прибор уже один раз сказал больше, чем знал. На настоящей встрече
/// пересечение вышло 97% меньшего канала и читалось как доказательство
/// общего звука, а объяснялось тем, что на `mic` речь занимала 97% всего
/// времени — любой отрезок с `system` пересекался с чем-нибудь просто так.
fn expected_shared_ms(mine: u64, theirs: u64, wall: u64) -> u64 {
    if wall == 0 {
        return 0;
    }
    ((mine as f64 / wall as f64) * theirs as f64) as u64
}

/// Выше ли наблюдение того, что даёт плотность.
///
/// Запас в пятую часть: доли считаются по границам сегментов, а те не
/// ложатся ровно, и требовать точного превышения значило бы ловить шум.
fn overlap_is_evidence(shared: u64, expected: u64) -> bool {
    shared > expected + expected / 5
}

/// Насколько два текста — один и тот же.
///
/// Доля общих слов от большего множества. Не по буквам и не точным
/// равенством: распознавание одного и того же звука дважды даёт почти
/// одинаковый текст, но редко буква в букву — разойдётся пунктуация,
/// заглавная буква, одно слово из десяти.
fn text_likeness(first: &str, second: &str) -> f32 {
    let words = |text: &str| -> Vec<String> {
        text.to_lowercase()
            .split(|c: char| !c.is_alphanumeric())
            .filter(|word| !word.is_empty())
            .map(str::to_string)
            .collect()
    };
    let (mut mine, mut theirs) = (words(first), words(second));
    if mine.is_empty() || theirs.is_empty() {
        return 0.0;
    }
    mine.sort();
    mine.dedup();
    theirs.sort();
    theirs.dedup();

    let shared = mine.iter().filter(|word| theirs.contains(word)).count();
    shared as f32 / mine.len().max(theirs.len()) as f32
}

/// Реплики, у которых на другой дорожке есть почти такая же и в то же
/// время.
///
/// Отвечает на вопрос, который со временем звучания не совпадает.
/// Пересечение по времени означало бы, что **звук** попал в оба канала —
/// дефект захвата. Одинаковый текст в одно и то же время означает, что
/// одно и то же **распознано дважды**, и это может быть как следствием
/// первого, так и артефактом распознавания. Различить их можно только
/// имея обе величины рядом.
///
/// Возвращает число таких реплик и их суммарную длительность.
fn doubled_text(first: &[Span], second: &[Span], likeness: f32) -> (usize, u64) {
    let mut count = 0usize;
    let mut ms = 0u64;
    for one in first {
        let doubled = second.iter().any(|other| {
            let overlap = one
                .end_ms
                .min(other.end_ms)
                .saturating_sub(one.start_ms.max(other.start_ms));
            overlap > 0 && text_likeness(&one.text, &other.text) >= likeness
        });
        if doubled {
            count += 1;
            ms += one.end_ms.saturating_sub(one.start_ms);
        }
    }
    (count, ms)
}

/// Насколько похожими считаются два текста, чтобы счесть их одним.
///
/// 0.8 — восемь общих слов из десяти. Ниже начинают склеиваться разные
/// реплики на общую тему, выше — не ловится расхождение в одно слово,
/// а распознавание одного звука дважды буква в букву совпадает редко.
const SAME_TEXT: f32 = 0.8;

/// Сколько времени два набора отрезков звучат одновременно.
///
/// Вынесено отдельно ради теста: величина, живущая только внутри печати,
/// не проверяется ничем и молча уезжает при первой же правке.
fn shared_ms(first: &[Span], second: &[Span]) -> u64 {
    let mut shared = 0u64;
    for one in first {
        for other in second {
            shared += one
                .end_ms
                .min(other.end_ms)
                .saturating_sub(one.start_ms.max(other.start_ms));
        }
    }
    shared
}

/// Не одна ли это речь, посчитанная дважды.
///
/// Вопрос заведён по живому прогону: на встрече вышло 1276 с речи на
/// `mic` и 709 с на `system` — 33 минуты на встрече в 22, — и все шестеро
/// нашлись на обеих дорожках. Либо микрофон слышал динамики, либо каналы
/// несут один и тот же разговор, и тогда раскладка по людям складывает
/// одного человека с ним же.
///
/// Само по себе пересечение каналов законно: люди перебивают друг друга,
/// и короткие наложения — норма. Тревожно другое — когда пересечение
/// **велико по доле**: разговор, где половина времени звучит сразу в двух
/// каналах, разговором двух каналов не является.
fn report_channel_overlap(spans: &[(AudioChannel, Vec<Span>)]) {
    if spans.len() < 2 {
        return;
    }
    let (first, second) = (&spans[0], &spans[1]);
    let total = |segments: &[Span]| -> u64 {
        segments
            .iter()
            .map(|span| span.end_ms.saturating_sub(span.start_ms))
            .sum()
    };
    let (mine, theirs) = (total(&first.1), total(&second.1));
    if mine == 0 || theirs == 0 {
        return;
    }

    let shared = shared_ms(&first.1, &second.1);
    let span = |segments: &[Span]| -> u64 {
        let start = segments.iter().map(|s| s.start_ms).min().unwrap_or(0);
        let end = segments.iter().map(|s| s.end_ms).max().unwrap_or(0);
        end.saturating_sub(start)
    };
    let wall = span(&first.1).max(span(&second.1));

    println!("\n  Каналы друг относительно друга");
    println!(
        "    речи: {} {:.1} с, {} {:.1} с, вместе {:.1} с при длине встречи {:.1} с",
        first.0.code(),
        mine as f64 / 1_000.0,
        second.0.code(),
        theirs as f64 / 1_000.0,
        (mine + theirs) as f64 / 1_000.0,
        wall as f64 / 1_000.0,
    );
    // Пересечение само по себе не улика, и это стоило прибору одной
    // ошибки. На настоящей встрече вышло 97% от меньшего канала — и
    // выглядело как доказательство, что звук попал в обе дорожки. На деле
    // на `mic` речь занимала 97% всей встречи, так что любой отрезок с
    // `system` пересекался с чем-нибудь на `mic` просто по плотности.
    //
    // Поэтому рядом печатается то, сколько дала бы одна плотность:
    // наблюдение сравнивается с ним, а не с нулём.
    let density = mine as f64 / wall.max(1) as f64;
    let expected = expected_shared_ms(mine, theirs, wall);
    println!(
        "    звучат одновременно {:.1} с — {:.0}% от меньшего канала",
        shared as f64 / 1_000.0,
        percent(shared, mine.min(theirs)),
    );
    println!(
        "      одна плотность речи ({:.0}% времени на {}) дала бы {:.1} с — {}",
        density * 100.0,
        first.0.code(),
        expected as f64 / 1_000.0,
        if overlap_is_evidence(shared, expected) {
            "наблюдение заметно выше, звук и правда общий"
        } else {
            "наблюдение не выше: пересечение объясняется плотностью, и улики в нём нет"
        }
    );

    // Две разные величины, и путать их нельзя. Общее время означает, что
    // звук попал в оба канала. Одинаковый текст в это же время означает,
    // что одно и то же распознано дважды. Первое — дефект захвата, второе
    // может быть и его следствием, и артефактом распознавания.
    let (doubled_here, doubled_ms) = doubled_text(&first.1, &second.1, SAME_TEXT);
    let (doubled_there, _) = doubled_text(&second.1, &first.1, SAME_TEXT);
    println!(
        "    повторено текстом: {} реплик из {} на {} ({:.0}%) и {} из {} на {} ({:.0}%)",
        doubled_here,
        first.1.len(),
        first.0.code(),
        percent(doubled_here as u64, first.1.len() as u64),
        doubled_there,
        second.1.len(),
        second.0.code(),
        percent(doubled_there as u64, second.1.len() as u64),
    );
    println!(
        "    это {:.1} с — {:.0}% речи канала {}",
        doubled_ms as f64 / 1_000.0,
        percent(doubled_ms, mine),
        first.0.code(),
    );

    // Порог назван вслух и выбран по смыслу, а не по этим данным: половина
    // — это уже не перебивания, а один разговор в двух каналах.
    if overlap_is_evidence(shared, expected) && shared * 2 > mine.min(theirs) {
        println!(
            "    ! каналы звучат вместе заметно чаще, чем объясняется плотностью.\n\
             \x20     Это не перебивания, а один разговор, попавший в обе дорожки:\n\
             \x20     тогда раскладка по людям выше складывает человека с ним же, а\n\
             \x20     «канал — источник истины» (ADR-012) на этой встрече не верно"
        );
    } else if mine * 10 > wall * 9 {
        println!(
            "    ! на {} речь занимает {:.0}% всей встречи. Для канала, который\n\
             \x20     должен нести одного человека, это много: похоже, он слышит\n\
             \x20     всех. Пересечение каналов при такой плотности ни о чём не\n\
             \x20     говорит — смотреть надо на строку про повторы текстом",
            first.0.code(),
            density * 100.0
        );
    } else if mine + theirs > wall + wall / 5 {
        println!(
            "    ! речи насчиталось заметно больше длины встречи, хотя каналы почти\n\
             \x20     не пересекаются. Смотреть надо на сами границы Final"
        );
    }
}

/// Прогнать слепки по репликам: сколько времени подписано и кому.
fn run_prints(
    replies: &[Reply],
    prints: &[(String, VoicePrint)],
    accept: f32,
) -> (u64, Vec<(String, u64)>) {
    let mut named = 0u64;
    let mut per_person: BTreeMap<String, u64> = BTreeMap::new();
    for reply in replies {
        if let Match::Named { name, .. } = best_match(&reply.vector, prints, accept, MARGIN) {
            named += reply.ms;
            *per_person.entry(name).or_default() += reply.ms;
        }
    }
    let mut out: Vec<(String, u64)> = per_person.into_iter().collect();
    out.sort_by_key(|(_, ms)| std::cmp::Reverse(*ms));
    (named, out)
}

/// Все реплики Final последней версии с посчитанными векторами.
fn read_replies(
    store: &AudioManifestStore,
    meeting_id: &str,
    channel: AudioChannel,
    embedder: &mut dyn VoiceEmbedder,
    pcm: &[i16],
) -> Result<Vec<Reply>, String> {
    let versions = store
        .list_final_transcripts(meeting_id)
        .map_err(|error| error.to_string())?;
    let latest = versions
        .first()
        .ok_or_else(|| "у встречи нет собранного Final".to_string())?;
    let names: BTreeMap<String, String> = store
        .list_speakers(meeting_id)
        .map_err(|error| error.to_string())?
        .into_iter()
        .filter(|speaker| !speaker.display_name.trim().is_empty())
        .map(|speaker| (speaker.id, speaker.display_name))
        .collect();

    let segments = store
        .list_final_segments(meeting_id, latest.version)
        .map_err(|error| error.to_string())?;
    let mut out = Vec::new();
    let mut short = 0usize;
    for segment in segments.into_iter().filter(|s| s.channel == channel) {
        let from = (segment.start_ms as usize * RATE as usize / 1_000).min(pcm.len());
        let to = (segment.end_ms as usize * RATE as usize / 1_000).min(pcm.len());
        if to <= from {
            short += 1;
            continue;
        }
        match embedder.embed(&pcm[from..to], RATE) {
            Ok(vector) => out.push(Reply {
                ms: segment.end_ms.saturating_sub(segment.start_ms),
                labelled: names
                    .get(&segment.speaker_id)
                    .cloned()
                    .unwrap_or(segment.speaker_id),
                pinned: segment.speaker_pinned,
                vector,
            }),
            Err(_) => short += 1,
        }
    }
    if out.is_empty() {
        return Err(format!(
            "на этой дорожке нет реплик Final, по которым считается вектор \
             (слишком коротких — {short})"
        ));
    }
    // Пропуск называется вслух: реплика короче минимума модели вектора не
    // даёт, и молча выпасть из отчёта она не имеет права — иначе «встреча
    // подписана на 90%» окажется утверждением про её половину.
    if short > 0 {
        println!("    {short} реплик короче, чем нужно модели для вектора — не считаются");
    }
    Ok(out)
}

/// Пороги похожести для проверки слепков.
///
/// Косинус между векторами одного человека обычно заметно выше, чем между
/// разными, но где проходит граница — свойство модели и материала, а не
/// величина из документации. Поэтому перебор, а не константа.
const ACCEPT: &[f32] = &[0.2, 0.3, 0.4, 0.5, 0.6, 0.7, 0.8];

/// Минимальный отрыв от следующего по похожести.
///
/// Держится постоянным, чтобы таблица оставалась про один порог. Смысл
/// его в другом: двое похожих без правила об отрыве делили бы реплики
/// монеткой, каждый раз уверенно.
const MARGIN: f32 = 0.05;

/// Кусок разметки с посчитанным вектором.
struct Sample {
    /// Кого назвал человек.
    truth: String,
    vector: Vec<f32>,
    ms: u64,
}

/// Сложить слепки по разметке и проверить их на отложенной части.
///
/// Задача не та, что у кластеризации, и более лёгкая: сколько в записи
/// людей, говорит человек, и угадывать нечего — остаётся померить
/// похожесть.
///
/// **Складывать и проверять на одном и том же нельзя.** Слепок, сложенный
/// по куску, на этом же куске похож на себя почти идеально, и отчёт вышел
/// бы прекрасным и пустым. Поэтому разметка делится, и делится дважды:
///
/// - **через один** — человек разметил понемногу по всей встрече. Проверка
///   идёт на кусках рядом с обучающими, то есть в тех же условиях: это
///   оценка сверху;
/// - **первая треть против остального** — человек разметил начало и хочет
///   остальное. Проверка идёт по материалу, которого слепок не видел ни
///   рядом, ни по времени: это оценка снизу.
///
/// Правда между ними, и обе печатаются.
fn enroll(root: &Path, session_id: &str, embedder: &mut dyn VoiceEmbedder) -> Result<(), String> {
    let store = AudioManifestStore::open(root).map_err(|error| error.to_string())?;
    let tracks = tracks(&store, session_id)?;

    let mut worked = 0usize;
    for (channel, pcm) in tracks {
        let labels = human_labels(&store, session_id, channel).unwrap_or_default();
        if labels.is_empty() {
            println!(
                "\n  Дорожка {} — разметки человека нет, складывать слепки не из чего",
                channel.code()
            );
            continue;
        }

        println!(
            "\n  Дорожка {} — {} реплик разметки",
            channel.code(),
            labels.len()
        );
        let (samples, skipped) = embed_labels(embedder, &pcm, &labels);
        if skipped > 0 {
            println!(
                "    {skipped} реплик короче, чем нужно модели для вектора — пропущены.\n\
                 \x20   Это не потеря разметки, а цена: короткая реплика вектора не даёт"
            );
        }
        if samples.len() < 4 {
            println!(
                "    векторов вышло {} — делить и проверять не на чем",
                samples.len()
            );
            continue;
        }
        worked += 1;

        by_split(
            "через один (разметка понемногу по всей встрече)",
            &samples,
            |index, _| index % 2 == 0,
        );
        by_split(
            "первая треть против остального",
            &samples,
            |index, total| index * 3 < total,
        );
    }

    if worked == 0 {
        return Err("складывать слепки не из чего ни на одной дорожке".to_string());
    }
    println!(
        "\n  Слепки нигде не сохранены: этот прогон складывает их в памяти и\n\
         \x20 забывает. Хранение между встречами — отдельное решение (задача 7),\n\
         \x20 и включаться оно должно осознанно."
    );
    Ok(())
}

/// Посчитать вектор по каждому размеченному куску.
///
/// Возвращает и число пропущенных: реплика короче минимума модели вектора
/// не даёт, и молча терять её нельзя — из десяти реплик человека восемь
/// коротких означают слепок по двум.
fn embed_labels(
    embedder: &mut dyn VoiceEmbedder,
    pcm: &[i16],
    labels: &[compare::Labelled],
) -> (Vec<Sample>, usize) {
    let mut out = Vec::new();
    let mut skipped = 0usize;
    for label in labels {
        let from = (label.start_ms as usize * RATE as usize / 1_000).min(pcm.len());
        let to = (label.end_ms as usize * RATE as usize / 1_000).min(pcm.len());
        if to <= from {
            skipped += 1;
            continue;
        }
        match embedder.embed(&pcm[from..to], RATE) {
            Ok(vector) => out.push(Sample {
                truth: label.speaker.clone(),
                vector,
                ms: label.duration_ms(),
            }),
            Err(_) => skipped += 1,
        }
    }
    (out, skipped)
}

/// Разложить куски человека на обучающие и отложенные.
///
/// Вынесено отдельно ради одного теста: **ни один кусок не имеет права
/// попасть и туда, и туда**. Слепок, сложенный по куску, на этом же куске
/// похож на себя почти идеально, и отчёт вышел бы прекрасным и пустым —
/// та же ошибка, что «тест, который может пройти на пустом входе», только
/// дороже: здесь она даёт не ноль, а девяносто процентов.
///
/// Делит **внутри каждого человека**: общий порядок отдал бы одного
/// целиком в обучение, а другого целиком в проверку, и слепка для второго
/// не оказалось бы вовсе.
fn split<'a>(
    by_person: &BTreeMap<&'a str, Vec<&'a Sample>>,
    is_fit: &impl Fn(usize, usize) -> bool,
) -> (Vec<(String, VoicePrint)>, Vec<&'a Sample>) {
    let mut prints: Vec<(String, VoicePrint)> = Vec::new();
    let mut trials: Vec<&Sample> = Vec::new();
    for (name, mine) in by_person {
        let total = mine.len();
        let mut fit: Vec<(Vec<f32>, f32)> = Vec::new();
        for (index, sample) in mine.iter().enumerate() {
            if is_fit(index, total) {
                fit.push((sample.vector.clone(), sample.ms as f32 / 1_000.0));
            } else {
                trials.push(sample);
            }
        }
        if let Some(print) = build_print(&fit) {
            prints.push(((*name).to_string(), print));
        }
    }
    (prints, trials)
}

/// Сложить слепки по одной части разметки и проверить на другой.
///
/// `is_fit` решает по номеру куска **внутри одного человека**: делить
/// общим порядком значило бы отдать одного целиком в обучение, а другого
/// целиком в проверку, и слепка для второго не оказалось бы вовсе.
fn by_split(title: &str, samples: &[Sample], is_fit: impl Fn(usize, usize) -> bool) {
    let mut by_person: BTreeMap<&str, Vec<&Sample>> = BTreeMap::new();
    for sample in samples {
        by_person.entry(&sample.truth).or_default().push(sample);
    }

    let (prints, trials) = split(&by_person, &is_fit);

    println!("\n    Деление: {title}");
    if prints.len() < 2 || trials.is_empty() {
        println!(
            "      слепков {} и отложенных кусков {} — проверять нечего",
            prints.len(),
            trials.len()
        );
        return;
    }
    let names: Vec<String> = prints
        .iter()
        .map(|(name, print)| format!("{name} ({} кусков, {:.1} с)", print.samples, print.seconds))
        .collect();
    println!("      слепки: {}", names.join(", "));
    println!(
        "      проверка на {} отложенных кусках, {:.1} с",
        trials.len(),
        trials.iter().map(|sample| sample.ms).sum::<u64>() as f64 / 1_000.0
    );
    println!("       порог  подписано  из подписанного неверно  не опознано");

    for accept in ACCEPT {
        let mut named = 0u64;
        let mut wrong = 0u64;
        let mut unknown = 0u64;
        for sample in &trials {
            match best_match(&sample.vector, &prints, *accept, MARGIN) {
                Match::Named { name, .. } => {
                    named += sample.ms;
                    if name != sample.truth {
                        wrong += sample.ms;
                    }
                }
                Match::Unknown { .. } => unknown += sample.ms,
            }
        }
        let total = named + unknown;
        println!(
            "       {accept:.2} {:>9.0}% {:>23.0}% {:>12.0}%",
            percent(named, total),
            percent(wrong, named),
            percent(unknown, total),
        );
    }
    println!(
        "      Главный столбец — средний: неверная подпись убедительна, и человек\n\
         \x20    на неё полагается. Неопознанное безобидно, оно видно как есть."
    );
}

/// Пороги для развёртки.
///
/// Снизу — то, что стоит в движке; сверху — заведомо слишком много.
/// Шаг крупный намеренно: проход по получасовой дорожке идёт минутами, и
/// мелкая сетка превращает замер в вечер.
const SWEEP: &[f32] = &[
    0.65, 0.70, 0.75, 0.80, 0.85, 0.90, 0.95, 1.00, 1.10, 1.20, 1.30,
];

/// Верхний конец доведён до 1.30 по живому прогону: на встрече
/// пользователя лучшим оказался 1.00 — то есть **край**, за который никто
/// не смотрел, — и настоящий оптимум лежал выше. Косинусное расстояние
/// между единичными векторами доходит до 2.0, так что 1.30 ещё не потолок,
/// но дальше начинает сливать людей.
///
/// Если голосов много и на верхнем конце, дело не в пороге: следующий
/// подозреваемый — `MIN_DURATION_ON`, отрезок в 0.3 с для слепка голоса
/// короток, и десятки коротких реплик дают десятки плохих векторов.
const _: () = ();

/// Перебрать пороги и сверить каждый с разметкой человека.
///
/// Ради чего: порог по умолчанию подобран на чужих записях по минуте, и
/// на настоящей встрече он оказался не тот — шестеро людей разошлись по
/// десяткам голосов. Гадать о новом значении незачем, когда есть
/// разметка: она и говорит, какой порог сходится **с нашей речью**.
///
/// Строки печатаются по мере счёта, а не в конце: прогон идёт минутами на
/// каждый порог, и молчащий прибор неотличим от повисшего.
fn sweep_thresholds(
    root: &Path,
    session_id: &str,
    engine: &mut dyn Diarizer,
) -> Result<(), String> {
    if !engine.set_cluster_threshold(SWEEP[0]) {
        return Err(
            "у этого движка порога нет — перебирать нечего (собрано без --features model?)"
                .to_string(),
        );
    }

    let store = AudioManifestStore::open(root).map_err(|error| error.to_string())?;
    let tracks = tracks(&store, session_id)?;

    let mut done = 0usize;
    for (channel, pcm) in tracks {
        let labels = human_labels(&store, session_id, channel).unwrap_or_default();
        if labels.is_empty() {
            println!(
                "\n  Дорожка {} — разметки человека нет, перебирать не с чем",
                channel.code()
            );
            continue;
        }
        done += 1;

        let labelled_ms: u64 = labels.iter().map(compare::Labelled::duration_ms).sum();
        println!(
            "\n  Дорожка {} — {} реплик разметки, {:.1} с",
            channel.code(),
            labels.len(),
            labelled_ms as f64 / 1_000.0
        );
        println!("   порог  голосов  накрыто  в свой голос  худшая цельность  худшая чистота");

        let mut rows: Vec<(f32, u32, f64)> = Vec::new();
        for threshold in SWEEP {
            engine.set_cluster_threshold(*threshold);
            let report = engine.diarize(&pcm, RATE);
            if let Some(reason) = report.refused {
                println!("   {threshold:.2}  отказ: {reason}");
                continue;
            }
            let seen = compare::compare(&labels, &report.turns);
            let worst = |values: Vec<f64>| values.into_iter().fold(1.0f64, f64::min);
            println!(
                "   {threshold:.2} {:>8} {:>8.0}% {:>12.0}% {:>16.0}% {:>14.0}%",
                report.speakers_found,
                seen.coverage() * 100.0,
                seen.accuracy() * 100.0,
                worst(
                    seen.per_speaker_wholeness()
                        .into_iter()
                        .map(|(_, whole, _)| whole)
                        .collect()
                ) * 100.0,
                worst(
                    seen.per_cluster_purity()
                        .into_iter()
                        .map(|(_, purity, _)| purity)
                        .collect()
                ) * 100.0,
            );
            rows.push((*threshold, report.speakers_found, seen.accuracy()));
        }

        summarise(&rows);
    }

    if done == 0 {
        return Err(
            "ни на одной дорожке нет разметки человека — перебирать пороги не с чем".to_string(),
        );
    }
    Ok(())
}

/// Назвать лучший порог — и сперва проверить, что перебор вообще что-то
/// перебирал.
///
/// Одинаковое число голосов на всех порогах означает, что `set_config`
/// молча ничего не сделал, и таблица выше — семь раз одна и та же строка
/// под разными заголовками. Прибор, печатающий такое как результат, врёт
/// убедительнее любого пропуска.
fn summarise(rows: &[(f32, u32, f64)]) -> Sweep {
    let verdict = judge_sweep(rows);
    match &verdict {
        Sweep::TooShort => {}
        Sweep::Stuck(found) => println!(
            "    ! на всех порогах ровно {found} голосов — порог не переставляется, и\n\
             \x20     строки выше это одна и та же строка. Верить им нельзя"
        ),
        Sweep::AtTheEdge {
            threshold,
            found,
            accuracy,
            higher,
        } => println!(
            "    ! лучшим вышел {threshold:.2} — это {} перебора, дальше не смотрели.\n\
             \x20     {found} голосов, {:.0}% попало в свой, и оптимум может лежать\n\
             \x20     {}. Диапазон надо расширить, а это число за ответ не брать",
            if *higher {
                "верхний край"
            } else {
                "нижний край"
            },
            accuracy * 100.0,
            if *higher { "выше" } else { "ниже" },
        ),
        Sweep::Best {
            thresholds,
            found,
            accuracy,
        } => {
            let middle = thresholds[thresholds.len() / 2];
            if thresholds.len() == 1 {
                println!(
                    "    Лучше всего сходится порог {middle:.2}: {found} голосов, {:.0}%\n\
                     \x20   попало в свой.",
                    accuracy * 100.0
                );
            } else {
                println!(
                    "    Одинаково хорошо сходятся пороги {:.2}…{:.2} ({:.0}% попало в свой).\n\
                     \x20   Брать надо середину — {middle:.2}, {found} голосов, — а не край:\n\
                     \x20   на краю плато соседняя запись съедет с него первой же.",
                    thresholds[0],
                    thresholds[thresholds.len() - 1],
                    accuracy * 100.0
                );
            }
            println!(
                "    Это **эта** запись, а не общий ответ; на другой встрече проверять\n\
                 \x20   заново."
            );
        }
    }
    verdict
}

/// Чем кончился перебор.
#[derive(Debug, PartialEq)]
pub enum Sweep {
    /// Строк меньше двух — сравнивать нечего.
    TooShort,
    /// Число голосов не сдвинулось ни на одном пороге.
    Stuck(u32),
    /// Лучший результат на **краю** перебора: оптимум может лежать за ним.
    ///
    /// Найдено живым прогоном: на дорожке mic лучшим вышел верхний порог
    /// диапазона, и прибор объявил его победителем, не заметив, что дальше
    /// не смотрели вовсе. Тот же капкан края, что и у плато, только теперь
    /// у границы перебора.
    AtTheEdge {
        threshold: f32,
        found: u32,
        accuracy: f64,
        /// Куда двигать границу.
        higher: bool,
    },
    /// Пороги, сошедшиеся лучше всех.
    ///
    /// Их бывает несколько, и это не мелочь: при равном результате взять
    /// верхний или нижний значит сесть на **край** плато, откуда соседняя
    /// запись съедет первой же. Поэтому список, а не одно число, — и
    /// берётся из него середина.
    Best {
        thresholds: Vec<f32>,
        found: u32,
        accuracy: f64,
    },
}

/// Насколько два результата считаются одинаковыми.
///
/// Полпроцента: доли считаются из целых миллисекунд, и точное равенство
/// поплыло бы от одного лишнего отсчёта на границе отрезка.
const SAME_ACCURACY: f64 = 0.005;

/// Вердикт отдельно от печати: ветка, видимая только на экране, тестом не
/// проверяется и снимается потом незамеченной.
fn judge_sweep(rows: &[(f32, u32, f64)]) -> Sweep {
    if rows.len() < 2 {
        return Sweep::TooShort;
    }
    if rows.iter().all(|(_, found, _)| *found == rows[0].1) {
        return Sweep::Stuck(rows[0].1);
    }
    let best = rows
        .iter()
        .max_by(|a, b| a.2.total_cmp(&b.2))
        .expect("строки есть");
    let plateau: Vec<&(f32, u32, f64)> = rows
        .iter()
        .filter(|(_, _, accuracy)| best.2 - accuracy <= SAME_ACCURACY)
        .collect();

    // Лучшее на самом краю означает, что за краем не смотрели, а не что
    // там хуже. Плато из одной точки на границе — тот же случай.
    let lowest = rows[0].0;
    let highest = rows[rows.len() - 1].0;
    if plateau.len() == 1 && (best.0 == lowest || best.0 == highest) {
        return Sweep::AtTheEdge {
            threshold: best.0,
            found: best.1,
            accuracy: best.2,
            higher: best.0 == highest,
        };
    }

    let middle = plateau[plateau.len() / 2];
    Sweep::Best {
        thresholds: plateau.iter().map(|(threshold, ..)| *threshold).collect(),
        found: middle.1,
        accuracy: best.2,
    }
}

/// Сверить найденные голоса с тем, что человек разметил в Final.
///
/// Лучший контроль из возможных, и единственный на нашей речи: чужие
/// записи отвечают только на «видит ли движок смену вообще», а здесь
/// видно, **тех ли** он разделил.
///
/// Отсутствие разметки — не ошибка и не молчание: печатается строкой, что
/// сверять не с чем и почему.
fn print_against_labels(
    store: &AudioManifestStore,
    meeting_id: &str,
    channel: AudioChannel,
    turns: &[diarize::VoiceTurn],
) {
    let labels = match human_labels(store, meeting_id, channel) {
        Ok(labels) => labels,
        Err(error) => {
            println!("\n    Сверить с разметкой не вышло: {error}");
            return;
        }
    };
    if labels.is_empty() {
        println!(
            "\n    Сверять не с чем: на этой дорожке нет реплик, которым спикера\n\
             \x20   поставил человек. Массовое назначение по каналу за разметку не\n\
             \x20   считается — оно говорит то же, что и сам канал, и совпадение с\n\
             \x20   ним ничего бы не значило."
        );
        return;
    }

    let seen = compare::compare(&labels, turns);
    println!(
        "\n    Сверка с разметкой человека: {} реплик, {:.1} с",
        labels.len(),
        seen.labelled_ms as f64 / 1_000.0
    );
    // Покрытие первым: при низком покрытии проценты ниже описывают
    // крохотный кусок встречи, а читаются как ответ про неё целиком.
    println!(
        "      движок накрыл отрезками {:.0}% размеченного времени",
        seen.coverage() * 100.0
    );
    if seen.covered_ms == 0 {
        println!("      пересечений нет вовсе — сравнивать нечего");
        return;
    }
    println!(
        "      из накрытого в «свой» голос попало {:.0}%",
        seen.accuracy() * 100.0
    );

    println!("\n      кто на какой голос лёг:");
    for (speaker, cluster, ms) in &seen.mapping {
        println!(
            "        {speaker:<24} голос {cluster:<3} {:.1} с",
            *ms as f64 / 1_000.0
        );
    }

    let split: Vec<String> = seen
        .per_speaker_wholeness()
        .into_iter()
        .filter(|(_, whole, _)| *whole < 0.9)
        .map(|(speaker, whole, ms)| {
            format!(
                "{speaker} — {:.0}% от своих {:.1} с",
                whole * 100.0,
                ms as f64 / 1_000.0
            )
        })
        .collect();
    if !split.is_empty() {
        println!("      разорваны на несколько голосов: {}", split.join("; "));
    }

    let mixed: Vec<String> = seen
        .per_cluster_purity()
        .into_iter()
        .filter(|(_, purity, _)| *purity < 0.9)
        .map(|(cluster, purity, ms)| {
            format!(
                "голос {cluster} — {:.0}% от своих {:.1} с",
                purity * 100.0,
                ms as f64 / 1_000.0
            )
        })
        .collect();
    if !mixed.is_empty() {
        println!("      смешали нескольких людей: {}", mixed.join("; "));
    }

    println!(
        "\n      Расхождение — не обязательно ошибка движка. Человек размечал\n\
         \x20     реплики в транскрипте, а не голоса на слух: если внутри реплики\n\
         \x20     заговорил второй, разметка этого не знает, а движок мог услышать\n\
         \x20     верно. Судить надо прослушиванием спорных отрезков."
    );
}

/// Реплики, которым спикера поставил **человек**, а не канал.
///
/// `speaker_pinned` — тот самый признак: массовое назначение по каналу
/// такие сегменты не трогает (миграция 6). Брать вместо него все
/// назначенные было бы самообманом: подпись по каналу повторяет то, что
/// и так известно из дорожки, и совпадение с ней ничего не проверяет.
fn human_labels(
    store: &AudioManifestStore,
    meeting_id: &str,
    channel: AudioChannel,
) -> Result<Vec<compare::Labelled>, String> {
    let versions = store
        .list_final_transcripts(meeting_id)
        .map_err(|error| error.to_string())?;
    // Список идёт от новых к старым: разметка живёт в последней версии,
    // на неё пересбор и переносит ручные решения.
    let Some(latest) = versions.first() else {
        return Err("у встречи нет собранного Final".to_string());
    };
    let segments = store
        .list_final_segments(meeting_id, latest.version)
        .map_err(|error| error.to_string())?;

    // Имя, а не id: отчёт читает человек, и «спикер a3f1…» ему ничего не
    // говорит. Имени нет — остаётся id, но подменять его пустой строкой
    // нельзя: две безымянных строки слились бы в одного человека.
    let names: std::collections::BTreeMap<String, String> = store
        .list_speakers(meeting_id)
        .map_err(|error| error.to_string())?
        .into_iter()
        .filter(|speaker| !speaker.display_name.trim().is_empty())
        .map(|speaker| (speaker.id, speaker.display_name))
        .collect();

    Ok(segments
        .into_iter()
        .filter(|segment| {
            segment.channel == channel && segment.speaker_pinned && !segment.speaker_id.is_empty()
        })
        .map(|segment| compare::Labelled {
            speaker: names
                .get(&segment.speaker_id)
                .cloned()
                .unwrap_or(segment.speaker_id),
            start_ms: segment.start_ms,
            end_ms: segment.end_ms,
        })
        .collect())
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

    /// Двойник, у которого число зависит от **расположения**, а не от
    /// длины: смотрит, громко ли начинается дорожка.
    ///
    /// Удвоение его не ловит — удвоенная запись начинается так же, как
    /// исходная. Ловит только перестановка половин, ради которой её и
    /// завели: по одному удвоению неустойчивость на настоящем движке была
    /// принята за рост от количества материала.
    struct UnstableUnderReordering;

    impl Diarizer for UnstableUnderReordering {
        fn diarize(&mut self, pcm: &[i16], sample_rate: u32) -> DiarizeReport {
            let ms = pcm.len() as u64 * 1_000 / u64::from(sample_rate);
            let head = &pcm[..pcm.len().min(1_000)];
            let loud = head.iter().any(|sample| sample.abs() > 1_000);
            let turns = if loud {
                vec![VoiceTurn::new(0, ms / 2, 0), VoiceTurn::new(ms / 2, ms, 1)]
            } else {
                vec![VoiceTurn::new(0, ms, 0)]
            };
            DiarizeReport::from_turns(turns)
        }
    }

    fn problem(engine: &mut dyn Diarizer, controls: &[Control], needle: &str) {
        let seen = self_check(engine, controls);
        assert!(
            seen.problems.iter().any(|line| line.contains(needle)),
            "вердикт не назвал «{needle}»: {seen:?}"
        );
    }

    /// Предупреждение — не беда: проверяется, что оно **сказано** и что
    /// прогон при этом продолжается.
    fn note(engine: &mut dyn Diarizer, controls: &[Control], needle: &str) {
        let seen = self_check(engine, controls);
        assert!(
            seen.notes.iter().any(|line| line.contains(needle)),
            "предупреждение не названо «{needle}»: {seen:?}"
        );
        assert!(
            seen.problems.is_empty(),
            "неточность остановила прогон: {seen:?}"
        );
    }

    /// Вердикт бывает зелёным. Без этого «прибор красный» ничего не
    /// значит — он мог бы быть красным всегда.
    #[test]
    fn the_self_check_passes_a_working_diarizer() {
        let seen = self_check(&mut PitchControl, &[two_voices(), one_voice()]);
        assert!(seen.problems.is_empty(), "{seen:?}");
        assert!(
            seen.notes.is_empty(),
            "рабочий движок получил предупреждение: {seen:?}"
        );
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
        let seen = self_check(&mut PitchControl, &[]);

        assert!(
            seen.problems
                .iter()
                .any(|line| line.contains("проверить его нечем")),
            "{seen:?}"
        );
        assert!(
            !seen.problems.iter().any(|line| line.contains("слились")),
            "движок объявлен слепым без материала: {seen:?}"
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
        note(&mut AlwaysSplits, &[one_voice()], "разорвал");
        note(
            &mut MultipliesWithLength,
            &[two_voices()],
            "переложенные иначе",
        );
    }

    /// Неустойчивость по расположению ловится **только** перестановкой
    /// половин: удвоение начинается так же, как исходная запись, и разницы
    /// не видит вовсе.
    ///
    /// Ради этого случая перестановка и заведена: на настоящем движке
    /// неустойчивость по одному удвоению была прочитана как рост от
    /// количества материала, и вывод оказался неверным.
    #[test]
    fn reordering_catches_what_doubling_misses() {
        // Контроль начинается громко: движок обязан увидеть двоих.
        let loud_then_quiet = control(
            "2-loud-first.wav",
            2,
            [
                voice(110.0, CASE_MS),
                vec![0i16; (RATE as u64 * CASE_MS / 1_000) as usize],
            ]
            .concat(),
        );

        let mut engine = UnstableUnderReordering;
        // Материал обязан пройти положительный случай, иначе тест
        // утверждал бы о неустойчивости на слепом движке.
        let straight = engine.diarize(&loud_then_quiet.pcm, RATE);
        assert_eq!(straight.speakers_found, 2, "двойник не нашёл двоих");

        // Удвоение слепо к этой неустойчивости — это и есть смысл теста.
        let doubled: Vec<i16> = loud_then_quiet
            .pcm
            .iter()
            .chain(loud_then_quiet.pcm.iter())
            .copied()
            .collect();
        assert_eq!(
            engine.diarize(&doubled, RATE).speakers_found,
            2,
            "удвоение обязано дать то же число — иначе ловит оно, а не перестановка"
        );

        // А перестановка — видит, и вердикт при этом не смертельный.
        note(
            &mut UnstableUnderReordering,
            &[loud_then_quiet],
            "переставленными половинами",
        );
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

    /// Заполнить Final с разметкой: часть реплик поставил человек, часть
    /// — массовое назначение по каналу.
    fn seed_final(root: &Path, meeting_id: &str, segments: Vec<domain::FinalSegment>) {
        let mut store = AudioManifestStore::open(root).expect("store");
        store
            .upsert_final_transcript(&domain::FinalTranscript {
                meeting_id: meeting_id.to_string(),
                version: 1,
                body_markdown: String::new(),
                created_at_ms: 0,
            })
            .expect("транскрипт");
        store
            .replace_final_segments(meeting_id, 1, &segments)
            .expect("сегменты");
        for (id, name) in [("s-anya", "Аня"), ("s-borya", "Боря")] {
            store
                .upsert_speaker(&domain::Speaker {
                    id: id.to_string(),
                    meeting_id: meeting_id.to_string(),
                    display_name: name.to_string(),
                    sort_index: 0,
                })
                .expect("спикер");
        }
    }

    fn final_segment(
        index: u32,
        start_ms: u64,
        end_ms: u64,
        channel: AudioChannel,
        speaker_id: &str,
        pinned: bool,
    ) -> domain::FinalSegment {
        domain::FinalSegment {
            index,
            start_ms,
            end_ms,
            channel,
            speaker_id: speaker_id.to_string(),
            speaker_pinned: pinned,
            text: "реплика".to_string(),
            text_edited: false,
            original_text: String::new(),
        }
    }

    /// Половина прибора, читающая разметку, проверяется на заведомом
    /// случае: три реплики, из них человек поставил спикера одной.
    ///
    /// Проверяются сразу три вещи, и каждая — про молчаливую подмену:
    /// берутся только ручные, только свой канал, и имя человека вместо
    /// непрозрачного id.
    #[test]
    fn only_the_segments_a_person_pinned_count_as_labels() {
        let root = tmp_root("labels");
        let _ = std::fs::remove_dir_all(&root);
        seed(&root, "m1", &voice(110.0, CASE_MS));
        seed_final(
            &root,
            "m1",
            vec![
                // Ручная — идёт в эталон.
                final_segment(0, 1_000, 5_000, AudioChannel::Mic, "s-anya", true),
                // Массовое назначение по каналу — не идёт: оно повторяет
                // то, что и так известно из дорожки.
                final_segment(1, 5_000, 9_000, AudioChannel::Mic, "s-borya", false),
                // Ручная, но на другой дорожке — не идёт в этот канал.
                final_segment(2, 9_000, 12_000, AudioChannel::System, "s-borya", true),
            ],
        );

        let store = AudioManifestStore::open(&root).expect("store");
        let labels = human_labels(&store, "m1", AudioChannel::Mic).expect("разметка");

        assert_eq!(
            labels.len(),
            1,
            "взято лишнее или потеряно своё: {labels:?}"
        );
        assert_eq!(labels[0].speaker, "Аня", "показан id вместо имени");
        assert_eq!((labels[0].start_ms, labels[0].end_ms), (1_000, 5_000));
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Final у встречи нет — отказ с причиной, а не пустая разметка.
    ///
    /// Пустая разметка и отсутствие Final читались бы одинаково: «сверять
    /// не с чем», — а чинятся по-разному.
    #[test]
    fn a_meeting_without_a_final_is_refused_by_reason() {
        let root = tmp_root("no-final");
        let _ = std::fs::remove_dir_all(&root);
        seed(&root, "m1", &voice(110.0, CASE_MS));

        let store = AudioManifestStore::open(&root).expect("store");
        let error = human_labels(&store, "m1", AudioChannel::Mic).expect_err("Final нет");

        assert!(error.contains("Final"), "{error}");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Порог, который ничего не переставляет, обязан быть назван.
    ///
    /// Самая опасная поломка перебора: таблица из семи одинаковых строк
    /// под разными заголовками выглядит как результат, и «лучший порог»
    /// из неё был бы взят с потолка.
    #[test]
    fn a_threshold_that_does_nothing_is_caught() {
        let stuck = vec![(0.65, 82, 0.61), (0.75, 82, 0.61), (0.85, 82, 0.61)];

        assert_eq!(judge_sweep(&stuck), Sweep::Stuck(82));
    }

    /// Лучшее на краю перебора — не ответ, а признак, что диапазон узок.
    ///
    /// Найдено живым прогоном: на дорожке mic лучшим вышел верхний порог
    /// диапазона (14 голосов, 81%), и прибор объявил его победителем, хотя
    /// дальше просто не смотрели. Настоящий оптимум лежал выше.
    #[test]
    fn a_winner_at_the_edge_of_the_range_is_not_an_answer() {
        let rising = vec![(0.65, 61, 0.61), (0.85, 27, 0.69), (1.00, 14, 0.81)];

        match judge_sweep(&rising) {
            Sweep::AtTheEdge {
                threshold, higher, ..
            } => {
                assert!((threshold - 1.00).abs() < 1e-6);
                assert!(higher, "край назван нижним, а он верхний");
            }
            other => panic!("край не замечен: {other:?}"),
        }
    }

    /// Нижний край — тот же случай, и сторона названа верно.
    #[test]
    fn a_winner_at_the_bottom_edge_points_downwards() {
        let falling = vec![(0.65, 6, 0.90), (0.85, 20, 0.70), (1.00, 30, 0.50)];

        match judge_sweep(&falling) {
            Sweep::AtTheEdge { higher, .. } => assert!(!higher, "край назван верхним"),
            other => panic!("край не замечен: {other:?}"),
        }
    }

    /// Пик внутри диапазона краем не считается — иначе предупреждение
    /// печаталось бы всегда и перестало что-либо значить.
    ///
    /// Настоящий случай: на дорожке system пик пришёлся на 0.90, а на
    /// 0.95 и 1.00 результат уже падал.
    #[test]
    fn a_peak_inside_the_range_is_a_real_answer() {
        let peak = vec![(0.85, 25, 0.85), (0.90, 20, 0.90), (0.95, 14, 0.86)];

        match judge_sweep(&peak) {
            Sweep::Best { thresholds, .. } => assert_eq!(thresholds, vec![0.90]),
            other => panic!("пик внутри диапазона принят за край: {other:?}"),
        }
    }

    /// Лучшим считается порог с наибольшей долей попавших в свой голос,
    /// а не с наименьшим числом голосов: мало голосов бывает и от того,
    /// что движок слил всех в одного.
    #[test]
    fn the_best_row_is_the_one_that_agrees_most() {
        let rows = vec![(0.65, 82, 0.61), (0.85, 6, 0.88), (0.95, 1, 0.30)];

        assert_eq!(
            judge_sweep(&rows),
            Sweep::Best {
                thresholds: vec![0.85],
                found: 6,
                accuracy: 0.88
            }
        );
    }

    /// Равные результаты — плато, и берётся его середина, а не край.
    ///
    /// Край выглядит так же хорошо на **этой** записи и съезжает на
    /// следующей. Ровно та же ошибка, что была бы с порогом 0.60 на
    /// контрольных записях: он тоже сходился, но стоял у самого обрыва.
    #[test]
    fn equal_results_are_a_plateau_and_the_middle_is_taken() {
        let rows = vec![
            (0.65, 6, 0.90),
            (0.70, 6, 0.90),
            (0.75, 6, 0.90),
            (0.90, 1, 0.40),
        ];

        let Sweep::Best {
            thresholds, found, ..
        } = judge_sweep(&rows)
        else {
            panic!("плато не найдено");
        };

        assert_eq!(thresholds, vec![0.65, 0.70, 0.75], "плато обрезано");
        assert_eq!(found, 6, "число голосов взято не из середины");
    }

    /// Одна строка — не перебор, и вердикта по ней нет.
    #[test]
    fn a_single_row_is_not_a_sweep() {
        assert_eq!(judge_sweep(&[(0.65, 82, 0.61)]), Sweep::TooShort);
        assert_eq!(judge_sweep(&[]), Sweep::TooShort);
    }

    /// Заглушка порога не имеет, и говорит об этом честно: `true` от неё
    /// заставил бы перебор печатать одно и то же под семью заголовками.
    #[test]
    fn an_engine_without_a_threshold_says_so() {
        assert!(!MockDiarizer::new().set_cluster_threshold(0.8));
        assert!(!NeverSplits.set_cluster_threshold(0.8));
    }

    fn sample(truth: &str, vector: Vec<f32>) -> Sample {
        Sample {
            truth: truth.to_string(),
            vector,
            ms: 1_000,
        }
    }

    fn grouped(samples: &[Sample]) -> BTreeMap<&str, Vec<&Sample>> {
        let mut out: BTreeMap<&str, Vec<&Sample>> = BTreeMap::new();
        for sample in samples {
            out.entry(&sample.truth).or_default().push(sample);
        }
        out
    }

    /// **Главный тест всей затеи.** Ни один кусок не идёт и в слепок, и в
    /// проверку.
    ///
    /// Слепок, сложенный по куску, на этом же куске похож на себя почти
    /// идеально: пересечение дало бы отчёт прекрасный и пустой. Ошибка
    /// того же рода, что «тест, который может пройти на пустом входе», но
    /// дороже — там ноль, здесь девяносто процентов.
    #[test]
    fn no_sample_is_both_fitted_and_tested() {
        let samples: Vec<Sample> = (0..10)
            .map(|index| sample("аня", vec![index as f32, 1.0]))
            .collect();
        let grouped = grouped(&samples);

        let (prints, trials) = split(&grouped, &|index, _| index % 2 == 0);

        assert_eq!(prints.len(), 1, "слепок не сложился");
        assert_eq!(prints[0].1.samples, 5, "в слепок ушло не то число кусков");
        assert_eq!(trials.len(), 5, "отложено не то число кусков");
        // Ни один вектор из проверки не должен встречаться среди
        // обучающих: сравниваем по самим векторам, а не по номерам.
        let fitted: Vec<&Vec<f32>> = samples
            .iter()
            .enumerate()
            .filter(|(index, _)| index % 2 == 0)
            .map(|(_, sample)| &sample.vector)
            .collect();
        for trial in &trials {
            assert!(
                !fitted.contains(&&trial.vector),
                "кусок {:?} попал и в слепок, и в проверку",
                trial.vector
            );
        }
    }

    /// Деление идёт внутри человека, а не по общему порядку.
    ///
    /// Общий порядок отдал бы одного целиком в обучение, другого целиком
    /// в проверку, и слепка для второго не оказалось бы вовсе — а таблица
    /// при этом что-то бы печатала.
    #[test]
    fn each_person_gets_both_a_print_and_trials() {
        let mut samples = Vec::new();
        for index in 0..6 {
            samples.push(sample("аня", vec![1.0, index as f32 / 10.0]));
        }
        for index in 0..6 {
            samples.push(sample("боря", vec![0.0, 1.0 + index as f32 / 10.0]));
        }
        let grouped = grouped(&samples);

        let (prints, trials) = split(&grouped, &|index, _| index % 2 == 0);

        assert_eq!(prints.len(), 2, "слепок вышел не у каждого: {prints:?}");
        assert!(
            trials.iter().any(|t| t.truth == "аня") && trials.iter().any(|t| t.truth == "боря"),
            "в проверке не все"
        );
    }

    /// Заведомо разделимый случай: у каждого свой вектор с малым шумом.
    /// Слепки обязаны узнать отложенные куски.
    ///
    /// Без этого положительного случая «никого не подписали» было бы
    /// неотличимо от осторожности.
    #[test]
    fn clearly_different_voices_are_recognised_on_held_out_samples() {
        let mut samples = Vec::new();
        for index in 0..6 {
            let jitter = index as f32 / 100.0;
            samples.push(sample("аня", vec![1.0, jitter]));
            samples.push(sample("боря", vec![jitter, 1.0]));
        }
        let grouped = grouped(&samples);
        let (prints, trials) = split(&grouped, &|index, _| index % 2 == 0);
        assert_eq!(prints.len(), 2);
        assert!(!trials.is_empty(), "проверять нечего");

        let named = trials
            .iter()
            .filter(|trial| {
                matches!(
                    best_match(&trial.vector, &prints, 0.8, MARGIN),
                    Match::Named { ref name, .. } if *name == trial.truth
                )
            })
            .count();

        assert_eq!(
            named,
            trials.len(),
            "узнаны не все: {named} из {}",
            trials.len()
        );
    }

    /// Заведомо неразделимый случай: у всех один и тот же голос. Никто не
    /// должен быть подписан — отрыва нет.
    #[test]
    fn indistinguishable_voices_are_left_unknown() {
        let mut samples = Vec::new();
        for _ in 0..6 {
            samples.push(sample("аня", vec![1.0, 0.0]));
            samples.push(sample("боря", vec![1.0, 0.0]));
        }
        let grouped = grouped(&samples);
        let (prints, trials) = split(&grouped, &|index, _| index % 2 == 0);
        assert_eq!(prints.len(), 2, "слепки не сложились");

        let named = trials
            .iter()
            .filter(|trial| {
                matches!(
                    best_match(&trial.vector, &prints, 0.5, MARGIN),
                    Match::Named { .. }
                )
            })
            .count();

        assert_eq!(named, 0, "подписаны {named} кусков при одинаковых голосах");
    }

    /// Деление «первая треть» идёт внутри каждого человека, а не по
    /// общему порядку.
    ///
    /// Найдено мутацией: перемежающееся деление к этому нечувствительно —
    /// при любом порядке каждый второй кусок достаётся обучению. А вот
    /// «первая треть», посчитанная по общему списку, отдала бы её целиком
    /// первому человеку, и слепка для второго не вышло бы вовсе. Таблица
    /// при этом печаталась бы как ни в чём не бывало.
    #[test]
    fn the_first_third_is_taken_from_each_person_not_from_the_list() {
        let mut samples = Vec::new();
        for index in 0..6 {
            samples.push(sample("аня", vec![1.0, index as f32 / 10.0]));
        }
        for index in 0..6 {
            samples.push(sample("боря", vec![0.0, 1.0 + index as f32 / 10.0]));
        }
        let grouped = grouped(&samples);

        let (prints, trials) = split(&grouped, &|index, total| index * 3 < total);

        assert_eq!(
            prints.len(),
            2,
            "слепок вышел не у каждого — треть отрезана по общему списку: {prints:?}"
        );
        assert!(
            prints.iter().all(|(_, print)| print.samples == 2),
            "в слепок ушла не треть от каждого: {prints:?}"
        );
        assert!(
            trials.iter().any(|t| t.truth == "аня") && trials.iter().any(|t| t.truth == "боря"),
            "в проверке не все"
        );
    }

    /// Первая треть против остального: слепок из начала, проверка по
    /// концу. Пересечения по-прежнему нет.
    #[test]
    fn the_first_third_split_holds_out_the_rest() {
        let samples: Vec<Sample> = (0..9)
            .map(|index| sample("аня", vec![1.0, index as f32]))
            .collect();
        let grouped = grouped(&samples);

        let (prints, trials) = split(&grouped, &|index, total| index * 3 < total);

        assert_eq!(prints[0].1.samples, 3, "в слепок ушла не треть");
        assert_eq!(trials.len(), 6, "отложено не две трети");
    }

    fn reply(labelled: &str, pinned: bool, vector: Vec<f32>, ms: u64) -> Reply {
        Reply {
            ms,
            labelled: labelled.to_string(),
            pinned,
            vector,
        }
    }

    /// Прогон по всем репликам подписывает похожие и оставляет чужого
    /// неопознанным.
    ///
    /// Заведомо положительный и заведомо отрицательный случай в одном:
    /// двое своих обязаны получить имена, а третий, чьего слепка нет, —
    /// остаться без. Без второй половины «подписано 100%» значило бы, что
    /// схема подписывает всех подряд.
    #[test]
    fn a_stranger_without_a_print_stays_unknown() {
        let prints = vec![
            (
                "аня".to_string(),
                build_print(&[(vec![1.0, 0.0, 0.0], 1.0)]).expect("слепок"),
            ),
            (
                "боря".to_string(),
                build_print(&[(vec![0.0, 1.0, 0.0], 1.0)]).expect("слепок"),
            ),
        ];
        let replies = vec![
            reply("аня", true, vec![1.0, 0.05, 0.0], 1_000),
            reply("боря", true, vec![0.05, 1.0, 0.0], 1_000),
            // Третий человек: ортогонален обоим слепкам.
            reply("", false, vec![0.0, 0.0, 1.0], 1_000),
        ];

        let (named, per_person) = run_prints(&replies, &prints, 0.9);

        assert_eq!(named, 2_000, "подписано не двое: {per_person:?}");
        assert_eq!(per_person.len(), 2, "чужой получил имя: {per_person:?}");
        assert!(per_person.iter().all(|(_, ms)| *ms == 1_000));
    }

    /// Время считается по репликам, а не по их числу: длинная реплика
    /// весит больше короткой, и отчёт про долю встречи иначе врал бы.
    #[test]
    fn time_is_counted_not_replies() {
        let prints = vec![(
            "аня".to_string(),
            build_print(&[(vec![1.0, 0.0], 1.0)]).expect("слепок"),
        )];
        let replies = vec![
            reply("аня", true, vec![1.0, 0.0], 10_000),
            reply("аня", true, vec![1.0, 0.0], 1_000),
        ];

        let (named, per_person) = run_prints(&replies, &prints, 0.9);

        assert_eq!(named, 11_000);
        assert_eq!(per_person[0].1, 11_000, "считались реплики, а не время");
    }

    /// Пересечение каналов считается по времени, а не по числу отрезков.
    ///
    /// Заведомо положительный и заведомо отрицательный случай: полное
    /// совпадение даёт всю длину, разнесённые отрезки — ноль.
    #[test]
    fn overlapping_channels_are_measured_in_time() {
        let same = vec![span(0, 10_000, "речь")];
        assert_eq!(shared_ms(&same, &same), 10_000);

        let apart = vec![span(20_000, 30_000, "речь")];
        assert_eq!(shared_ms(&same, &apart), 0);

        // Частичное наложение: 8000..10000.
        assert_eq!(shared_ms(&same, &[span(8_000, 12_000, "речь")]), 2_000);
    }

    /// Касание границами пересечением не является: конец одного отрезка и
    /// начало другого — ноль, а не отрицательная величина.
    #[test]
    fn touching_segments_do_not_overlap() {
        assert_eq!(
            shared_ms(&[span(0, 5_000, "речь")], &[span(5_000, 9_000, "речь")]),
            0
        );
    }

    fn span(start_ms: u64, end_ms: u64, text: &str) -> Span {
        Span {
            start_ms,
            end_ms,
            text: text.to_string(),
        }
    }

    /// Похожесть текстов считается по словам, а не по буквам: одно и то же
    /// распознанное дважды расходится пунктуацией и словом из десяти.
    #[test]
    fn the_same_phrase_recognised_twice_reads_as_the_same() {
        assert!(
            text_likeness("Давайте начнём, коллеги", "давайте начнем коллеги") >= 0.6,
            "{}",
            text_likeness("Давайте начнём, коллеги", "давайте начнем коллеги")
        );
        assert!(text_likeness("совершенно другая реплика", "давайте начнём") < 0.2);
        assert_eq!(
            text_likeness("", "что-то"),
            0.0,
            "пустое ни на что не похоже"
        );
    }

    /// Повтор засчитывается только при пересечении **и** по времени, и по
    /// тексту.
    ///
    /// Одного текста мало: в разговоре «да» и «угу» повторяются постоянно
    /// в разных местах, и считать их дублированием каналов значило бы
    /// объявить дефектом обычную речь.
    #[test]
    fn a_repeat_needs_both_the_time_and_the_words() {
        let mic = vec![span(0, 2_000, "давайте начнём коллеги")];

        // Тот же текст, но в другом месте записи — не повтор канала.
        let elsewhere = vec![span(60_000, 62_000, "давайте начнём коллеги")];
        assert_eq!(doubled_text(&mic, &elsewhere, SAME_TEXT), (0, 0));

        // Пересекается по времени, но говорят разное — тоже не повтор.
        let different = vec![span(1_000, 3_000, "совсем про другое речь")];
        assert_eq!(doubled_text(&mic, &different, SAME_TEXT), (0, 0));

        // И то и другое — повтор, с длительностью своей реплики.
        let same = vec![span(1_000, 3_000, "давайте начнём, коллеги")];
        assert_eq!(doubled_text(&mic, &same, SAME_TEXT), (1, 2_000));
    }

    /// Заведомо чистый случай: разные реплики в разное время — ноль.
    ///
    /// Без него «повторов не найдено» было бы неотличимо от прибора,
    /// который не ищет вовсе.
    #[test]
    fn two_honest_channels_show_no_repeats() {
        let mic = vec![
            span(0, 2_000, "я скажу первым"),
            span(5_000, 7_000, "и добавлю"),
        ];
        let system = vec![
            span(2_500, 4_000, "а я отвечу"),
            span(8_000, 9_000, "и я тоже"),
        ];

        assert_eq!(doubled_text(&mic, &system, SAME_TEXT), (0, 0));
        assert_eq!(shared_ms(&mic, &system), 0, "каналы не должны пересекаться");
    }

    /// Настоящие числа встречи: пересечение объясняется плотностью, и
    /// уликой не является.
    ///
    /// Прибор на этих же числах утверждал обратное — «один разговор в двух
    /// дорожках», — потому что смотрел на голую долю. Тест написан по
    /// самому случаю, чтобы вывод нельзя было вернуть незаметно.
    #[test]
    fn a_dense_channel_explains_the_overlap_by_itself() {
        // mic 1276.7 с речи, system 709.1 с, встреча 1319.5 с.
        let expected = expected_shared_ms(1_276_700, 709_100, 1_319_500);

        assert!(
            (686_000..=690_000).contains(&expected),
            "ожидаемое по плотности вышло {expected}"
        );
        assert!(
            !overlap_is_evidence(690_200, expected),
            "наблюдённые 690.2 с приняты за улику, хотя плотность даёт столько же"
        );
    }

    /// Заведомая улика: канал редко говорит, а пересечение почти полное.
    ///
    /// Без этого случая «улик нет» было бы неотличимо от прибора, который
    /// не признаёт улик никогда.
    #[test]
    fn a_quiet_channel_with_full_overlap_is_evidence() {
        // На mic речь лишь пятую часть встречи, а звучат вместе почти всё
        // время system.
        let expected = expected_shared_ms(200_000, 300_000, 1_000_000);

        assert_eq!(expected, 60_000, "плотность 20% от 300 с — это 60 с");
        assert!(
            overlap_is_evidence(280_000, expected),
            "полное пересечение при редкой речи не признано уликой"
        );
    }

    /// Нулевая длина встречи не даёт деления на ноль.
    #[test]
    fn an_empty_meeting_expects_nothing() {
        assert_eq!(expected_shared_ms(100, 100, 0), 0);
    }

    /// Заведомо один и тот же звук: та же дорожка, приглушённая и
    /// сдвинутая. Совпадение обязано найтись, и сдвиг — назваться.
    ///
    /// Приглушение здесь несущее: в микрофон копия попадает тихой, и
    /// прибор, который ловит только равные по громкости дорожки, на
    /// настоящих данных промолчал бы.
    #[test]
    fn the_same_sound_is_found_even_when_quiet_and_late() {
        // Слоги: громко-тихо по 200 мс, две секунды.
        let loud: Vec<i16> = (0..32_000)
            .map(|i| if (i / 3_200) % 2 == 0 { 6_000 } else { 60 })
            .collect();
        // Копия впятеро тише, сдвинутая на 100 мс (1600 отсчётов).
        let mut quiet = vec![30i16; 1_600];
        quiet.extend(loud.iter().map(|s| s / 5));

        let (value, lag) = envelope_match(&loud, &quiet, 16_000, MAX_ENVELOPE_LAG_MS);

        assert!(value >= SAME_SOUND, "тихая копия не найдена: {value:.2}");
        assert_eq!(lag, 100, "сдвиг назван неверно");
    }

    /// Заведомо разный звук: слоги идут вразнобой. Совпадения быть не
    /// должно, иначе прибор объявит дефектом любой разговор.
    #[test]
    fn different_speech_does_not_look_like_one_sound() {
        let one: Vec<i16> = (0..32_000)
            .map(|i| if (i / 3_200) % 2 == 0 { 6_000 } else { 60 })
            .collect();
        // Втрое более длинные слоги — те же уровни, другой ритм.
        let other: Vec<i16> = (0..32_000)
            .map(|i| if (i / 9_600) % 2 == 0 { 60 } else { 6_000 })
            .collect();

        let (value, _) = envelope_match(&one, &other, 16_000, MAX_ENVELOPE_LAG_MS);

        assert!(
            value < SAME_SOUND,
            "разная речь принята за один звук: {value:.2}"
        );
    }

    /// Тишина ни на что не похожа: постоянный уровень корреляции не
    /// создаёт, иначе две молчащие дорожки читались бы как одна.
    #[test]
    fn silence_matches_nothing() {
        let quiet = vec![0i16; 32_000];
        let speech: Vec<i16> = (0..32_000)
            .map(|i| if (i / 3_200) % 2 == 0 { 6_000 } else { 60 })
            .collect();

        assert_eq!(envelope_match(&quiet, &speech, 16_000, 500).0, 0.0);
        assert_eq!(envelope_match(&quiet, &quiet, 16_000, 500).0, 0.0);
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
