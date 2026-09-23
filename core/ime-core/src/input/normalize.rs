use super::{HardwareKeyEvent, KeyPhase, LogicalKey};

/// Basic portable interpretation of hardware events.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct InputNormalizer;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum NormalizedHardwareEvent {
    Logical(LogicalInput),
    PassThrough,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum LogicalInput {
    InsertText(String),
    Backspace { count: u16 },
    DeleteForward { count: u16 },
    MoveCompositionCursor { grapheme_delta: i32 },
    Commit,
    Cancel,
}

impl InputNormalizer {
    pub(crate) fn normalize(
        self,
        event: HardwareKeyEvent,
        is_composing: bool,
    ) -> NormalizedHardwareEvent {
        if event.phase != KeyPhase::Down || event.modifiers.has_command_modifier() {
            return NormalizedHardwareEvent::PassThrough;
        }

        let repeat_count = event.repeat_count.max(1);
        let logical = match event.logical_key {
            LogicalKey::Character(character) => LogicalInput::InsertText(
                std::iter::repeat_n(character, repeat_count.into()).collect(),
            ),
            LogicalKey::Backspace if is_composing => LogicalInput::Backspace {
                count: repeat_count,
            },
            LogicalKey::Delete if is_composing => LogicalInput::DeleteForward {
                count: repeat_count,
            },
            LogicalKey::Enter if is_composing => LogicalInput::Commit,
            LogicalKey::Escape if is_composing => LogicalInput::Cancel,
            LogicalKey::ArrowLeft if is_composing => LogicalInput::MoveCompositionCursor {
                grapheme_delta: -i32::from(repeat_count),
            },
            LogicalKey::ArrowRight if is_composing => LogicalInput::MoveCompositionCursor {
                grapheme_delta: i32::from(repeat_count),
            },
            LogicalKey::Backspace
            | LogicalKey::Delete
            | LogicalKey::Enter
            | LogicalKey::Escape
            | LogicalKey::ArrowLeft
            | LogicalKey::ArrowRight
            | LogicalKey::Unknown => return NormalizedHardwareEvent::PassThrough,
        };

        NormalizedHardwareEvent::Logical(logical)
    }
}
