//! Live STT engines (ADR-005).

mod engine;
mod hallucination;
mod mock;
mod model_path;
mod window;

#[cfg(feature = "whisper")]
mod whisper;

pub use engine::SttEngine;
pub use hallucination::is_whisper_hallucination;
pub use mock::MockSttEngine;
pub use model_path::{models_dir, resolve_whisper_model, whisper_filename_for_id};
pub use window::{LiveCaptionPipeline, SttBackendKind, pcm_bytes_to_i16};

#[cfg(feature = "whisper")]
pub use whisper::WhisperSttEngine;
