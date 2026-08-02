//! Движок meeting session MeetingRaft.

mod engine;
mod fake_captions;

pub use engine::{MeetingSession, SessionError};
pub use fake_captions::FakeCaptionProducer;