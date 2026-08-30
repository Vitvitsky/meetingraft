//! Чтение WAV — общее с приборами, запись — своя.
//!
//! Разбор берётся у соседнего прибора **тем же файлом**, как это уже
//! сделано в `stt-probe`: формат нужен ровно один — 16-битный моно PCM
//! живого пути (ADR-005), — и вторая его реализация означала бы две
//! правды об одном, с гарантией разойтись.
//!
//! Запись нужна только здесь: её делает `export`, выкладывая встречу из
//! данных приложения в случай стенда. В приборах её нет, и тянуть ради
//! неё общий файл незачем.

#[path = "../../diarize-probe/src/wav.rs"]
mod reader;

pub use reader::read;

use std::path::Path;

/// Записать 16-битный моно PCM.
///
/// Заголовок собирается руками по той же причине, по которой руками
/// написан разбор: формат один, а крейт под него тянул бы за собой
/// поддержку всего остального — включая те форматы, которые прибор
/// обязан **отвергать**.
pub fn write(path: &Path, pcm: &[i16], sample_rate: u32) -> Result<(), String> {
    let data_bytes = (pcm.len() * 2) as u32;
    let mut out = Vec::with_capacity(44 + pcm.len() * 2);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_bytes).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes()); // размер куска fmt
    out.extend_from_slice(&1u16.to_le_bytes()); // PCM без сжатия
    out.extend_from_slice(&1u16.to_le_bytes()); // моно
    out.extend_from_slice(&sample_rate.to_le_bytes());
    out.extend_from_slice(&(sample_rate * 2).to_le_bytes()); // байт в секунду
    out.extend_from_slice(&2u16.to_le_bytes()); // выравнивание блока
    out.extend_from_slice(&16u16.to_le_bytes()); // бит на отсчёт
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_bytes.to_le_bytes());
    for sample in pcm {
        out.extend_from_slice(&sample.to_le_bytes());
    }
    std::fs::write(path, out).map_err(|error| format!("{}: {error}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Записанное читается обратно **тем самым** разбором, которым живут
    /// приборы.
    ///
    /// Своя запись, разошедшаяся со своим же чтением, — самый тихий
    /// способ подать движку чужой звук: файл открывается, числа
    /// печатаются, и неправда видна только на слух.
    #[test]
    fn what_we_write_our_own_reader_reads_back() {
        let path = std::env::temp_dir().join(format!("bench-wav-{}.wav", std::process::id()));
        let pcm: Vec<i16> = (0..1000).map(|index| (index * 7) as i16).collect();
        write(&path, &pcm, 16_000).expect("записать");
        let back = read(&path).expect("прочитать");
        assert_eq!(back.sample_rate, 16_000);
        assert_eq!(back.pcm, pcm);
        let _ = std::fs::remove_file(&path);
    }

    /// Запись без отсчётов — **отказ на чтении**, и это правило проекта,
    /// а не дефект.
    ///
    /// Тест писался с обратным ожиданием («пустой файл законен») и упал:
    /// общий разбор отвергает такое по имени. Он прав. Пустой звук,
    /// принятый молча, доехал бы до движка и вернулся пустой
    /// расшифровкой — то есть сломанный экспорт выглядел бы как встреча,
    /// на которой никто не говорил.
    #[test]
    fn a_recording_without_samples_is_refused_on_reading() {
        let path = std::env::temp_dir().join(format!("bench-wav-empty-{}.wav", std::process::id()));
        write(&path, &[], 16_000).expect("записать");
        let error = read(&path).expect_err("пустая запись обязана быть отвергнута");
        assert!(
            error.contains("data"),
            "в причине должен быть кусок: {error}"
        );
        let _ = std::fs::remove_file(&path);
    }
}
