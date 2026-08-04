//! Локальная сборка post-call транскриптов и артефактов.

mod assemble;
mod diff;
mod edits;
mod jobs;
mod llm;
mod llm_http;
mod merge;
mod polish;
mod prompts;
mod speakers;
mod templates;
mod term_from_edit;

pub use assemble::assemble_final;
pub use diff::{DiffOp, DiffSpan, diff_words};
pub use edits::reattach_edits;
pub use jobs::{
    InlineSpawner, JobHandle, RebuildJobs, RebuildProgress, RebuildState, Spawner, ThreadSpawner,
};
pub use llm::{LlmClient, LlmError, NullLlmClient};
pub use llm_http::{OllamaNativeClient, OpenAiCompatLlmClient};
pub use merge::{merge_channels, render_segments};
pub use polish::{
    DEFAULT_BATCH_SIZE, PolishReport, format_batch, parse_polished, polish_prompts, polish_segments,
};
pub use prompts::{brief_prompts, follow_up_prompts};
pub use speakers::{SpeakerStat, speaker_stats};
pub use templates::{make_artifact, render_brief, render_follow_up};
pub use term_from_edit::term_from_edit;
