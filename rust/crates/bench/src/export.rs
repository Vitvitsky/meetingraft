//! Выкладка встречи из данных приложения в случай стенда.
//!
//! Гоняется там, где лежат встречи — на Маке, — а каталог случая
//! переезжает туда, где идёт разработка. Подкоманда стенда, а не
//! отдельный прибор: формат случая знает стенд, и вторая его реализация
//! разошлась бы с первой ровно так же, как разошлись бы две реализации
//! WER.
//!
//! **Всё, чего экспорт не знает, остаётся пустым.** Число людей, язык
//! отрезка, вид эталона проставляет человек, который слушал запись.
//! Выдуманное здесь число говорящих молча испортило бы cpWER, а
//! объявленный эталон, которого нет, — уронил бы чтение случая.

use std::path::Path;

use domain::AudioChannel;
use storage::AudioManifestStore;

use crate::case::RATE;
use crate::wav;

/// Что получилось выложить.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Exported {
    pub mic_ms: u64,
    pub system_ms: Option<u64>,
    pub channel_clock_unified: bool,
}

pub fn export(data_root: &Path, meeting_id: &str, out_dir: &Path) -> Result<Exported, String> {
    std::fs::create_dir_all(out_dir).map_err(|error| format!("{}: {error}", out_dir.display()))?;

    let store = AudioManifestStore::open(data_root).map_err(|error| error.to_string())?;

    let mic = store
        .read_session_pcm(meeting_id, AudioChannel::Mic)
        .map_err(|error| format!("микрофонный канал: {error}"))?;
    // Пустой канал — отказ. Молчаливо выложенная пустая запись доехала бы
    // до движка и вернулась пустой расшифровкой: сломанный экспорт
    // выглядел бы как встреча, на которой никто не говорил.
    if mic.is_empty() {
        return Err(format!("у встречи {meeting_id} пустой микрофонный канал"));
    }
    wav::write(&out_dir.join("mic.wav"), &mic, RATE)?;

    // Системного канала может не быть вовсе — встреча могла идти без
    // второго источника. Это законно и отказом не считается.
    let system = store
        .read_session_pcm(meeting_id, AudioChannel::System)
        .unwrap_or_default();
    let system_ms = if system.is_empty() {
        None
    } else {
        wav::write(&out_dir.join("system.wav"), &system, RATE)?;
        Some(duration_ms(system.len()))
    };

    // Признак берётся из базы, а не проставляется наугад: у записей до
    // Epic 25 он `0` навсегда, и восстановить их сдвиг нечем. Экспорт,
    // написавший здесь `true` из вежливости, соврал бы о самой записи.
    let channel_clock_unified = store
        .channel_clock_unified(meeting_id)
        .map_err(|error| format!("признак общего времени каналов: {error}"))?
        .ok_or_else(|| format!("встречи {meeting_id} нет в базе"))?;

    let mic_ms = duration_ms(mic.len());
    std::fs::write(
        out_dir.join("meta.toml"),
        draft_meta(meeting_id, channel_clock_unified),
    )
    .map_err(|error| format!("meta.toml: {error}"))?;

    Ok(Exported {
        mic_ms,
        system_ms,
        channel_clock_unified,
    })
}

fn duration_ms(samples: usize) -> u64 {
    samples as u64 * 1000 / u64::from(RATE)
}

/// Заготовка описания.
///
/// `reference_kind = "none"` и `speakers_expected = 0` — не небрежность,
/// а отказ выдумывать: и то и другое знает человек, слушавший запись, а
/// не экспорт.
fn draft_meta(meeting_id: &str, channel_clock_unified: bool) -> String {
    format!(
        r#"case = "{meeting_id}"
language = "ru"
# Ноль означает «неизвестно». Проставь руками — по этому числу считается cpWER.
speakers_expected = 0
source = "meetingraft:{meeting_id}"
channel_clock_unified = {channel_clock_unified}
# none | typed | edited-draft. Появится эталон — поменяй здесь и положи
# reference.txt рядом, иначе случай не прочитается.
reference_kind = "none"
notes = ""
"#
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Заготовка не выдумывает того, чего экспорт знать не может.
    #[test]
    fn the_generated_meta_claims_no_reference_and_no_speaker_count() {
        let meta = draft_meta("MEETING-1", true);
        assert!(meta.contains(r#"reference_kind = "none""#), "{meta}");
        assert!(meta.contains("speakers_expected = 0"), "{meta}");
        assert!(meta.contains("channel_clock_unified = true"), "{meta}");
    }

    /// Признак общего времени каналов едет из базы, а не ставится
    /// константой: у записей до Epic 25 он `false` навсегда.
    #[test]
    fn the_clock_flag_follows_the_meeting_not_a_default() {
        assert!(draft_meta("m", true).contains("channel_clock_unified = true"));
        assert!(draft_meta("m", false).contains("channel_clock_unified = false"));
    }

    /// Заготовка обязана читаться тем самым разбором, который читает
    /// случаи. Иначе экспорт молча производит каталоги, которые стенд не
    /// возьмёт, и узнается это уже на Маке.
    #[test]
    fn the_generated_meta_is_readable_by_the_case_loader() {
        let dir = std::env::temp_dir().join(format!("bench-export-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("создать каталог");
        std::fs::write(dir.join("meta.toml"), draft_meta("MEETING-1", false)).expect("meta");
        wav::write(&dir.join("mic.wav"), &vec![0i16; RATE as usize], RATE).expect("mic");

        let case = crate::case::load(&dir).expect("заготовка обязана читаться");
        assert_eq!(case.meta.case, "MEETING-1");
        assert_eq!(case.meta.reference_kind, crate::case::ReferenceKind::None);
        assert!(case.reference.is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
