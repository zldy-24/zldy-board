use std::fmt;

use crate::{ActionId, CandidateId, StateRevision};

/// A recoverable error reported by the Core API.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ImeError {
    /// Engine configuration violates a Core invariant.
    InvalidConfig(&'static str),
    /// One input event carries more UTF-8 data than allowed.
    EventTextTooLong { actual: usize, max: usize },
    /// Applying an event would grow the composition beyond its hard limit.
    CompositionTooLong { actual: usize, max: usize },
    /// A context field exceeds its hard UTF-8 byte limit.
    ContextTextTooLong {
        field: &'static str,
        actual: usize,
        max: usize,
    },
    /// The platform has not acknowledged enough actions to accept another one.
    TooManyOutstandingActions { max: usize },
    /// An acknowledgement refers to an action that this Session does not know.
    UnknownAction(ActionId),
    /// A candidate selection was produced for an older Session revision.
    StaleRevision {
        expected: StateRevision,
        actual: StateRevision,
    },
    /// A candidate ID does not exist in the current snapshot.
    UnknownCandidate(CandidateId),
    /// A monotonic identifier reached its representable limit.
    CounterExhausted(&'static str),
}

impl fmt::Display for ImeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidConfig(reason) => write!(formatter, "invalid engine config: {reason}"),
            Self::EventTextTooLong { actual, max } => {
                write!(formatter, "event text is {actual} bytes; maximum is {max}")
            }
            Self::CompositionTooLong { actual, max } => {
                write!(
                    formatter,
                    "composition would be {actual} bytes; maximum is {max}"
                )
            }
            Self::ContextTextTooLong { field, actual, max } => {
                write!(
                    formatter,
                    "context field {field} is {actual} bytes; maximum is {max}"
                )
            }
            Self::TooManyOutstandingActions { max } => {
                write!(formatter, "too many outstanding actions; maximum is {max}")
            }
            Self::UnknownAction(action_id) => {
                write!(formatter, "unknown action id {}", action_id.get())
            }
            Self::StaleRevision { expected, actual } => write!(
                formatter,
                "stale state revision {}; current revision is {}",
                actual.get(),
                expected.get()
            ),
            Self::UnknownCandidate(candidate_id) => {
                write!(formatter, "unknown candidate id {}", candidate_id.get())
            }
            Self::CounterExhausted(counter) => write!(formatter, "{counter} counter exhausted"),
        }
    }
}

impl std::error::Error for ImeError {}
