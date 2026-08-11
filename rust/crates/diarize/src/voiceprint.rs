//! Слепок голоса: вектор по размеченным человеком кускам.
//!
//! Другая задача, чем кластеризация, и **более лёгкая**. Кластеризация
//! решает без подсказок, сколько в записи людей и где границы; на трудном
//! материале она это угадывает плохо — шестеро разошлись по сотне
//! голосов. Здесь число людей называет человек, и угадывать нечего:
//! остаётся померить похожесть.
//!
//! Обучения здесь нет ни в каком виде, и слово «дообучить» описывает не
//! то, что происходит. Модель не меняется — она считает вектор по куску
//! звука и всё. «Дообучение» на новых кусках сводится к **среднему по
//! большему числу примеров**: слепок становится устойчивее, потому что
//! случайности отдельных реплик взаимно гасятся. Отсюда и приятное
//! свойство: добавить примеров дёшево, и делать это можно сколько угодно
//! раз без прогонов чего бы то ни было.
//!
//! Всё, что здесь, — арифметика над векторами, и она собирается и
//! проверяется без фичи `model`. Считает векторы движок, а сравнивает их
//! этот файл.

/// Слепок: усреднённый вектор голоса и то, из чего он посчитан.
#[derive(Debug, Clone, PartialEq)]
pub struct VoicePrint {
    /// Единичной длины: сравнение идёт косинусом, и ненормированный
    /// вектор дал бы разную похожесть от одной лишь громкости.
    pub vector: Vec<f32>,
    /// Из скольки кусков усреднён.
    pub samples: usize,
    /// Сколько секунд материала в нём.
    pub seconds: f32,
}

/// Сложить слепок из векторов размеченных кусков.
///
/// `None` — складывать нечего либо все векторы вырожденные. Пустой слепок
/// не возвращается намеренно: он сравнивался бы со всем подряд с
/// похожестью ноль и выглядел бы как «человек не найден» вместо «слепка
/// нет».
pub fn build_print(vectors: &[(Vec<f32>, f32)]) -> Option<VoicePrint> {
    let dim = vectors.first()?.0.len();
    if dim == 0 || vectors.iter().any(|(vector, _)| vector.len() != dim) {
        return None;
    }

    // Нормируем **до** усреднения, а не после. Иначе громкий кусок тянет
    // среднее на себя пропорционально своей длине вектора, то есть слепок
    // получается по самой громкой реплике, а не по человеку.
    let mut sum = vec![0.0f32; dim];
    let mut used = 0usize;
    let mut seconds = 0.0f32;
    for (vector, length) in vectors {
        let Some(unit) = normalise(vector) else {
            continue;
        };
        for (slot, value) in sum.iter_mut().zip(unit) {
            *slot += value;
        }
        used += 1;
        seconds += length;
    }
    if used == 0 {
        return None;
    }
    Some(VoicePrint {
        vector: normalise(&sum)?,
        samples: used,
        seconds,
    })
}

/// Косинус между вектором и слепком: от -1 до 1, больше — похожее.
pub fn similarity(vector: &[f32], print: &VoicePrint) -> f32 {
    let Some(unit) = normalise(vector) else {
        return -1.0;
    };
    if unit.len() != print.vector.len() {
        return -1.0;
    }
    unit.iter()
        .zip(&print.vector)
        .map(|(a, b)| a * b)
        .sum::<f32>()
        .clamp(-1.0, 1.0)
}

/// К кому отнести кусок.
#[derive(Debug, Clone, PartialEq)]
pub enum Match {
    /// Имя, похожесть, отрыв от следующего.
    Named {
        name: String,
        similarity: f32,
        margin: f32,
    },
    /// Никого достаточно похожего либо двое одинаково похожи.
    ///
    /// Отдельный ответ, а не имя с низкой уверенностью: неверная подпись
    /// убедительна, и человек на неё полагается. Спека это правило
    /// называет главным.
    Unknown { best: f32, margin: f32 },
}

/// Отнести вектор к слепку — или честно отказаться.
///
/// Два условия, и оба обязательны. Похожесть не ниже `accept` — иначе
/// подписан будет любой, лишь бы он был ближе прочих. И отрыв от
/// следующего не меньше `margin` — иначе двое похожих делят реплики
/// монеткой, причём каждый раз уверенно.
pub fn best_match(
    vector: &[f32],
    prints: &[(String, VoicePrint)],
    accept: f32,
    margin: f32,
) -> Match {
    let mut scored: Vec<(&String, f32)> = prints
        .iter()
        .map(|(name, print)| (name, similarity(vector, print)))
        .collect();
    scored.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(b.0)));

    let Some((name, best)) = scored
        .first()
        .map(|(name, score)| ((*name).clone(), *score))
    else {
        return Match::Unknown {
            best: -1.0,
            margin: 0.0,
        };
    };
    let runner_up = scored.get(1).map(|(_, score)| *score).unwrap_or(-1.0);
    let gap = best - runner_up;

    if best >= accept && gap >= margin {
        Match::Named {
            name,
            similarity: best,
            margin: gap,
        }
    } else {
        Match::Unknown { best, margin: gap }
    }
}

/// Вектор единичной длины; `None` — вектор нулевой либо пустой.
fn normalise(vector: &[f32]) -> Option<Vec<f32>> {
    let norm = vector.iter().map(|value| value * value).sum::<f32>().sqrt();
    if !norm.is_finite() || norm <= f32::EPSILON {
        return None;
    }
    Some(vector.iter().map(|value| value / norm).collect())
}

/// Движок, считающий вектор голоса по куску звука.
pub trait VoiceEmbedder {
    fn embed(&mut self, pcm: &[i16], sample_rate: u32) -> Result<Vec<f32>, String>;
    /// Длина вектора. Векторы разных моделей несравнимы, и размерность —
    /// первое, чем это видно.
    fn dim(&self) -> usize;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn print_of(vectors: &[Vec<f32>]) -> VoicePrint {
        build_print(
            &vectors
                .iter()
                .map(|vector| (vector.clone(), 1.0))
                .collect::<Vec<_>>(),
        )
        .expect("слепок")
    }

    #[test]
    fn a_print_of_one_vector_is_that_vector() {
        let print = print_of(&[vec![3.0, 4.0]]);

        assert_eq!(print.samples, 1);
        assert!((print.vector[0] - 0.6).abs() < 1e-6);
        assert!((print.vector[1] - 0.8).abs() < 1e-6);
    }

    /// Громкий кусок не перевешивает: длина вектора снимается до
    /// усреднения.
    ///
    /// Иначе слепок описывал бы самую громкую реплику человека, а не его
    /// голос, и на тихих местах встречи переставал бы узнавать.
    #[test]
    fn a_loud_sample_does_not_outweigh_a_quiet_one() {
        let quiet = vec![1.0, 0.0];
        let loud = vec![0.0, 100.0];

        let print = print_of(&[quiet, loud]);

        // Ровно посередине: 45 градусов.
        assert!(
            (print.vector[0] - print.vector[1]).abs() < 1e-5,
            "слепок съехал к громкому: {:?}",
            print.vector
        );
    }

    #[test]
    fn nothing_to_average_is_not_an_empty_print() {
        assert_eq!(build_print(&[]), None);
        assert_eq!(build_print(&[(vec![0.0, 0.0], 1.0)]), None);
        // Разная размерность — не слепок, а смесь двух моделей.
        assert_eq!(
            build_print(&[(vec![1.0], 1.0), (vec![1.0, 0.0], 1.0)]),
            None
        );
    }

    #[test]
    fn similarity_is_one_for_the_same_direction() {
        let print = print_of(&[vec![1.0, 1.0]]);

        assert!((similarity(&[2.0, 2.0], &print) - 1.0).abs() < 1e-6);
        assert!(similarity(&[1.0, -1.0], &print).abs() < 1e-6);
        assert!((similarity(&[-1.0, -1.0], &print) + 1.0).abs() < 1e-6);
    }

    /// Похожий, но не дотянувший до порога, остаётся неопознанным.
    #[test]
    fn below_the_accept_threshold_nobody_is_named() {
        let prints = vec![("аня".to_string(), print_of(&[vec![1.0, 0.0]]))];

        let seen = best_match(&[1.0, 1.0], &prints, 0.9, 0.0);

        assert!(
            matches!(seen, Match::Unknown { .. }),
            "подписан при похожести 0.71: {seen:?}"
        );
    }

    /// Двое одинаково похожи — не имя, а отказ.
    ///
    /// Самый опасный случай: похожесть высока у обоих, и без правила об
    /// отрыве реплики делились бы монеткой, каждый раз уверенно.
    #[test]
    fn a_tie_between_two_people_names_neither() {
        let prints = vec![
            ("аня".to_string(), print_of(&[vec![1.0, 0.0]])),
            ("боря".to_string(), print_of(&[vec![0.0, 1.0]])),
        ];

        let seen = best_match(&[1.0, 1.0], &prints, 0.5, 0.1);

        match seen {
            Match::Unknown { margin, .. } => assert!(margin.abs() < 1e-6, "отрыв {margin}"),
            other => panic!("подписан при равной похожести: {other:?}"),
        }
    }

    /// Уверенный случай подписывается — иначе правило отказа не проверено
    /// ничем: отказывать всегда умеет и сломанный.
    #[test]
    fn a_clear_winner_is_named() {
        let prints = vec![
            ("аня".to_string(), print_of(&[vec![1.0, 0.0]])),
            ("боря".to_string(), print_of(&[vec![0.0, 1.0]])),
        ];

        let seen = best_match(&[10.0, 1.0], &prints, 0.9, 0.2);

        match seen {
            Match::Named { name, .. } => assert_eq!(name, "аня"),
            other => panic!("уверенный случай не подписан: {other:?}"),
        }
    }

    /// Слепков нет вовсе — отказ, а не паника.
    #[test]
    fn without_prints_everything_is_unknown() {
        assert!(matches!(
            best_match(&[1.0, 0.0], &[], 0.5, 0.1),
            Match::Unknown { .. }
        ));
    }

    /// Порядок слепков на ответ не влияет: иначе один и тот же отчёт
    /// менялся бы от порядка строк в базе.
    #[test]
    fn the_order_of_prints_does_not_change_the_answer() {
        let anya = ("аня".to_string(), print_of(&[vec![1.0, 0.0]]));
        let borya = ("боря".to_string(), print_of(&[vec![0.0, 1.0]]));

        let straight = best_match(&[10.0, 1.0], &[anya.clone(), borya.clone()], 0.9, 0.2);
        let reversed = best_match(&[10.0, 1.0], &[borya, anya], 0.9, 0.2);

        assert_eq!(straight, reversed);
    }
}
