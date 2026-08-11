//! Минимальный разбор WAV — только то, чем записан наш звук.
//!
//! Своими руками, а не крейтом, по двум причинам. Формат нужен ровно
//! один: 16-битный моно PCM, тот же, что пишет живой путь (ADR-005) и
//! тот, в котором лежат контрольные записи. И разбор обязан **отказывать**
//! на всём остальном: контроль в 48 кГц или в стерео, принятый молча,
//! дал бы движку чужой звук и числа, которые не значат ничего.

use std::path::Path;

/// Разобранная запись.
#[derive(Debug)]
pub struct Wav {
    pub pcm: Vec<i16>,
    pub sample_rate: u32,
}

/// Прочитать 16-битный моно PCM. Всё прочее — отказ с причиной.
pub fn read(path: &Path) -> Result<Wav, String> {
    let bytes = std::fs::read(path).map_err(|error| format!("{}: {error}", path.display()))?;
    let name = path.display();

    if bytes.len() < 12 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return Err(format!("{name}: это не WAV"));
    }

    let mut offset = 12;
    let mut format: Option<(u16, u16, u32, u16)> = None;
    while offset + 8 <= bytes.len() {
        let id = &bytes[offset..offset + 4];
        let size = u32::from_le_bytes(
            bytes[offset + 4..offset + 8]
                .try_into()
                .map_err(|_| format!("{name}: обрезан заголовок куска"))?,
        ) as usize;
        let body = offset + 8;
        let end = body
            .checked_add(size)
            .unwrap_or(bytes.len())
            .min(bytes.len());

        if id == b"fmt " {
            if end - body < 16 {
                return Err(format!("{name}: кусок fmt короче 16 байт"));
            }
            let u16_at = |at: usize| u16::from_le_bytes([bytes[at], bytes[at + 1]]);
            let u32_at = |at: usize| {
                u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]])
            };
            format = Some((
                u16_at(body),
                u16_at(body + 2),
                u32_at(body + 4),
                u16_at(body + 14),
            ));
        } else if id == b"data" {
            let (tag, channels, rate, bits) =
                format.ok_or_else(|| format!("{name}: data идёт раньше fmt"))?;
            if tag != 1 {
                return Err(format!("{name}: не PCM (формат {tag})"));
            }
            if channels != 1 {
                return Err(format!("{name}: каналов {channels}, нужен моно"));
            }
            if bits != 16 {
                return Err(format!("{name}: {bits} бит на отсчёт, нужно 16"));
            }
            let pcm = bytes[body..end]
                .chunks_exact(2)
                .map(|pair| i16::from_le_bytes([pair[0], pair[1]]))
                .collect::<Vec<i16>>();
            if pcm.is_empty() {
                return Err(format!("{name}: в data нет отсчётов"));
            }
            return Ok(Wav {
                pcm,
                sample_rate: rate,
            });
        }

        // Куски выравниваются по чётной границе — нечётный размер
        // дополняется байтом, и без этого разбор уезжает на кусок вперёд.
        offset = body + size + (size & 1);
    }
    Err(format!("{name}: в файле нет куска data"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Собрать WAV заданного формата — материал для проверок ниже.
    fn build(tag: u16, channels: u16, rate: u32, bits: u16, samples: &[i16]) -> Vec<u8> {
        let data: Vec<u8> = samples
            .iter()
            .flat_map(|sample| sample.to_le_bytes())
            .collect();
        let mut out = Vec::new();
        out.extend(b"RIFF");
        out.extend(((36 + data.len()) as u32).to_le_bytes());
        out.extend(b"WAVE");
        out.extend(b"fmt ");
        out.extend(16u32.to_le_bytes());
        out.extend(tag.to_le_bytes());
        out.extend(channels.to_le_bytes());
        out.extend(rate.to_le_bytes());
        out.extend((rate * u32::from(channels) * u32::from(bits) / 8).to_le_bytes());
        out.extend((channels * bits / 8).to_le_bytes());
        out.extend(bits.to_le_bytes());
        out.extend(b"data");
        out.extend((data.len() as u32).to_le_bytes());
        out.extend(data);
        out
    }

    fn write(name: &str, bytes: &[u8]) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!(
            "mr-wav-{name}-{:?}.wav",
            std::thread::current().id()
        ));
        std::fs::write(&path, bytes).expect("файл");
        path
    }

    #[test]
    fn reads_mono_16_bit_pcm() {
        let path = write("ok", &build(1, 1, 16_000, 16, &[1, -1, 32_000, -32_000]));

        let wav = read(&path).expect("разбор");

        assert_eq!(wav.sample_rate, 16_000);
        assert_eq!(wav.pcm, vec![1, -1, 32_000, -32_000]);
        let _ = std::fs::remove_file(path);
    }

    /// Стерео и 8 бит — отказ, а не половина отсчётов и не тишина.
    /// Принятый молча чужой формат дал бы движку другой звук.
    #[test]
    fn other_formats_are_refused_by_name() {
        let stereo = write("stereo", &build(1, 2, 16_000, 16, &[1, 2, 3, 4]));
        let eight = write("8bit", &build(1, 1, 16_000, 8, &[1, 2]));

        assert!(
            read(&stereo).expect_err("стерео").contains("моно"),
            "отказ не назвал причину"
        );
        assert!(read(&eight).expect_err("8 бит").contains("16"));
        let _ = std::fs::remove_file(stereo);
        let _ = std::fs::remove_file(eight);
    }

    /// Кусок нечётной длины дополняется байтом. Без учёта дополнения
    /// разбор уезжает и `data` не находится вовсе.
    #[test]
    fn an_odd_sized_chunk_does_not_derail_the_walk() {
        let mut bytes = build(1, 1, 16_000, 16, &[7, 8]);
        // Вставить перед data кусок LIST в 3 байта (+1 байт дополнения).
        let at = bytes
            .windows(4)
            .position(|w| w == b"data")
            .expect("кусок data");
        let mut extra = Vec::new();
        extra.extend(b"LIST");
        extra.extend(3u32.to_le_bytes());
        extra.extend([1u8, 2, 3, 0]);
        bytes.splice(at..at, extra);
        let path = write("odd", &bytes);

        let wav = read(&path).expect("разбор");

        assert_eq!(wav.pcm, vec![7, 8]);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn a_file_that_is_not_a_wav_is_refused() {
        let path = write("garbage", b"not a wave at all");
        assert!(read(&path).expect_err("не WAV").contains("не WAV"));
        let _ = std::fs::remove_file(path);
    }
}
