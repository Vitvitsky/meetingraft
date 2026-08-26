//! Live STT engines (ADR-005).

mod batch;
mod echo;
mod engine;
mod gigaam_path;
mod hallucination;
mod local_agreement;
mod mock;
mod model_path;
mod noise_gate;
mod pacing;
mod window;

#[cfg(feature = "gigaam")]
mod gigaam;

#[cfg(feature = "whisper")]
mod whisper;
#[cfg(feature = "whisper")]
mod whisper_batch;

pub use batch::{BatchTranscribeError, BatchTranscriber, MockBatchTranscriber, normalize_segments};
pub use echo::{EchoReport, EchoWindow, detect_echo};
pub use engine::SttEngine;
pub use gigaam_path::{
    DECODER_FILE, ENCODER_FILE, GigaamModels, JOINER_FILE, TOKENS_FILE, gigaam_models_dir,
    resolve_gigaam_models,
};
pub use hallucination::{is_hallucination_prefix, is_whisper_hallucination};
pub use local_agreement::{
    HypothesisWord, LocalAgreement, Stabilized, backfill_end_ms, words_from_char_tokens,
    words_from_tokens,
};
pub use mock::MockSttEngine;
pub use model_path::{models_dir, resolve_whisper_model, whisper_filename_for_id};
pub use noise_gate::NoiseGate;
pub use pacing::{InferencePacer, MIN_SPEECH_FRAMES, PARTIAL_MIN_FRAMES, SILENCE_FRAMES};
pub use window::{LiveCaptionPipeline, SttBackendKind, pcm_bytes_to_i16};

#[cfg(feature = "gigaam")]
pub use gigaam::{
    GigaamBatchTranscriber, GigaamHypothesis, GigaamRecognizer, MODEL_ID as GIGAAM_MODEL_ID,
};

#[cfg(feature = "whisper")]
pub use whisper::WhisperSttEngine;
#[cfg(feature = "whisper")]
pub use whisper_batch::WhisperBatchTranscriber;
