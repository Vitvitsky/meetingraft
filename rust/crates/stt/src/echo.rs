//! Обнаружение эха динамиков в микрофонном канале (Epic 16).
//!
//! Микрофон ловит динамики, поэтому в дорожке `mic` звучит и собеседник.
//! Пакетный проход распознаёт дорожки по отдельности (ADR-011), и канал
//! сегмента считается говорящим (ADR-012) — значит чужие слова получают
//! подпись владельца машины. Те же слова уже есть в дорожке `system` со
//! своей верной подписью, так что видимый симптом — задвоение реплики,
//! один раз не от того человека.
//!
//! Сигнал здесь **не меняется**. Задача не в том, чтобы очистить звук, а
//! в том, чтобы не подписать чужие слова чужим именем; для неё достаточно
//! понять, какие куски дорожки `mic` — эхо.
//!
//! Отличает эхо одно измеримое свойство: оно коррелирует с системным
//! каналом на постоянном сдвиге. Голос владельца в системный канал не
//! попадает и не коррелирует с ним никак.

/// Окно анализа. Короче — шумит оценка, длиннее — реплика целиком не
/// поместится и решение размажется по соседям.
const WINDOW_MS: u64 = 300;
/// Наибольшая правдоподобная задержка «динамик → воздух → микрофон»
/// вместе с буферами вывода.
const MAX_DELAY_MS: u64 = 250;
/// Длина куска, на котором ищется задержка. Полный перебор по всей
/// записи стоил бы часы, а задержка задана акустикой и постоянна.
const DELAY_PROBE_MS: u64 = 4_000;
/// Сколько громких мест пробовать при поиске задержки.
const DELAY_PROBES: usize = 3;
/// Ниже этой корреляции оценка сдвига считается ненайденной.
///
/// Помечать окна по случайному сдвигу опаснее, чем не помечать вовсе:
/// цена ложной пометки — выброшенная реплика человека.
const MIN_DELAY_CORRELATION: f32 = 0.3;
/// Какую долю энергии микрофона системный канал должен объяснять, чтобы
/// окно считалось эхом.
///
/// Это `corr²` — доля дисперсии, объяснённая линейной моделью. Отсюда же
/// берётся защита от собственной речи поверх эха: она с системным
/// каналом не коррелирует, добавляет энергии и долю сбивает вниз.
const ECHO_EXPLAINED: f32 = 0.5;
/// Длина оценки отклика комнаты, отсчётов (~64 мс на 16 кГц).
///
/// Одиночного сдвига не хватает, и это замерено: отражения размазывают
/// энергию, корреляция на одном лаге ловит её часть, и настоящее эхо
/// давало `explained` 0.24–0.28 при пороге 0.6 — то есть не ловилось
/// вовсе. Фильтр покрывает ранние отражения, на которых держится
/// разборчивость.
const RESPONSE_TAPS: usize = 1_024;
/// Шаг подстройки фильтра. Больше — быстрее сходится и сильнее шумит.
const NLMS_STEP: f32 = 0.5;
/// Ниже этого уровня окно считается тишиной и эхом не помечается.
///
/// Иначе детектор метил бы паузы — а по ним потом выбрасывались бы
/// соседние реплики.
const MIN_WINDOW_RMS: f32 = 120.0;

/// Окно микрофонной дорожки с решением по нему.
#[derive(Debug, Clone, PartialEq)]
pub struct EchoWindow {
    pub start_ms: u64,
    pub end_ms: u64,
    /// Доля энергии микрофона, объяснённая системным каналом (`corr²`).
    pub explained: f32,
    pub mic_rms: f32,
    pub is_echo: bool,
}

/// Что детектор увидел на паре дорожек.
#[derive(Debug, Clone, PartialEq)]
pub struct EchoReport {
    /// Найденная задержка микрофона относительно системного канала.
    pub delay_ms: u64,
    /// Корреляция на этой задержке; ниже порога — сдвиг не найден.
    pub delay_correlation: f32,
    pub windows: Vec<EchoWindow>,
}

impl EchoReport {
    /// Пусто: сравнивать не с чем.
    fn empty() -> Self {
        Self {
            delay_ms: 0,
            delay_correlation: 0.0,
            windows: Vec::new(),
        }
    }

    pub fn echo_windows(&self) -> usize {
        self.windows.iter().filter(|window| window.is_echo).count()
    }
}

/// Найти в микрофонной дорожке куски, объяснённые системным каналом.
///
/// Дорожки сравниваются по пересечению: каналы стартуют не одновременно,
/// и хвост микрофона, которому нечего сопоставить, эхом не помечается.
pub fn detect_echo(mic: &[i16], system: &[i16], sample_rate: u32) -> EchoReport {
    if sample_rate == 0 || mic.is_empty() || system.is_empty() {
        return EchoReport::empty();
    }

    let max_lag = frames_for(MAX_DELAY_MS, sample_rate);
    let Some((delay, correlation)) = estimate_delay(mic, system, max_lag, sample_rate) else {
        return EchoReport::empty();
    };

    let window = frames_for(WINDOW_MS, sample_rate).max(1);
    let residual = residual_energy(mic, system, delay, window);

    let mut windows = Vec::with_capacity(residual.len());
    for (index, (mic_energy, error_energy)) in residual.iter().enumerate() {
        let start = index * window;
        let mic_level = (mic_energy / window as f64).sqrt() as f32;
        // Доля энергии микрофона, объяснённая откликом комнаты на
        // системный канал. Своя речь с ним не коррелирует, добавляет
        // энергии в остаток и долю сбивает вниз — отсюда же защита от
        // одновременной речи.
        let explained = if *mic_energy > 0.0 {
            (1.0 - error_energy / mic_energy).clamp(0.0, 1.0) as f32
        } else {
            0.0
        };
        windows.push(EchoWindow {
            start_ms: ms_for(start, sample_rate),
            end_ms: ms_for(start + window, sample_rate),
            explained,
            mic_rms: mic_level,
            is_echo: mic_level >= MIN_WINDOW_RMS && explained >= ECHO_EXPLAINED,
        });
    }

    EchoReport {
        delay_ms: ms_for(delay, sample_rate),
        delay_correlation: correlation,
        windows,
    }
}

/// Энергия микрофона и энергия остатка по окнам.
///
/// Отклик комнаты подстраивается на ходу (NLMS): он неизвестен заранее и
/// зависит от расстановки динамиков. **Сигнал при этом не меняется** —
/// остаток нужен только чтобы понять, чем окно объясняется; в запись
/// уходит ровно то, что пришло с микрофона.
fn residual_energy(mic: &[i16], system: &[i16], delay: usize, window: usize) -> Vec<(f64, f64)> {
    let mut filter = vec![0f32; RESPONSE_TAPS];
    let mut out = Vec::new();
    let mut mic_energy = 0.0f64;
    let mut error_energy = 0.0f64;
    let mut counted = 0usize;

    for (index, sample) in mic.iter().enumerate() {
        // Опорный кусок системного канала: эхо приходит позже источника.
        //
        // Верхняя граница включает сам `index - delay`: отражение с нулевым
        // смещением — самое сильное, и сдвиг окна на отсчёт назад выкидывал
        // бы именно его.
        let reference_end = index.checked_sub(delay).map(|value| value + 1);
        let target = f32::from(*sample);

        let usable = reference_end.filter(|end| *end <= system.len() && *end >= RESPONSE_TAPS);
        let (estimate, power) = if let Some(end) = usable {
            let slice = &system[end - RESPONSE_TAPS..end];
            let mut sum = 0.0f32;
            let mut power = 0.0f32;
            for (tap, sample) in filter.iter().zip(slice.iter().rev()) {
                let value = f32::from(*sample);
                sum += tap * value;
                power += value * value;
            }
            (sum, power)
        } else {
            (0.0, 0.0)
        };

        let error = target - estimate;
        if let (Some(end), true) = (usable, power > 0.0) {
            let scale = NLMS_STEP * error / (power + 1.0);
            let slice = &system[end - RESPONSE_TAPS..end];
            for (tap, sample) in filter.iter_mut().zip(slice.iter().rev()) {
                *tap += scale * f32::from(*sample);
            }
        }

        mic_energy += f64::from(target) * f64::from(target);
        error_energy += f64::from(error) * f64::from(error);
        counted += 1;
        if counted == window {
            out.push((mic_energy, error_energy));
            mic_energy = 0.0;
            error_energy = 0.0;
            counted = 0;
        }
    }
    out
}

/// Сдвиг, на котором дорожки похожи сильнее всего.
///
/// Ищется не по всей записи: перебор сдвигов квадратичен, а задержка
/// задана акустикой комнаты и в пределах записи постоянна. Кусок берётся
/// там, где системный канал громче всего, — на тишине искать нечего.
fn estimate_delay(
    mic: &[i16],
    system: &[i16],
    max_lag: usize,
    sample_rate: u32,
) -> Option<(usize, f32)> {
    let probe = frames_for(DELAY_PROBE_MS, sample_rate).max(1);
    let mut best: Option<(usize, f32)> = None;

    for probe_start in loudest_starts(system, probe, DELAY_PROBES) {
        // Окно микрофона начинается позже системного на искомую задержку,
        // поэтому запас берётся с обеих сторон.
        let mic_to = (probe_start + probe + max_lag).min(mic.len());
        if mic_to <= probe_start + max_lag {
            continue;
        }
        for lag in 0..=max_lag {
            let mic_start = probe_start + lag;
            let length = (mic_to - mic_start).min(probe);
            if length == 0 || probe_start + length > system.len() {
                continue;
            }
            let value = correlation_of(
                &mic[mic_start..mic_start + length],
                &system[probe_start..probe_start + length],
            );
            if best.is_none_or(|(_, previous)| value > previous) {
                best = Some((lag, value));
            }
        }
    }

    best.filter(|(_, value)| *value >= MIN_DELAY_CORRELATION)
}

/// Начала нескольких самых громких кусков длиной `window`, без перекрытия.
///
/// Одного зонда мало: самое громкое место системного канала может
/// прийтись на момент, когда владелец говорит сам, — эха в микрофоне там
/// нет, и задержка не найдётся при том, что она есть. Это не искусственный
/// случай, а обычный: люди перебивают друг друга.
fn loudest_starts(samples: &[i16], window: usize, count: usize) -> Vec<usize> {
    if samples.len() < window || window == 0 || count == 0 {
        return Vec::new();
    }
    let step = (window / 4).max(1);
    let mut candidates = Vec::new();
    let mut start = 0;
    while start + window <= samples.len() {
        candidates.push((rms(&samples[start..start + window]), start));
        start += step;
    }
    candidates.sort_by(|a, b| b.0.total_cmp(&a.0));

    let mut chosen: Vec<usize> = Vec::new();
    for (_, position) in candidates {
        if chosen
            .iter()
            .all(|taken| position.abs_diff(*taken) >= window)
        {
            chosen.push(position);
            if chosen.len() == count {
                break;
            }
        }
    }
    chosen
}

/// Нормированная взаимная корреляция; 0 на тишине.
fn correlation_of(left: &[i16], right: &[i16]) -> f32 {
    let length = left.len().min(right.len());
    if length == 0 {
        return 0.0;
    }
    let mut product = 0.0f64;
    let mut left_energy = 0.0f64;
    let mut right_energy = 0.0f64;
    for index in 0..length {
        let a = f64::from(left[index]);
        let b = f64::from(right[index]);
        product += a * b;
        left_energy += a * a;
        right_energy += b * b;
    }
    let denominator = (left_energy * right_energy).sqrt();
    if denominator <= f64::EPSILON {
        return 0.0;
    }
    (product / denominator) as f32
}

fn rms(samples: &[i16]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    let sum: f64 = samples.iter().map(|s| f64::from(*s) * f64::from(*s)).sum();
    (sum / samples.len() as f64).sqrt() as f32
}

fn frames_for(milliseconds: u64, sample_rate: u32) -> usize {
    (milliseconds * u64::from(sample_rate) / 1_000) as usize
}

fn ms_for(frames: usize, sample_rate: u32) -> u64 {
    frames as u64 * 1_000 / u64::from(sample_rate)
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: u32 = 16_000;

    /// Речеподобный сигнал: шум под огибающей слогов.
    ///
    /// Шум, а не синус: два независимых синуса на близких частотах
    /// коррелируют, и тест «чужая речь не помечена» прошёл бы по
    /// случайности. У двух независимых шумов корреляция около нуля.
    fn speechlike(frames: usize, seed: u32) -> Vec<i16> {
        let mut noise = seed | 1;
        (0..frames)
            .map(|n| {
                noise = noise.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                let sample = ((noise >> 16) as i32 - 32_768) as f32 / 32_768.0;
                let envelope = {
                    let t = n as f32 / RATE as f32;
                    0.4 + 0.6 * (2.0 * std::f32::consts::PI * 2.5 * t).sin().abs()
                };
                (sample * envelope * 6_000.0) as i16
            })
            .collect()
    }

    /// Эхо так, как его слышит микрофон: несколько отражений, сглаживание
    /// и собственный шум.
    ///
    /// Идеальная задержанная копия давала бы корреляцию ровно 1.0, и порог
    /// перестал бы что-либо значить — тест проходил бы при любом его
    /// значении вплоть до единицы. Настоящее эхо профильтровано комнатой и
    /// таким чистым не бывает.
    fn echo_of(source: &[i16], delay: usize, attenuation: f32) -> Vec<i16> {
        let taps = [(0usize, 1.0f32), (137, 0.45), (289, 0.2), (523, 0.1)];
        let mut wide = vec![0f32; source.len()];
        for (offset, gain) in taps {
            let shift = delay + offset;
            for index in shift..source.len() {
                wide[index] += f32::from(source[index - shift]) * gain * attenuation;
            }
        }
        // Сглаживание: воздух и корпус срезают верх.
        let mut noise = 0x9E37_79B9u32;
        (0..wide.len())
            .map(|index| {
                let smoothed = if index >= 2 {
                    (wide[index] + wide[index - 1] + wide[index - 2]) / 3.0
                } else {
                    wide[index]
                };
                noise = noise.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                let hiss = ((noise >> 16) as i32 - 32_768) as f32 / 32_768.0 * 200.0;
                (smoothed + hiss).clamp(-32_768.0, 32_767.0) as i16
            })
            .collect()
    }

    /// Задержка находится с точностью до окна оценки.
    #[test]
    fn the_delay_of_a_synthetic_echo_is_found() {
        let system = speechlike(RATE as usize * 6, 1);
        let delay = 1_200; // 75 мс
        let mic = echo_of(&system, delay, 0.4);

        let report = detect_echo(&mic, &system, RATE);

        assert_eq!(report.delay_ms, 75, "сдвиг найден неверно");
        // Не единица: комната размазывает отражения, и корреляция на
        // одном сдвиге ловит лишь часть энергии. Важно, что она уверенно
        // выше порога принятия сдвига.
        assert!(
            report.delay_correlation > MIN_DELAY_CORRELATION * 1.5,
            "корреляция {} слишком мала, сдвиг принят случайно",
            report.delay_correlation
        );
    }

    /// Основной случай: в микрофоне только эхо.
    #[test]
    fn windows_of_pure_echo_are_marked() {
        let system = speechlike(RATE as usize * 6, 1);
        let mic = echo_of(&system, 1_200, 0.4);

        let report = detect_echo(&mic, &system, RATE);

        assert!(
            report.echo_windows() * 4 >= report.windows.len() * 3,
            "помечено {} окон из {} — эхо не распознано",
            report.echo_windows(),
            report.windows.len()
        );
    }

    /// Своя речь помечаться не должна — **при найденной задержке**.
    ///
    /// Главный тест работы: ложная пометка кончится выброшенной репликой
    /// человека, и человек об этом не узнает.
    ///
    /// Первая половина записи — чистое эхо, чтобы задержка нашлась; иначе
    /// детектор вышел бы раньше окон, и тест проверял бы поиск сдвига, а
    /// не решение по окну. На этом он один раз уже и проходил зря.
    #[test]
    fn independent_speech_is_never_marked() {
        let seconds = RATE as usize;
        let system = speechlike(seconds * 8, 1);
        let mut mic = echo_of(&system, 1_200, 0.4);
        // Вторую половину владелец говорит сам, а собеседник молчит.
        let own = speechlike(seconds * 4, 7);
        mic[seconds * 4..].copy_from_slice(&own);

        let report = detect_echo(&mic, &system, RATE);

        assert!(report.delay_correlation > 0.0, "задержка не найдена вовсе");
        assert!(!report.windows.is_empty(), "окон нет — проверять нечего");
        let marked: Vec<_> = report
            .windows
            .iter()
            .filter(|w| w.is_echo && w.start_ms >= 4_000)
            .map(|w| (w.start_ms, w.explained))
            .collect();
        assert!(marked.is_empty(), "своя речь помечена эхом: {marked:?}");
    }

    /// Речь владельца поверх эха обязана пережить детектор.
    ///
    /// Первая половина — чистое эхо, чтобы задержка нашлась. Без неё
    /// корреляция под собственной речью падает ниже порога принятия
    /// сдвига, детектор выходит раньше окон, и тест «ноль помеченных
    /// окон» проходит из пустоты. Один раз он так и прошёл.
    #[test]
    fn own_speech_over_echo_is_not_marked() {
        let seconds = RATE as usize;
        let system = speechlike(seconds * 8, 1);
        let echo = echo_of(&system, 1_200, 0.4);
        let own = speechlike(seconds * 4, 7);
        let mut mic = echo.clone();
        for (index, sample) in own.iter().enumerate() {
            let position = seconds * 4 + index;
            mic[position] = echo[position].saturating_add(*sample);
        }

        let report = detect_echo(&mic, &system, RATE);

        assert!(!report.windows.is_empty(), "окон нет — проверять нечего");
        let marked: Vec<_> = report
            .windows
            .iter()
            .filter(|w| w.is_echo && w.start_ms >= 4_000)
            .map(|w| (w.start_ms, w.explained))
            .collect();
        assert!(
            marked.is_empty(),
            "одновременная речь принята за эхо и была бы выброшена: {marked:?}"
        );
    }

    /// Записи без собеседника детектор не касается.
    #[test]
    fn an_empty_system_track_yields_nothing() {
        let mic = speechlike(RATE as usize * 3, 1);

        let report = detect_echo(&mic, &[], RATE);

        assert!(report.windows.is_empty());
        assert_eq!(report.delay_ms, 0);
    }

    /// Тишина эхом не считается: иначе метились бы паузы.
    #[test]
    fn silence_is_not_echo() {
        let silence = vec![0i16; RATE as usize * 6];

        let report = detect_echo(&silence, &silence, RATE);

        assert_eq!(report.echo_windows(), 0);
    }

    /// Дорожки разной длины сравниваются по пересечению.
    #[test]
    fn tracks_of_different_lengths_compare_on_the_overlap() {
        let system = speechlike(RATE as usize * 6, 1);
        let mut mic = echo_of(&system, 1_200, 0.4);
        // Микрофон продолжает писать после конца системной дорожки.
        mic.extend(speechlike(RATE as usize * 2, 7));

        let report = detect_echo(&mic, &system, RATE);

        assert!(report.echo_windows() > 0, "эхо в пересечении не найдено");
        let tail_marked = report
            .windows
            .iter()
            .filter(|w| w.start_ms >= 6_000 && w.is_echo)
            .count();
        assert_eq!(tail_marked, 0, "хвост без пары помечен эхом");
    }
}
