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
}
