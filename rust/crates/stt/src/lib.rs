//! Live STT engines (ADR-005).

mod batch;
mod echo;
mod engine;
mod engine_choice;
mod gigaam_path;
mod hallucination;
mod hypothesis;
mod local_agreement;
mod mock;
mod model_path;
mod noise_gate;
mod pacing;
mod parakeet_path;
mod speech_decider;
mod tone_path;
mod vad_path;
mod window;

#[cfg(feature = "gigaam")]
mod gigaam;

#[cfg(feature = "parakeet")]
mod parakeet;

#[cfg(feature = "tone")]
mod tone;

#[cfg(feature = "vad")]
mod vad;
#[cfg(feature = "vad")]
mod vad_segments;

#[cfg(feature = "whisper")]
mod whisper;
#[cfg(feature = "whisper")]
mod whisper_batch;

pub use batch::{BatchTranscribeError, BatchTranscriber, MockBatchTranscriber, normalize_segments};
pub use echo::{EchoReport, EchoWindow, detect_echo};
pub use engine::SttEngine;
pub use engine_choice::{BatchEngine, EngineDecision, decide_batch_engine, gigaam_ready};
pub use gigaam_path::{
    DECODER_FILE, ENCODER_FILE, GigaamModels, JOINER_FILE, MODEL_ID as GIGAAM_MODEL_ID,
    TOKENS_FILE, gigaam_models_dir, resolve_gigaam_models,
};
pub use hallucination::{is_hallucination_prefix, is_whisper_hallucination};
pub use hypothesis::{Biasing, DEFAULT_HOTWORDS_SCORE, TransducerHypothesis};
pub use local_agreement::{
    HypothesisWord, LocalAgreement, Stabilized, backfill_end_ms, words_from_char_tokens,
    words_from_tokens,
};
pub use mock::MockSttEngine;
pub use model_path::{models_dir, resolve_whisper_model, whisper_filename_for_id};
pub use noise_gate::{NoiseGate, frame_rms};
pub use pacing::{InferencePacer, MIN_SPEECH_FRAMES, PARTIAL_MIN_FRAMES, SILENCE_FRAMES};
pub use speech_decider::SpeechDecider;
pub use vad_path::{SILERO_FILE, resolve_vad_model, vad_models_dir, vad_ready};
pub use window::{LiveCaptionPipeline, SttBackendKind, pcm_bytes_to_i16};

#[cfg(feature = "gigaam")]
pub use gigaam::{GigaamBatchTranscriber, GigaamRecognizer};

pub use parakeet_path::{
    PARAKEET_DECODER_FILE, PARAKEET_ENCODER_FILE, PARAKEET_JOINER_FILE, PARAKEET_MODEL_ID,
    PARAKEET_TOKENS_FILE, ParakeetModels, parakeet_models_dir, parakeet_ready,
    resolve_parakeet_models,
};

#[cfg(feature = "parakeet")]
pub use parakeet::{ParakeetBatchTranscriber, ParakeetRecognizer};

pub use tone_path::{
    TONE_MODEL_FILE, TONE_MODEL_ID, TONE_TOKENS_FILE, ToneModel, resolve_tone_model,
    tone_models_dir, tone_ready,
};

#[cfg(feature = "tone")]
pub use tone::{CHUNK_MS as TONE_CHUNK_MS, ToneStreamer};

#[cfg(feature = "vad")]
pub use vad::SileroGate;
#[cfg(feature = "vad")]
pub use vad_segments::speech_segments;

#[cfg(feature = "whisper")]
pub use whisper::WhisperSttEngine;
#[cfg(feature = "whisper")]
pub use whisper_batch::WhisperBatchTranscriber;
