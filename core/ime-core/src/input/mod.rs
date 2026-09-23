//! Portable logical and hardware input events.

mod normalize;

use std::ops::{BitOr, BitOrAssign};

use crate::{CandidateId, ContextSnapshot, ModeId, StateRevision};

pub(crate) use normalize::{InputNormalizer, LogicalInput, NormalizedHardwareEvent};

/// One platform-independent input operation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InputEvent {
    InsertText(String),
    Backspace,
    DeleteForward,
    MoveCompositionCursor {
        grapheme_delta: i32,
    },
    HardwareKey(HardwareKeyEvent),
    SelectCandidate {
        candidate_id: CandidateId,
        state_revision: StateRevision,
    },
    MoveCandidateSelection {
        delta: i32,
    },
    NextCandidatePage,
    PreviousCandidatePage,
    Commit,
    Cancel,
    SwitchMode(ModeId),
    ContextChanged(ContextSnapshot),
    FocusChanged(FocusChange),
    Reset,
}

/// Adapter-local focus notification.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FocusChange {
    pub focused: bool,
    pub context_epoch: u64,
}

/// A hardware event after platform key-code normalization.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HardwareKeyEvent {
    pub physical_code: u32,
    pub logical_key: LogicalKey,
    pub modifiers: Modifiers,
    pub phase: KeyPhase,
    pub repeat_count: u16,
}

/// Portable logical meaning of a hardware key.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LogicalKey {
    Character(char),
    Backspace,
    Delete,
    Enter,
    Escape,
    ArrowLeft,
    ArrowRight,
    Unknown,
}

/// Hardware-key transition.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyPhase {
    Down,
    Up,
}

/// Portable modifier mask. Platform-specific modifier bits never enter Core.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Modifiers(u8);

impl Modifiers {
    pub const SHIFT: Self = Self(1 << 0);
    pub const CONTROL: Self = Self(1 << 1);
    pub const ALT: Self = Self(1 << 2);
    pub const META: Self = Self(1 << 3);

    /// An empty modifier set.
    pub const fn empty() -> Self {
        Self(0)
    }

    /// Returns whether all bits in `other` are present.
    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    pub(crate) const fn has_command_modifier(self) -> bool {
        self.contains(Self::CONTROL) || self.contains(Self::ALT) || self.contains(Self::META)
    }
}

impl BitOr for Modifiers {
    type Output = Self;

    fn bitor(self, rhs: Self) -> Self::Output {
        Self(self.0 | rhs.0)
    }
}

impl BitOrAssign for Modifiers {
    fn bitor_assign(&mut self, rhs: Self) {
        self.0 |= rhs.0;
    }
}
