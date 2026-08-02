//! Live STT engines (ADR-005).

mod engine;
mod mock;
mod model_path;
mod window;

#[cfg(feature = "whisper")]
mod whisper;

pub use engine::SttEngine;
pub use mock::MockSttEngine;
pub use model_path::{models_dir, resolve_whisper_model};
pub use window::{LiveCaptionPipeline, SttBackendKind};

#[cfg(feature = "whisper")]
pub use whisper::WhisperSttEngine;
