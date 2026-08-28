//! Три способа разрезать запись на куски перед распознаванием.
//!
//! Смысл оси в том, что все три дают внешне одинаковое — список кусков с
//! временем. Разница в том, **откуда взялись границы**, и видна она
//! только в метриках: у окон границы стоят по часам, у VAD — в паузах,
//! у диаризации — на смене голоса.
//!
//! Резать нужно ещё и потому, что офлайновый transducer на длинном куске
//! ведёт себя непредсказуемо по памяти и не даёт ни одной точки отмены.
//! Но это следствие, а не причина: причина — в том, что тридцать секунд
//! одним сегментом теряют и границы реплик, и смену говорящего внутри.

/// Как резать.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Strategy {
    /// Окна по 30 секунд — то, что делает сегодня `GigaamBatchTranscriber`.
    ///
    /// В стенде это **база сравнения**, а не кандидат: способ, с которым
    /// сравнивают остальные.
    Windows30,
    /// Границы от детектора речи.
    Vad,
    /// Границы от разделения голосов: метка едет вместе с куском.
    Diarize,
}

impl Strategy {
    pub fn name(self) -> &'static str {
        match self {
            Self::Windows30 => "windows30",
            Self::Vad => "vad",
            Self::Diarize => "diarize",
        }
    }

    pub fn parse(text: &str) -> Result<Self, String> {
        match text {
            "windows30" => Ok(Self::Windows30),
            "vad" => Ok(Self::Vad),
            "diarize" => Ok(Self::Diarize),
            other => Err(format!(
                "нарезка бывает windows30, vad или diarize, а не {other}"
            )),
        }
    }
}

/// Кусок записи, который поедет в движок отдельно.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Piece {
    pub start_ms: u64,
    pub end_ms: u64,
    /// Метка голоса, если её дала нарезка.
    ///
    /// У окон и у VAD её нет вовсе, и `None` здесь честнее нуля: нулевой
    /// кластер — это «первый голос», а не «голос неизвестен».
    pub speaker: Option<u32>,
}

/// Длиннее этого кусок движку не отдаётся.
///
/// Нужно всем трём способам. Без потолка нарезка по речи на непрерывном
/// монологе выродилась бы в те же тридцать секунд, только без окон, — и
/// сравнение способов показало бы, что «разницы нет», по причине,
/// которая к способам отношения не имеет.
pub const MAX_PIECE_MS: u64 = 30_000;

/// Окна одинаковой длины, безотносительно речи.
pub fn split_windows(total_ms: u64, window_ms: u64) -> Vec<Piece> {
    if total_ms == 0 || window_ms == 0 {
        return Vec::new();
    }
    let mut out = Vec::new();
    let mut start = 0;
    while start < total_ms {
        let end = (start + window_ms).min(total_ms);
        out.push(Piece {
            start_ms: start,
            end_ms: end,
            speaker: None,
        });
        start = end;
    }
    out
}

/// Поле вокруг отрезка речи, мс.
///
/// **Замер, а не вкус.** На контрольной записи GigaAM нарезка ровно по
/// границам VAD дала WER 0.038 против 0.000 у окон: отрезок начался на
/// 120 мс, и «ничьих» приехало как «чьих» — детектор срезал начало
/// первого слова. Замер PR #172 такого не показал, потому что мерил
/// признак «речь/не речь» покадрово, а не то, что услышит движок.
///
/// Двести миллисекунд — вдвое больше срезанного там. Своё число этот
/// параметр получает на встречах, а не на одной записи.
pub const SPEECH_PAD_MS: u64 = 200;

/// Куски по отрезкам речи, с полями вокруг каждого.
///
/// `total_ms` нужен, чтобы поле не вылезло за конец записи: кусок,
/// уехавший за её край, дал бы пустые отсчёты и молча потерянную реплику.
pub fn from_speech(speech: &[(u64, u64)], total_ms: u64) -> Vec<Piece> {
    from_speech_padded(speech, SPEECH_PAD_MS, total_ms)
}

/// То же с явным полем — им меряют цену самого поля.
pub fn from_speech_padded(speech: &[(u64, u64)], pad_ms: u64, total_ms: u64) -> Vec<Piece> {
    let mut padded: Vec<(u64, u64)> = speech
        .iter()
        .map(|(start, end)| {
            (
                start.saturating_sub(pad_ms),
                (end + pad_ms).min(total_ms.max(*end)),
            )
        })
        .collect();
    padded.sort_unstable();

    // Поля сближают отрезки, и соседние начинают перекрываться. Оставить
    // так нельзя: перекрытие означает, что общий кусок речи уедет в
    // движок дважды и дважды попадёт в расшифровку. Удвоенная реплика
    // читается как оговорка человека, а не как дефект нарезки.
    let mut merged: Vec<(u64, u64)> = Vec::new();
    for (start, end) in padded {
        match merged.last_mut() {
            Some((_, last_end)) if start <= *last_end => *last_end = (*last_end).max(end),
            _ => merged.push((start, end)),
        }
    }

    cap_length(
        merged
            .into_iter()
            .map(|(start, end)| Piece {
                start_ms: start,
                end_ms: end,
                speaker: None,
            })
            .collect(),
    )
}

/// Куски по отрезкам с меткой голоса.
pub fn from_turns(turns: &[(u64, u64, u32)]) -> Vec<Piece> {
    cap_length(
        turns
            .iter()
            .map(|(start, end, cluster)| Piece {
                start_ms: *start,
                end_ms: *end,
                speaker: Some(*cluster),
            })
            .collect(),
    )
}

/// Разрезать слишком длинные куски, сохранив порядок и метки.
pub fn cap_length(pieces: Vec<Piece>) -> Vec<Piece> {
    let mut out = Vec::new();
    for piece in pieces {
        let mut start = piece.start_ms;
        while piece.end_ms.saturating_sub(start) > MAX_PIECE_MS {
            out.push(Piece {
                start_ms: start,
                end_ms: start + MAX_PIECE_MS,
                speaker: piece.speaker,
            });
            start += MAX_PIECE_MS;
        }
        if piece.end_ms > start {
            out.push(Piece {
                start_ms: start,
                end_ms: piece.end_ms,
                speaker: piece.speaker,
            });
        }
    }
    out
}

/// Отсчёты куска. Границы за пределами записи обрезаются.
pub fn samples<'a>(pcm: &'a [i16], sample_rate: u32, piece: &Piece) -> &'a [i16] {
    let rate = u64::from(sample_rate.max(1));
    let start = (piece.start_ms * rate / 1000) as usize;
    let end = ((piece.end_ms * rate / 1000) as usize).min(pcm.len());
    if start >= end { &[] } else { &pcm[start..end] }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Окна не смотрят на речь вовсе — это их определение, и потому они
    /// база сравнения, а не кандидат.
    #[test]
    fn windows_are_evenly_spaced_regardless_of_content() {
        let pieces = split_windows(70_000, 30_000);
        assert_eq!(pieces.len(), 3);
        assert_eq!(pieces[0].start_ms, 0);
        assert_eq!(pieces[1].start_ms, 30_000);
        assert_eq!(pieces[2].end_ms, 70_000, "хвост не потерялся");
        assert!(pieces.iter().all(|piece| piece.speaker.is_none()));
    }

    /// Пустая запись даёт ноль кусков, а не один пустой.
    #[test]
    fn nothing_produces_no_pieces_at_all() {
        assert!(split_windows(0, 30_000).is_empty());
        assert!(from_speech(&[], 10_000).is_empty());
        assert!(from_turns(&[]).is_empty());
    }

    /// Поле вокруг отрезка речи прирастает с обеих сторон и не уезжает за
    /// начало записи.
    ///
    /// Это то самое, чего не хватило на контрольной записи: отрезок
    /// начинался на 120 мс, и движок терял начало первого слова.
    #[test]
    fn speech_pieces_get_a_margin_on_both_sides() {
        let pieces = from_speech_padded(&[(120, 8546)], 200, 11_290);
        assert_eq!(pieces.len(), 1);
        assert_eq!(pieces[0].start_ms, 0, "за начало записи поле не уходит");
        assert_eq!(pieces[0].end_ms, 8746);
    }

    /// Поля сближают соседей, и перекрытие обязано схлопнуться.
    ///
    /// Иначе общий кусок речи уедет в движок дважды и дважды попадёт в
    /// расшифровку — а удвоенная реплика читается как оговорка человека,
    /// не как дефект нарезки.
    #[test]
    fn overlapping_margins_are_merged_not_transcribed_twice() {
        let pieces = from_speech_padded(&[(1000, 2000), (2100, 3000)], 200, 10_000);
        assert_eq!(pieces.len(), 1, "перекрытие не схлопнулось: {pieces:?}");
        assert_eq!(pieces[0].start_ms, 800);
        assert_eq!(pieces[0].end_ms, 3200);
    }

    /// А далёкие друг от друга отрезки полями не склеиваются.
    ///
    /// Пара к предыдущему: слияние, срабатывающее всегда, прошло бы тот
    /// тест и превратило бы нарезку по речи в одно окно.
    #[test]
    fn distant_pieces_stay_apart() {
        let pieces = from_speech_padded(&[(1000, 2000), (8000, 9000)], 200, 10_000);
        assert_eq!(pieces.len(), 2, "{pieces:?}");
    }

    /// Кусок длиннее потолка режется, и метка голоса переезжает на все
    /// части: иначе хвост длинной реплики потерял бы говорящего.
    #[test]
    fn a_long_piece_is_cut_and_every_part_keeps_its_speaker() {
        let pieces = from_turns(&[(0, 70_000, 3)]);
        assert_eq!(pieces.len(), 3, "70 секунд при потолке 30: {pieces:?}");
        assert!(pieces.iter().all(|piece| piece.speaker == Some(3)));
        assert_eq!(pieces[2].end_ms, 70_000, "хвост не потерялся");
    }

    /// Куски по речи покрывают только речь — в отличие от окон.
    ///
    /// Это то самое различие, ради которого заведена ось: сравнение
    /// осмысленно, только если способы дают **разное** на одном входе.
    #[test]
    fn speech_pieces_skip_the_silence_that_windows_swallow() {
        let speech = [(0u64, 4000u64), (26_000, 30_000)];
        let by_speech = from_speech_padded(&speech, 0, 30_000);
        let by_windows = split_windows(30_000, 30_000);

        let speech_covered: u64 = by_speech
            .iter()
            .map(|piece| piece.end_ms - piece.start_ms)
            .sum();
        let windows_covered: u64 = by_windows
            .iter()
            .map(|piece| piece.end_ms - piece.start_ms)
            .sum();

        assert_eq!(speech_covered, 8000);
        assert_eq!(windows_covered, 30_000, "окно берёт и тишину");
    }

    /// Отсчёты куска берутся по его границам, а не «примерно там».
    ///
    /// Проверяется заведомо известным входом: вторая секунда заполнена
    /// единицами, остальное — нулями.
    #[test]
    fn samples_come_from_the_named_place() {
        let mut pcm = vec![0i16; 16_000 * 3];
        pcm[16_000..32_000].fill(1);

        let piece = Piece {
            start_ms: 1000,
            end_ms: 2000,
            speaker: None,
        };
        let taken = samples(&pcm, 16_000, &piece);
        assert_eq!(taken.len(), 16_000);
        assert!(taken.iter().all(|sample| *sample == 1), "взят не тот кусок");
    }

    /// Кусок за пределами записи даёт пустоту, а не панику и не чужие
    /// отсчёты.
    #[test]
    fn a_piece_past_the_end_yields_nothing() {
        let pcm = vec![0i16; 16_000];
        let piece = Piece {
            start_ms: 5000,
            end_ms: 6000,
            speaker: None,
        };
        assert!(samples(&pcm, 16_000, &piece).is_empty());
    }
}
