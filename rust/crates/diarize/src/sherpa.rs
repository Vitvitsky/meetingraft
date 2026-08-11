//! Разделение голосов через sherpa-onnx (кандидат из таблицы спеки).
//!
//! Связка та же, что описана в спеке: сегментация pyannote решает, **кто
//! когда говорит**, эмбеддинг превращает каждый отрезок в вектор, быстрая
//! кластеризация сводит похожие вместе. Имён здесь нет — только метки.
//!
//! **Чего в замысле не было.** Крейт `sherpa-onnx` не собирает C++ из
//! исходников: его `build.rs` качает готовые статические библиотеки с
//! GitHub Releases и линкует их. Значит `cmake` не нужен, а сборка
//! **ходит в сеть** — и то и другое меняет цену, посчитанную в спеке.
//! Отсюда правило: фича выключена по умолчанию, и обычная сборка,
//! включая CI и Linux, не качает ничего.
//!
//! Числа этого движка — задача 3 плана. Здесь только его подключение.

use sherpa_onnx::{
    FastClusteringConfig, OfflineSpeakerDiarization, OfflineSpeakerDiarizationConfig,
    OfflineSpeakerSegmentationModelConfig, OfflineSpeakerSegmentationPyannoteModelConfig,
    SpeakerEmbeddingExtractorConfig,
};

use crate::{DiarizeModels, DiarizeReport, Diarizer, VoiceTurn};

/// Порог кластеризации: насколько далеко голоса должны разойтись, чтобы
/// считаться разными.
///
/// **Снято замером, а не взято из документации.** Умолчание крейта — 0.5,
/// и на контрольных записях оно разрывает четверых на пятерых. Прогон по
/// двум записям с известным ответом (Linux, 2026-08-11):
///
/// | порог | запись на 2 чел | она же дважды | запись на 4 чел | она же дважды |
/// |---|---|---|---|---|
/// | 0.45 | 2 | 2 | 6 | 7 |
/// | 0.50 | 2 | 2 | 5 | 7 |
/// | 0.55 | 2 | 2 | 5 | 6 |
/// | **0.60** | **2** | 2 | **4** | 6 |
/// | 0.65 | 2 | 2 | 3 | 5 |
/// | 0.70 | 2 | 2 | 3 | 3 |
/// | 0.75 | 2 | 2 | 2 | 2 |
/// | 0.80 | 3 | 2 | 2 | 2 |
///
/// 0.60 — единственное значение, где обе записи сходятся точно. Это одна
/// машина и две чужие записи, то есть **исходная точка, а не решение**:
/// свой порог назначает замер на наших встречах (задача 3).
///
/// Столбцы «дважды» — та же запись, склеенная сама с собой. Людей в ней по
/// построению столько же, а голосов движок находит больше почти на всех
/// порогах. Первое прочтение было «число растёт от количества материала»
/// — и оно **неверно**, проверено отдельным замером на пороге 0.60:
///
/// | как переложена запись на 4 человек | длина | голосов |
/// |---|---|---|
/// | исходная | 57 с | 4 |
/// | половины переставлены местами | 57 с | 5 |
/// | дважды подряд | 114 с | 6 |
/// | дважды через секунду тишины | 115 с | 6 |
/// | **трижды подряд** | 171 с | **4** |
/// | четырежды подряд | 228 с | 7 |
///
/// Роста нет: утроение даёт верное число, а удвоение — завышенное. Нет и
/// случайности — тот же вход трижды подряд даёт 4, 4, 4. И это не свойство
/// движка вообще: запись с двумя далёкими голосами даёт 2 при любом
/// расположении.
///
/// Остаётся **неустойчивость на трудном материале**: кластеризация, которой
/// не сказали, сколько людей, решает по порогу, а на близких голосах
/// решение переворачивается от перекладывания. Для нас это важно вдвойне:
/// встречи по-русски, а модель эмбеддинга обучена на английском VoxCeleb,
/// то есть наш случай ближе к трудному. Многоязычная модель эмбеддинга —
/// первый рычаг для задачи 3.
const CLUSTER_THRESHOLD: f32 = 0.60;
/// Число голосов не назначается: `-1` — считать по порогу.
///
/// Спека прямо запрещает обратное. Кластеризация, которой сказали «их
/// двое», найдёт ровно двоих и в записи, где говорил один.
const NUM_CLUSTERS: i32 = -1;
/// Короче этого речь отрезком не считается, секунды.
const MIN_DURATION_ON: f32 = 0.3;
/// Пауза короче этой отрезок не разрывает, секунды.
const MIN_DURATION_OFF: f32 = 0.5;
/// Потоков на инференс. Проход идёт post-call, живому пути не мешает.
const NUM_THREADS: i32 = 2;

/// Движок sherpa-onnx поверх пары моделей.
pub struct SherpaDiarizer {
    inner: OfflineSpeakerDiarization,
}

impl SherpaDiarizer {
    /// Поднять движок на паре моделей.
    ///
    /// Ошибка — строкой: `create` отдаёт `None` и печатает причину в
    /// stderr сам, так что единственное, что мы можем добавить, — назвать
    /// файлы, на которых не поднялось.
    pub fn open(models: &DiarizeModels) -> Result<Self, String> {
        let config = OfflineSpeakerDiarizationConfig {
            segmentation: OfflineSpeakerSegmentationModelConfig {
                pyannote: OfflineSpeakerSegmentationPyannoteModelConfig {
                    model: Some(path_string(&models.segmentation)?),
                },
                num_threads: NUM_THREADS,
                debug: false,
                provider: None,
            },
            embedding: SpeakerEmbeddingExtractorConfig {
                model: Some(path_string(&models.embedding)?),
                num_threads: NUM_THREADS,
                debug: false,
                provider: None,
            },
            clustering: FastClusteringConfig {
                num_clusters: NUM_CLUSTERS,
                threshold: CLUSTER_THRESHOLD,
            },
            min_duration_on: MIN_DURATION_ON,
            min_duration_off: MIN_DURATION_OFF,
        };

        OfflineSpeakerDiarization::create(&config)
            .map(|inner| Self { inner })
            .ok_or_else(|| {
                format!(
                    "sherpa-onnx не поднялся на моделях {} и {} (причина — в stderr выше)",
                    models.segmentation.display(),
                    models.embedding.display()
                )
            })
    }
}

impl Diarizer for SherpaDiarizer {
    fn diarize(&mut self, pcm: &[i16], sample_rate: u32) -> DiarizeReport {
        // Частоту спрашиваем у самого движка, а не берём из головы.
        // Модели обучены на своей частоте, и подача 48 кГц под видом 16
        // не отказала бы — она просто разделяла бы неверно, а отрезки на
        // выходе выглядели бы точно так же.
        let expected = self.inner.sample_rate();
        if expected <= 0 {
            return DiarizeReport::refused("движок не назвал свою частоту дискретизации");
        }
        if expected as u32 != sample_rate {
            return DiarizeReport::refused(format!(
                "частота дорожки {sample_rate} Гц, а модели ждут {expected} Гц — \
                 пересчёт здесь не делается, иначе разделение считалось бы по чужому звуку"
            ));
        }
        if pcm.is_empty() {
            return DiarizeReport::refused("дорожка пуста — делить нечего");
        }

        let samples: Vec<f32> = pcm.iter().map(|s| f32::from(*s) / 32_768.0).collect();
        let Some(result) = self.inner.process(&samples) else {
            return DiarizeReport::refused("проход по дорожке не состоялся");
        };

        let turns = result
            .sort_by_start_time()
            .into_iter()
            .map(|segment| {
                VoiceTurn::new(
                    seconds_to_ms(segment.start),
                    seconds_to_ms(segment.end),
                    segment.speaker.max(0) as u32,
                )
            })
            .collect();
        DiarizeReport::from_turns(turns)
    }
}

/// Путь в строку. Не-UTF-8 путь — отказ, а не молчаливая порча: C API
/// принимает только `CString`, и обрезанный путь дал бы «модель не
/// найдена» вместо «имя каталога не в той кодировке».
fn path_string(path: &std::path::Path) -> Result<String, String> {
    path.to_str()
        .map(str::to_owned)
        .ok_or_else(|| format!("путь не в UTF-8: {}", path.display()))
}

/// Секунды в миллисекунды. Отрицательное время движок отдавать не должен,
/// но если отдаст — ноль, а не переполнение при приведении к `u64`.
fn seconds_to_ms(seconds: f32) -> u64 {
    if seconds <= 0.0 {
        return 0;
    }
    (f64::from(seconds) * 1_000.0).round() as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn negative_time_becomes_zero() {
        assert_eq!(seconds_to_ms(-1.0), 0);
        assert_eq!(seconds_to_ms(0.0), 0);
    }

    #[test]
    fn seconds_round_to_the_nearest_millisecond() {
        assert_eq!(seconds_to_ms(1.2345), 1_235);
        assert_eq!(seconds_to_ms(12.0), 12_000);
    }

    /// Несуществующие модели — отказ с именами файлов, а не паника.
    #[test]
    fn missing_models_refuse_by_name() {
        let models = DiarizeModels {
            segmentation: std::path::PathBuf::from("/нет/такого/segmentation.onnx"),
            embedding: std::path::PathBuf::from("/нет/такого/embedding.onnx"),
        };

        let error = match SherpaDiarizer::open(&models) {
            Err(error) => error,
            Ok(_) => panic!("движок поднялся на несуществующих моделях"),
        };

        assert!(error.contains("segmentation.onnx"), "{error}");
    }
}
