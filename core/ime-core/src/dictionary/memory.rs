use std::collections::BTreeMap;

use super::{Dictionary, DictionaryEntry};
use crate::ImeError;

/// Small deterministic dictionary used by Phase 1B tests and the development CLI.
#[derive(Clone, Debug, Default)]
pub struct InMemoryDictionary {
    entries_by_code: BTreeMap<String, Vec<DictionaryEntry>>,
}

impl InMemoryDictionary {
    /// Builds an immutable dictionary with a stable record order.
    pub fn new(entries: impl IntoIterator<Item = DictionaryEntry>) -> Self {
        let mut entries_by_code: BTreeMap<String, Vec<DictionaryEntry>> = BTreeMap::new();
        for entry in entries {
            entries_by_code
                .entry(entry.input_code.clone())
                .or_default()
                .push(entry);
        }
        for values in entries_by_code.values_mut() {
            values.sort_by(|left, right| {
                left.lexeme_id
                    .cmp(&right.lexeme_id)
                    .then_with(|| left.text.cmp(&right.text))
                    .then_with(|| right.base_frequency.cmp(&left.base_frequency))
            });
        }
        Self { entries_by_code }
    }

    fn append_bounded(
        output: &mut Vec<DictionaryEntry>,
        entries: &[DictionaryEntry],
        limit: usize,
    ) {
        let remaining = limit.saturating_sub(output.len());
        output.extend(entries.iter().take(remaining).cloned());
    }
}

impl Dictionary for InMemoryDictionary {
    fn lookup_exact(
        &self,
        input_code: &str,
        limit: usize,
    ) -> Result<Vec<DictionaryEntry>, ImeError> {
        if input_code.is_empty() || limit == 0 {
            return Ok(Vec::new());
        }
        Ok(self
            .entries_by_code
            .get(input_code)
            .into_iter()
            .flatten()
            .take(limit)
            .cloned()
            .collect())
    }

    fn lookup_prefix(
        &self,
        input_prefix: &str,
        limit: usize,
    ) -> Result<Vec<DictionaryEntry>, ImeError> {
        if input_prefix.is_empty() || limit == 0 {
            return Ok(Vec::new());
        }

        let mut output = Vec::with_capacity(limit.min(64));
        for (code, entries) in self.entries_by_code.range(input_prefix.to_owned()..) {
            if !code.starts_with(input_prefix) || output.len() == limit {
                break;
            }
            Self::append_bounded(&mut output, entries, limit);
        }
        Ok(output)
    }
}

#[cfg(test)]
mod tests {
    use super::{Dictionary, DictionaryEntry, InMemoryDictionary};
    use crate::LexemeId;

    fn dictionary() -> InMemoryDictionary {
        InMemoryDictionary::new([
            DictionaryEntry::new(LexemeId::new(3), "nihao", "你号", 100),
            DictionaryEntry::new(LexemeId::new(1), "ni", "你", 1000),
            DictionaryEntry::new(LexemeId::new(2), "nihao", "你好", 3000),
            DictionaryEntry::new(LexemeId::new(4), "nihao", "你好", 2500),
        ])
    }

    #[test]
    fn exact_lookup_is_stable_and_preserves_duplicates() {
        let dictionary = dictionary();
        let first = dictionary.lookup_exact("nihao", 10).expect("lookup works");
        let second = dictionary.lookup_exact("nihao", 10).expect("lookup works");
        assert_eq!(first, second);
        assert_eq!(first.len(), 3);
        assert_eq!(first[0].text, "你好");
        assert_eq!(first[0].lexeme_id, LexemeId::new(2));
    }

    #[test]
    fn prefix_lookup_is_bounded_and_unknown_safe() {
        let dictionary = dictionary();
        let matches = dictionary.lookup_prefix("ni", 2).expect("lookup works");
        assert_eq!(matches.len(), 2);
        assert!(
            matches
                .iter()
                .all(|entry| entry.input_code.starts_with("ni"))
        );
        assert!(
            dictionary
                .lookup_prefix("unknown", 10)
                .expect("lookup works")
                .is_empty()
        );
    }

    #[test]
    fn empty_prefix_never_dumps_dictionary() {
        let dictionary = dictionary();
        assert!(
            dictionary
                .lookup_prefix("", usize::MAX)
                .expect("lookup works")
                .is_empty()
        );
        assert!(
            dictionary
                .lookup_exact("", usize::MAX)
                .expect("lookup works")
                .is_empty()
        );
    }
}
