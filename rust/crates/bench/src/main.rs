//! Стенд сравнения распознавателей.
//!
//! Спека — `docs/superpowers/specs/2026-08-28-asr-bench-design.md`,
//! план — `docs/superpowers/plans/2026-08-28-asr-bench.md`.
//!
//! В приложение не входит: это прибор, как `stt-probe` и `diarize-probe`,
//! и с той же дисциплиной — заведомо положительный и заведомо
//! отрицательный случай раньше настоящих данных.
//!
//! Отвечает на вопрос, на который `stt-probe` отвечать отказывается: не
//! «работает ли движок вообще», а «который из них и с какой нарезкой
//! лучше на наших встречах». Разница в том, что здесь есть эталон;
//! без него сравнение расшифровки с расшифровкой — это сверка догадки
//! самой с собой.

mod case;
mod wav;

use std::path::Path;
use std::process::ExitCode;

const USAGE: &str = "\
meetingraft-bench <подкоманда>

  show <каталог-случая>
      прочитать случай и напечатать, что в нём есть

  cut <каталог-случая> <от-мс> <до-мс> [mic|system]
      вырезать отрезок в <каталог-случая>/cut-<от>-<до>.wav —
      то, по чему печатается эталон
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
        other => {
            eprintln!("неизвестная подкоманда {other}\n{USAGE}");
            ExitCode::FAILURE
        }
    }
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
