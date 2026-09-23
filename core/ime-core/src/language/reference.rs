use super::{CandidateIdentity, LanguageEngine, NormalizedInput, ParsedInput};
use crate::ImeError;

/// Small reference implementation used to validate the language boundary.
#[derive(Clone, Copy, Debug, Default)]
pub struct ReferenceLanguageEngine;

impl LanguageEngine for ReferenceLanguageEngine {
    fn normalize_input(&self, composition: &str) -> Result<NormalizedInput, ImeError> {
        let code = composition
            .chars()
            .map(|character| {
                if character.is_ascii_uppercase() {
                    character.to_ascii_lowercase()
                } else {
                    character
                }
            })
            .collect();
        Ok(NormalizedInput::new(code))
    }

    fn parse_input(&self, input: &NormalizedInput) -> Result<ParsedInput, ImeError> {
        Ok(ParsedInput::new(input.code().to_owned()))
    }

    fn candidate_identity(&self, candidate_text: &str) -> CandidateIdentity {
        CandidateIdentity::from_text(candidate_text)
    }
}

#[cfg(test)]
mod tests {
    use super::{LanguageEngine, ReferenceLanguageEngine};

    #[test]
    fn normalizes_ascii_uppercase() {
        let normalized = ReferenceLanguageEngine
            .normalize_input("NIHAO")
            .expect("normalization succeeds");
        assert_eq!(normalized.code(), "nihao");
    }

    #[test]
    fn preserves_ascii_lowercase() {
        let normalized = ReferenceLanguageEngine
            .normalize_input("nihao")
            .expect("normalization succeeds");
        assert_eq!(normalized.code(), "nihao");
    }

    #[test]
    fn preserves_other_unicode() {
        let normalized = ReferenceLanguageEngine
            .normalize_input("你好Ü")
            .expect("normalization succeeds");
        assert_eq!(normalized.code(), "你好Ü");
    }

    #[test]
    fn accepts_empty_input() {
        let normalized = ReferenceLanguageEngine
            .normalize_input("")
            .expect("normalization succeeds");
        let parsed = ReferenceLanguageEngine
            .parse_input(&normalized)
            .expect("parsing succeeds");
        assert!(parsed.normalized_code().is_empty());
    }
}
