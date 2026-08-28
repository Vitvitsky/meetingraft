//! Командная строка стенда. Всё содержательное — в библиотеке рядом
//! (`lib.rs`), здесь только разбор аргументов и печать.

use meetingraft_bench::run as bench_run;
use meetingraft_bench::segmentation::{self, Strategy};
use meetingraft_bench::{case, engines, wav};

use std::path::Path;
use std::process::ExitCode;

const USAGE: &str = "\
meetingraft-bench <подкоманда>

  show <каталог-случая>
      прочитать случай и напечатать, что в нём есть

  cut <каталог-случая> <от-мс> <до-мс> [mic|system]
      вырезать отрезок в <каталог-случая>/cut-<от>-<до>.wav —
      то, по чему печатается эталон

  export <каталог-данных> <id-встречи> <каталог-случая>
      выложить встречу из данных приложения в случай
      (только сборка с --features export, то есть на Маке)

  run <каталог-случая> <каталог-данных> <движок> <нарезка> [каталог-выхода]
      движок:  gigaam
      нарезка: windows30 | vad | diarize
";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(command) = args.first().map(String::as_str) else {
        eprintln!("{USAGE}");
        return ExitCode::FAILURE;
    };

    match command {
        "show" => match args.get(1) {
            Some(dir) => show(Path::new(dir)),
            None => {
                eprintln!("не сказано, какой случай читать\n{USAGE}");
                ExitCode::FAILURE
            }
        },
        "cut" => cut(&args[1..]),
        "export" => export(&args[1..]),
        "run" => run(&args[1..]),
        other => {
            eprintln!("неизвестная подкоманда {other}\n{USAGE}");
            ExitCode::FAILURE
        }
    }
}

/// Один прогон: случай × движок × нарезка.
fn run(args: &[String]) -> ExitCode {
    let (Some(case_dir), Some(data_root), Some(engine_name), Some(strategy_name)) =
        (args.first(), args.get(1), args.get(2), args.get(3))
    else {
        eprintln!("нужно: run <каталог-случая> <каталог-данных> <движок> <нарезка>\n{USAGE}");
        return ExitCode::FAILURE;
    };

    let case = match case::load(Path::new(case_dir)) {
        Ok(case) => case,
        Err(error) => {
            eprintln!("случай не прочитан: {error}");
            return ExitCode::FAILURE;
        }
    };
    let strategy = match Strategy::parse(strategy_name) {
        Ok(strategy) => strategy,
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::FAILURE;
        }
    };
    let engine = match engines::open(engine_name, Path::new(data_root)) {
        Ok(engine) => engine,
        Err(error) => {
            eprintln!("движок не открыт: {error}");
            return ExitCode::FAILURE;
        }
    };

    // Разметка речи нужна не только нарезке по VAD: по ней считается доля
    // границ, не разрезающих речь, — то есть по ней судят **все три**
    // способа. Без неё метрика границ не печатается вовсе, и это честнее,
    // чем печатать единицу от отсутствия разметки.
    let speech = match speech_marks(Path::new(data_root), &case) {
        Ok(speech) => speech,
        Err(error) => {
            eprintln!("речь не размечена: {error}");
            Vec::new()
        }
    };

    let split_started = std::time::Instant::now();
    let pieces = match build_pieces(strategy, Path::new(data_root), &case, &speech) {
        Ok(pieces) => pieces,
        Err(error) => {
            eprintln!("нарезка не вышла: {error}");
            return ExitCode::FAILURE;
        }
    };
    let split_ms = split_started.elapsed().as_secs_f32() * 1000.0;

    let plan = bench_run::Plan {
        pieces,
        speech,
        strategy,
        split_ms,
        engine: engine.as_ref(),
    };
    let result = bench_run::execute(&case, plan, "none");

    let out = args
        .get(4)
        .cloned()
        .unwrap_or_else(|| format!("{case_dir}/out/{engine_name}-{strategy_name}-none"));
    if let Err(error) = bench_run::save(&result, Path::new(&out)) {
        eprintln!("результат не записан: {error}");
        return ExitCode::FAILURE;
    }

    print_run(&result, &out);
    if result.refused.is_some() {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

/// Разметка речи по независимому источнику.
#[cfg(feature = "vad")]
fn speech_marks(data_root: &Path, case: &case::Case) -> Result<Vec<(u64, u64)>, String> {
    stt::speech_segments(data_root, &case.mic, case.sample_rate)
}

#[cfg(not(feature = "vad"))]
fn speech_marks(_data_root: &Path, _case: &case::Case) -> Result<Vec<(u64, u64)>, String> {
    Err("стенд собран без --features vad: границы судить нечем".to_string())
}

/// Куски по выбранному способу.
fn build_pieces(
    strategy: Strategy,
    data_root: &Path,
    case: &case::Case,
    speech: &[(u64, u64)],
) -> Result<Vec<segmentation::Piece>, String> {
    match strategy {
        Strategy::Windows30 => Ok(segmentation::split_windows(
            case.duration_ms(),
            segmentation::MAX_PIECE_MS,
        )),
        Strategy::Vad => {
            if speech.is_empty() {
                // Ноль отрезков речи и отсутствие разметки — разные вещи,
                // но обе дают пустой прогон, а пустой прогон читается как
                // «движок ничего не услышал». Отказываем вслух.
                return Err("речи не размечено ни одного отрезка".to_string());
            }
            Ok(segmentation::from_speech(speech, case.duration_ms()))
        }
        Strategy::Diarize => diarize_pieces(data_root, case),
    }
}

#[cfg(feature = "diarize")]
fn diarize_pieces(data_root: &Path, case: &case::Case) -> Result<Vec<segmentation::Piece>, String> {
    use diarize::Diarizer;

    let models = diarize::resolve_diarize_models(data_root)?;
    let mut engine = diarize::SherpaDiarizer::open(&models)?;
    let report = engine.diarize(&case.mic, case.sample_rate);
    if let Some(reason) = report.refused {
        return Err(reason);
    }
    let turns: Vec<(u64, u64, u32)> = report
        .turns
        .iter()
        .map(|turn| (turn.start_ms, turn.end_ms, turn.cluster))
        .collect();
    Ok(segmentation::from_turns(&turns))
}

#[cfg(not(feature = "diarize"))]
fn diarize_pieces(
    _data_root: &Path,
    _case: &case::Case,
) -> Result<Vec<segmentation::Piece>, String> {
    Err("стенд собран без --features diarize".to_string())
}

fn print_run(result: &bench_run::Run, out: &str) {
    println!("случай         {}", result.case);
    println!(
        "прогон         {} + {} + {}",
        result.engine, result.segmentation, result.biasing
    );
    if let Some(reason) = &result.refused {
        println!("ОТКАЗ          {reason}");
        println!("результат      {out}");
        return;
    }
    println!(
        "кусков         {} подано, {} вернулись пустыми",
        result.pieces_fed, result.pieces_empty
    );
    if let Some(found) = result.speakers_found {
        println!("голосов        {found} нашла нарезка");
    }
    if let Some(stats) = &result.stats {
        println!(
            "сегментов      {} (медиана {} мс, p10 {} мс, p90 {} мс)",
            stats.count, stats.median_ms, stats.p10_ms, stats.p90_ms
        );
        println!("покрытие       {:.3}", stats.coverage);
        println!("границы в паузе {:.3}", stats.boundaries_in_pause);
    }
    match (result.wer, result.cer) {
        (Some(wer), Some(cer)) => {
            let caveat = if result.reference_kind == "EditedDraft" {
                "  (эталон — правленный черновик, льстит своему движку)"
            } else {
                ""
            };
            println!("WER            {wer:.3}{caveat}");
            println!("CER            {cer:.3}");
        }
        _ => println!("WER            — (эталона или его границ нет)"),
    }
    println!("нарезка        {:.0} мс", result.split_ms);
    println!(
        "модель         {:.0} мс на секунду речи",
        result.model_ms_per_audio_second
    );
    println!("результат      {out}");
}

/// Выложить встречу из данных приложения в случай.
///
/// Без фичи `export` подкоманда **отказывает вслух**, а не молчит: та же
/// сборка на Маке и на Linux ведёт себя по-разному, и человек, набравший
/// её здесь, должен узнать причину, а не увидеть «неизвестная
/// подкоманда».
#[cfg(feature = "export")]
fn export(args: &[String]) -> ExitCode {
    let (Some(data_root), Some(meeting), Some(out)) = (args.first(), args.get(1), args.get(2))
    else {
        eprintln!("нужно: export <каталог-данных> <id-встречи> <каталог-случая>\n{USAGE}");
        return ExitCode::FAILURE;
    };
    match meetingraft_bench::export::export(Path::new(data_root), meeting, Path::new(out)) {
        Ok(done) => {
            println!("микрофон       {} мс", done.mic_ms);
            match done.system_ms {
                Some(ms) => println!("система        {ms} мс"),
                None => println!("система        нет канала"),
            }
            println!("общее время    {}", done.channel_clock_unified);
            println!();
            println!("дальше руками в {out}/meta.toml: speakers_expected, notes,");
            println!("и — когда напечатан эталон — reference_kind с reference_covers_ms");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("встреча не выложена: {error}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(not(feature = "export"))]
fn export(_args: &[String]) -> ExitCode {
    eprintln!(
        "стенд собран без --features export: данных приложения здесь нет, \
         и выкладывать нечего"
    );
    ExitCode::FAILURE
}

/// Вырезать отрезок записи в отдельный WAV.
///
/// Нужна ради эталона: напечатать три минуты с нуля можно только с
/// файла, в котором ровно эти три минуты. Границы, названные здесь,
/// потом слово в слово переезжают в `reference_covers_ms` — и это не
/// удобство, а единственный способ не разойтись: WER считается по
/// отрезку, а не по всей встрече.
fn cut(args: &[String]) -> ExitCode {
    let (Some(dir), Some(from), Some(to)) = (args.first(), args.get(1), args.get(2)) else {
        eprintln!("нужно: cut <каталог-случая> <от-мс> <до-мс> [mic|system]\n{USAGE}");
        return ExitCode::FAILURE;
    };
    let (Ok(from_ms), Ok(to_ms)) = (from.parse::<u64>(), to.parse::<u64>()) else {
        eprintln!("границы задаются целыми миллисекундами, а не {from}..{to}");
        return ExitCode::FAILURE;
    };
    let channel = args.get(3).map(String::as_str).unwrap_or("mic");

    let case = match case::load(Path::new(dir)) {
        Ok(case) => case,
        Err(error) => {
            eprintln!("случай не прочитан: {error}");
            return ExitCode::FAILURE;
        }
    };

    let pcm = match channel {
        "mic" => &case.mic,
        "system" => match case.system.as_ref() {
            Some(system) => system,
            None => {
                eprintln!("у случая нет системного канала");
                return ExitCode::FAILURE;
            }
        },
        other => {
            eprintln!("канал бывает mic или system, а не {other}");
            return ExitCode::FAILURE;
        }
    };

    let rate = u64::from(case.sample_rate);
    let total_ms = pcm.len() as u64 * 1000 / rate;
    // Молча укоротить отрезок нельзя: человек напечатает эталон по тому,
    // что услышит, а `reference_covers_ms` останется с той границей,
    // которую он назвал, — и WER посчитается не по тому месту.
    if to_ms > total_ms || from_ms >= to_ms {
        eprintln!("отрезок {from_ms}..{to_ms} не лежит внутри записи длиной {total_ms} мс");
        return ExitCode::FAILURE;
    }

    let start = (from_ms * rate / 1000) as usize;
    let end = (to_ms * rate / 1000) as usize;
    let out = case.dir.join(format!("cut-{from_ms}-{to_ms}.wav"));
    match wav::write(&out, &pcm[start..end], case.sample_rate) {
        Ok(()) => {
            println!("{}", out.display());
            println!("в meta.toml: reference_covers_ms = [{from_ms}, {to_ms}]");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("не записалось: {error}");
            ExitCode::FAILURE
        }
    }
}

/// Прочитать случай и напечатать его состав.
///
/// Нужна не для отладки: это первая проверка того, что каталог, приехавший
/// с Мака, доехал целиком. Молчаливо принятый случай без системного
/// канала или с эталоном не на том отрезке даст числа, которые не о чем.
fn show(dir: &Path) -> ExitCode {
    match case::load(dir) {
        Ok(case) => {
            println!("случай         {}", case.meta.case);
            println!("язык           {}", case.meta.language);
            println!("источник       {}", case.meta.source);
            println!("микрофон       {} мс", case.duration_ms());
            match &case.system {
                Some(system) => println!(
                    "система        {} мс",
                    system.len() as u64 * 1000 / u64::from(case.sample_rate)
                ),
                None => println!("система        нет канала"),
            }
            println!("голосов ждём   {}", case.meta.speakers_expected);
            println!("общее время    {}", case.meta.channel_clock_unified);
            match (&case.reference, case.meta.reference_covers_ms) {
                (Some(text), Some([from, to])) => println!(
                    "эталон         {:?}, {} слов, покрывает {from}..{to} мс",
                    case.meta.reference_kind,
                    text.split_whitespace().count()
                ),
                (Some(text), None) => println!(
                    "эталон         {:?}, {} слов, отрезок не указан — WER не считается",
                    case.meta.reference_kind,
                    text.split_whitespace().count()
                ),
                (None, _) => println!("эталон         нет"),
            }
            if !case.meta.notes.is_empty() {
                println!("замечания      {}", case.meta.notes);
            }
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("случай не прочитан: {error}");
            ExitCode::FAILURE
        }
    }
}
