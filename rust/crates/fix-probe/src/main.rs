//! Прибор для исправлений, предложенных моделью (Phase 13).
//!
//! Отвечает на вопрос, от которого зависит, строить ли очередь одобрения:
//! **сколько среди предложений порч** — случаев, где исходное слово было
//! верным. Доля верных исправлений на этот вопрос не отвечает: она
//! складывает безобидное с опасным.
//!
//! Три кучи, и прибор не различает ни одной — их размечает человек по
//! напечатанному:
//!
//! 1. **верное исправление** — ради чего всё;
//! 2. **промах** — безобиден: не одобрил, и всё;
//! 3. **порча** — исходное слово было верным. Опасна: одобренная, она
//!    уедет в глоссарий и начнёт переписывать будущие расшифровки.
//!
//! Третью никто бы не догадался считать, а именно она решает, можно ли
//! источнику доверять. «Модель предложила сорок исправлений» звучит
//! прекрасно и может означать двадцать пять порч. Поэтому печатается не
//! пара, а пара с репликой, опорой и тайм-кодом: отметить третью кучу
//! иначе нельзя, а по тайм-коду сомнительное можно послушать.
//!
//! Самопроверка до всяких данных, по образцу `brief-probe`. Главный
//! случай здесь **отрицательный**: на чистой расшифровке модель обязана
//! ответить `НЕТ`. Та, что предлагает что-нибудь всегда, — генератор
//! порч, и её вывод на настоящих данных не значит ничего.
//!
//! Три исхода различаются и не сливаются: **прибор слеп** (модель
//! неповторяема, недоступна, не нашла подложенное или предлагает на
//! чистом тексте), **размечать нечего** (нет Final, нет сегментов, модель
//! ответила `НЕТ`), **посмотрели и вот что нашли**.

use std::path::Path;
use std::process::ExitCode;
use std::time::{Duration, Instant};

use domain::{AudioChannel, FinalSegment, GlossaryTerm, SpeakerSource, SpeechLanguage, TermFix};
use postcall::{
    LlmClient, LlmError, OllamaNativeClient, ParsedFixes, RejectReport, fix_prompts, parse_fixes,
    resolve_fixes,
};
use storage::AudioManifestStore;

const DEFAULT_BASE_URL: &str = "http://127.0.0.1:11434";
const DEFAULT_MODEL: &str = "gemma2";
/// Локальная модель на длинной расшифровке считает долго.
const TIMEOUT: Duration = Duration::from_secs(600);

const USAGE: &str = "\
Прибор для исправлений, предложенных моделью.

    fix-probe <путь-к-данным> <id-встречи>
              [--model gemma2] [--url http://127.0.0.1:11434]

Отдаёт модели всю расшифровку и печатает предложенные пары с репликой,
опорой и тайм-кодом. До этого проверяет, что модель повторяема, находит
подложенную порчу и молчит на чистом тексте: без этого напечатанное
ничего не значит.";

struct Options {
    root: String,
    meeting: String,
    model: String,
    url: String,
}

fn main() -> ExitCode {
    let options = match parse(std::env::args().skip(1).collect()) {
        Ok(Some(options)) => options,
        Ok(None) => {
            println!("{USAGE}");
            return ExitCode::SUCCESS;
        }
        Err(message) => {
            eprintln!("{message}\n\n{USAGE}");
            return ExitCode::FAILURE;
        }
    };

    // База открывается до самопроверки, но **не читается**: проверяется
    // только, что путь и встреча существуют. Правило «сперва прибор,
    // потом данные» про суждение о данных, а не про опечатку в пути — а
    // три запроса самопроверки с потолком в десять минут каждый стоят
    // получаса, потраченного впустую на неверный id.
    let store = match AudioManifestStore::open(Path::new(&options.root)) {
        Ok(store) => store,
        Err(error) => {
            eprintln!("База не открылась: {error}");
            return ExitCode::FAILURE;
        }
    };
    if matches!(store.get_final_transcript(&options.meeting), Ok(None)) {
        eprintln!("Размечать нечего: у встречи {} нет Final.", options.meeting);
        return ExitCode::FAILURE;
    }

    let client = OllamaNativeClient::with_timeout(&options.url, &options.model, TIMEOUT);

    // Сперва прибор, потом данные: обратный порядок позволил бы прочитать
    // список порч как список исправлений.
    match self_check(&client) {
        Verdict::Blind(reason) => {
            eprintln!("\nПрибор слеп: {reason}");
            eprintln!("До настоящей встречи дело не дошло — размечать было бы нечего.");
            return ExitCode::FAILURE;
        }
        Verdict::Fit => {}
    }

    run(&options, &client)
}

enum Verdict {
    Fit,
    Blind(String),
}

/// Реплика для синтетических случаев самопроверки.
fn probe_segment(index: u32, text: &str) -> FinalSegment {
    FinalSegment {
        index,
        start_ms: u64::from(index) * 5000,
        end_ms: u64::from(index) * 5000 + 4000,
        channel: AudioChannel::Mic,
        speaker_id: String::new(),
        speaker_source: SpeakerSource::None,
        text: text.to_string(),
        text_edited: false,
        original_text: String::new(),
    }
}

/// Расшифровка с подложенной порчей редкого слова.
///
/// Верная форма однозначна из соседних реплик — на то и расчёт всей
/// затеи: декодер с окном в тридцать секунд их не видит, а модель видит.
fn planted() -> Vec<FinalSegment> {
    vec![
        probe_segment(0, "давайте посмотрим на регрессию по вчерашней выборке"),
        probe_segment(1, "кобриаты туда добавили или нет"),
        probe_segment(2, "добавили, и коэффициенты сразу стали устойчивее"),
    ]
}

/// Что обязано найтись в подложенной расшифровке — обе стороны пары.
const PLANTED_SURFACE: &str = "кобриаты";
const PLANTED_CANONICAL: &str = "ковариат";

/// Расшифровка без единой порчи: здесь предлагать нечего.
fn clean() -> Vec<FinalSegment> {
    vec![
        probe_segment(0, "давайте перенесём релиз на среду"),
        probe_segment(1, "согласен, в среду успеем"),
    ]
}

/// Проверка прибора до всяких данных.
fn self_check(client: &dyn LlmClient) -> Verdict {
    println!("=== Самопроверка ===");

    let planted_segments = planted();
    let (system, user) = fix_prompts(&planted_segments, SpeechLanguage::Ru);

    let first = match client.complete(&system, &user) {
        Ok(text) => text,
        Err(LlmError::NotConfigured | LlmError::Transport(_)) => {
            return Verdict::Blind("модель недоступна по указанному адресу".into());
        }
        Err(error) => return Verdict::Blind(error.to_string()),
    };
    let second = match client.complete(&system, &user) {
        Ok(text) => text,
        Err(error) => return Verdict::Blind(error.to_string()),
    };

    if first != second {
        println!("  повторяемость: НЕТ");
        println!("    первый:  {}", shorten(&first, 90));
        println!("    второй:  {}", shorten(&second, 90));
        return Verdict::Blind(
            "модель неповторяема. Сборка переведена на temperature 0; \
             если разница осталась, дело в самой модели — возьмите другую"
                .into(),
        );
    }
    println!("  повторяемость: да, два прогона совпали дословно");

    // Заведомо положительный случай: подложенное обязано найтись.
    let parsed = match parse_fixes(&first) {
        Ok(parsed) => parsed,
        Err(reason) => {
            println!("  подложенная порча: ОТВЕТ НЕ РАЗОБРАН");
            println!("    ответ: {}", shorten(&first, 120));
            return Verdict::Blind(format!("{reason}. Подложенное найти не удалось"));
        }
    };
    let (fixes, _) = resolve_fixes(&parsed, &planted_segments, &[]);
    // Сверяются обе стороны пары. По одному canonical положительный
    // контроль выполнялся бы ответом «выборке → ковариаты»: слово то
    // самое, место не то, подложенное не найдено — а прибор объявил бы
    // себя годным и пошёл к настоящим данным.
    let found = fixes.iter().any(|fix| {
        fix.surface.to_lowercase() == PLANTED_SURFACE
            && fix.canonical.to_lowercase().contains(PLANTED_CANONICAL)
    });
    if !found {
        println!("  подложенная порча: НЕ НАЙДЕНА");
        println!("    ответ: {}", shorten(&first, 120));
        return Verdict::Blind(
            "модель не нашла подложенную порчу, чья верная форма однозначна \
             из соседних реплик. На настоящей встрече она не найдёт и подавно"
                .into(),
        );
    }
    println!("  подложенная порча: найдена");

    // Заведомо отрицательный случай, и здесь он главный: на чистом
    // тексте предлагать нечего. Модель, предлагающая что-нибудь всегда,
    // — генератор третьей кучи.
    let (clean_system, clean_user) = fix_prompts(&clean(), SpeechLanguage::Ru);
    let clean_answer = match client.complete(&clean_system, &clean_user) {
        Ok(text) => text,
        Err(error) => return Verdict::Blind(error.to_string()),
    };
    match parse_fixes(&clean_answer) {
        Ok(parsed) if parsed.fixes.is_empty() => {
            println!("  чистый текст: молчит, как и должна");
        }
        Ok(parsed) => {
            println!("  чистый текст: ПРЕДЛОЖЕНО {}", parsed.fixes.len());
            println!("    ответ: {}", shorten(&clean_answer, 120));
            return Verdict::Blind(
                "модель предлагает исправления на чистом тексте, где портить \
                 нечего. Всё, что она напечатает дальше, неотличимо от порчи"
                    .into(),
            );
        }
        Err(reason) => {
            println!("  чистый текст: ОТВЕТ НЕ РАЗОБРАН");
            println!("    ответ: {}", shorten(&clean_answer, 120));
            return Verdict::Blind(reason);
        }
    }

    println!("  прибор годен\n");
    Verdict::Fit
}

fn run(options: &Options, client: &dyn LlmClient) -> ExitCode {
    let store = match AudioManifestStore::open(Path::new(&options.root)) {
        Ok(store) => store,
        Err(error) => {
            eprintln!("База не открылась: {error}");
            return ExitCode::FAILURE;
        }
    };

    let final_transcript = match store.get_final_transcript(&options.meeting) {
        Ok(Some(transcript)) => transcript,
        Ok(None) => {
            eprintln!("Размечать нечего: у встречи {} нет Final.", options.meeting);
            return ExitCode::FAILURE;
        }
        Err(error) => {
            eprintln!("Final не прочитался: {error}");
            return ExitCode::FAILURE;
        }
    };
    let segments: Vec<FinalSegment> =
        match store.list_final_segments(&options.meeting, final_transcript.version) {
            Ok(segments) => segments,
            Err(error) => {
                eprintln!("Сегменты не прочитались: {error}");
                return ExitCode::FAILURE;
            }
        };
    if segments.is_empty() {
        eprintln!(
            "Размечать нечего: у версии {} нет сегментов — она собрана из live-субтитров.",
            final_transcript.version
        );
        eprintln!("Исправления работают по репликам; нужен пересбор Final.");
        return ExitCode::FAILURE;
    }
    let known: Vec<GlossaryTerm> = store.list_glossary_terms().unwrap_or_default();

    let (system, user) = fix_prompts(&segments, SpeechLanguage::Ru);
    println!(
        "Встреча {} · Final v{} · реплик {} · вход {} символов · терминов в глоссарии {}",
        options.meeting,
        final_transcript.version,
        segments.len(),
        user.chars().count(),
        known.len()
    );

    let started = Instant::now();
    let answer = match client.complete(&system, &user) {
        Ok(text) => text,
        Err(error) => {
            eprintln!("Модель не ответила: {error}");
            return ExitCode::FAILURE;
        }
    };
    println!("Ответ за {:.1} с\n", started.elapsed().as_secs_f64());

    let parsed = match parse_fixes(&answer) {
        Ok(parsed) => parsed,
        Err(reason) => {
            eprintln!("Прибор слеп на этой встрече: {reason}");
            eprintln!("Ответ целиком:\n{answer}");
            return ExitCode::FAILURE;
        }
    };
    if parsed.fixes.is_empty() {
        println!(
            "Модель ответила «нечего предлагать».\n\
             Размечать нечего — и это такой же ответ, как обратный: на этой\n\
             встрече источник молчит, а не ошибается."
        );
        return ExitCode::SUCCESS;
    }

    let (fixes, report) = resolve_fixes(&parsed, &segments, &known);
    print_fixes(&fixes, &segments);
    print_rejects(&parsed, &report, fixes.len());
    print_three_piles();
    ExitCode::SUCCESS
}

fn print_fixes(fixes: &[TermFix], segments: &[FinalSegment]) {
    println!("=== Предложено {} ===", fixes.len());
    for (number, fix) in fixes.iter().enumerate() {
        let reply = segments
            .iter()
            .position(|segment| segment.start_ms == fix.start_ms && segment.channel == fix.channel)
            .map_or(0, |index| index + 1);
        println!(
            "\n{}. {} → {}   [{} · реплика {} · {}]",
            number + 1,
            fix.surface,
            fix.canonical,
            timecode(fix.start_ms),
            reply,
            channel_name(fix.channel)
        );
        if fix.reason.is_empty() {
            println!("   опора: не названа");
        } else {
            println!("   опора: {}", fix.reason);
        }
        println!("   реплика: {}", fix.replica_text);
    }
}

fn print_rejects(parsed: &ParsedFixes, report: &RejectReport, accepted: usize) {
    println!(
        "\n=== Отброшено {} из {} ===",
        report.total(),
        parsed.fixes.len()
    );
    println!("  нет в названной реплике: {}", report.not_in_replica);
    println!("  ничего не меняет: {}", report.no_change);
    println!("  номер реплики вне расшифровки: {}", report.out_of_range);
    println!("  длиннее трёх слов: {}", report.too_long);
    println!("  уже в глоссарии: {}", report.already_known);
    println!("  повтор той же пары: {}", report.duplicates);
    println!("  строк не по формату: {}", parsed.skipped_lines);
    println!("\nДошло до человека: {accepted}");
}

fn print_three_piles() {
    println!(
        "\nДальше глазами и ухом. Прибор не различает три кучи и различить \
         не может:\n\
         \x20 1. верное исправление — ради чего всё;\n\
         \x20 2. промах — безобиден: не одобрил, и всё;\n\
         \x20 3. ПОРЧА: исходное слово было верным. Одобренная, она уедет \
         в глоссарий\n\
         \x20    и начнёт переписывать будущие расшифровки.\n\
         \n\
         Считается третья. «Модель предложила сорок исправлений» звучит \
         прекрасно\n\
         и может означать двадцать пять порч. Сомнительное — послушать по \
         тайм-коду:\n\
         из текста «услышано верно, просто слово редкое» не отличить, и в \
         этом всё дело."
    );
}

fn timecode(ms: u64) -> String {
    let total = ms / 1000;
    format!("{:02}:{:02}", total / 60, total % 60)
}

fn channel_name(channel: AudioChannel) -> &'static str {
    match channel {
        AudioChannel::Mic => "микрофон",
        AudioChannel::System => "система",
    }
}

fn shorten(text: &str, limit: usize) -> String {
    let single_line = text.replace('\n', " ");
    if single_line.chars().count() <= limit {
        return single_line;
    }
    single_line.chars().take(limit).collect::<String>() + "…"
}

fn parse(args: Vec<String>) -> Result<Option<Options>, String> {
    let mut positional: Vec<String> = Vec::new();
    let mut model = DEFAULT_MODEL.to_string();
    let mut url = DEFAULT_BASE_URL.to_string();

    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--model" => {
                model = args.get(index + 1).ok_or("--model без значения")?.clone();
                index += 2;
            }
            "--url" => {
                url = args.get(index + 1).ok_or("--url без значения")?.clone();
                index += 2;
            }
            other => {
                positional.push(other.to_string());
                index += 1;
            }
        }
    }

    match positional.as_slice() {
        [] => Ok(None),
        [_] => Err("нужен id встречи вторым аргументом".into()),
        [root, meeting] => Ok(Some(Options {
            root: root.clone(),
            meeting: meeting.clone(),
            model,
            url,
        })),
        _ => Err("лишние аргументы".into()),
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use super::*;

    /// Ответ, называющий подложенную порчу.
    const PLANTED_ANSWER: &str = "2 | кобриаты | ковариаты | рядом «регрессия» и «выборка»";

    /// Клиент, отдающий заранее заготовленные ответы по порядку.
    struct ScriptedClient {
        answers: RefCell<Vec<Result<String, LlmError>>>,
    }

    impl ScriptedClient {
        fn new(answers: Vec<Result<String, LlmError>>) -> Self {
            Self {
                answers: RefCell::new(answers),
            }
        }
    }

    impl LlmClient for ScriptedClient {
        fn complete(&self, _system: &str, _user: &str) -> Result<String, LlmError> {
            self.answers
                .borrow_mut()
                .pop()
                .unwrap_or(Err(LlmError::EmptyResponse))
        }
    }

    fn blind_reason(verdict: Verdict) -> Option<String> {
        match verdict {
            Verdict::Blind(reason) => Some(reason),
            Verdict::Fit => None,
        }
    }

    /// Ответы кладутся в обратном порядке: `pop` берёт с конца.
    fn scripted(answers: [&str; 3]) -> ScriptedClient {
        ScriptedClient::new(
            answers
                .iter()
                .rev()
                .map(|text| Ok((*text).to_string()))
                .collect(),
        )
    }

    /// Заведомо положительный и заведомо отрицательный случаи сошлись.
    #[test]
    fn a_model_that_finds_the_planted_fix_and_stays_quiet_on_clean_text_is_fit() {
        let client = scripted([PLANTED_ANSWER, PLANTED_ANSWER, "НЕТ"]);

        assert!(blind_reason(self_check(&client)).is_none());
    }

    /// То, на чём сорвался первый заход Epic 8.
    #[test]
    fn a_model_that_answers_differently_twice_is_blind() {
        let client = scripted([
            PLANTED_ANSWER,
            "2 | кобриаты | кубраты | другая догадка",
            "НЕТ",
        ]);

        let reason = blind_reason(self_check(&client)).expect("прибор обязан признать себя слепым");
        assert!(reason.contains("неповторяем"), "{reason}");
    }

    /// Главный отрицательный контроль: предложение там, где портить
    /// нечего. Без него «сорок исправлений» читались бы как успех.
    #[test]
    fn a_model_that_proposes_something_on_clean_text_is_blind() {
        let client = scripted([
            PLANTED_ANSWER,
            PLANTED_ANSWER,
            "1 | среду | среду вечером | контекст",
        ]);

        let reason = blind_reason(self_check(&client)).expect("прибор обязан признать себя слепым");
        assert!(reason.contains("на чистом тексте"), "{reason}");
    }

    /// Молчаливая модель повторяема и совершенно бесполезна.
    #[test]
    fn a_model_that_misses_the_planted_fix_is_blind() {
        let client = scripted(["НЕТ", "НЕТ", "НЕТ"]);

        let reason = blind_reason(self_check(&client)).expect("прибор обязан признать себя слепым");
        assert!(reason.contains("подложенн"), "{reason}");
    }

    /// Верное слово не с того места — тоже промах. Пара «выборке →
    /// ковариаты» разрешается без единой придирки: слово стоит в первой
    /// реплике, отличается от предложения и коротко. Но подложенное
    /// осталось ненайденным, и по одному canonical это неотличимо.
    #[test]
    fn the_right_canonical_in_the_wrong_place_is_not_a_hit() {
        let misplaced = "1 | выборке | ковариаты | звучит похоже";
        let client = scripted([misplaced, misplaced, "НЕТ"]);

        let reason = blind_reason(self_check(&client)).expect("прибор обязан признать себя слепым");
        assert!(reason.contains("подложенн"), "{reason}");
    }

    /// Пара, названная моделью, но отсутствующая в подложенном тексте, —
    /// не находка: `resolve_fixes` её отбросит, и прибор обязан считать
    /// подложенное ненайденным.
    #[test]
    fn a_planted_fix_invented_out_of_thin_air_does_not_count_as_found() {
        let invented = "1 | ковариаты | ковариаты действий | выдумка";
        let client = scripted([invented, invented, "НЕТ"]);

        let reason = blind_reason(self_check(&client)).expect("прибор обязан признать себя слепым");
        assert!(reason.contains("подложенн"), "{reason}");
    }

    #[test]
    fn an_unreachable_model_is_blind_not_silent() {
        let client = ScriptedClient::new(vec![Err(LlmError::Transport("refused".into()))]);

        let reason = blind_reason(self_check(&client)).expect("прибор обязан признать себя слепым");
        assert!(reason.contains("недоступна"), "{reason}");
    }

    /// Прозаический ответ на подложенном тексте — слепота, а не пустота.
    #[test]
    fn prose_instead_of_a_list_is_blindness() {
        let prose = "Конечно! Вот исправленная расшифровка встречи.";
        let client = scripted([prose, prose, "НЕТ"]);

        let reason = blind_reason(self_check(&client)).expect("прибор обязан признать себя слепым");
        assert!(reason.contains("не по формату"), "{reason}");
    }

    #[test]
    fn timecode_counts_minutes_and_seconds() {
        assert_eq!(timecode(762_000), "12:42");
        assert_eq!(timecode(0), "00:00");
    }

    /// Подложенная порча обязана быть настоящей: если слово, которое
    /// прибор считает испорченным, в тексте не стоит, самопроверка
    /// проверяет пустоту.
    #[test]
    fn the_planted_transcript_actually_contains_the_corruption() {
        assert!(planted().iter().any(|s| s.text.contains("кобриаты")));
        assert!(!clean().iter().any(|s| s.text.contains("кобриаты")));
    }
}
