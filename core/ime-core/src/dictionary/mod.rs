//! Read-only dictionary abstractions for candidate generation.

mod memory;

use crate::{ImeError, LexemeId};

pub use memory::InMemoryDictionary;

/// One immutable dictionary record.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DictionaryEntry {
    /// Stable dictionary identity, separate from revision-scoped candidate IDs.
    pub lexeme_id: LexemeId,
    /// Normalized input code used for lookup.
    pub input_code: String,
    /// UTF-8 text proposed for commit.
    pub text: String,
    /// Non-negative corpus-derived weight supplied by the dictionary builder.
    pub base_frequency: u64,
}

impl DictionaryEntry {
    /// Creates a dictionary entry. Phase 1B deliberately performs no language-specific validation.
    pub fn new(
        lexeme_id: LexemeId,
        input_code: impl Into<String>,
        text: impl Into<String>,
        base_frequency: u64,
    ) -> Self {
        Self {
            lexeme_id,
            input_code: input_code.into(),
            text: text.into(),
            base_frequency,
        }
    }
}

/// Synchronous, deterministic, bounded, and read-only dictionary access.
pub trait Dictionary: std::fmt::Debug + Send + Sync {
    /// Returns at most `limit` entries whose input code exactly matches `input_code`.
    fn lookup_exact(
        &self,
        input_code: &str,
        limit: usize,
    ) -> Result<Vec<DictionaryEntry>, ImeError>;

    /// Returns at most `limit` entries whose input code starts with `input_prefix`.
    fn lookup_prefix(
        &self,
        input_prefix: &str,
        limit: usize,
    ) -> Result<Vec<DictionaryEntry>, ImeError>;
}
