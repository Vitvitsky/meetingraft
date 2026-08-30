//! Разметка записи по фразам: гипотезы всех движков и ответ человека.
//!
//! Спека — `docs/superpowers/specs/2026-08-30-labelled-datasets-design.md`.
//!
//! Главное правило формата: **непроверенное в эталон не идёт никогда**.
//! Молча считать неразмеченную фразу верной значит взять эталоном текст
//! движка, то есть сверять догадку саму с собой — ровно то, от чего
//! стенд и заводился.
//!
//! Второе правило: **пометить фразу верной автоматически нельзя**.
//! Способа выразить это в формате нет и не будет. Согласие движков
//! показывает, куда смотреть первым, и ничего не говорит о правде: они
//! ошибаются похоже.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::wer::normalize;

/// Что человек сказал про фразу.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum State {
    /// Не смотрели. Умолчание, и единственное состояние, которое ставит
    /// программа.
    #[default]
    Unchecked,
    /// Посмотрели, текст движка верен.
    Correct,
    /// Посмотрели, текст исправлен.
    Corrected,
    /// В датасет не годится: неразборчиво, наложение голосов, не речь.
    Skip,
}

impl State {
    /// Идёт ли фраза в эталон и в датасет.
    pub fn verified(self) -> bool {
        matches!(self, Self::Correct | Self::Corrected)
    }
}

/// Чем именно движок ошибся.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    /// Термин или имя распознаны неверно. Даёт пару для глоссария.
    Term,
    /// Обычная ошибка слуха: «дут» вместо «дуб».
    Misheard,
    /// Речь есть, текста нет — или текст есть, а речи не было.
    Missing,
    /// Текст верен, но реплика разорвана или склеена с чужой.
    ///
    /// Единственный вид, который **не портит эталон**: он про нарезку, а
    /// не про слова.
    Boundary,
}

/// Одна фраза разметки.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Phrase {
    pub id: String,
    pub start_ms: u64,
    pub end_ms: u64,
    /// Что услышал каждый движок. Ключ — имя движка.
    #[serde(default)]
    pub hypotheses: BTreeMap<String, String>,
    /// Верный текст. Пока фраза не проверена — то, что предложено
    /// движками, и в эталон это не идёт.
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub state: State,
    #[serde(default)]
    pub kinds: Vec<Kind>,
    #[serde(default)]
    pub speaker: Option<String>,
    #[serde(default)]
    pub note: String,
}

impl Phrase {
    pub fn duration_ms(&self) -> u64 {
        self.end_ms.saturating_sub(self.start_ms)
    }

    /// Сколько **разных** ответов дали движки.
    ///
    /// Единица означает, что все сошлись. Это подсказка, куда смотреть
    /// первым, и только она: движки ошибаются похоже, и согласие правдой
    /// не является.
    pub fn distinct_hypotheses(&self) -> usize {
        let mut seen: Vec<Vec<String>> = self
            .hypotheses
            .values()
            .map(|text| normalize(text))
            .collect();
        seen.sort();
        seen.dedup();
        seen.len()
    }

    /// Все ли движки сошлись.
    pub fn engines_agree(&self) -> bool {
        self.hypotheses.len() > 1 && self.distinct_hypotheses() == 1
    }
}

/// Разметка одной записи.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Labels {
    pub case: String,
    /// Растёт при каждом сохранении человеком. Едет в историю замеров:
    /// без неё непонятно, от чего изменился WER — от модели или от
    /// правки эталона.
    #[serde(default)]
    pub version: u32,
    /// Чем резали. Сегодня всегда `vad`.
    pub boundaries: String,
    /// Какие движки участвовали.
    #[serde(default)]
    pub engines: Vec<String>,
    #[serde(default)]
    pub updated_ms: u64,
    pub phrases: Vec<Phrase>,
}

impl Labels {
    /// Проверенные фразы — те и только те, что идут в эталон.
    pub fn verified(&self) -> impl Iterator<Item = &Phrase> {
        self.phrases.iter().filter(|phrase| phrase.state.verified())
    }

    /// Сколько фраз в каком состоянии.
    pub fn progress(&self) -> Progress {
        let mut progress = Progress::default();
        for phrase in &self.phrases {
            match phrase.state {
                State::Unchecked => progress.unchecked += 1,
                State::Correct => progress.correct += 1,
                State::Corrected => progress.corrected += 1,
                State::Skip => progress.skipped += 1,
            }
        }
        progress
    }

    /// Слить свежие гипотезы со старой разметкой.
    ///
    /// Ручная работа **не теряется**: состояние, текст, виды правки,
    /// говорящий и замечание переезжают из старой разметки в новую по
    /// совпадению границ.
    ///
    /// Границы совпадать обязаны. Если VAD нарезал иначе — другая
    /// модель, другой отступ, — сопоставить фразы нечем, и правильный
    /// ответ здесь **отказ**, а не «перенесём что похоже»: разметка,
    /// приехавшая не на ту фразу, хуже потерянной, потому что выглядит
    /// целой.
    pub fn merge_from(&mut self, previous: &Labels) -> Result<usize, String> {
        let mut carried = 0usize;
        let mut lost = Vec::new();

        for old in previous
            .phrases
            .iter()
            .filter(|p| p.state != State::Unchecked)
        {
            match self
                .phrases
                .iter_mut()
                .find(|new| new.start_ms == old.start_ms && new.end_ms == old.end_ms)
            {
                Some(new) => {
                    new.text = old.text.clone();
                    new.state = old.state;
                    new.kinds = old.kinds.clone();
                    new.speaker = old.speaker.clone();
                    new.note = old.note.clone();
                    carried += 1;
                }
                None => lost.push(old.id.clone()),
            }
        }

        if !lost.is_empty() {
            return Err(format!(
                "границы фраз изменились, и {} размеченных не на что положить \
                 (первые: {}). Перенести разметку нечем: фраза, приехавшая не на \
                 своё место, хуже потерянной",
                lost.len(),
                lost.iter().take(3).cloned().collect::<Vec<_>>().join(", ")
            ));
        }
        self.version = previous.version;
        Ok(carried)
    }
}

/// Сколько сделано.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)]
pub struct Progress {
    pub unchecked: usize,
    pub correct: usize,
    pub corrected: usize,
    pub skipped: usize,
}

impl Progress {
    pub fn total(&self) -> usize {
        self.unchecked + self.correct + self.corrected + self.skipped
    }

    pub fn verified(&self) -> usize {
        self.correct + self.corrected
    }
}

/// Прочитать разметку.
pub fn load(path: &std::path::Path) -> Result<Labels, String> {
    let text =
        std::fs::read_to_string(path).map_err(|error| format!("{}: {error}", path.display()))?;
    serde_json::from_str(&text).map_err(|error| format!("{}: {error}", path.display()))
}

/// Записать разметку.
pub fn save(labels: &Labels, path: &std::path::Path) -> Result<(), String> {
    let json = serde_json::to_string_pretty(labels).map_err(|error| error.to_string())?;
    std::fs::write(path, json).map_err(|error| format!("{}: {error}", path.display()))
}

/// Собрать страницу разметки рядом со случаем.
///
/// Один самодостаточный файл: ни одной сетевой загрузки, звук берётся из
/// `mic.wav` в том же каталоге. Разметка вшивается в страницу, а не
/// читается ею с диска: браузер файлы рядом с собой читать не даёт, и
/// притворяться, что даёт, было бы тем самым молчаливым отказом.
///
/// Сохранение — скачиванием: Safari не позволяет странице писать в
/// файловую систему. Человек кладёт `labels.json` обратно сам, и в этом
/// нет ничего страшного, пока об этом сказано прямо.
pub fn write_page(labels: &Labels, path: &std::path::Path) -> Result<(), String> {
    // Признак согласия считается **здесь**, а не в странице. Иначе
    // нормализация текста была бы написана дважды — в `wer.rs` и на
    // JavaScript, — и две реализации одного правила разошлись бы молча:
    // страница показывала бы согласие там, где замер его не видит.
    let view = serde_json::json!({
        "case": labels.case,
        "version": labels.version,
        "boundaries": labels.boundaries,
        "engines": labels.engines,
        "updated_ms": labels.updated_ms,
        "phrases": labels
            .phrases
            .iter()
            .map(|phrase| {
                let mut value = serde_json::to_value(phrase).unwrap_or(serde_json::Value::Null);
                if let Some(object) = value.as_object_mut() {
                    object.insert("agree".into(), phrase.engines_agree().into());
                    object.insert(
                        "distinct".into(),
                        phrase.distinct_hypotheses().into(),
                    );
                }
                value
            })
            .collect::<Vec<_>>(),
    });
    let data = serde_json::to_string(&view).map_err(|error| error.to_string())?;
    // Закрывающий тег внутри строки закрыл бы <script> посреди данных.
    let data = data.replace("</", "<\\/");
    let page = PAGE.replace("__DATA__", &data);
    std::fs::write(path, page).map_err(|error| format!("{}: {error}", path.display()))
}

const PAGE: &str = r##"<!doctype html>
<html lang="ru">
<head>
<meta charset="utf-8">
<title>Разметка</title>
<style>
:root { color-scheme: light dark; --gap: 12px; }
body { font: 15px/1.5 -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif;
       margin: 0; padding: var(--gap); }
header { position: sticky; top: 0; background: Canvas; padding: 8px 0;
         border-bottom: 1px solid GrayText; z-index: 2; }
h1 { font-size: 18px; margin: 0 0 4px; }
.bar { display: flex; gap: var(--gap); align-items: center; flex-wrap: wrap; }
.count { font-variant-numeric: tabular-nums; }
.phrase { border: 1px solid GrayText; border-radius: 8px; padding: 10px;
          margin: var(--gap) 0; }
.phrase[data-state="correct"] { border-color: green; }
.phrase[data-state="corrected"] { border-color: orange; }
.phrase[data-state="skip"] { opacity: .55; }
.head { display: flex; gap: var(--gap); align-items: center; flex-wrap: wrap; }
.time { font-variant-numeric: tabular-nums; color: GrayText; }
.agree { color: green; }
.differ { color: orange; }
.hyp { margin: 6px 0; padding: 4px 6px; border-radius: 6px; cursor: pointer;
       border: 1px dashed GrayText; }
.hyp:hover { border-style: solid; }
.hyp .who { color: GrayText; font-size: 13px; margin-right: 6px; }
textarea { width: 100%; box-sizing: border-box; font: inherit; min-height: 3em; }
.kinds { display: flex; gap: 10px; flex-wrap: wrap; margin-top: 6px;
         font-size: 14px; }
button { font: inherit; padding: 4px 10px; }
.hidden { display: none; }
footer { position: sticky; bottom: 0; background: Canvas; padding: 8px 0;
         border-top: 1px solid GrayText; }
.note { color: GrayText; font-size: 13px; }
</style>
</head>
<body>
<header>
  <h1>Разметка <span id="case"></span></h1>
  <div class="bar">
    <audio id="audio" src="mic.wav" preload="auto"></audio>
    <span class="count" id="progress"></span>
    <label><input type="checkbox" id="only-open"> только непроверенные</label>
    <label><input type="checkbox" id="only-differ"> только расхождения</label>
    <button id="save">Скачать labels.json</button>
    <button id="copy">Скопировать</button>
  </div>
  <div class="note">Файл кладётся обратно в каталог случая руками: писать на
    диск браузеру нельзя, и делать вид, что можно, — хуже.</div>
</header>
<main id="list"></main>
<footer class="note">
  Согласие движков говорит, куда смотреть первым, и ничего не говорит о
  правде: они ошибаются похоже. Галочку ставит ухо.
</footer>
<script id="data" type="application/json">__DATA__</script>
<script>
const KINDS = [
  ["term", "термин или имя"],
  ["misheard", "услышано неверно"],
  ["missing", "пропущено или выдумано"],
  ["boundary", "граница не там"],
];
const data = JSON.parse(document.getElementById("data").textContent);
const audio = document.getElementById("audio");
const list = document.getElementById("list");
document.getElementById("case").textContent = data.case;

// Нормализации здесь нет намеренно: правило живёт в одном месте — в
// `wer.rs`, — и его результат приезжает готовым полем `agree`. Вторая
// реализация того же правила на JavaScript разошлась бы с первой, и
// страница показывала бы согласие там, где замер его не видит.

let stopAt = null;
audio.addEventListener("timeupdate", () => {
  if (stopAt !== null && audio.currentTime >= stopAt) { audio.pause(); stopAt = null; }
});
function play(phrase) {
  stopAt = phrase.end_ms / 1000;
  audio.currentTime = phrase.start_ms / 1000;
  audio.play();
}

function setState(phrase, node, state) {
  phrase.state = state;
  node.dataset.state = state;
  renderProgress();
}

function renderProgress() {
  const counts = { unchecked: 0, correct: 0, corrected: 0, skip: 0 };
  for (const phrase of data.phrases) counts[phrase.state] = (counts[phrase.state] || 0) + 1;
  document.getElementById("progress").textContent =
    `всего ${data.phrases.length} · верных ${counts.correct} · правленых ${counts.corrected}` +
    ` · пропущено ${counts.skip} · осталось ${counts.unchecked}`;
}

function build(phrase) {
  const node = document.createElement("section");
  node.className = "phrase";
  node.dataset.state = phrase.state;

  const agree = phrase.agree;
  node.dataset.differ = agree ? "no" : "yes";

  const head = document.createElement("div");
  head.className = "head";
  const play_button = document.createElement("button");
  play_button.textContent = "▶";
  play_button.onclick = () => play(phrase);
  const time = document.createElement("span");
  time.className = "time";
  const seconds = (ms) => (ms / 1000).toFixed(1);
  time.textContent = `${phrase.id}  ${seconds(phrase.start_ms)}–${seconds(phrase.end_ms)} с`;
  const mark = document.createElement("span");
  mark.className = agree ? "agree" : "differ";
  mark.textContent = agree
    ? `движки сошлись (${Object.keys(phrase.hypotheses).length})`
    : `разошлись: ${phrase.distinct}`;
  head.append(play_button, time, mark);

  const area = document.createElement("textarea");
  area.value = phrase.text || "";

  const hyps = document.createElement("div");
  for (const [engine, text] of Object.entries(phrase.hypotheses)) {
    const row = document.createElement("div");
    row.className = "hyp";
    row.title = "взять как верный текст";
    const who = document.createElement("span");
    who.className = "who";
    who.textContent = engine;
    row.append(who, document.createTextNode(text));
    row.onclick = () => { area.value = text; area.dispatchEvent(new Event("input")); };
    hyps.append(row);
  }

  const kinds = document.createElement("div");
  kinds.className = "kinds";
  for (const [key, label] of KINDS) {
    const box = document.createElement("label");
    const input = document.createElement("input");
    input.type = "checkbox";
    input.checked = (phrase.kinds || []).includes(key);
    input.onchange = () => {
      const set = new Set(phrase.kinds || []);
      input.checked ? set.add(key) : set.delete(key);
      phrase.kinds = [...set];
    };
    box.append(input, document.createTextNode(" " + label));
    kinds.append(box);
  }

  const buttons = document.createElement("div");
  buttons.className = "bar";
  const ok = document.createElement("button");
  ok.textContent = "верно";
  const draft = phrase.text;
  ok.onclick = () => {
    // Сравнение точное, а не нормализованное: правило нормализации живёт
    // в замере и сюда не дублируется. Поправленная пунктуация — тоже
    // правка, и честнее назвать её правкой, чем молча приравнять к
    // исходному тексту.
    phrase.text = area.value.trim();
    setState(phrase, node, phrase.text === draft.trim() ? "correct" : "corrected");
  };
  const skip = document.createElement("button");
  skip.textContent = "не годится";
  skip.onclick = () => { phrase.text = area.value.trim(); setState(phrase, node, "skip"); };
  const reset = document.createElement("button");
  reset.textContent = "вернуть";
  reset.onclick = () => setState(phrase, node, "unchecked");
  buttons.append(ok, skip, reset);

  const speaker = document.createElement("input");
  speaker.placeholder = "кто говорит";
  speaker.value = phrase.speaker || "";
  speaker.oninput = () => { phrase.speaker = speaker.value.trim() || null; };
  buttons.append(speaker);

  area.oninput = () => { phrase.text = area.value; };

  node.append(head, hyps, area, kinds, buttons);
  return node;
}

const nodes = data.phrases.map((phrase) => {
  const node = build(phrase);
  list.append(node);
  return node;
});

function applyFilters() {
  const openOnly = document.getElementById("only-open").checked;
  const differOnly = document.getElementById("only-differ").checked;
  nodes.forEach((node, index) => {
    const phrase = data.phrases[index];
    const hide = (openOnly && phrase.state !== "unchecked")
      || (differOnly && node.dataset.differ === "no");
    node.classList.toggle("hidden", hide);
  });
}
document.getElementById("only-open").onchange = applyFilters;
document.getElementById("only-differ").onchange = applyFilters;

function serialise() {
  data.version = (data.version || 0) + 1;
  data.updated_ms = Date.now();
  // Служебные поля страницы обратно не едут: их считает стенд, и
  // сохранённые здесь они однажды разошлись бы с пересчитанными.
  const out = { ...data, phrases: data.phrases.map((p) => {
    const { agree, distinct, ...rest } = p;
    return rest;
  }) };
  return JSON.stringify(out, null, 2);
}
document.getElementById("save").onclick = () => {
  const blob = new Blob([serialise()], { type: "application/json" });
  const link = document.createElement("a");
  link.href = URL.createObjectURL(blob);
  link.download = "labels.json";
  link.click();
};
document.getElementById("copy").onclick = async () => {
  await navigator.clipboard.writeText(serialise());
  document.getElementById("copy").textContent = "скопировано";
};

renderProgress();
</script>
</body>
</html>
"##;

#[cfg(test)]
mod tests {
    use super::*;

    fn phrase(id: &str, start: u64, end: u64, hypotheses: &[(&str, &str)]) -> Phrase {
        Phrase {
            id: id.to_string(),
            start_ms: start,
            end_ms: end,
            hypotheses: hypotheses
                .iter()
                .map(|(engine, text)| (engine.to_string(), text.to_string()))
                .collect(),
            text: hypotheses
                .first()
                .map(|(_, t)| t.to_string())
                .unwrap_or_default(),
            state: State::Unchecked,
            kinds: Vec::new(),
            speaker: None,
            note: String::new(),
        }
    }

    fn labels(phrases: Vec<Phrase>) -> Labels {
        Labels {
            case: "test".to_string(),
            version: 1,
            boundaries: "vad".to_string(),
            engines: vec!["a".to_string(), "b".to_string()],
            updated_ms: 0,
            phrases,
        }
    }

    /// Согласие движков считается после нормализации: регистр и точка не
    /// расхождение.
    #[test]
    fn agreement_ignores_case_and_punctuation() {
        let one = phrase("1", 0, 1000, &[("a", "привет мир"), ("b", "Привет, мир.")]);
        assert!(one.engines_agree());
        assert_eq!(one.distinct_hypotheses(), 1);

        let two = phrase("2", 0, 1000, &[("a", "привет мир"), ("b", "привет мор")]);
        assert!(!two.engines_agree());
        assert_eq!(two.distinct_hypotheses(), 2);
    }

    /// Один движок — не согласие. Иначе прогон с единственным движком
    /// объявлял бы согласными все фразы подряд.
    #[test]
    fn a_single_engine_never_agrees_with_itself() {
        let alone = phrase("1", 0, 1000, &[("a", "привет мир")]);
        assert!(!alone.engines_agree());
    }

    /// Непроверенное в эталон не идёт — даже когда движки сошлись.
    ///
    /// Тот самый случай, ради которого заведено состояние: согласие
    /// движков выглядит убедительно и правдой не является.
    #[test]
    fn agreement_alone_does_not_make_a_phrase_verified() {
        let agreed = phrase("1", 0, 1000, &[("a", "привет мир"), ("b", "привет мир")]);
        assert!(agreed.engines_agree());
        assert!(!agreed.state.verified(), "согласие — не проверка");

        let set = labels(vec![agreed]);
        assert_eq!(set.verified().count(), 0);
    }

    /// Ручная работа переезжает при повторной разметке.
    #[test]
    fn human_work_survives_a_second_pass() {
        let mut old = labels(vec![phrase("1", 0, 1000, &[("a", "юнифай")])]);
        old.phrases[0].text = "униффи".to_string();
        old.phrases[0].state = State::Corrected;
        old.phrases[0].kinds = vec![Kind::Term];
        old.phrases[0].speaker = Some("аня".to_string());

        // Второй прогон: тот же VAD, добавился движок.
        let mut fresh = labels(vec![phrase(
            "1",
            0,
            1000,
            &[("a", "юнифай"), ("b", "унифи")],
        )]);
        let carried = fresh.merge_from(&old).expect("границы те же");

        assert_eq!(carried, 1);
        assert_eq!(fresh.phrases[0].text, "униффи");
        assert_eq!(fresh.phrases[0].state, State::Corrected);
        assert_eq!(fresh.phrases[0].kinds, vec![Kind::Term]);
        assert_eq!(fresh.phrases[0].speaker.as_deref(), Some("аня"));
        assert_eq!(
            fresh.phrases[0].hypotheses.len(),
            2,
            "свежие гипотезы обязаны приехать"
        );
    }

    /// А при съехавших границах — отказ, а не перенос наугад.
    ///
    /// Заведомо отрицательный случай к предыдущему: слияние, которое
    /// всегда что-нибудь переносит, прошло бы тот тест и молча
    /// приписало бы правку чужой фразе.
    #[test]
    fn shifted_boundaries_refuse_instead_of_guessing() {
        let mut old = labels(vec![phrase("1", 0, 1000, &[("a", "юнифай")])]);
        old.phrases[0].state = State::Corrected;

        let mut fresh = labels(vec![phrase("1", 200, 1200, &[("a", "юнифай")])]);
        let error = fresh.merge_from(&old).expect_err("обязан отказать");
        assert!(error.contains("границы фраз изменились"), "{error}");
    }

    /// Непроверенные фразы при слиянии не переносятся и отказа не
    /// вызывают: терять в них нечего.
    #[test]
    fn unchecked_phrases_do_not_block_a_reboundary() {
        let old = labels(vec![phrase("1", 0, 1000, &[("a", "что-то")])]);
        let mut fresh = labels(vec![phrase("1", 500, 1500, &[("a", "что-то")])]);
        assert!(fresh.merge_from(&old).is_ok());
    }

    /// Страница самодостаточна: ни одной сетевой загрузки.
    ///
    /// Проверяется, потому что ломается молча: страница со ссылкой на
    /// шрифт или скрипт открывается и выглядит целой, пока есть сеть, и
    /// перестаёт работать ровно тогда, когда человек сел размечать в
    /// самолёте.
    #[test]
    fn the_page_loads_nothing_from_the_network() {
        let page = render("network");
        assert!(!page.contains("http://"), "внешняя ссылка в странице");
        assert!(!page.contains("https://"), "внешняя ссылка в странице");
        assert!(page.contains("src=\"mic.wav\""), "звук берётся рядом");
    }

    /// Правило нормализации в странице не повторяется.
    ///
    /// Оно живёт в `wer.rs`, и его результат приезжает готовым полем
    /// `agree`. Вторая реализация на JavaScript разошлась бы с первой, и
    /// страница показывала бы согласие там, где замер его не видит, —
    /// ровно тот случай, которым этот проект уже обжигался на двух
    /// правдах об одном.
    #[test]
    fn the_page_does_not_normalise_text_on_its_own() {
        let page = render("norm");
        assert!(
            !page.contains("toLowerCase"),
            "в странице появилась своя нормализация — правило раздвоилось"
        );
        assert!(page.contains("phrase.agree"), "признак приезжает готовым");
    }

    /// Разметка доезжает до страницы целиком и с признаком согласия.
    #[test]
    fn the_page_carries_the_labels_and_the_agreement() {
        let page = render("agree");
        assert!(page.contains("\"agree\":true"), "согласие посчитано здесь");
        assert!(page.contains("\"agree\":false"), "и расхождение тоже");
        assert!(page.contains("юнифай"), "гипотезы доехали");
    }

    /// Закрывающий тег внутри данных не рвёт страницу пополам.
    ///
    /// Расшифровка встречи может содержать что угодно, включая `</script>`
    /// — и тогда страница молча обрывается на середине.
    #[test]
    fn a_closing_tag_inside_the_data_does_not_break_the_page() {
        let mut set = labels(vec![phrase(
            "1",
            0,
            1000,
            &[("a", "сказал </script> и всё")],
        )]);
        set.phrases[0].text = "сказал </script> и всё".to_string();
        let path = std::env::temp_dir().join(format!("bench-page-tag-{}.html", std::process::id()));
        write_page(&set, &path).expect("страница записалась");
        let page = std::fs::read_to_string(&path).expect("прочиталась");
        let _ = std::fs::remove_file(&path);

        // Тегов ровно столько, сколько их в самой странице: данные свой
        // не добавили.
        assert_eq!(
            page.matches("</script>").count(),
            2,
            "данные добавили закрывающий тег и разорвали страницу"
        );
    }

    /// Собрать страницу из двух фраз: согласной и расходящейся.
    ///
    /// Имя файла своё у каждого теста: тесты идут параллельно, и на общем
    /// имени они затирали файл друг у друга. Ровно та же ловушка, что с
    /// каталогами случаев в `case.rs`.
    fn render(name: &str) -> String {
        let set = labels(vec![
            phrase("1", 0, 1000, &[("a", "привет мир"), ("b", "Привет, мир.")]),
            phrase("2", 1000, 2000, &[("a", "юнифай"), ("b", "унифи")]),
        ]);
        let path =
            std::env::temp_dir().join(format!("bench-page-{}-{name}.html", std::process::id()));
        write_page(&set, &path).expect("страница записалась");
        let page = std::fs::read_to_string(&path).expect("прочиталась");
        let _ = std::fs::remove_file(&path);
        page
    }

    /// Счётчик состояний считает всё и ничего не теряет.
    #[test]
    fn progress_counts_every_phrase_once() {
        let mut set = labels(vec![
            phrase("1", 0, 1000, &[("a", "раз")]),
            phrase("2", 1000, 2000, &[("a", "два")]),
            phrase("3", 2000, 3000, &[("a", "три")]),
            phrase("4", 3000, 4000, &[("a", "четыре")]),
        ]);
        set.phrases[1].state = State::Correct;
        set.phrases[2].state = State::Corrected;
        set.phrases[3].state = State::Skip;

        let progress = set.progress();
        assert_eq!(progress.total(), set.phrases.len());
        assert_eq!(progress.unchecked, 1);
        assert_eq!(progress.verified(), 2);
        assert_eq!(progress.skipped, 1);
        assert_eq!(set.verified().count(), 2, "skip в эталон не идёт");
    }
}
