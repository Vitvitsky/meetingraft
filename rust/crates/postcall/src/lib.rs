//! Локальная сборка post-call транскриптов и артефактов.

mod assemble;
mod llm;
mod llm_http;
mod prompts;
mod templates;

pub use assemble::assemble_final;
pub use llm::{LlmClient, LlmError, NullLlmClient};
pub use llm_http::{OllamaNativeClient, OpenAiCompatLlmClient};
pub use prompts::{brief_prompts, follow_up_prompts};
pub use templates::{make_artifact, render_brief, render_follow_up};
