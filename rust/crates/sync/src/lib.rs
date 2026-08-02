//! HTTP sync client для backend jobs (ADR-007).

mod client;
mod dto;
mod error;

pub use client::SyncClient;
pub use dto::{ArtifactDto, CreateJobRequest, JobDto, JobKind, JobStatus};
pub use error::SyncError;
