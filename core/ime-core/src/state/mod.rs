use crate::{ActionId, CandidateId, ModeId, ResourceGeneration, SessionId, StateRevision};

/// Portable session phase derived from composition and candidate state.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SessionPhase {
    #[default]
    Idle,
    Composing,
    CandidateSelecting,
}

/// Whether the platform should consume an original hardware event.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EventHandling {
    Consumed,
    PassThrough,
}

/// High-level processing status for an otherwise valid event.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImeStatus {
    Ok,
    NoOp,
    Unsupported,
}

/// Why text is being committed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CommitKind {
    Candidate,
    RawFallback,
    DirectInput,
}

/// Why Core requests reconciliation with the platform text system.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlatformResetReason {
    ExplicitReset,
    StateMismatch,
}

/// A platform side effect requested by Core.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ImeActionKind {
    SetCompositionText {
        text_utf8: String,
        cursor_utf8_byte_offset: usize,
    },
    FinishComposition,
    CancelComposition,
    CommitText {
        text_utf8: String,
        commit_kind: CommitKind,
    },
    DeleteBackward {
        operation_count: u16,
    },
    RequestPlatformReset {
        reason: PlatformResetReason,
    },
}

/// A versioned Session action awaiting a platform disposition.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImeAction {
    pub action_id: ActionId,
    pub session_id: SessionId,
    pub originating_revision: StateRevision,
    pub kind: ImeActionKind,
}

/// The platform's weak acknowledgement of an action.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ActionDisposition {
    Applied,
    Rejected,
    Unavailable,
    Superseded,
    Failed,
}

/// Result of recording an acknowledgement.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AckOutcome {
    Recorded {
        action_id: ActionId,
        disposition: ActionDisposition,
    },
    AlreadyResolved {
        action_id: ActionId,
        disposition: ActionDisposition,
    },
}

/// Minimal candidate data exposed to platform renderers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CandidateView {
    /// Identifier valid only with the containing Session revision.
    pub candidate_id: CandidateId,
    /// UTF-8 text committed when this candidate is selected.
    pub text: String,
}

/// A renderable snapshot of portable Session state.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImeState {
    pub phase: SessionPhase,
    pub composition_utf8: String,
    pub composition_cursor_grapheme: usize,
    pub composition_cursor_utf8_byte_offset: usize,
    pub candidates: Vec<CandidateView>,
    pub selected_candidate: Option<CandidateId>,
    pub mode: ModeId,
}

/// Optional component that was unavailable while producing a result.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DegradedComponent {
    Language,
    Dictionary,
    Candidate,
    Ranking,
}

/// Additional bounded-result information.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResultFlag {}

/// Complete output of one Core input operation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImeResult {
    pub status: ImeStatus,
    pub event_handling: EventHandling,
    pub session_id: SessionId,
    pub state_revision: StateRevision,
    pub resource_generation: ResourceGeneration,
    pub state: ImeState,
    pub actions: Vec<ImeAction>,
    pub degraded_components: Vec<DegradedComponent>,
    pub result_flags: Vec<ResultFlag>,
}
