//! Post-call артефакты (отдельно от live caption — ADR-002).

/// Вид post-call артефакта.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ArtifactKind {
    Brief,
    FollowUp,
}

/// Финальный транскрипт встречи (refined, post-call).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FinalTranscript {
    pub meeting_id: String,
    pub version: u32,
    pub body_markdown: String,
    pub created_at_ms: u64,
}

/// Post-call артефакт (brief, follow-up и т.д.).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Artifact {
    pub id: String,
    pub meeting_id: String,
    pub kind: ArtifactKind,
    pub template_id: String,
    pub body_markdown: String,
    pub created_at_ms: u64,
}

/// Краткая сводка встречи для списка/истории.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MeetingSummary {
    pub id: String,
    pub started_at_ms: u64,
    pub has_final: bool,
    pub artifact_count: u64,
}

#[cfg(test)]
mod tests {
    use super::ArtifactKind;

    #[test]
    fn artifact_kind_brief_distinct_from_follow_up() {
        assert_ne!(ArtifactKind::Brief, ArtifactKind::FollowUp);
    }
}
