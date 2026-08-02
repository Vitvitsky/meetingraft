//! DTO зеркало `shared/openapi.yaml`.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobKind {
    Refine,
    Translate,
    Brief,
    FollowUp,
}

impl JobKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Refine => "refine",
            Self::Translate => "translate",
            Self::Brief => "brief",
            Self::FollowUp => "follow_up",
        }
    }

    pub fn from_code(code: &str) -> Option<Self> {
        match code {
            "refine" => Some(Self::Refine),
            "translate" => Some(Self::Translate),
            "brief" => Some(Self::Brief),
            "follow_up" => Some(Self::FollowUp),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobStatus {
    Queued,
    Running,
    Succeeded,
    Failed,
}

impl JobStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Running => "running",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CreateJobRequest {
    pub meeting_id: String,
    pub kind: JobKind,
    pub primary_language: String,
    pub allowed_languages: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub payload: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct JobDto {
    pub id: String,
    pub meeting_id: String,
    pub kind: JobKind,
    pub status: JobStatus,
    #[serde(default)]
    pub error: Option<String>,
    pub artifact_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ArtifactDto {
    pub id: String,
    pub kind: JobKind,
    pub body_markdown: String,
    pub created_at: String,
}
