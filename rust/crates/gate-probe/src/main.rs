//! Прибор для гейта речи (Epic 16, Epic 18).
//!
//! Гейт по фону комнаты (`stt::NoiseGate`) заменил постоянный порог,
//! потому что константа не обслуживает и городской шум, и тихого
//! собеседника. Что он при этом дал — **неизвестно**: замера отдельно по
//! гейту нет, а прежние 7.8 Вт на «тишине» мерились ваттметром на Маке.
//!
//! Ватты меряются только там, но главная величина считается и здесь:
//! **сколько раз запустится модель**. Её видно из сохранённых дорожек
//! (ADR-006), и она же переводится в ватты.
//!
//! Доля пропущенных гейтом кадров — не она. Первая версия прибора
//! печатала «пропущенный кадр — это запуск Whisper», и это было неверно:
//! движок копит речь до `MIN_SPEECH_FRAMES` и держит темп
//! (`InferencePacer`), зато конец каждой реплики стоит отдельного
//! прохода. Одиночный всплеск, не дотянувший до порога речи, всё равно
//! обходится в проход — по проценту кадров этого не увидеть.
//!
//! Доля тоже печатается, и рядом с ней та же доля для постоянного порога
//! 450 — того самого, который городской шум пробивал. Одно число без
//! второго ничего не говорит: «гейт пропустил 4% кадров» звучит хорошо
//! ровно до вопроса, сколько пропускала константа на этой же записи.
//!
//! Каждый запуск начинается с заведомо шумного и заведомо речевого
//! случая. Правило писано кровью: `scripts/count-audio-taps.swift`
//! показал ноль tap'ов, ноль прочли как «утечки нет», а скрипт был слеп
//! (`CLAUDE.md`). Ноль пропущенных кадров от слепого прибора выглядит
//! точно так же, как тишина в комнате.

// Разбор WAV берётся у соседнего прибора **тем же файлом**, а не копией:
// формат один (16-битный моно PCM живого пути), и вторая реализация
// означала бы две правды об одном.
#[path = "../../diarize-probe/src/wav.rs"]
mod wav;

use std::path::Path;
use std::process::ExitCode;

use domain::AudioChannel;
use session::{ChannelMixer, MixedFrame};
use storage::AudioManifestStore;
use stt::{
    InferencePacer, MIN_SPEECH_FRAMES, NoiseGate, PARTIAL_MIN_FRAMES, SILENCE_FRAMES,
    SpeechDecider, frame_rms as rms,
};

/// Частота живого пути; ею же пишутся чанки на диск (ADR-005).
const RATE: u32 = 16_000;
/// Кадр живого пути: `AudioChunkPipeline.chunkDurationMs` в Swift.
const FRAME_MS: usize = 100;
/// Постоянный порог, стоявший до гейта. Ради сравнения с ним прибор и
/// печатает вторую колонку.
const OLD_THRESHOLD: f32 = 450.0;
/// Ширина строки в таблице по времени.
const BUCKET_MS: u64 = 10_000;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();

    // Сперва прибор, потом данные: обратный порядок позволил бы прочитать
    // чистый ноль там, где гейт просто ничего не считает.
    if !self_check() {
        eprintln!("\nПрибор слеп: до настоящих данных дело не дошло.");
        return ExitCode::FAILURE;
    }

    let result = match args.as_slice() {
        [] => {
            println!("\n{USAGE}");
            return ExitCode::SUCCESS;
        }
        [root] => list_sessions(Path::new(root)),
        [root, wav] if wav.ends_with(".wav") => wav_mode(Path::new(root), Path::new(wav)),
        [root, session] => probe(Path::new(root), session),
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
  gate-probe <каталог-данных>             — список сессий
  gate-probe <каталог-данных> <сессия>    — прогнать гейт по сессии

Каталог данных — тот, где лежит meetingraft.sqlite3.

Мерить надо запись комнаты в её обычном состоянии: открытое окно, улица,
никто не говорит. Число запусков модели на такой записи и есть то,
сколько работы достаётся Whisper на пустом месте.";

/// Заведомо шумный и заведомо речевой случай.
///
/// Возвращает `false`, если гейт принял городской шум за речь или не
/// пропустил заведомую речь. Печатаются обе доли и обе для старого
/// порога: важен не вердикт, а **разница** между приборами — ради неё
/// вторая колонка и заводилась.
fn self_check() -> bool {
    // Городской шум: ровный уровень 600. Постоянный порог 450 такой шум
    // пропускал целиком — с этого Epic 18 и начался.
    let noise: Vec<Vec<i16>> = (0..200).map(|_| frame_at(600.0)).collect();
    let quiet = gated(&noise);

    // Речь: всплески поверх тихой комнаты. Первые кадры — фон, иначе
    // гейту не от чего отсчитывать превышение.
    let mut speech: Vec<Vec<i16>> = (0..50).map(|_| frame_at(200.0)).collect();
    speech.extend((0..50).map(|_| frame_at(2_500.0)));
    let talking = gated(&speech);

    println!("Проверка прибора на синтетике");
    println!(
        "  городской шум: гейт пропустил {} кадров из {}, постоянный порог {OLD_THRESHOLD:.0} — {}",
        quiet.accepted, quiet.total, quiet.accepted_old
    );
    println!(
        "  речь:          гейт пропустил {} кадров из {} (речевых {})",
        talking.accepted, talking.total, 50
    );

    // Та же комната, но перед ней тишина: запись всегда начинается
    // раньше, чем в ней появляется звук. Оценка фона берётся с первого
    // кадра, а поднимать её во время «речи» гейт не станет — и комната,
    // которая сама себя не пробивала, начинает пробивать.
    let mut after_silence: Vec<Vec<i16>> = (0..10).map(|_| frame_at(0.0)).collect();
    after_silence.extend((0..100).map(|_| frame_at(177.0)));
    let restarted = gated(&after_silence);
    println!(
        "  комната после тишины: гейт пропустил {} кадров из {} (тот же уровень, что комната сама по себе)",
        restarted.accepted, 100
    );
    if restarted.accepted * 2 > 100 {
        println!(
            "    ^ ловушка запуска: оценка фона осталась от тишины, и ровный шум\n\
             \x20     читается как речь. Числа ниже смотреть с этим в уме — в первых\n\
             \x20     строках таблицы колонка «фон» покажет, случилось ли это на записи"
        );
    }

    let mut ok = true;
    if quiet.total == 0 || talking.total == 0 {
        println!("  ВЕРДИКТ: кадров нет вовсе — проверять было нечего");
        ok = false;
    }
    if quiet.accepted * 10 > quiet.total {
        println!("  ВЕРДИКТ: городской шум принят за речь — гейт не работает");
        ok = false;
    }
    if quiet.accepted_old * 2 < quiet.total {
        println!("  ВЕРДИКТ: постоянный порог этот шум не пропускает — сравнивать не с чем");
        ok = false;
    }
    if talking.accepted < 45 {
        println!("  ВЕРДИКТ: заведомая речь не прошла — гейт глухой");
        ok = false;
    }
    if ok {
        println!("  ВЕРДИКТ: прибор различает шум и речь, числам ниже можно верить");
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
        // Достаточно одной дорожки: гейт видит микс, и микс из одного
        // канала — законный случай (tap не запущен).
        let mark = if mic > 0 || system > 0 { "+" } else { " " };
        println!("  {mark} {} — mic {mic}, system {system}", meeting.id);
    }
    println!("\nСтрока с «+» годится для прогона: gate-probe <каталог> <сессия>");
    println!("Отдельный файл: gate-probe <каталог> запись.wav (16 кГц моно)");
    Ok(())
}

fn probe(root: &Path, session_id: &str) -> Result<(), String> {
    let store = AudioManifestStore::open(root).map_err(|error| error.to_string())?;
    let frames = mixed_frames(&store, session_id)?;
    let totals = gated(
        &frames
            .iter()
            .map(|frame| frame.pcm.clone())
            .collect::<Vec<_>>(),
    );

    println!("\nСессия {session_id}, {RATE} Гц");
    println!(
        "  кадров {} по {FRAME_MS} мс — {:.1} с",
        totals.total,
        (totals.total * FRAME_MS) as f64 / 1_000.0
    );

    print_buckets(&frames);

    println!(
        "\n  Всего: гейт пропустил {} кадров из {} ({:.1}%), постоянный порог {OLD_THRESHOLD:.0} — {} ({:.1}%)",
        totals.accepted,
        totals.total,
        percent(totals.accepted, totals.total),
        totals.accepted_old,
        percent(totals.accepted_old, totals.total),
    );

    let pcm: Vec<Vec<i16>> = frames.iter().map(|frame| frame.pcm.clone()).collect();
    let quiet_path = inferences(&pcm, 0);
    let talking_path = inferences(&pcm, 1);
    let seconds = (totals.total * FRAME_MS) as f64 / 1_000.0;
    println!(
        "\n  Запусков модели за запись: {} (partial {}, по концам реплик {})",
        quiet_path.total(),
        quiet_path.partials,
        quiet_path.flushes,
    );
    if talking_path.total() != quiet_path.total() {
        println!(
            "  При базовом темпе (модель фиксирует слова): {} — темп разжимается\n\
             \x20 только тогда, когда фиксировать нечего",
            talking_path.total()
        );
    }
    println!(
        "  Это {:.1} запуска в минуту.",
        quiet_path.total() as f64 * 60.0 / seconds.max(1.0)
    );
    // Прибор не знает, говорили на записи или нет, и утверждать этого не
    // должен: прежняя строка печатала «где никто не говорит» на любой
    // записи, включая двадцатиминутную встречу.
    println!(
        "  Говорили ли на этой записи, прибор не знает. На молчащей комнате\n\
         \x20 число должно быть близко к нулю; на встрече оно и есть работа\n\
         \x20 живого пути, и сравнивать его надо с колонкой постоянного порога."
    );
    if quiet_path.skipped > 0 {
        println!(
            "\n  Пропущено {} концов реплик короче {} мс — движок по ним не\n\
             \x20 запускается (правка 2026-08-11). Без неё было бы {} запусков,\n\
             \x20 то есть {:.1} в минуту.\n\
             \x20 Плата за это — очень короткое «да» распознано не будет. Каждый\n\
             \x20 пропуск пишется в журнал диагностики: потеря видима.",
            quiet_path.skipped,
            MIN_SPEECH_FRAMES * 1_000 / RATE as usize,
            quiet_path.total() + quiet_path.skipped,
            (quiet_path.total() + quiet_path.skipped) as f64 * 60.0 / seconds.max(1.0),
        );
    }
    compare_with_vad(root, &pcm, seconds, quiet_path.total());

    if let Ok(raw) = raw_frames(&store, session_id)
        && raw.len() > 1
    {
        println!("\n  Откуда берётся фон (микс против сырых дорожек):");
        println!("                  медиана  последняя треть  максимум  запусков");
        let mut highest_tail = 0.0f32;
        for (channel, frames) in &raw {
            let seen = floor_and_runs(frames);
            highest_tail = highest_tail.max(seen.tail_median);
            println!(
                "    только {:<7} {:>7.0} {:>16.0} {:>9.0} {:>9}",
                channel.code(),
                seen.median,
                seen.tail_median,
                seen.peak,
                seen.runs
            );
        }
        let mixed = floor_and_runs(&pcm);
        println!(
            "    микс          {:>7.0} {:>16.0} {:>9.0} {:>9}",
            mixed.median, mixed.tail_median, mixed.peak, mixed.runs
        );

        // Вывод считается, а не печатается заранее, и считается по той
        // же величине, о которой спрашивали: фон рос к концу записи.
        if mixed.tail_median > highest_tail * 1.5 {
            println!(
                "    В последней трети фон на миксе выше самой шумной дорожки в {:.1}\n\
                 \x20   раза — поднял его микшер: тихое он тянет к цели с усилением\n\
                 \x20   до пятикратного (ADR-009), а гейт по тихому и считает фон.",
                mixed.tail_median / highest_tail.max(1.0)
            );
        } else {
            println!(
                "    Микшер фон не поднимал: на миксе он не выше, чем на дорожках.\n\
                 \x20   Значит рос он в самом звуке — смотреть надо, что звучало\n\
                 \x20   в тихих местах последней трети."
            );
        }

        if let Some((_, mic)) = raw
            .iter()
            .find(|(channel, _)| *channel == AudioChannel::Mic)
            && let Some(gain) = gain_in_tail(&pcm, mic)
        {
            println!(
                "    Микс громче дорожки mic в {gain:.1} раза (последняя треть).\n\
                 \x20   Сложение каналов больше двух дать не может; всё сверх этого —\n\
                 \x20   усиление микшера, а его потолок пятикратный."
            );
        }
    }

    println!(
        "  Доля кадров запуском не является: движок копит речь до порога и\n\
         \x20 держит темп. Зато конец каждой реплики стоит отдельного прохода,\n\
         \x20 поэтому одиночные всплески дороже, чем выглядят по процентам."
    );
    Ok(())
}

/// Сколько кадров прошло гейт и сколько прошло бы старый порог.
#[derive(Debug, Default)]
struct Gated {
    total: usize,
    accepted: usize,
    accepted_old: usize,
}

/// Во что пропущенные кадры обходятся на самом деле.
///
/// Пропущенный кадр — **не** запуск Whisper, и первая версия прибора
/// утверждала обратное. Движок копит речь и запускается, когда её
/// набралось `MIN_SPEECH_FRAMES`, но не чаще, чем разрешает темп
/// (`InferencePacer`). Зато конец каждой реплики стоит отдельного
/// прохода: после `SILENCE_FRAMES` тишины остаток фиксируется
/// принудительно, ждать согласия больше не с чем.
///
/// Отсюда следствие, которого по доле кадров не видно: **одиночный
/// всплеск дороже, чем кажется**. До порога речи он не дотягивает,
/// partial не запускает, но реплику открывает — и через 0.3 с закрывает
/// её проходом по буферу. Именно эта величина переводится в ватты.
#[derive(Debug, Default)]
struct Inferences {
    partials: usize,
    flushes: usize,
    /// Концы реплик, не набравших порога речи: движок их **пропускает**.
    ///
    /// В `total` не входят — прохода по ним нет. Считаются и печатаются
    /// потому, что правка от 2026-08-11 стоила отказа от распознавания
    /// очень коротких реплик, и величина этой платы должна оставаться на
    /// виду, а не превращаться в тишину.
    ///
    /// Замер, по которому правка сделана: 22 таких конца из 48 проходов
    /// на двух минутах молчащей комнаты.
    skipped: usize,
}

impl Inferences {
    fn total(&self) -> usize {
        self.partials + self.flushes
    }
}

/// Медиана оценки фона и число запусков модели по одному входу.
///
/// Нужно, чтобы разделить два подозреваемых: сама комната и **микшер**.
/// Тот подтягивает тихий канал к цели (`TARGET_RMS`, ADR-009) с
/// усилением до пятикратного, а подтягивает он ровно то, по чему гейт
/// считает фон, — тихие места. Если на миксе фон заметно выше, чем на
/// сырых дорожках, растёт он не в комнате.
fn floor_and_runs(frames: &[Vec<i16>]) -> FloorRun {
    let mut gate = NoiseGate::new();
    let mut floors: Vec<f32> = Vec::with_capacity(frames.len());
    for frame in frames {
        gate.accepts(rms(frame));
        floors.push(gate.noise_floor());
    }
    // Медиана по всей записи прячет то, ради чего замер и заводился:
    // фон рос к концу встречи, а первые две трети держали его низким.
    // Статистика обязана смотреть туда же, куда смотрит вопрос.
    let tail_from = floors.len() * 2 / 3;
    FloorRun {
        median: median(floors.iter().copied()),
        tail_median: median(floors[tail_from..].iter().copied()),
        peak: floors.iter().copied().fold(0.0, f32::max),
        runs: inferences(frames, 0).total(),
    }
}

/// Во сколько раз микс громче микрофонной дорожки в последней трети.
///
/// Решающая величина: сложение двух каналов дать больше чем вдвое не
/// может, а усиление микшера — до пятикратного (`MAX_GAIN`, ADR-009).
/// Отношение около пяти означает, что микшер упёрся в свой потолок,
/// то есть тянул к цели то, что речью не было.
///
/// Кадры сопоставляются по номеру: обе дорожки нарезаны от начала своей
/// записи одним и тем же шагом.
fn gain_in_tail(mixed: &[Vec<i16>], mic: &[Vec<i16>]) -> Option<f32> {
    let len = mixed.len().min(mic.len());
    if len == 0 {
        return None;
    }
    let ratios: Vec<f32> = (len * 2 / 3..len)
        .filter_map(|index| {
            let quiet = rms(&mic[index]);
            // Делить на тишину бессмысленно: отношение улетит в небо и
            // медиана перестанет описывать усиление.
            (quiet >= 1.0).then(|| rms(&mixed[index]) / quiet)
        })
        .collect();
    (!ratios.is_empty()).then(|| median(ratios.into_iter()))
}

/// Как вёл себя фон на одном входе.
struct FloorRun {
    median: f32,
    /// Медиана по последней трети — там, где фон и рос.
    tail_median: f32,
    peak: f32,
    runs: usize,
}

/// Дорожка, нарезанная на кадры живого пути, но **без** микшера.
type RawTrack = (AudioChannel, Vec<Vec<i16>>);

/// Дорожки сессии в кадрах живого пути, без микшера.
fn raw_frames(store: &AudioManifestStore, session_id: &str) -> Result<Vec<RawTrack>, String> {
    let frame_len = RATE as usize * FRAME_MS / 1_000;
    let mut out = Vec::new();
    for channel in [AudioChannel::Mic, AudioChannel::System] {
        let track = store
            .read_session_pcm(session_id, channel)
            .map_err(|error| error.to_string())?;
        if track.is_empty() {
            continue;
        }
        let frames = track
            .chunks(frame_len)
            .map(<[i16]>::to_vec)
            .collect::<Vec<_>>();
        out.push((channel, frames));
    }
    Ok(out)
}

/// Прогнать кадры через гейт ровно так, как это делает живой путь:
/// один `accepts` на кадр, состояние между кадрами сохраняется.
fn gated(frames: &[Vec<i16>]) -> Gated {
    gated_by(frames, &mut NoiseGate::new())
}

/// То же любым решателем «есть ли речь».
///
/// Колонка постоянного порога считается здесь всегда и не зависит от
/// решателя: она — общая точка отсчёта, с которой сравниваются оба.
fn gated_by(frames: &[Vec<i16>], decider: &mut dyn SpeechDecider) -> Gated {
    let mut out = Gated::default();
    for frame in frames {
        out.total += 1;
        if decider.accepts_frame(frame) {
            out.accepted += 1;
        }
        if rms(frame) > OLD_THRESHOLD {
            out.accepted_old += 1;
        }
    }
    out
}

/// Повторить цепочку живого пути от гейта до запуска модели.
///
/// Правила и константы берутся из `stt`, а не переписываются здесь:
/// копия разошлась бы с движком молча, и прибор мерил бы правило,
/// которого уже нет.
///
/// `committed_words` — единственное, чего без Whisper не узнать: темп
/// разжимается, когда слова перестают фиксироваться. Поэтому считается
/// дважды, по обеим границам: 0 слов (на шуме модель ничего не отдаёт,
/// темп разжимается до втрое реже) и 1 слово (речь идёт, темп базовый).
fn inferences(frames: &[Vec<i16>], committed_words: usize) -> Inferences {
    inferences_by(frames, committed_words, &mut NoiseGate::new())
}

/// То же любым решателем. **Меняется только он** — накопление речи,
/// закрытие реплики и темп остаются теми же, иначе разница в запусках
/// окажется разницей двух правил сразу.
fn inferences_by(
    frames: &[Vec<i16>],
    committed_words: usize,
    decider: &mut dyn SpeechDecider,
) -> Inferences {
    let frame_len = RATE as usize * FRAME_MS / 1_000;
    let mut pacer = InferencePacer::new(PARTIAL_MIN_FRAMES);
    let mut out = Inferences::default();

    let (mut in_speech, mut speech, mut since_partial, mut silence) =
        (false, 0usize, 0usize, 0usize);
    for frame in frames {
        if decider.accepts_frame(frame) {
            in_speech = true;
            silence = 0;
            speech += frame_len;
            since_partial += frame_len;
            if speech >= MIN_SPEECH_FRAMES && since_partial >= pacer.frames_until_next() {
                out.partials += 1;
                pacer.record(committed_words);
                since_partial = 0;
            }
        } else if in_speech {
            silence += frame_len;
            if silence >= SILENCE_FRAMES {
                // Конец реплики: проход по буферу, если речь в ней была.
                if speech < MIN_SPEECH_FRAMES {
                    out.skipped += 1;
                } else {
                    out.flushes += 1;
                }
                in_speech = false;
                speech = 0;
                since_partial = 0;
                silence = 0;
                pacer.reset();
            }
        }
    }
    // Запись кончилась посреди реплики — остановка тоже фиксирует остаток.
    if in_speech {
        if speech < MIN_SPEECH_FRAMES {
            out.skipped += 1;
        } else {
            out.flushes += 1;
        }
    }
    out
}

fn print_buckets(frames: &[MixedFrame]) {
    if frames.is_empty() {
        return;
    }
    // Оценка фона печатается рядом с решениями: без неё таблица
    // показывает результат и молчит о причине. Гейт решает по
    // превышению уровня над фоном, и «пропустил много» значит одно, когда
    // фон высок, и совсем другое, когда фон прилип к нулю.
    println!("\n  время, с      RMS медиана   фон   порог   гейт   порог {OLD_THRESHOLD:.0}");

    let mut gate = NoiseGate::new();
    let start = frames[0].timestamp_ms;
    let mut bucket = start;
    let mut levels: Vec<f32> = Vec::new();
    let (mut accepted, mut accepted_old, mut total) = (0usize, 0usize, 0usize);

    let flush = |bucket: u64,
                 levels: &mut Vec<f32>,
                 floor: f32,
                 threshold: f32,
                 accepted,
                 accepted_old,
                 total| {
        if total == 0 {
            return;
        }
        println!(
            "  {:>5.0}–{:<7.0} {:>10.0} {:>5.0} {:>7.0}  {:>3}/{:<3} {:>3}/{:<3}",
            (bucket - start) as f64 / 1_000.0,
            (bucket - start + BUCKET_MS) as f64 / 1_000.0,
            median(levels.iter().copied()),
            floor,
            threshold,
            accepted,
            total,
            accepted_old,
            total,
        );
        levels.clear();
    };

    for frame in frames {
        while frame.timestamp_ms >= bucket + BUCKET_MS {
            flush(
                bucket,
                &mut levels,
                gate.noise_floor(),
                gate.threshold(),
                accepted,
                accepted_old,
                total,
            );
            bucket += BUCKET_MS;
            accepted = 0;
            accepted_old = 0;
            total = 0;
        }
        let level = rms(&frame.pcm);
        levels.push(level);
        total += 1;
        if gate.accepts(level) {
            accepted += 1;
        }
        if level > OLD_THRESHOLD {
            accepted_old += 1;
        }
    }
    flush(
        bucket,
        &mut levels,
        gate.noise_floor(),
        gate.threshold(),
        accepted,
        accepted_old,
        total,
    );
}

/// Кадры так, как их видит распознавание: через микшер каналов (ADR-009).
///
/// Гейт стоит **после** микшера, а тот подтягивает тихий канал. Считать
/// по сырой дорожке значило бы мерить не тот сигнал, который доходит до
/// гейта живьём.
///
/// Границы кадров восстанавливаются, а не читаются: в манифесте лежат
/// **пачки записи**, а не кадры живого пути. Накопитель хранилища
/// сливает несколько чанков в один файл, и его `frame_count` к 100 мс
/// отношения не имеет. Отсюда нарезка по `FRAME_MS` от начала каждой
/// дорожки; начало берётся из метки первого чанка канала, чтобы поздно
/// поднявшийся системный tap не сдвинул выравнивание.
fn mixed_frames(store: &AudioManifestStore, session_id: &str) -> Result<Vec<MixedFrame>, String> {
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

    let started = |channel: AudioChannel| -> u64 {
        chunks
            .iter()
            .filter(|chunk| chunk.channel == channel)
            .map(|chunk| chunk.timestamp_ms)
            .min()
            .unwrap_or(0)
    };

    let frame_len = RATE as usize * FRAME_MS / 1_000;
    let mut tracks = Vec::new();
    for channel in [AudioChannel::Mic, AudioChannel::System] {
        let track = store
            .read_session_pcm(session_id, channel)
            .map_err(|error| error.to_string())?;
        if !track.is_empty() {
            tracks.push((channel, started(channel), track));
        }
    }
    if tracks.is_empty() {
        return Err("обе дорожки пусты — мерить нечего".to_string());
    }

    let mut mixer = ChannelMixer::new();
    mixer.set_system_expected(
        tracks
            .iter()
            .any(|(channel, ..)| *channel == AudioChannel::System),
    );

    // Подача идёт по времени, кадр за кадром: микшер отдаёт слот, когда
    // пришли все ожидаемые каналы либо истёк допуск, и подача целыми
    // дорожками подряд развалила бы выравнивание.
    let longest = tracks
        .iter()
        .map(|(_, _, track)| track.len().div_ceil(frame_len))
        .max()
        .unwrap_or(0);
    let mut frames = Vec::new();
    for index in 0..longest {
        for (channel, start_ms, track) in &tracks {
            let from = index * frame_len;
            if from >= track.len() {
                continue;
            }
            let to = (from + frame_len).min(track.len());
            mixer.push(
                *channel,
                &track[from..to],
                start_ms + (index * FRAME_MS) as u64,
            );
        }
        frames.extend(mixer.drain());
    }
    frames.extend(mixer.flush());

    if frames.is_empty() {
        return Err("микшер не отдал ни одного кадра — мерить нечего".to_string());
    }
    Ok(frames)
}

/// Кадр заданного уровня: знакопеременный сигнал даёт RMS ровно `level`.
fn frame_at(level: f32) -> Vec<i16> {
    let frames = RATE as usize * FRAME_MS / 1_000;
    (0..frames)
        .map(|index| if index % 2 == 0 { level } else { -level } as i16)
        .collect()
}

/// Прогон по отдельному WAV — заведомо известному материалу.
///
/// Сессии в базе бывают только на Маке, а проверить, что колонка VAD
/// вообще что-то слышит, надо там же, где её пишут. Годится любая
/// запись 16 кГц моно; контрольная речь лежит рядом с моделью GigaAM
/// (`models/gigaam/check/example.wav`).
fn wav_mode(root: &Path, path: &Path) -> Result<(), String> {
    let audio = wav::read(path)?;
    if audio.sample_rate != RATE {
        return Err(format!(
            "{}: {} Гц, а живой путь работает на {RATE}",
            path.display(),
            audio.sample_rate
        ));
    }
    let frame_len = RATE as usize * FRAME_MS / 1_000;
    let pcm: Vec<Vec<i16>> = audio.pcm.chunks(frame_len).map(<[i16]>::to_vec).collect();
    let seconds = audio.pcm.len() as f64 / RATE as f64;

    println!(
        "\n{} — {:.1} с, кадров {}",
        path.display(),
        seconds,
        pcm.len()
    );

    let by_gate = gated(&pcm);
    let gate_runs = inferences(&pcm, 0);
    println!(
        "  гейт по фону: пропущено {} кадров из {} ({:.1}%), запусков модели {}",
        by_gate.accepted,
        by_gate.total,
        percent(by_gate.accepted, by_gate.total),
        gate_runs.total()
    );
    println!(
        "    первое «речь»: {}",
        first_speech(&pcm, &mut NoiseGate::new())
    );

    compare_with_vad(root, &pcm, seconds, gate_runs.total());
    Ok(())
}

/// Когда решатель впервые сказал «речь».
///
/// Число значит что-то **только на записи с известным началом речи** —
/// например собранной из тихой комнаты и заведомо речевого куска. На
/// произвольной встрече это просто «когда он открылся впервые», и
/// выводов о запаздывании отсюда делать нельзя.
///
/// Ради него режим по WAV и заведён: у VAD запаздывание — главная
/// известная плата, а у гейта её нет вовсе, и увидеть разницу можно
/// только там, где начало речи знаешь заранее.
fn first_speech(pcm: &[Vec<i16>], decider: &mut dyn SpeechDecider) -> String {
    for (index, frame) in pcm.iter().enumerate() {
        if decider.accepts_frame(frame) {
            return format!("{:.1} с", (index * FRAME_MS) as f64 / 1_000.0);
        }
    }
    "не сказал ни разу".to_string()
}

/// Открыть Silero VAD — или сказать, почему нельзя.
#[cfg(feature = "vad")]
fn open_vad(root: &Path) -> Result<Box<dyn SpeechDecider>, String> {
    stt::SileroGate::open(root, RATE).map(|gate| Box::new(gate) as Box<dyn SpeechDecider>)
}

#[cfg(not(feature = "vad"))]
fn open_vad(_root: &Path) -> Result<Box<dyn SpeechDecider>, String> {
    Err("собрано без --features vad: сравнивать не с чем".to_string())
}

/// Вторая колонка: во что обходится тот же материал с Silero VAD.
///
/// Печатается **рядом** с гейтом, а не вместо него. Одно число без
/// второго ничего не значит: «VAD дал 12 запусков в минуту» звучит
/// хорошо ровно до вопроса, сколько дал гейт на этой же записи и не
/// потерял ли VAD при этом речь.
fn compare_with_vad(root: &Path, pcm: &[Vec<i16>], seconds: f64, gate_runs: usize) {
    let mut vad = match open_vad(root) {
        Ok(vad) => vad,
        Err(error) => {
            println!("\n  VAD: {error}");
            return;
        }
    };

    let runs = inferences_by(pcm, 0, vad.as_mut());
    vad.reset();
    let passed = gated_by(pcm, vad.as_mut());
    let per_minute = runs.total() as f64 * 60.0 / seconds.max(1.0);

    println!("\n  Silero VAD на том же материале ({}):", vad.name());
    println!(
        "    пропущено кадров {} из {} ({:.1}%)",
        passed.accepted,
        passed.total,
        percent(passed.accepted, passed.total)
    );
    println!(
        "    запусков модели {} ({:.1} в минуту), partial {}, по концам реплик {}",
        runs.total(),
        per_minute,
        runs.partials,
        runs.flushes
    );
    println!(
        "    у гейта на этой же записи — {gate_runs} запусков ({:.1} в минуту)",
        gate_runs as f64 * 60.0 / seconds.max(1.0)
    );
    vad.reset();
    println!("    первое «речь»: {}", first_speech(pcm, vad.as_mut()));

    // Ноль от VAD читается как «тишина» и выглядит прекрасно — ровно так
    // же выглядит слепой прибор. Различает их только заведомо речевая
    // запись, и здесь про неё сказано вслух.
    if passed.accepted == 0 {
        println!(
            "\n    VAD не принял НИ ОДНОГО кадра. Прежде чем читать это как тишину,\n\
             \x20   прогоните заведомо речевую запись: gate-probe <каталог> речь.wav —\n\
             \x20   ноль на ней означает, что слеп прибор, а не запись молчит."
        );
    }

    disagreements(root, pcm);
}

/// Где решатели разошлись — с временем, чтобы это можно было послушать.
///
/// Считать это ошибками VAD или ошибками гейта прибор не имеет права:
/// правды о том, была ли там речь, у него нет. Он показывает места и
/// молчит о том, кто прав, — судит человек, открыв запись на этой
/// секунде.
fn disagreements(root: &Path, pcm: &[Vec<i16>]) {
    let Ok(mut vad) = open_vad(root) else {
        return;
    };
    let mut gate = NoiseGate::new();

    let mut runs: Vec<(usize, usize, bool)> = Vec::new();
    for (index, frame) in pcm.iter().enumerate() {
        let by_gate = gate.accepts_frame(frame);
        let by_vad = vad.accepts_frame(frame);
        if by_gate == by_vad {
            continue;
        }
        match runs.last_mut() {
            Some(last) if last.1 + 1 == index && last.2 == by_vad => last.1 = index,
            _ => runs.push((index, index, by_vad)),
        }
    }

    if runs.is_empty() {
        println!("    расхождений нет вовсе — а это подозрительно: два разных");
        println!("    способа решать на настоящей записи так не совпадают");
        return;
    }

    let frames_total = pcm.len().max(1);
    let differing: usize = runs.iter().map(|(from, to, _)| to - from + 1).sum();
    println!(
        "\n    Разошлись на {} кадрах из {} ({:.1}%). Самые длинные места:",
        differing,
        frames_total,
        percent(differing, frames_total)
    );

    let mut longest = runs.clone();
    longest.sort_by_key(|(from, to, _)| std::cmp::Reverse(to - from));
    for (from, to, vad_said_speech) in longest.into_iter().take(5) {
        let start = (from * FRAME_MS) as f64 / 1_000.0;
        let length = ((to - from + 1) * FRAME_MS) as f64 / 1_000.0;
        let who = if vad_said_speech {
            "речь слышит только VAD"
        } else {
            "речь слышит только гейт"
        };
        println!("      {start:8.1} с  длиной {length:5.1} с — {who}");
    }
    println!("    Кто из них прав, прибор не знает: эталона нет. Слушать по времени.");
}

fn percent(part: usize, whole: usize) -> f64 {
    if whole == 0 {
        return 0.0;
    }
    part as f64 * 100.0 / whole as f64
}

fn median(values: impl Iterator<Item = f32>) -> f32 {
    let mut values: Vec<f32> = values.collect();
    if values.is_empty() {
        return 0.0;
    }
    values.sort_by(f32::total_cmp);
    values[values.len() / 2]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_root(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "mr-gate-probe-{name}-{:?}",
            std::thread::current().id()
        ))
    }

    fn bytes_of(pcm: &[i16]) -> Vec<u8> {
        pcm.iter().flat_map(|sample| sample.to_le_bytes()).collect()
    }

    /// Сессия с записанной дорожкой: чанки по 100 мс, как в живом пути.
    fn seed(root: &std::path::Path, session_id: &str, frames: &[Vec<i16>]) {
        let mut store = AudioManifestStore::open(root).expect("store");
        store
            .begin_session(session_id, 0, "проба")
            .expect("session");
        for (index, frame) in frames.iter().enumerate() {
            store
                .append_chunk(
                    AudioChannel::Mic,
                    &bytes_of(frame),
                    RATE,
                    (index * FRAME_MS) as u64,
                )
                .expect("chunk");
        }
        store.end_session(1_000).expect("end");
    }

    /// Половина прибора, ходящая в базу, проверяется тем же заведомо
    /// шумным случаем, что и половина, считающая уровни.
    ///
    /// Иначе «гейт пропустил ноль кадров» на настоящей записи ничего не
    /// значило бы: дорожка могла не прочитаться вовсе, и выглядело бы это
    /// точно так же, как молчащая комната.
    #[test]
    fn reads_stored_tracks_and_gates_a_known_noise() {
        let root = tmp_root("known-noise");
        let _ = std::fs::remove_dir_all(&root);
        let frames: Vec<Vec<i16>> = (0..200).map(|_| frame_at(600.0)).collect();
        seed(&root, "s1", &frames);

        let store = AudioManifestStore::open(&root).expect("store");
        let mixed = mixed_frames(&store, "s1").expect("кадры");

        assert_eq!(mixed.len(), frames.len(), "дорожка прочлась не вся");
        let totals = gated(
            &mixed
                .iter()
                .map(|frame| frame.pcm.clone())
                .collect::<Vec<_>>(),
        );
        assert_eq!(totals.total, frames.len(), "кадров нет — считать нечего");
        assert!(
            totals.accepted * 10 <= totals.total,
            "заведомый шум прошёл гейт: {} из {}",
            totals.accepted,
            totals.total
        );
        assert_eq!(
            totals.accepted_old, totals.total,
            "постоянный порог этот шум пропускал целиком — иначе сравнение бессмысленно"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Одиночный всплеск до порога речи не дотягивает: модель по нему не
    /// запускается вовсе, но пропуск обязан быть посчитан.
    ///
    /// По доле кадров этого не видно вовсе: один кадр из ста.
    #[test]
    fn a_single_blip_no_longer_costs_a_pass() {
        let mut frames: Vec<Vec<i16>> = (0..50).map(|_| frame_at(100.0)).collect();
        frames.push(frame_at(3_000.0));
        frames.extend((0..50).map(|_| frame_at(100.0)));

        // Материал обязан пробить гейт ровно один раз: не пробей он вовсе,
        // тест утверждал бы ноль о пустоте.
        let passed = gated(&frames);
        assert_eq!(
            passed.accepted, 1,
            "всплеск не прошёл гейт — считать нечего"
        );

        let runs = inferences(&frames, 0);

        assert_eq!(runs.partials, 0, "0.1 с речи до порога не дотягивает");
        assert_eq!(
            runs.flushes, 0,
            "прохода быть не должно: речи в реплике нет"
        );
        assert_eq!(
            runs.skipped, 1,
            "пропуск обязан быть посчитан, а не потерян"
        );
        assert_eq!(
            runs.total(),
            0,
            "модель по этому куску не запускается вовсе"
        );
    }

    /// Настоящая реплика порога достигает, и её конец фиксировать надо:
    /// кандидат на отмену не должен задевать речь.
    #[test]
    fn a_real_utterance_is_not_counted_as_short() {
        let mut frames: Vec<Vec<i16>> = (0..20).map(|_| frame_at(100.0)).collect();
        // Секунда речи — десять кадров, впятеро больше порога.
        frames.extend((0..10).map(|_| frame_at(3_000.0)));
        frames.extend((0..20).map(|_| frame_at(100.0)));

        let passed = gated(&frames);
        assert!(
            passed.accepted >= 9,
            "речь не прошла гейт: {}",
            passed.accepted
        );

        let runs = inferences(&frames, 0);

        assert_eq!(runs.flushes, 1, "реплика кончилась — фиксировать надо");
        assert_eq!(
            runs.skipped, 0,
            "секунда речи посчиталась недостаточной — правка съела бы реплику"
        );
    }

    /// Речь подряд: проходы идут по темпу, а не по кадрам.
    #[test]
    fn continuous_speech_runs_on_the_pace_not_per_frame() {
        let mut frames: Vec<Vec<i16>> = (0..20).map(|_| frame_at(100.0)).collect();
        // 10 секунд речи — сто кадров.
        frames.extend((0..100).map(|_| frame_at(3_000.0)));

        let passed = gated(&frames);
        assert!(
            passed.accepted > 90,
            "речь не прошла гейт: {}",
            passed.accepted
        );

        let runs = inferences(&frames, 1);

        assert!(
            runs.partials >= 8 && runs.partials <= 10,
            "при базовом темпе раз в секунду за 10 с ожидались ~9 проходов, вышло {}",
            runs.partials
        );
        assert!(
            runs.partials < passed.accepted / 5,
            "проходов почти столько же, сколько кадров — темп не работает"
        );
    }

    /// Усиление тихого канала микшером поднимает и оценку фона: гейт
    /// считает фон именно по тихому.
    ///
    /// Проверяется на заведомом случае — тихая дорожка, которую микшер
    /// обязан подтянуть (уровень между `GAIN_NOISE_FLOOR_RMS` и
    /// `QUIET_CEILING_RMS`).
    #[test]
    fn the_mixer_lifts_the_floor_the_gate_reads() {
        let quiet: Vec<Vec<i16>> = (0..200).map(|_| frame_at(200.0)).collect();
        let raw_floor = floor_and_runs(&quiet).median;

        // Уровень заведомо в окне усиления: ниже — микшер не тронет,
        // выше — тоже, и тест проверял бы пустоту.
        assert!(
            (150.0..700.0).contains(&raw_floor),
            "материал вне окна усиления микшера: фон {raw_floor}"
        );

        let boosted: Vec<Vec<i16>> = quiet
            .iter()
            .map(|frame| frame.iter().map(|s| s.saturating_mul(5)).collect())
            .collect();
        let boosted_floor = floor_and_runs(&boosted).median;

        assert!(
            boosted_floor > raw_floor * 3.0,
            "подъём дорожки не поднял фон: {raw_floor} -> {boosted_floor}"
        );
    }

    /// Заведомое усиление: тихая дорожка в окне подъёма, микс — та же
    /// дорожка, поднятая впятеро. Прибор обязан назвать пять.
    #[test]
    fn the_gain_is_measured_not_guessed() {
        let mic: Vec<Vec<i16>> = (0..90).map(|_| frame_at(200.0)).collect();
        let mixed: Vec<Vec<i16>> = mic
            .iter()
            .map(|frame| frame.iter().map(|s| s.saturating_mul(5)).collect())
            .collect();

        let gain = gain_in_tail(&mixed, &mic).expect("кадры есть");

        assert!(
            (4.5..5.5).contains(&gain),
            "заведомо пятикратный подъём измерен как {gain}"
        );
    }

    /// Тишина в знаменателе отношение не создаёт: иначе медиана
    /// описывала бы деление на ноль, а не усиление.
    #[test]
    fn silence_does_not_invent_a_gain() {
        let mic: Vec<Vec<i16>> = (0..90).map(|_| frame_at(0.0)).collect();
        let mixed: Vec<Vec<i16>> = (0..90).map(|_| frame_at(1_000.0)).collect();

        assert!(gain_in_tail(&mixed, &mic).is_none());
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
        let error = mixed_frames(&store, "s1").expect_err("пустая сессия — отказ");

        assert!(error.contains("нет чанков"), "{error}");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Заведомо положительный случай для самого вердикта прибора.
    #[test]
    fn the_self_check_passes_on_a_healthy_gate() {
        assert!(self_check());
    }
}
