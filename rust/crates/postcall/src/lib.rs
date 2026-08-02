//! Локальная сборка post-call транскриптов и артефактов.

mod assemble;
mod llm;
mod templates;

pub use assemble::assemble_final;
pub use llm::{LlmClient, LlmError, NullLlmClient};
pub use templates::{make_artifact, render_brief, render_follow_up};
