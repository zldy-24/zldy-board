use std::fmt;

use crate::ImeError;

macro_rules! id_type {
    ($name:ident) => {
        #[doc = concat!("Strongly typed `", stringify!($name), "` value.")]
        #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name(u64);

        impl $name {
            /// Creates an identifier from its numeric representation.
            pub const fn new(value: u64) -> Self {
                Self(value)
            }

            /// Returns the numeric representation.
            pub const fn get(self) -> u64 {
                self.0
            }
        }
    };
}

id_type!(EngineId);
id_type!(SessionId);
id_type!(ResourceGeneration);
id_type!(StateRevision);
id_type!(ActionId);
id_type!(CandidateId);
id_type!(DocumentIdentity);

/// An opaque mode identifier. Phase 1A uses mode zero only.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ModeId(u32);

impl ModeId {
    /// Creates a mode identifier.
    pub const fn new(value: u32) -> Self {
        Self(value)
    }

    /// Returns the numeric representation.
    pub const fn get(self) -> u32 {
        self.0
    }
}

/// Hard safety limits enforced by the Core.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CoreLimits {
    pub max_event_text_bytes: usize,
    pub max_composition_text_bytes: usize,
    pub max_context_text_bytes: usize,
    pub max_unknown_context_text_bytes: usize,
    pub max_outstanding_actions: usize,
    pub max_resolved_action_history: usize,
}

impl Default for CoreLimits {
    fn default() -> Self {
        Self {
            max_event_text_bytes: 4 * 1024,
            max_composition_text_bytes: 16 * 1024,
            max_context_text_bytes: 1024,
            max_unknown_context_text_bytes: 0,
            max_outstanding_actions: 64,
            max_resolved_action_history: 128,
        }
    }
}

impl CoreLimits {
    pub(crate) fn validate(&self) -> Result<(), ImeError> {
        if self.max_event_text_bytes == 0 {
            return Err(ImeError::InvalidConfig(
                "max_event_text_bytes must be greater than zero",
            ));
        }
        if self.max_composition_text_bytes < self.max_event_text_bytes {
            return Err(ImeError::InvalidConfig(
                "max_composition_text_bytes must cover one maximum-size event",
            ));
        }
        if self.max_unknown_context_text_bytes > self.max_context_text_bytes {
            return Err(ImeError::InvalidConfig(
                "unknown-context limit cannot exceed the normal context limit",
            ));
        }
        if self.max_outstanding_actions == 0 {
            return Err(ImeError::InvalidConfig(
                "max_outstanding_actions must be greater than zero",
            ));
        }
        if self.max_resolved_action_history == 0 {
            return Err(ImeError::InvalidConfig(
                "max_resolved_action_history must be greater than zero",
            ));
        }
        Ok(())
    }
}

/// Host support for marked composition text.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MarkedTextSupport {
    #[default]
    None,
    Basic,
}

/// Host support for bounded surrounding text.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SurroundingTextSupport {
    #[default]
    None,
    BeforeCursor,
    Bidirectional,
}

/// Capabilities that materially change portable Core behavior.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PlatformCapabilities {
    pub marked_text_support: MarkedTextSupport,
    pub surrounding_text_support: SurroundingTextSupport,
    pub selected_text_read_support: bool,
    pub hardware_key_support: bool,
}

/// Privacy-relevant classification of the active host input field.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum InputScope {
    Normal,
    Search,
    Email,
    Url,
    Number,
    Phone,
    Password,
    Sensitive,
    #[default]
    Unknown,
}

impl InputScope {
    /// Returns whether persistent learning is permitted by the default policy.
    pub const fn permits_persistent_learning(self) -> bool {
        !matches!(self, Self::Password | Self::Sensitive | Self::Unknown)
    }

    /// Returns whether private-history prediction is permitted by default.
    pub const fn permits_private_history_prediction(self) -> bool {
        !matches!(self, Self::Password | Self::Sensitive | Self::Unknown)
    }

    const fn is_strictly_sensitive(self) -> bool {
        matches!(self, Self::Password | Self::Sensitive)
    }
}

/// A bounded, adapter-local snapshot of context around the host cursor.
#[derive(Clone, PartialEq, Eq)]
pub struct ContextSnapshot {
    before_cursor_utf8: String,
    selected_text_utf8: String,
    after_cursor_utf8: String,
    before_truncated: bool,
    after_truncated: bool,
    input_scope: InputScope,
    context_epoch: u64,
    document_identity: Option<DocumentIdentity>,
}

impl ContextSnapshot {
    /// Creates a snapshot. Text is sanitized when the Session accepts the event.
    pub fn new(
        before_cursor_utf8: impl Into<String>,
        selected_text_utf8: impl Into<String>,
        after_cursor_utf8: impl Into<String>,
        input_scope: InputScope,
        context_epoch: u64,
    ) -> Self {
        Self {
            before_cursor_utf8: before_cursor_utf8.into(),
            selected_text_utf8: selected_text_utf8.into(),
            after_cursor_utf8: after_cursor_utf8.into(),
            before_truncated: false,
            after_truncated: false,
            input_scope,
            context_epoch,
            document_identity: None,
        }
    }

    /// Records whether the adapter truncated the surrounding buffers.
    pub const fn with_truncation(mut self, before: bool, after: bool) -> Self {
        self.before_truncated = before;
        self.after_truncated = after;
        self
    }

    /// Attaches an adapter-local opaque document identity.
    pub const fn with_document_identity(mut self, identity: DocumentIdentity) -> Self {
        self.document_identity = Some(identity);
        self
    }

    /// Creates an empty snapshot for a scope and adapter-local epoch.
    pub fn empty(input_scope: InputScope, context_epoch: u64) -> Self {
        Self::new(
            String::new(),
            String::new(),
            String::new(),
            input_scope,
            context_epoch,
        )
    }

    /// Returns the bounded context before the cursor.
    pub fn before_cursor_utf8(&self) -> &str {
        &self.before_cursor_utf8
    }

    /// Returns the bounded selected text.
    pub fn selected_text_utf8(&self) -> &str {
        &self.selected_text_utf8
    }

    /// Returns the bounded context after the cursor.
    pub fn after_cursor_utf8(&self) -> &str {
        &self.after_cursor_utf8
    }

    /// Returns whether the adapter truncated text before the cursor.
    pub const fn before_truncated(&self) -> bool {
        self.before_truncated
    }

    /// Returns whether the adapter truncated text after the cursor.
    pub const fn after_truncated(&self) -> bool {
        self.after_truncated
    }

    /// Returns the privacy scope.
    pub const fn input_scope(&self) -> InputScope {
        self.input_scope
    }

    /// Returns the adapter-local context generation.
    pub const fn context_epoch(&self) -> u64 {
        self.context_epoch
    }

    /// Returns the optional adapter-local document identity.
    pub const fn document_identity(&self) -> Option<DocumentIdentity> {
        self.document_identity
    }

    pub(crate) fn sanitized(
        mut self,
        limits: &CoreLimits,
        capabilities: PlatformCapabilities,
    ) -> Result<Self, ImeError> {
        let max = if self.input_scope.is_strictly_sensitive() {
            0
        } else if self.input_scope == InputScope::Unknown {
            limits.max_unknown_context_text_bytes
        } else {
            limits.max_context_text_bytes
        };

        if max == 0 {
            self.before_cursor_utf8.clear();
            self.selected_text_utf8.clear();
            self.after_cursor_utf8.clear();
            return Ok(self);
        }

        match capabilities.surrounding_text_support {
            SurroundingTextSupport::None => {
                self.before_cursor_utf8.clear();
                self.after_cursor_utf8.clear();
            }
            SurroundingTextSupport::BeforeCursor => self.after_cursor_utf8.clear(),
            SurroundingTextSupport::Bidirectional => {}
        }
        if !capabilities.selected_text_read_support {
            self.selected_text_utf8.clear();
        }

        validate_context_field("before_cursor_utf8", &self.before_cursor_utf8, max)?;
        validate_context_field("selected_text_utf8", &self.selected_text_utf8, max)?;
        validate_context_field("after_cursor_utf8", &self.after_cursor_utf8, max)?;
        Ok(self)
    }
}

impl Default for ContextSnapshot {
    fn default() -> Self {
        Self::empty(InputScope::Unknown, 0)
    }
}

impl fmt::Debug for ContextSnapshot {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ContextSnapshot")
            .field("before_cursor_bytes", &self.before_cursor_utf8.len())
            .field("selected_text_bytes", &self.selected_text_utf8.len())
            .field("after_cursor_bytes", &self.after_cursor_utf8.len())
            .field("before_truncated", &self.before_truncated)
            .field("after_truncated", &self.after_truncated)
            .field("input_scope", &self.input_scope)
            .field("context_epoch", &self.context_epoch)
            .field("has_document_identity", &self.document_identity.is_some())
            .finish()
    }
}

fn validate_context_field(field: &'static str, value: &str, max: usize) -> Result<(), ImeError> {
    if value.len() > max {
        return Err(ImeError::ContextTextTooLong {
            field,
            actual: value.len(),
            max,
        });
    }
    Ok(())
}
