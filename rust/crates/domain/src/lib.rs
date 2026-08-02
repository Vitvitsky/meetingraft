//! Доменные модели MeetingRaft. Наполняется в Phase 2 (см. docs/roadmap.md).

/// Версия доменного крейта; используется smoke-тестом сборки.
pub const CRATE_VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg(test)]
mod tests {
    use super::*;

    /// Smoke-тест сборки workspace: версия крейта совпадает с манифестом.
    #[test]
    fn crate_version_matches_manifest() {
        assert_eq!(CRATE_VERSION, "0.1.0");
    }
}
