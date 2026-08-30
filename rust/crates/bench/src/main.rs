//! Командная строка стенда. Всё содержательное — в библиотеке рядом
//! (`lib.rs`), здесь только разбор аргументов и печать.

#[cfg(feature = "judge")]
use meetingraft_bench::judge;
use meetingraft_bench::run as bench_run;
use meetingraft_bench::segmentation::{self, Strategy};
use meetingraft_bench::{case, engines, history, hotwords, labels, wav};

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

  run <каталог-случая> <каталог-данных> <движок> <нарезка> [смещение] [каталог-выхода]
      движок:  gigaam | parakeet | tone | whisper
      нарезка: windows30 | vad | diarize | native (свои границы: tone, whisper)
      смещение: none | hotwords (нужен <каталог-случая>/glossary.txt)

  label <каталог-случая> <каталог-данных> [движки через запятую]
      нарезать по речи и прогнать каждую фразу через все движки;
      кладёт labels.json и label.html рядом со случаем

  history <каталог-случая> [имя-случая]
      журнал замеров: что, когда, с какой разметкой и с какими числами
      (журнал лежит рядом со случаем и переезжает вместе с ним)

  judge <прогон-A> <прогон-B> [адрес-llm] [модель] [зерно]
      слепое парное сравнение двух прогонов
      адрес по умолчанию http://localhost:11434, модель llama3.1
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
        "judge" => judge_runs(&args[1..]),
        "label" => label(&args[1..]),
        "history" => history(&args[1..]),
        other => {
            eprintln!("неизвестная подкоманда {other}\n{USAGE}");
            ExitCode::FAILURE
        }
    }
}

/// Журнал замеров.
///
/// Записи, у которых разный случай, источник эталона или версия
/// разметки, **не сравниваются между собой** — и это помечается прямо в
/// таблице. Иначе падение WER от правки эталона выглядело бы как
/// улучшение движка.
fn history(args: &[String]) -> ExitCode {
    let root = args.first().map(String::as_str).unwrap_or(".");
    let only = args.get(1).map(String::as_str);

    let path = Path::new(root).join(history::FILE);
    let (entries, broken) = match history::load(&path) {
        Ok(pair) => pair,
        Err(error) => {
            eprintln!("журнал не прочитан: {error}");
            return ExitCode::FAILURE;
        }
    };
    if !broken.is_empty() {
        // Битые строки называются вслух: журнал, молча теряющий записи,
        // показывает историю без провалов.
        eprintln!("битых строк: {} (номера: {broken:?})", broken.len());
    }
    let entries: Vec<&history::Entry> = entries
        .iter()
        .filter(|entry| only.is_none_or(|case| entry.case == case))
        .collect();
    if entries.is_empty() {
        println!("записей нет: {}", path.display());
        return ExitCode::SUCCESS;
    }

    println!(
        "{:<20} {:<10} {:<10} {:<8} {:>7} {:>7} {:>7} {:<12} версия",
        "случай", "движок", "нарезка", "смещ.", "WER", "CER", "мс/с", "эталон",
    );
    // Группа сравнимости — то, что можно ставить рядом. Ключ печатается
    // не как строка, а как разделитель: увидев смену, человек знает, что
    // числа выше и ниже про разное.
    let mut previous: Option<&history::Entry> = None;
    for entry in &entries {
        if let Some(before) = previous
            && !history::comparable(before, entry)
        {
            println!("{}", "—".repeat(96));
        }
        let show = |value: Option<f32>| match value {
            Some(number) => format!("{number:.3}"),
            None => "—".to_string(),
        };
        println!(
            "{:<20} {:<10} {:<10} {:<8} {:>7} {:>7} {:>7.0} {:<12} {}",
            truncate(&entry.case, 20),
            truncate(&entry.engine, 10),
            truncate(&entry.segmentation, 10),
            truncate(&entry.biasing, 8),
            show(entry.wer),
            show(entry.cer),
            entry.ms_per_second,
            entry.reference_source.name(),
            entry
                .labels_version
                .map(|version| version.to_string())
                .unwrap_or_else(|| "—".to_string()),
        );
        previous = Some(entry);
    }
    println!();
    println!("записей        {}", entries.len());
    println!(
        "черта          между записями, которые сравнивать нельзя: другой случай, \
         другой эталон либо другая версия разметки"
    );
    ExitCode::SUCCESS
}

fn truncate(text: &str, width: usize) -> String {
    if text.chars().count() <= width {
        text.to_string()
    } else {
        text.chars().take(width - 1).collect::<String>() + "…"
    }
}

/// Разметка: фразы по речи, гипотезы всех движков.
///
/// Тяжёлая часть — прогон движков — делается **один раз**, а потом
/// разметка правится сколько угодно: файл уезжает туда, где удобно
/// слушать, и возвращается обратно.
fn label(args: &[String]) -> ExitCode {
    let (Some(case_dir), Some(data_root)) = (args.first(), args.get(1)) else {
        eprintln!("нужно: label <каталог-случая> <каталог-данных> [движки]\n{USAGE}");
        return ExitCode::FAILURE;
    };
    let names: Vec<String> = args
        .get(2)
        .map(|list| list.split(',').map(str::trim).map(str::to_string).collect())
        .unwrap_or_else(|| {
            vec![
                "gigaam".to_string(),
                "parakeet".to_string(),
                "whisper".to_string(),
            ]
        });

    let case = match case::load(Path::new(case_dir)) {
        Ok(case) => case,
        Err(error) => {
            eprintln!("случай не прочитан: {error}");
            return ExitCode::FAILURE;
        }
    };

    // Границы даёт VAD и только он: нарезка по репликам движка сделала бы
    // датасет пристрастным к тому движку, чей черновик размечали.
    let speech = match speech_marks(Path::new(data_root), &case) {
        Ok(speech) if !speech.is_empty() => speech,
        Ok(_) => {
            eprintln!("речи не найдено ни одного отрезка: размечать нечего");
            return ExitCode::FAILURE;
        }
        Err(error) => {
            eprintln!("речь не размечена: {error}");
            return ExitCode::FAILURE;
        }
    };
    let pieces = segmentation::from_speech(&speech, case.duration_ms());
    println!("фраз по речи   {}", pieces.len());

    // Движок, которого нет в сборке или без модели, — пропускается с
    // объяснением, а не роняет разметку: гипотез станет меньше, и это
    // видно в отчёте.
    let mut engines: Vec<(String, Box<dyn engines::Recognize>)> = Vec::new();
    for name in &names {
        match engines::open(name, Path::new(data_root), None, None) {
            Ok(engine) => {
                println!("движок         {name}");
                engines.push((name.clone(), engine));
            }
            Err(error) => println!("движок         {name} — пропущен: {error}"),
        }
    }
    if engines.is_empty() {
        eprintln!("ни одного движка не открылось: размечать нечем");
        return ExitCode::FAILURE;
    }

    let mut phrases = Vec::with_capacity(pieces.len());
    for (index, piece) in pieces.iter().enumerate() {
        let samples = segmentation::samples(&case.mic, case.sample_rate, piece);
        if samples.is_empty() {
            continue;
        }
        let mut hypotheses = std::collections::BTreeMap::new();
        for (name, engine) in &engines {
            match engine.transcribe(samples, case.sample_rate) {
                Ok(heard) if !heard.text.trim().is_empty() => {
                    hypotheses.insert(name.clone(), heard.text.trim().to_string());
                }
                Ok(_) => {}
                Err(error) => {
                    eprintln!("фраза {index}, движок {name}: {error}");
                }
            }
        }
        // Первая гипотеза становится текстом только как черновик:
        // состояние остаётся `unchecked`, и в эталон это не попадёт.
        let draft = hypotheses.values().next().cloned().unwrap_or_default();
        phrases.push(labels::Phrase {
            id: format!("{:04}", index + 1),
            start_ms: piece.start_ms,
            end_ms: piece.end_ms,
            hypotheses,
            text: draft,
            state: labels::State::Unchecked,
            kinds: Vec::new(),
            speaker: None,
            note: String::new(),
        });
        if (index + 1) % 20 == 0 {
            println!("  размечено {} из {}", index + 1, pieces.len());
        }
    }

    let mut fresh = labels::Labels {
        case: case.meta.case.clone(),
        version: 0,
        boundaries: "vad".to_string(),
        engines: engines.iter().map(|(name, _)| name.clone()).collect(),
        updated_ms: now_ms(),
        phrases,
    };

    let path = Path::new(case_dir).join("labels.json");
    if path.exists() {
        match labels::load(&path) {
            Ok(previous) => match fresh.merge_from(&previous) {
                Ok(carried) => println!("перенесено     {carried} размеченных фраз"),
                Err(error) => {
                    eprintln!("разметка не перенесена: {error}");
                    return ExitCode::FAILURE;
                }
            },
            Err(error) => {
                eprintln!("старая разметка не прочитана: {error}");
                return ExitCode::FAILURE;
            }
        }
    }

    if let Err(error) = labels::save(&fresh, &path) {
        eprintln!("разметка не записана: {error}");
        return ExitCode::FAILURE;
    }
    let page = Path::new(case_dir).join("label.html");
    if let Err(error) = labels::write_page(&fresh, &page) {
        eprintln!("страница не записана: {error}");
        return ExitCode::FAILURE;
    }

    let agreed = fresh.phrases.iter().filter(|p| p.engines_agree()).count();
    println!();
    println!("фраз           {}", fresh.phrases.len());
    println!("сошлись все    {agreed} — смотреть их быстрее, но проверить надо тоже");
    println!("разметка       {}", path.display());
    println!("страница       {}", page.display());
    ExitCode::SUCCESS
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|value| value.as_millis() as u64)
        .unwrap_or(0)
}

/// Слепое парное сравнение двух прогонов.
///
/// Результаты сравнения печатаются **только** если судья прошёл свои
/// контроли. Показать их с оговоркой хуже: оговорку прочтут один раз, а
/// число запомнят.
#[cfg(feature = "judge")]
fn judge_runs(args: &[String]) -> ExitCode {
    let (Some(first_dir), Some(second_dir)) = (args.first(), args.get(1)) else {
        eprintln!("нужно: judge <прогон-A> <прогон-B>\n{USAGE}");
        return ExitCode::FAILURE;
    };
    let endpoint = args
        .get(2)
        .map(String::as_str)
        .unwrap_or("http://localhost:11434");
    let model = args.get(3).map(String::as_str).unwrap_or("llama3.1");
    let seed: u64 = args
        .get(4)
        .and_then(|value| value.parse().ok())
        .unwrap_or(1);

    let (first, second) = match (
        bench_run::load(Path::new(first_dir)),
        bench_run::load(Path::new(second_dir)),
    ) {
        (Ok(first), Ok(second)) => (first, second),
        (Err(error), _) | (_, Err(error)) => {
            eprintln!("прогон не прочитан: {error}");
            return ExitCode::FAILURE;
        }
    };
    if first.case != second.case {
        // Сравнивать прогоны по разным записям бессмысленно, и молча
        // сравнить — значит выдать бессмыслицу за результат.
        eprintln!(
            "это разные случаи: {} и {}. Сравнивать можно только прогоны по одной записи",
            first.case, second.case
        );
        return ExitCode::FAILURE;
    }

    let to_chunks = |run: &bench_run::Run| -> Vec<(u64, String)> {
        run.segments
            .iter()
            .map(|segment| (segment.start_ms, segment.text.clone()))
            .collect()
    };
    let pairs = judge::build_pairs(
        &to_chunks(&first),
        &to_chunks(&second),
        first.audio_ms.max(second.audio_ms),
        seed,
    );
    if pairs.is_empty() {
        eprintln!("сравнивать нечего: оба прогона пусты");
        return ExitCode::FAILURE;
    }

    let client = postcall::OllamaNativeClient::new(endpoint, model);
    let llm_judge = judge::LlmJudge { client: &client };
    let report = judge::evaluate(&llm_judge, &pairs);

    println!("случай         {}", first.case);
    println!(
        "A              {} + {} + {}",
        first.engine, first.segmentation, first.biasing
    );
    println!(
        "B              {} + {} + {}",
        second.engine, second.segmentation, second.biasing
    );
    println!(
        "контроль A/A   {:.2} «ничья» на {} парах (порог {:.2})",
        report.same_text_tie_share,
        report.same_text_pairs,
        judge::SAME_TEXT_MIN_TIE
    );
    println!(
        "контроль слов  {:.2} верных на {} парах (порог {:.2})",
        report.shuffled_correct_share,
        report.shuffled_pairs,
        judge::SHUFFLED_MIN_CORRECT
    );
    if report.errors > 0 {
        println!(
            "ошибок судьи   {} (не ответил или ответ не разобран)",
            report.errors
        );
    }

    if let Some(reason) = &report.refused {
        println!();
        println!("СУДЬЯ ОТВЕРГНУТ: {reason}");
        println!("Результаты сравнения не показываются: доверять им нечего.");
        return ExitCode::FAILURE;
    }

    println!();
    println!("A выиграл      {}", report.first_wins);
    println!("B выиграл      {}", report.second_wins);
    println!(
        "ничьих         {} ({:.2} от настоящих пар)",
        report.ties,
        report.real_tie_share()
    );

    // Где есть эталон, решает WER. Судья мерит связность и не слышал
    // звук: гладкая выдумка выигрывает у корявой правды.
    match (first.wer, second.wer) {
        (Some(a), Some(b)) => {
            println!();
            println!("WER            A {a:.3} против B {b:.3}");
            if let Some(found) =
                judge::divergence(report.first_wins, report.second_wins, first.wer, second.wer)
            {
                println!(
                    "РАСХОЖДЕНИЕ    судья выбрал {}, а WER лучше у {}. \
                     Решает эталон; судья говорит лишь о связности",
                    if found.judge_prefers_first { "A" } else { "B" },
                    if found.reference_prefers_first {
                        "A"
                    } else {
                        "B"
                    }
                );
            }
        }
        _ => println!("\nЭталона нет: судья — единственное, что здесь сказано, и он о связности"),
    }
    ExitCode::SUCCESS
}

#[cfg(not(feature = "judge"))]
fn judge_runs(_args: &[String]) -> ExitCode {
    eprintln!("стенд собран без --features judge: судить нечем");
    ExitCode::FAILURE
}

/// Один прогон: случай × движок × нарезка.
fn run(args: &[String]) -> ExitCode {
    let (Some(case_dir), Some(data_root), Some(engine_name), Some(strategy_name)) =
        (args.first(), args.get(1), args.get(2), args.get(3))
    else {
        eprintln!("нужно: run <каталог-случая> <каталог-данных> <движок> <нарезка>\n{USAGE}");
        return ExitCode::FAILURE;
    };

    let biasing_name = args
        .get(4)
        .map(String::as_str)
        .filter(|value| *value == "none" || *value == "hotwords")
        .unwrap_or("none");

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
    // Сочетание движка и нарезки проверяется **до** открытия модели:
    // ждать 622 МБ ради отказа по несочетаемым аргументам незачем.
    let native = strategy == Strategy::Native;
    if native && !engines::supports_native(engine_name) {
        eprintln!(
            "{engine_name} своих границ не ставит: нарезку ему надо задать. \
             Нужно: ... {engine_name} windows30|vad|diarize"
        );
        return ExitCode::FAILURE;
    }
    if !native && engines::requires_native(engine_name) {
        eprintln!(
            "{engine_name} работает только своими границами: он принимает звук \
             чанками и сам говорит, где кончилась реплика. Нужно: ... {engine_name} native"
        );
        return ExitCode::FAILURE;
    }

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

    // Термины читаются **до** открытия модели: движок со смещением
    // открывается иначе, и узнать об отсутствии глоссария после загрузки
    // 622 МБ было бы обидно.
    let terms = if biasing_name == "hotwords" {
        let path = Path::new(case_dir).join("glossary.txt");
        match hotwords::read_terms(&path) {
            Ok(terms) if terms.is_empty() => {
                eprintln!("{} пуст: смещать нечем", path.display());
                return ExitCode::FAILURE;
            }
            Ok(terms) => terms,
            Err(error) => {
                eprintln!("глоссарий не прочитан: {error}");
                return ExitCode::FAILURE;
            }
        }
    } else {
        Vec::new()
    };

    let engine_model_id;
    let result = if native {
        let engine = match engines::open_native(engine_name, Path::new(data_root), &terms) {
            Ok(Some(engine)) => engine,
            Ok(None) => {
                eprintln!("движок {engine_name} заявил свои границы, но открыть его нечем");
                return ExitCode::FAILURE;
            }
            Err(error) => {
                eprintln!("движок не открыт: {error}");
                return ExitCode::FAILURE;
            }
        };
        engine_model_id = engine.model_id();
        bench_run::execute_stream(&case, engine.as_ref(), speech, biasing_name)
    } else {
        let biasing = match make_biasing(&terms, case_dir) {
            Ok(biasing) => biasing,
            Err(error) => {
                eprintln!("смещение не настроено: {error}");
                return ExitCode::FAILURE;
            }
        };
        let engine = match engines::open(
            engine_name,
            Path::new(data_root),
            biasing.as_ref(),
            Some(&terms),
        ) {
            Ok(engine) => engine,
            Err(error) => {
                eprintln!("движок не открыт: {error}");
                return ExitCode::FAILURE;
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

        engine_model_id = engine.model_id();
        let plan = bench_run::Plan {
            pieces,
            speech,
            strategy,
            split_ms,
            engine: engine.as_ref(),
        };
        bench_run::execute(&case, plan, biasing_name)
    };

    let mut result = result;
    bench_run::add_biasing_report(&mut result, &case, &terms);

    // Разметка, если она есть, — эталон точнее: проверенные фразы
    // разбросаны по встрече, а `reference_covers_ms` покрывает один
    // отрезок. Подмена источника печатается, а не происходит молча.
    let labels_path = Path::new(case_dir).join("labels.json");
    if labels_path.exists() {
        match labels::load(&labels_path) {
            Ok(labels) => bench_run::apply_labels(&mut result, &labels),
            Err(error) => {
                eprintln!("разметка не прочитана: {error}");
                return ExitCode::FAILURE;
            }
        }
    }

    let out = args
        .get(5)
        .cloned()
        .unwrap_or_else(|| format!("{case_dir}/out/{engine_name}-{strategy_name}-{biasing_name}"));
    if let Err(error) = bench_run::save(&result, Path::new(&out)) {
        eprintln!("результат не записан: {error}");
        return ExitCode::FAILURE;
    }

    // Журнал ведётся у каталога случая: он переезжает вместе со случаем,
    // и история не отрывается от записи, к которой относится.
    let journal = Path::new(case_dir).join(history::FILE);
    if let Err(error) = history::append(
        &journal,
        &history::Entry {
            at_ms: now_ms(),
            case: result.case.clone(),
            engine: result.engine.clone(),
            segmentation: result.segmentation.clone(),
            biasing: result.biasing.clone(),
            model_id: engine_model_id.clone(),
            reference_source: result.reference_source,
            labels_version: result.labels_version,
            phrases: result.phrase_score.as_ref().map(|score| score.phrases),
            wer: result.wer,
            cer: result.cer,
            segments: result.segments.len(),
            ms_per_second: result.model_ms_per_audio_second,
            commit: history::current_commit(),
        },
    ) {
        eprintln!("журнал не дописан: {error}");
    }

    print_run(&result, &out);
    if result.refused.is_some() {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

/// Собрать файл терминов для sherpa рядом со случаем.
///
/// `None` означает «смещения нет», и это не ошибка: прогон без него —
/// половина замера.
#[cfg(feature = "biasing")]
fn make_biasing(terms: &[String], case_dir: &str) -> Result<Option<stt::Biasing>, String> {
    if terms.is_empty() {
        return Ok(None);
    }
    let path = Path::new(case_dir).join("hotwords.txt");
    hotwords::write_hotwords(terms, &path)?;
    Ok(Some(stt::Biasing {
        hotwords: path,
        score: stt::DEFAULT_HOTWORDS_SCORE,
    }))
}

#[cfg(not(feature = "biasing"))]
fn make_biasing(terms: &[String], _case_dir: &str) -> Result<Option<()>, String> {
    if terms.is_empty() {
        return Ok(None);
    }
    Err("стенд собран без движка, умеющего смещение".to_string())
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
        // Сюда не попадают: сочетание проверено до открытия модели. Но
        // ветка настоящая, а не `unreachable!`: правило живёт в одном
        // месте, и упасть паникой оно не должно даже если то место
        // однажды перепишут.
        Strategy::Native => Err("потоковому движку нарезка не задаётся".to_string()),
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
    if let Some(report) = &result.biasing_report {
        println!(
            "глоссарий      поймано {}, упущено {}, притянуто {}",
            report.caught, report.missed, report.pulled_in
        );
    }
    println!("эталон         {}", result.reference_source.name());
    if let Some(version) = result.labels_version {
        println!("версия         разметка {version}");
    }
    if let Some(score) = &result.phrase_score {
        println!(
            "фраз в счёте   {} ({} слов)",
            score.phrases, score.reference_words
        );
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
    if let Some(score) = &result.phrase_score
        && !score.worst.is_empty()
    {
        let worst: Vec<String> = score
            .worst
            .iter()
            .map(|(id, rate)| format!("{id}:{rate:.2}"))
            .collect();
        println!("хуже всего     {}", worst.join(", "));
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
