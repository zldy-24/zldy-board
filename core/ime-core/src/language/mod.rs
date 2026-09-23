//! Language-specific normalization and parsing boundaries.

mod reference;

use crate::ImeError;

pub use reference::ReferenceLanguageEngine;

/// Normalized input that remains independent of a concrete language parser.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NormalizedInput {
    code: String,
}

impl NormalizedInput {
    /// Creates normalized input from an owned code string.
    pub fn new(code: String) -> Self {
        Self { code }
    }

    /// Returns the normalized input code.
    pub fn code(&self) -> &str {
        &self.code
    }
}

/// Minimal parsed representation used by the Phase 1B candidate pipeline.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParsedInput {
    normalized_code: String,
}

impl ParsedInput {
    /// Creates a parsed input from its normalized code.
    pub fn new(normalized_code: String) -> Self {
        Self { normalized_code }
    }

    /// Returns the normalized code used for dictionary lookup.
    pub fn normalized_code(&self) -> &str {
        &self.normalized_code
    }
}

/// Stable identity used to merge equivalent candidate proposals.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CandidateIdentity(String);

impl CandidateIdentity {
    /// Creates an identity from a candidate's committed text.
    pub fn from_text(text: &str) -> Self {
        Self(text.to_owned())
    }

    /// Returns the identity text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Minimal language abstraction required by candidate generation.
pub trait LanguageEngine: std::fmt::Debug + Send + Sync {
    /// Normalizes raw composition without applying language parsing rules.
    fn normalize_input(&self, composition: &str) -> Result<NormalizedInput, ImeError>;

    /// Parses normalized input into the minimal representation consumed by providers.
    fn parse_input(&self, input: &NormalizedInput) -> Result<ParsedInput, ImeError>;

    /// Returns the identity used to merge semantically equivalent candidate proposals.
    fn candidate_identity(&self, candidate_text: &str) -> CandidateIdentity;
}
