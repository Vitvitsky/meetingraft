//! Прибор для свёртки дублей на живой LLM (Epic 8, задача 5).
//!
//! Отвечает на вопрос, ради которого свёртка и писалась: **меняется ли
//! бриф оттого, что удвоенные реплики свёрнуты**. Сама свёртка написана и
//! проверена тестами, порог 0.6 сошёлся на трёх записях, но включена она
//! до сих пор не была: не показано, что от неё что-то меняется к лучшему.
//!
//! Запускать нужно там, где есть база и Ollama, — то есть на Маке.
//! Настройки в приложении для порога нет и не будет, пока нет этих
//! чисел: переключатель за неизмеренной величиной — заглушка (`AGENTS.md`).
//! Прибор её и заменяет.
//!
//! **Первый заход 2026-08-16 не состоялся, и это устройство прибора.**
//! Тогда два прогона на одном и том же входе разошлись сильнее, чем
//! свёрнутый вход разошёлся с несвёрнутым, — то есть сравнивать было
//! нечего. Поэтому прибор начинает с проверки повторяемости: **два брифа
//! из одного входа обязаны совпасть дословно**. Не совпали — до сравнения
//! дело не доходит.
//!
//! Путь до промпта тот же, что у приложения: `collapse_doubles` →
//! `render_segments` → `brief_prompts`. Свои копии этих шагов мерили бы
//! выдумку прибора, а не поведение продукта.
//!
//! Три исхода различаются и не сливаются: **прибор слеп** (модель
//! неповторяема или недоступна), **сравнивать нечего** (нет Final,
//! сегментов или дублей), **сравнили и вот разница**.

use std::path::Path;
use std::process::ExitCode;
use std::time::{Duration, Instant};

use domain::{FinalSegment, SpeechLanguage};
use postcall::{
    LlmClient, LlmError, OllamaNativeClient, brief_prompts, collapse_doubles, render_segments,
};
use storage::AudioManifestStore;

/// Порог свёртки, сошедшийся на трёх записях (`dup-probe`, 2026-08-16).
const DEFAULT_THRESHOLD: f32 = 0.6;
const DEFAULT_BASE_URL: &str = "http://127.0.0.1:11434";
const DEFAULT_MODEL: &str = "gemma2";
/// Локальная модель на длинной расшифровке считает долго.
const TIMEOUT: Duration = Duration::from_secs(600);

const USAGE: &str = "\
Прибор для свёртки дублей на живой LLM.

    brief-probe <путь-к-данным> <id-встречи> [--threshold 0.6]
                [--model gemma2] [--url http://127.0.0.1:11434]

Собирает бриф дважды — из несвёрнутой расшифровки и из свёрнутой — и
печатает оба вместе с разницей. До этого проверяет, что модель вообще
повторяема: без этого сравнение ничего не значит.";

struct Options {
    root: String,
    meeting: String,
    threshold: f32,
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

    let client = OllamaNativeClient::with_timeout(&options.url, &options.model, TIMEOUT);

    // Сперва прибор, потом данные. Здесь это не формальность: неповторяемая
    // модель делает всю дальнейшую арифметику бессмысленной, и ровно на
    // этом сорвался первый заход.
    match self_check(&client) {
        Verdict::Blind(reason) => {
            eprintln!("\nПрибор слеп: {reason}");
            eprintln!("До сравнения дело не дошло — любая разница ниже была бы разницей прогонов.");
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

/// Проверка повторяемости модели на коротком входе.
fn self_check(client: &dyn LlmClient) -> Verdict {
    println!("=== Самопроверка ===");

    let (system, user) = brief_prompts(
        "**Пётр:** давайте перенесём релиз на среду.\n\n\
         **Анна:** согласна, в среду успеем.\n",
        SpeechLanguage::Ru,
    );

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
        println!("  два прогона на одном входе разошлись:");
        println!("    первый:  {}", shorten(&first, 90));
        println!("    второй:  {}", shorten(&second, 90));
        return Verdict::Blind(
            "модель неповторяема. Сборка переведена на temperature 0; \
             если разница осталась, дело в самой модели — возьмите другую"
                .into(),
        );
    }
    println!("  повторяемость: да, два прогона совпали дословно");

    // Заведомо отрицательный случай: разный вход обязан дать разный
    // ответ. Без него «совпало» выполнялось бы и на модели, которая
    // всегда отдаёт одну и ту же строку.
    let (other_system, other_user) = brief_prompts(
        "**Иван:** бюджет урезали вдвое, набор откладывается.\n",
        SpeechLanguage::Ru,
    );
    match client.complete(&other_system, &other_user) {
        Ok(other) if other == first => {
            println!("  различимость: НЕТ");
            return Verdict::Blind(
                "разный вход дал тот же ответ — модель отвечает не на промпт".into(),
            );
        }
        Ok(_) => println!("  различимость: да, другой вход дал другой ответ"),
        Err(error) => return Verdict::Blind(error.to_string()),
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
            eprintln!(
                "Сравнивать нечего: у встречи {} нет Final.",
                options.meeting
            );
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
            "Сравнивать нечего: у версии {} нет сегментов — она собрана из live-субтитров.",
            final_transcript.version
        );
        eprintln!("Свёртка работает по репликам; нужен пересбор Final.");
        return ExitCode::FAILURE;
    }
    let speakers = store.list_speakers(&options.meeting).unwrap_or_default();

    let collapsed = match collapse_doubles(&segments, options.threshold) {
        Ok(collapsed) => collapsed,
        Err(error) => {
            eprintln!("Свёртка отказала: {error}");
            return ExitCode::FAILURE;
        }
    };

    println!(
        "Встреча {} · Final v{} · реплик {}",
        options.meeting,
        final_transcript.version,
        segments.len()
    );
    println!(
        "Свёртка при пороге {}: осталось {}, свёрнуто {}, \
         оставлено рукой {}, оставлено коротких {}",
        options.threshold,
        collapsed.report.kept,
        collapsed.report.collapsed,
        collapsed.report.kept_by_hand,
        collapsed.report.kept_short
    );

    if collapsed.report.collapsed == 0 {
        println!(
            "\nСравнивать нечего: свёртка не нашла ни одного дубля.\n\
             Это не «свёртка не помогает» — это отсутствие материала.\n\
             Возьмите встречу, где микрофон писал созвон через динамики."
        );
        return ExitCode::FAILURE;
    }

    let plain = final_transcript.body_markdown.clone();
    let folded = render_segments(&collapsed.segments, &speakers);
    println!(
        "Вход брифа: {} символов без свёртки, {} со свёрткой ({:+.1}%)",
        plain.chars().count(),
        folded.chars().count(),
        percent_change(plain.chars().count(), folded.chars().count())
    );

    let plain_brief = match timed_brief(client, &plain) {
        Ok(result) => result,
        Err(error) => {
            eprintln!("Бриф без свёртки не собрался: {error}");
            return ExitCode::FAILURE;
        }
    };
    let folded_brief = match timed_brief(client, &folded) {
        Ok(result) => result,
        Err(error) => {
            eprintln!("Бриф со свёрткой не собрался: {error}");
            return ExitCode::FAILURE;
        }
    };

    println!(
        "\nВремя сборки: без свёртки {:.1} с, со свёрткой {:.1} с",
        plain_brief.seconds, folded_brief.seconds
    );

    println!("\n=== Бриф БЕЗ свёртки ===\n{}", plain_brief.text);
    println!("\n=== Бриф СО свёрткой ===\n{}", folded_brief.text);

    println!("\n=== Итог ===");
    if plain_brief.text == folded_brief.text {
        println!(
            "  Брифы совпали дословно. Свёртка на этой встрече ничего не\n\
             изменила — включать её незачем, и это такой же ответ, как\n\
             обратный."
        );
    } else {
        println!("  Брифы разошлись.");
    }
    println!(
        "\nДальше глазами, и без этого числа выше ничего не решают:\n\
         пропали ли из второго брифа повторы и перестал ли он перевешивать\n\
         в сторону удалённых участников. Прибор этого не знает — он не\n\
         отличает справедливый пересказ от несправедливого."
    );
    ExitCode::SUCCESS
}

struct Timed {
    text: String,
    seconds: f64,
}

fn timed_brief(client: &dyn LlmClient, body: &str) -> Result<Timed, LlmError> {
    let (system, user) = brief_prompts(body, SpeechLanguage::Ru);
    let started = Instant::now();
    let text = client.complete(&system, &user)?;
    Ok(Timed {
        text,
        seconds: started.elapsed().as_secs_f64(),
    })
}

fn percent_change(before: usize, after: usize) -> f64 {
    if before == 0 {
        return 0.0;
    }
    (after as f64 - before as f64) / before as f64 * 100.0
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
    let mut threshold = DEFAULT_THRESHOLD;
    let mut model = DEFAULT_MODEL.to_string();
    let mut url = DEFAULT_BASE_URL.to_string();

    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--threshold" => {
                let value = args
                    .get(index + 1)
                    .and_then(|v| v.parse::<f32>().ok())
                    .ok_or("--threshold без числа")?;
                threshold = value;
                index += 2;
            }
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
            threshold,
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

    #[test]
    fn a_repeatable_and_responsive_model_is_fit() {
        let client = scripted(["одно и то же", "одно и то же", "другое"]);

        assert!(blind_reason(self_check(&client)).is_none());
    }

    /// То, на чём сорвался первый заход: разные ответы на один вход.
    #[test]
    fn a_model_that_answers_differently_twice_is_blind() {
        let client = scripted(["первый ответ", "второй ответ", "третий"]);

        let reason = blind_reason(self_check(&client)).expect("прибор обязан признать себя слепым");
        assert!(reason.contains("неповторяем"), "{reason}");
    }

    /// Заведомо отрицательный случай: модель, отвечающая одинаково на всё,
    /// повторяема — и совершенно бесполезна.
    #[test]
    fn a_model_that_always_answers_the_same_is_blind() {
        let client = scripted(["константа", "константа", "константа"]);

        let reason = blind_reason(self_check(&client)).expect("прибор обязан признать себя слепым");
        assert!(reason.contains("не на промпт"), "{reason}");
    }

    #[test]
    fn an_unreachable_model_is_blind_not_silent() {
        let client = ScriptedClient::new(vec![Err(LlmError::Transport("refused".into()))]);

        let reason = blind_reason(self_check(&client)).expect("прибор обязан признать себя слепым");
        assert!(reason.contains("недоступна"), "{reason}");
    }

    #[test]
    fn percent_change_is_negative_when_input_shrinks() {
        assert!(percent_change(100, 80) < 0.0);
        assert_eq!(percent_change(0, 10), 0.0);
    }
}
