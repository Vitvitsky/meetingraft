//! Live STT engines (ADR-005).

mod batch;
mod engine;
mod hallucination;
mod local_agreement;
mod mock;
mod model_path;
mod window;

#[cfg(feature = "whisper")]
mod whisper;
#[cfg(feature = "whisper")]
mod whisper_batch;

pub use batch::{BatchTranscribeError, BatchTranscriber, MockBatchTranscriber, normalize_segments};
pub use engine::SttEngine;
pub use hallucination::{is_hallucination_prefix, is_whisper_hallucination};
pub use local_agreement::{
    HypothesisWord, LocalAgreement, Stabilized, backfill_end_ms, words_from_tokens,
};
pub use mock::MockSttEngine;
pub use model_path::{models_dir, resolve_whisper_model, whisper_filename_for_id};
pub use window::{LiveCaptionPipeline, SttBackendKind, pcm_bytes_to_i16};

#[cfg(feature = "whisper")]
pub use whisper::WhisperSttEngine;
#[cfg(feature = "whisper")]
pub use whisper_batch::WhisperBatchTranscriber;
