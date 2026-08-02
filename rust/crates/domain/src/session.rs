//! Состояние meeting session.

/// Явные состояния сессии (упрощённый автомат Phase 2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SessionState {
    Idle,
    Live,
    Ended,
}
