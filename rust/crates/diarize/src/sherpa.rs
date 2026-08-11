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
    SpeakerEmbeddingExtractor, SpeakerEmbeddingExtractorConfig,
};

use crate::{
    DiarizeModels, DiarizeReport, Diarizer, VoiceEmbedder, VoiceTurn, resolve_diarize_models,
};

/// Порог кластеризации: насколько далеко голоса должны разойтись, чтобы
/// считаться разными.
///
/// **Снято замером, а не взято из документации.** Умолчание крейта — 0.5.
/// Прогон по двум записям с известным ответом (Linux, 2026-08-11); в
/// каждой клетке — исходная запись, она же с переставленными половинами,
/// она же дважды подряд. Людей во всех трёх по построению столько же:
///
/// | порог | запись на 4 человек | запись на 2 человек |
/// |---|---|---|
/// | 0.55 | 4 / **5** / 4 | 2 / 2 / 2 |
/// | 0.60 | 4 / 4 / 4 | 2 / 2 / 2 |
/// | **0.65** | **4 / 4 / 4** | **2 / 2 / 2** |
/// | 0.70 | 4 / 4 / 4 | 2 / 2 / 2 |
/// | 0.75 | 4 / 4 / 4 | 2 / 2 / 2 |
/// | 0.80 | **3** / 3 / 3 | 2 / 2 / 2 |
///
/// Устойчивое плато — 0.60…0.75, и 0.65 стоит в нём с запасом в обе
/// стороны, а не на краю. Ниже плато число едет от перекладывания, выше —
/// четверо сливаются в троих.
///
/// Это две чужие записи на одной машине, то есть исходная точка, а не
/// решение: свой порог назначает замер на наших встречах (задача 3).
const CLUSTER_THRESHOLD: f32 = 0.65;
/// Число голосов не назначается: `-1` — считать по порогу.
///
/// Спека прямо запрещает обратное. Кластеризация, которой сказали «их
/// двое», найдёт ровно двоих и в записи, где говорил один.
const NUM_CLUSTERS: i32 = -1;
/// Короче этого речь отрезком не считается, секунды.
const MIN_DURATION_ON: f32 = 0.3;
/// Пауза короче этой отрезок не разрывает, секунды.
const MIN_DURATION_OFF: f32 = 0.5;
/// Потоков на инференс.
///
/// Было жёстко два, «чтобы не мешать живому пути», и это оказалось
/// главным тормозом: проход по двадцатиминутной встрече занимал шесть с
/// половиной минут. Живому пути он не мешает по другой причине —
/// диаризация идёт **post-call**, когда запись уже кончилась, — так что
/// экономить было не на чем.
///
/// Берётся из числа ядер, потолок восемь: выше отдача падает, а память
/// под каждый поток растёт. Переопределяется `MEETINGRAFT_DIARIZE_THREADS`.
fn num_threads() -> i32 {
    if let Ok(value) = std::env::var("MEETINGRAFT_DIARIZE_THREADS")
        && let Ok(threads) = value.trim().parse::<i32>()
        && threads > 0
    {
        return threads;
    }
    std::thread::available_parallelism()
        .map(|cores| (cores.get() as i32).clamp(1, 8))
        .unwrap_or(2)
}

/// Чем считать: `cpu`, `coreml` (Apple), `cuda`.
///
/// Берётся из `MEETINGRAFT_DIARIZE_PROVIDER`, умолчание — `cpu`.
///
/// **Переменной, а не константой, и вот почему.** В macOS-сборке
/// провайдер CoreML действительно есть — `libonnxruntime.a` несёт
/// `CoreMLExecutionProvider`, а sherpa знает строку `coreml`. Но
/// «есть» не значит «быстрее»: модели здесь маленькие, а CoreML режет
/// граф на куски и часть возвращает на CPU, и на малых графах накладные
/// расходы съедают выигрыш регулярно. Числа снимаются на Маке, и до них
/// выбирать нечего.
///
/// Metal/MPS в этой сборке нет вовсе: единственный путь к ускорителю —
/// CoreML, и он сам решает, что отдать ANE, что GPU, а что CPU.
///
/// Отказ здесь **молчаливый со стороны sherpa**: не найдя провайдера, она
/// печатает «Fallback to cpu!» в stderr и считает дальше. Поэтому имя
/// провайдера прибор печатает сам — иначе замер «на CoreML» мог бы
/// оказаться замером на CPU, и отличить их было бы нечем.
fn provider() -> Option<String> {
    match std::env::var("MEETINGRAFT_DIARIZE_PROVIDER") {
        Ok(name) if !name.trim().is_empty() => Some(name.trim().to_string()),
        _ => None,
    }
}

/// Чем **просили** считать. Именно просили, а не считали.
///
/// Различие несущее: sherpa, не найдя провайдера, печатает
/// «Fallback to cpu!» в stderr и считает дальше. Узнать снаружи, что
/// именно случилось, нельзя — она не отдаёт этого никак. Поэтому строка
/// говорит «запрошен», и рядом сказано, где смотреть отказ. Написать
/// «считал coreml» значило бы утверждать то, чего мы не знаем: первая
/// версия так и делала и врала на первой же машине без CoreML.
pub fn requested_provider() -> String {
    provider().unwrap_or_else(|| "cpu".to_string())
}

/// Сколько потоков ушло на инференс — для печати рядом со временем.
pub fn threads_in_use() -> i32 {
    num_threads()
}

/// Движок sherpa-onnx поверх пары моделей.
pub struct SherpaDiarizer {
    inner: OfflineSpeakerDiarization,
    /// Копия конфигурации: `set_config` принимает её целиком, а поменять
    /// нужно одно поле. Хранить её дешевле, чем собирать заново, и
    /// главное — так исключено, что вместе с порогом молча уедет
    /// что-нибудь ещё.
    config: OfflineSpeakerDiarizationConfig,
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
                num_threads: num_threads(),
                debug: false,
                provider: provider(),
            },
            embedding: SpeakerEmbeddingExtractorConfig {
                model: Some(path_string(&models.embedding)?),
                num_threads: num_threads(),
                debug: false,
                provider: provider(),
            },
            clustering: FastClusteringConfig {
                num_clusters: NUM_CLUSTERS,
                threshold: CLUSTER_THRESHOLD,
            },
            min_duration_on: MIN_DURATION_ON,
            min_duration_off: MIN_DURATION_OFF,
        };

        OfflineSpeakerDiarization::create(&config)
            .map(|inner| Self {
                inner,
                config: config.clone(),
            })
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
    fn set_cluster_threshold(&mut self, threshold: f32) -> bool {
        self.config.clustering.threshold = threshold;
        self.inner.set_config(&self.config);
        true
    }

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

/// Считает вектор голоса по куску звука.
///
/// Отдельно от `SherpaDiarizer` намеренно: диаризация решает, **сколько**
/// в записи людей, а здесь число называет человек, и остаётся померить
/// похожесть. Это разные задачи с разной ценой ошибки, и одна ручка на
/// две была бы ручкой ни на что.
pub struct SherpaEmbedder {
    inner: SpeakerEmbeddingExtractor,
}

impl SherpaEmbedder {
    pub fn open(models: &DiarizeModels) -> Result<Self, String> {
        let config = SpeakerEmbeddingExtractorConfig {
            model: Some(path_string(&models.embedding)?),
            num_threads: num_threads(),
            debug: false,
            provider: provider(),
        };
        SpeakerEmbeddingExtractor::create(&config)
            .map(|inner| Self { inner })
            .ok_or_else(|| {
                format!(
                    "sherpa-onnx не поднял модель голосов {} (причина — в stderr выше)",
                    models.embedding.display()
                )
            })
    }
}

impl VoiceEmbedder for SherpaEmbedder {
    fn dim(&self) -> usize {
        self.inner.dim().max(0) as usize
    }

    fn embed(&mut self, pcm: &[i16], sample_rate: u32) -> Result<Vec<f32>, String> {
        if pcm.is_empty() {
            return Err("кусок пуст — считать вектор не по чему".to_string());
        }
        let stream = self
            .inner
            .create_stream()
            .ok_or_else(|| "поток для вектора не создался".to_string())?;
        let samples: Vec<f32> = pcm.iter().map(|s| f32::from(*s) / 32_768.0).collect();
        stream.accept_waveform(sample_rate as i32, &samples);
        stream.input_finished();

        // Модель требует минимума материала, и это не мелочь: слишком
        // короткая реплика вектора не даёт вовсе. Отказ здесь честнее
        // вектора, посчитанного по трети слова, — тот выглядел бы как
        // полноценный и тянул бы слепок в сторону.
        if !self.inner.is_ready(&stream) {
            return Err(format!(
                "кусок в {:.2} с слишком короток для вектора голоса",
                pcm.len() as f32 / sample_rate as f32
            ));
        }
        self.inner
            .compute(&stream)
            .ok_or_else(|| "вектор не посчитался".to_string())
    }
}

/// Движок векторов по сборке и по тому, что лежит на диске.
///
/// `Err` со строкой вместо заглушки: у слепков нет безобидного
/// «отказываюсь считать» — если векторов нет, то и подписывать нечем, и
/// вызывающий обязан это увидеть сразу.
pub fn voice_embedder(data_root: impl AsRef<std::path::Path>) -> Result<SherpaEmbedder, String> {
    let models = resolve_diarize_models(data_root.as_ref())?;
    SherpaEmbedder::open(&models)
}
