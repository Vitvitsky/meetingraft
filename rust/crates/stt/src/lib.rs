//! Live STT engines (ADR-005).

mod engine;
mod hallucination;
mod local_agreement;
mod mock;
mod model_path;
mod window;

#[cfg(feature = "whisper")]
mod whisper;

pub use engine::SttEngine;
pub use hallucination::is_whisper_hallucination;
pub use local_agreement::{
    HypothesisWord, LocalAgreement, Stabilized, backfill_end_ms, words_from_tokens,
};
pub use mock::MockSttEngine;
pub use model_path::{models_dir, resolve_whisper_model, whisper_filename_for_id};
pub use window::{LiveCaptionPipeline, SttBackendKind, pcm_bytes_to_i16};

#[cfg(feature = "whisper")]
pub use whisper::WhisperSttEngine;
