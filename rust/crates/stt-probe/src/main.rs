//! Прибор для русского распознавателя GigaAM v3.
//!
//! Шестой рядом с `echo-probe`, `gate-probe`, `diarize-probe`,
//! `dup-probe` и `term-probe`, и с той же дисциплиной: **сперва случай с
//! известным ответом, потом настоящие данные** (`CLAUDE.md`).
//!
//! Отвечает на два вопроса и только на них:
//!
//! 1. **Распознаёт ли движок вообще** — по записи, текст которой известен
//!    независимо от любого распознавателя (Пушкин, `check/example.txt`).
//! 2. **Сколько это стоит по времени** — миллисекунд на секунду аудио.
//!
//! ## Почему отрицательный случай обязателен
//!
//! Прибор, который печатает маленький WER **всегда** — например потому,
//! что считает его неверно или подаёт движку не тот звук, — выглядит
//! точно как работающий. Отличить его можно ровно одним способом: дать
//! заведомо не-речь и убедиться, что число уехало вверх. Поэтому вторым
//! проходом идёт шум той же длины и той же громкости, и низкий WER на нём
//! — отказ прибора, а не забавный результат.
//!
//! Урок не теоретический: `count-audio-taps.swift` показал ноль tap'ов
//! после убийства процесса, и ноль прочли как «утечки нет». Скрипт был
//! слеп.
//!
//! ## Чего прибор не делает
//!
//! Не судит качество на наших встречах. Эталона на них нет, а сверять
//! расшифровку с расшифровкой другого движка — это сверять догадку саму с
//! собой. На своём звуке прибор печатает текст и время; читает и решает
//! человек.

mod wer;

// Разбор WAV берётся у соседнего прибора **тем же файлом**, а не копией.
// Формат один (16-битный моно PCM живого пути), и вторая его реализация
// означала бы две правды об одном — с гарантией разойтись. Связывать
// крейты приборов ради этого не за чем: они независимы по устройству.
#[path = "../../diarize-probe/src/wav.rs"]
mod wav;

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Instant;

use wer::wer;

/// Частота живого пути; в ней же лежат контрольные записи (ADR-005).
const RATE: u32 = 16_000;

/// Выше этого WER на контрольной записи движок считается не работающим.
///
/// Это граница «распознаёт вообще», а не оценка качества модели: на
/// чистой студийной записи, по которой её и экспортировали, приличный
/// движок обязан быть много ниже. Качество на наших встречах меряется не
/// здесь.
const POSITIVE_MAX_WER: f32 = 0.20;

/// Ниже этого WER на шуме прибор считает **себя** сломанным.
const NEGATIVE_MIN_WER: f32 = 0.80;

/// Что услышал движок.
#[derive(Debug, Default)]
struct Heard {
    text: String,
    /// Время окончания каждого слова, мс от начала куска.
    ///
    /// Отдельно от текста, потому что теряется отдельно: когда длины
    /// токенов и тайм-кодов расходятся, движок отдаёт **правильный текст
    /// без времени**, и на слух такая потеря неотличима от нормы. А
    /// пакетный проход строит по этому времени границы сегментов.
    word_end_ms: Vec<u64>,
}

/// Движок, спрятанный за фичей.
///
/// Отказ движка едет `Err`, а не пустым текстом: сломанный проход,
/// выглядящий как молчание, — худшее, что прибор может показать.
trait Recognize {
    fn transcribe(&self, pcm: &[i16], sample_rate: u32) -> Result<Heard, String>;
}

#[cfg(feature = "gigaam")]
impl Recognize for stt::GigaamRecognizer {
    fn transcribe(&self, pcm: &[i16], sample_rate: u32) -> Result<Heard, String> {
        let hypothesis = stt::GigaamRecognizer::transcribe(self, pcm, sample_rate)
            .map_err(|error| error.to_string())?;
        Ok(Heard {
            text: hypothesis.text,
            word_end_ms: hypothesis.words.iter().map(|word| word.end_ms).collect(),
        })
    }
}

#[cfg(feature = "gigaam")]
fn open_engine(root: &Path) -> Result<Box<dyn Recognize>, String> {
    stt::GigaamRecognizer::open(root)
        .map(|engine| Box::new(engine) as Box<dyn Recognize>)
        // Подробности про недостающие файлы `ModelMissing` не несёт
        // намеренно (там идентификатор модели, который показывают
        // человеку). Спрашиваем их у резолвера сами.
        .map_err(|error| match stt::resolve_gigaam_models(root) {
            Err(details) => details,
            Ok(_) => error.to_string(),
        })
}

#[cfg(not(feature = "gigaam"))]
fn open_engine(_root: &Path) -> Result<Box<dyn Recognize>, String> {
    Err("собрано без --features gigaam: распознавать нечем".to_string())
}

/// Шум той же длины и той же громкости, что и запись.
///
/// Свой генератор, а не `rand`: приборам нужен **повторяемый** шум, иначе
/// отрицательный случай сегодня и завтра — разные случаи. Линейный
/// конгруэнтный, зерно зашито.
///
/// Размах — `rms * √3`, а не `rms`. У равномерного шума с размахом `a`
/// собственный RMS равен `a/√3`, то есть прямая подстановка дала бы шум
/// **тише** записи почти на 5 дБ — и «той же громкости» в описании было
/// бы неправдой. Пики при этом упираются в потолок i16 и срезаются;
/// на нашей записи это единицы отсчётов.
fn noise_like(pcm: &[i16]) -> Vec<i16> {
    let energy: f64 = pcm.iter().map(|s| (*s as f64) * (*s as f64)).sum();
    let rms = (energy / pcm.len().max(1) as f64).sqrt().max(1.0);
    let amplitude = rms * 3.0_f64.sqrt();
    let mut state: u64 = 0x2026_0826;
    pcm.iter()
        .map(|_| {
            state = state
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            let unit = ((state >> 33) as f64 / (1u64 << 31) as f64) * 2.0 - 1.0;
            (unit * amplitude).clamp(i16::MIN as f64, i16::MAX as f64) as i16
        })
        .collect()
}

/// Один проход: услышанное и время на секунду аудио.
fn run(engine: &dyn Recognize, pcm: &[i16], sample_rate: u32) -> Result<(Heard, f32), String> {
    let started = Instant::now();
    let heard = engine.transcribe(pcm, sample_rate)?;
    let elapsed_ms = started.elapsed().as_secs_f32() * 1000.0;
    let audio_seconds = pcm.len() as f32 / sample_rate as f32;
    Ok((heard, elapsed_ms / audio_seconds.max(0.001)))
}

/// Прогреть движок до всяких замеров.
///
/// Первый проход на свежем `OfflineRecognizer` несёт разовую подготовку
/// onnxruntime — графы, арены памяти. Без прогрева она целиком садится
/// на **первый** замер, а решение про живой путь принимается именно по
/// этому числу (`docs/mac-verification.md`). Полсекунды тишины стоят
/// дёшево и снимают вопрос.
fn warm_up(engine: &dyn Recognize, sample_rate: u32) -> Result<(), String> {
    let silence = vec![0i16; (sample_rate / 2) as usize];
    engine.transcribe(&silence, sample_rate)?;
    Ok(())
}

/// Проверить, что тайм-коды слов пригодны для сборки сегментов.
///
/// Без этой проверки потеря времени молчит: текст на месте, WER низкий,
/// и прибор доволен — а пакетный проход кладёт всю тридцатисекундную
/// пачку одним сегментом с границами окна.
///
/// Число слов сверяется с текстом, и это не педантизм. Стоит подать
/// посимвольные токены в `words_from_tokens` вместо
/// `words_from_char_tokens` — ровно та путаница, о которой предупреждает
/// шапка `stt::gigaam`, — и вся фраза склеится **в одно слово**. Список
/// времён останется непустым и монотонным, WER не шелохнётся (он
/// считается по `text`, а не по словам), и проверка без этой строки
/// пройдёт. Проверка, которая не может сработать, хуже отсутствующей.
fn check_word_times(word_end_ms: &[u64], text: &str, audio_ms: u64) -> Result<(), String> {
    if word_end_ms.is_empty() {
        return Err(
            "движок не отдал тайм-кодов слов: текст есть, времени нет — по такому \
             результату границы сегментов не построить"
                .to_string(),
        );
    }
    let words_in_text = wer::normalize(text).len();
    if word_end_ms.len() != words_in_text {
        return Err(format!(
            "слов с временем {}, а в тексте {words_in_text} — сборка слов из токенов \
             разошлась с расшифровкой",
            word_end_ms.len()
        ));
    }
    if word_end_ms.windows(2).any(|pair| pair[0] > pair[1]) {
        return Err("тайм-коды слов идут назад".to_string());
    }
    let last = *word_end_ms.last().unwrap_or(&0);
    if last > audio_ms {
        return Err(format!(
            "последнее слово кончается на {last} мс при длине записи {audio_ms} мс"
        ));
    }
    Ok(())
}

fn usage() {
    println!("Использование: stt-probe <каталог-данных> [файл.wav ...]");
    println!();
    println!("Каталог данных — тот, где лежит meetingraft.sqlite3, и тот же,");
    println!("что просит scripts/fetch-gigaam-models.sh.");
    println!();
    println!("Прибор всегда начинает с самопроверки: контрольная запись против");
    println!("эталона и шум против того же эталона. Не разошлись — до ваших");
    println!("файлов дело не доходит.");
    println!();
    println!("Рядом с вашим WAV можно положить .txt того же имени — тогда");
    println!("посчитается и WER. Без него печатается только текст и время.");
}

/// Самопроверка. `Ok` — можно верить дальнейшему.
fn self_check(engine: &dyn Recognize, root: &Path) -> Result<(), String> {
    let dir = root.join("models").join("gigaam").join("check");
    let wav_path = dir.join("example.wav");
    let reference_path = dir.join("example.txt");
    for path in [&wav_path, &reference_path] {
        if !path.is_file() {
            return Err(format!(
                "нет контрольной записи: {} — скачать: scripts/fetch-gigaam-models.sh <каталог-данных>",
                path.display()
            ));
        }
    }

    let reference = std::fs::read_to_string(&reference_path)
        .map_err(|error| format!("{}: {error}", reference_path.display()))?;
    let control = wav::read(&wav_path)?;
    if control.sample_rate != RATE {
        return Err(format!(
            "{}: {} Гц, а живой путь и модель работают на {RATE}",
            wav_path.display(),
            control.sample_rate
        ));
    }

    println!("Самопроверка");
    println!(
        "  запись: {} ({:.1} с)",
        wav_path.display(),
        control.pcm.len() as f32 / control.sample_rate as f32
    );

    let audio_ms = (control.pcm.len() as u64 * 1000) / control.sample_rate.max(1) as u64;
    warm_up(engine, control.sample_rate)?;
    let (heard, positive_pace) = run(engine, &control.pcm, control.sample_rate)?;
    let positive = wer(&reference, &heard.text);
    println!("  эталон:     {}", reference.trim());
    println!("  распознано: {}", heard.text);
    println!(
        "  WER {:.3} (замен {}, вставок {}, пропусков {}) при пороге {POSITIVE_MAX_WER:.2}",
        positive.rate(),
        positive.substitutions,
        positive.insertions,
        positive.deletions
    );
    println!("  время: {positive_pace:.0} мс на секунду аудио");

    if positive.reference_words == 0 {
        return Err("эталон пуст: мерить нечем".to_string());
    }
    if positive.rate() > POSITIVE_MAX_WER {
        return Err(format!(
            "движок не распознал контрольную запись (WER {:.3} > {POSITIVE_MAX_WER:.2}). \
             Первое, что стоит проверить, — частоту звука и то, что все четыре файла \
             из одного экспорта: заявленная частота вдвое выше настоящей даёт ровно \
             такой результат (пустую расшифровку), а громкость движку безразлична",
            positive.rate()
        ));
    }

    // Тайм-коды проверяются здесь же, а не «как-нибудь потом»: без них
    // пакетный проход молча кладёт всё окно одним сегментом с границами
    // окна, и заметить это по тексту нельзя.
    check_word_times(&heard.word_end_ms, &heard.text, audio_ms)?;
    println!(
        "  тайм-коды: {} слов, последнее на {} мс при длине {audio_ms} мс",
        heard.word_end_ms.len(),
        heard.word_end_ms.last().copied().unwrap_or(0)
    );

    let noise = noise_like(&control.pcm);
    let (on_noise, negative_pace) = run(engine, &noise, control.sample_rate)?;
    let negative = wer(&reference, &on_noise.text);
    println!(
        "  на шуме той же длины и громкости: {:?}",
        on_noise.text.trim()
    );
    println!(
        "  WER {:.3} при пороге не ниже {NEGATIVE_MIN_WER:.2}",
        negative.rate()
    );
    println!("  время: {negative_pace:.0} мс на секунду аудио");

    if negative.rate() < NEGATIVE_MIN_WER {
        return Err(format!(
            "на шуме WER {:.3} — прибор не отличает речь от не-речи, и всем его \
             числам верить нельзя",
            negative.rate()
        ));
    }

    println!("  разошлись: положительный и отрицательный случай различимы");
    Ok(())
}

/// Настоящий звук: текст и время. Вердикта о качестве здесь нет.
fn transcribe_file(engine: &dyn Recognize, path: &Path) -> Result<(), String> {
    let wav = wav::read(path)?;
    let (heard, pace) = run(engine, &wav.pcm, wav.sample_rate)?;

    println!();
    println!("{}", path.display());
    println!(
        "  {:.1} с, {} Гц, {pace:.0} мс на секунду аудио, слов с временем {}",
        wav.pcm.len() as f32 / wav.sample_rate as f32,
        wav.sample_rate,
        heard.word_end_ms.len()
    );
    println!("  {}", heard.text);

    let reference_path = path.with_extension("txt");
    if reference_path.is_file() {
        let reference = std::fs::read_to_string(&reference_path)
            .map_err(|error| format!("{}: {error}", reference_path.display()))?;
        let report = wer(&reference, &heard.text);
        if report.reference_words == 0 {
            println!(
                "  эталон {} пуст — WER не считался",
                reference_path.display()
            );
        } else {
            println!(
                "  WER {:.3} по {} (замен {}, вставок {}, пропусков {})",
                report.rate(),
                reference_path.display(),
                report.substitutions,
                report.insertions,
                report.deletions
            );
        }
    } else {
        println!("  эталона рядом нет — судит человек, а не прибор");
    }
    Ok(())
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some((root, files)) = args.split_first() else {
        usage();
        return ExitCode::FAILURE;
    };
    let root = PathBuf::from(root);
    let files: Vec<PathBuf> = files.iter().map(PathBuf::from).collect();

    let engine = match open_engine(&root) {
        Ok(engine) => engine,
        Err(error) => {
            eprintln!("stt-probe: {error}");
            return ExitCode::FAILURE;
        }
    };

    if let Err(error) = self_check(engine.as_ref(), &root) {
        eprintln!();
        eprintln!("stt-probe: самопроверка не прошла — {error}");
        return ExitCode::FAILURE;
    }

    let mut failed = false;
    for path in &files {
        if let Err(error) = transcribe_file(engine.as_ref(), path) {
            eprintln!("stt-probe: {error}");
            failed = true;
        }
    }
    if files.is_empty() {
        println!();
        println!("Своих файлов не задано. Дальше: stt-probe <каталог-данных> запись.wav");
    }

    if failed {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}
