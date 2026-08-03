//! Локальное хранилище MeetingRaft (ADR-006).

mod audio_manifest;
mod migrations;

pub use audio_manifest::{AudioManifestError, AudioManifestStore, ManifestChunk};
pub use migrations::schema_version;
