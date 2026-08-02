//! Ошибки sync-клиента.

use thiserror::Error;

#[derive(Debug, Error)]
pub enum SyncError {
    #[error("API base URL не задан")]
    NotConfigured,
    #[error("HTTP {0}: {1}")]
    Http(u16, String),
    #[error("транспорт: {0}")]
    Transport(String),
    #[error("JSON: {0}")]
    Json(String),
}

impl From<reqwest::Error> for SyncError {
    fn from(value: reqwest::Error) -> Self {
        Self::Transport(value.to_string())
    }
}

impl From<serde_json::Error> for SyncError {
    fn from(value: serde_json::Error) -> Self {
        Self::Json(value.to_string())
    }
}
