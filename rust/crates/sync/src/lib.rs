//! HTTP sync client для backend jobs (ADR-007).

mod client;
mod dto;
mod error;
mod job_poll;

pub use client::SyncClient;
pub use dto::{
    ArtifactDto, CreateJobRequest, JobDto, JobKind, JobStatus, ListModelsResponse, LlmModelRefDto,
};
pub use error::SyncError;
pub use job_poll::wait_for_job_artifact;
