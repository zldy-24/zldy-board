use std::sync::Arc;

use super::{CandidateProposal, CandidateSource, MatchKind};
use crate::{ImeError, dictionary::Dictionary, language::ParsedInput, state::DegradedComponent};

/// Converts parsed language input into bounded, unranked proposals.
pub trait CandidateProvider: std::fmt::Debug + Send + Sync {
    /// Produces at most `limit` unranked proposals.
    fn generate(
        &self,
        input: &ParsedInput,
        limit: usize,
    ) -> Result<Vec<CandidateProposal>, ImeError>;

    /// Identifies the degraded component reported if this provider fails.
    fn degraded_component(&self) -> DegradedComponent;
}

/// Phase 1B provider backed by one read-only dictionary.
#[derive(Debug)]
pub struct DictionaryCandidateProvider {
    dictionary: Arc<dyn Dictionary>,
    source: CandidateSource,
}

impl DictionaryCandidateProvider {
    /// Creates a system-dictionary provider.
    pub fn system(dictionary: Arc<dyn Dictionary>) -> Self {
        Self {
            dictionary,
            source: CandidateSource::SystemDictionary,
        }
    }
}

impl CandidateProvider for DictionaryCandidateProvider {
    fn generate(
        &self,
        input: &ParsedInput,
        limit: usize,
    ) -> Result<Vec<CandidateProposal>, ImeError> {
        if input.normalized_code().is_empty() || limit == 0 {
            return Ok(Vec::new());
        }
        let entries = self
            .dictionary
            .lookup_prefix(input.normalized_code(), limit)?;
        Ok(entries
            .into_iter()
            .map(|entry| CandidateProposal {
                match_kind: if entry.input_code == input.normalized_code() {
                    MatchKind::Exact
                } else {
                    MatchKind::Prefix
                },
                lexeme_id: entry.lexeme_id,
                text: entry.text,
                input_code: entry.input_code,
                source: self.source,
                base_frequency: entry.base_frequency,
            })
            .collect())
    }

    fn degraded_component(&self) -> DegradedComponent {
        DegradedComponent::Dictionary
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::{CandidateProvider, DictionaryCandidateProvider};
    use crate::{
        LexemeId,
        candidate::MatchKind,
        dictionary::{DictionaryEntry, InMemoryDictionary},
        language::ParsedInput,
    };

    #[test]
    fn generates_exact_and_prefix_proposals() {
        let provider = DictionaryCandidateProvider::system(Arc::new(InMemoryDictionary::new([
            DictionaryEntry::new(LexemeId::new(1), "ni", "你", 1000),
            DictionaryEntry::new(LexemeId::new(2), "nihao", "你好", 3000),
        ])));
        let proposals = provider
            .generate(&ParsedInput::new("ni".to_owned()), 10)
            .expect("generation succeeds");
        assert_eq!(proposals.len(), 2);
        assert_eq!(proposals[0].match_kind, MatchKind::Exact);
        assert_eq!(proposals[1].match_kind, MatchKind::Prefix);
    }
}
