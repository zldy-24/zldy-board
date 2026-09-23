//! Platform-independent foundations for the offline input-method engine.

mod engine;
mod error;
pub mod input;
mod session;
pub mod state;
mod types;

pub mod composer;

pub use engine::{EngineConfig, ImeEngine};
pub use error::ImeError;
pub use input::{FocusChange, HardwareKeyEvent, InputEvent, KeyPhase, LogicalKey, Modifiers};
pub use session::ImeSession;
pub use state::{
    AckOutcome, ActionDisposition, CommitKind, EventHandling, ImeAction, ImeActionKind, ImeResult,
    ImeState, ImeStatus, PlatformResetReason, SessionPhase,
};
pub use types::{
    ActionId, CandidateId, ContextSnapshot, CoreLimits, DocumentIdentity, EngineId, InputScope,
    MarkedTextSupport, ModeId, PlatformCapabilities, ResourceGeneration, SessionId, StateRevision,
    SurroundingTextSupport,
};
