//! Аудиоканалы захвата (ADR-004).

/// Канал PCM-потока: mic = me, system = others.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AudioChannel {
    Mic,
    System,
}

impl AudioChannel {
    /// Имя каталога на диске.
    pub fn dir_name(self) -> &'static str {
        match self {
            Self::Mic => "mic",
            Self::System => "system",
        }
    }

    /// Код для хранения и границы UniFFI.
    pub fn code(self) -> &'static str {
        match self {
            Self::Mic => "mic",
            Self::System => "system",
        }
    }

    /// Разбор кода; неизвестное значение считается микрофоном — записи до
    /// появления атрибуции сделаны только с микрофона.
    pub fn from_code(code: &str) -> Self {
        match code {
            "system" => Self::System,
            _ => Self::Mic,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn code_round_trips() {
        for channel in [AudioChannel::Mic, AudioChannel::System] {
            assert_eq!(AudioChannel::from_code(channel.code()), channel);
        }
    }

    #[test]
    fn unknown_code_falls_back_to_mic() {
        assert_eq!(AudioChannel::from_code("whatever"), AudioChannel::Mic);
    }
}
