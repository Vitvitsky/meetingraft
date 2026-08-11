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

use std::path::Path;
use std::process::ExitCode;

use domain::AudioChannel;
use session::{ChannelMixer, MixedFrame};
use storage::AudioManifestStore;
use stt::{InferencePacer, MIN_SPEECH_FRAMES, NoiseGate, PARTIAL_MIN_FRAMES, SILENCE_FRAMES};

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
        "  Это {:.1} запуска в минуту на записи, где никто не говорит.",
        quiet_path.total() as f64 * 60.0 / seconds.max(1.0)
    );
    if quiet_path.short_flushes > 0 {
        println!(
            "\n  Из них {} — концы реплик, не набравших порога речи ({} мс).\n\
             \x20 Разбирать там нечего: движок уже решил, что это не речь, и\n\
             \x20 partial по такому куску не гонял. Не гоняй он и конец —\n\
             \x20 осталось бы {} запусков вместо {}, то есть {:.1} в минуту.\n\
             \x20 Это оценка правки, а не сама правка: движок не тронут.",
            quiet_path.short_flushes,
            MIN_SPEECH_FRAMES * 1_000 / RATE as usize,
            quiet_path.total() - quiet_path.short_flushes,
            quiet_path.total(),
            (quiet_path.total() - quiet_path.short_flushes) as f64 * 60.0 / seconds.max(1.0),
        );
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
    /// Проходы по концам реплик, которые до порога речи так и не дотянули.
    ///
    /// Считаются отдельно, потому что это кандидат на правку: движок уже
    /// решил, что меньше `MIN_SPEECH_FRAMES` — не речь, и partial по
    /// такому куску не гоняет. Конец реплики гоняет всё равно, хотя
    /// разбирать там нечего: 0.1–0.2 с шума. На таком куске Whisper и
    /// выдаёт титры (Epic 16).
    ///
    /// Прибор считает это **до** правки движка: сперва цена, потом
    /// решение.
    short_flushes: usize,
}

impl Inferences {
    fn total(&self) -> usize {
        self.partials + self.flushes
    }
}

/// Прогнать кадры через гейт ровно так, как это делает живой путь:
/// один `accepts` на кадр, состояние между кадрами сохраняется.
fn gated(frames: &[Vec<i16>]) -> Gated {
    let mut gate = NoiseGate::new();
    let mut out = Gated::default();
    for frame in frames {
        let level = rms(frame);
        out.total += 1;
        if gate.accepts(level) {
            out.accepted += 1;
        }
        if level > OLD_THRESHOLD {
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
    let frame_len = RATE as usize * FRAME_MS / 1_000;
    let mut gate = NoiseGate::new();
    let mut pacer = InferencePacer::new(PARTIAL_MIN_FRAMES);
    let mut out = Inferences::default();

    let (mut in_speech, mut speech, mut since_partial, mut silence) =
        (false, 0usize, 0usize, 0usize);
    for frame in frames {
        if gate.accepts(rms(frame)) {
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
                // Конец реплики: принудительный проход по буферу.
                out.flushes += 1;
                if speech < MIN_SPEECH_FRAMES {
                    out.short_flushes += 1;
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
        out.flushes += 1;
        if speech < MIN_SPEECH_FRAMES {
            out.short_flushes += 1;
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

/// Уровень кадра — тот же расчёт, что и в живом пути (`whisper.rs`).
fn rms(pcm: &[i16]) -> f32 {
    if pcm.is_empty() {
        return 0.0;
    }
    let sum: f64 = pcm.iter().map(|s| f64::from(*s) * f64::from(*s)).sum();
    ((sum / pcm.len() as f64).sqrt()) as f32
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

    /// Одиночный всплеск до порога речи не дотягивает, partial не
    /// запускает — но реплику открывает, и её конец стоит прохода.
    ///
    /// По доле кадров этого не видно вовсе: один кадр из ста.
    #[test]
    fn a_single_blip_still_costs_one_pass() {
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
        assert_eq!(runs.flushes, 1, "конец реплики обязан стоить прохода");
        assert_eq!(
            runs.short_flushes, 1,
            "этот проход и есть кандидат на отмену: речи в нём нет"
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
            runs.short_flushes, 0,
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
