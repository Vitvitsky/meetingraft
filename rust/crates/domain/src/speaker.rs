//! Спикеры встречи (ручные метки; diarization — позже).

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Speaker {
    pub id: String,
    pub meeting_id: String,
    pub display_name: String,
    pub sort_index: i64,
}
