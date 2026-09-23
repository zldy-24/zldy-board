//! Deterministic integer ranking for bounded candidate sets.

use std::cmp::Ordering;

use crate::{
    ImeError,
    candidate::{CandidateProposal, MatchKind},
};

const EXACT_MATCH_BONUS: i64 = 2_000_000;
const MAX_FREQUENCY_SCORE: i64 = 1_000_000;

/// Phase 1B's explainable, platform-independent ranking implementation.
#[derive(Clone, Copy, Debug, Default)]
pub struct RankingEngine;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct RankedCandidate {
    pub(crate) proposal: CandidateProposal,
    pub(crate) score: i64,
}

impl RankingEngine {
    /// Ranks proposals with a stable total ordering and returns at most `top_k` entries.
    pub(crate) fn rank(
        &self,
        proposals: Vec<CandidateProposal>,
        top_k: usize,
    ) -> Result<Vec<RankedCandidate>, ImeError> {
        let mut ranked: Vec<_> = proposals
            .into_iter()
            .map(|proposal| RankedCandidate {
                score: score(&proposal),
                proposal,
            })
            .collect();
        ranked.sort_by(compare_ranked);
        ranked.truncate(top_k);
        Ok(ranked)
    }
}

fn score(candidate: &CandidateProposal) -> i64 {
    let exact = if candidate.match_kind == MatchKind::Exact {
        EXACT_MATCH_BONUS
    } else {
        0
    };
    let frequency = i64::try_from(candidate.base_frequency)
        .unwrap_or(i64::MAX)
        .clamp(0, MAX_FREQUENCY_SCORE);
    exact
        .saturating_add(frequency)
        .saturating_add(candidate.source.priority())
}

fn compare_ranked(left: &RankedCandidate, right: &RankedCandidate) -> Ordering {
    right
        .score
        .cmp(&left.score)
        .then_with(|| right.proposal.match_kind.cmp(&left.proposal.match_kind))
        .then_with(|| {
            right
                .proposal
                .base_frequency
                .cmp(&left.proposal.base_frequency)
        })
        .then_with(|| left.proposal.input_code.cmp(&right.proposal.input_code))
        .then_with(|| left.proposal.text.cmp(&right.proposal.text))
        .then_with(|| left.proposal.lexeme_id.cmp(&right.proposal.lexeme_id))
}

#[cfg(test)]
mod tests {
    use super::RankingEngine;
    use crate::{
        LexemeId,
        candidate::{CandidateProposal, CandidateSource, MatchKind},
    };

    fn proposal(id: u64, text: &str, frequency: u64, match_kind: MatchKind) -> CandidateProposal {
        CandidateProposal {
            lexeme_id: LexemeId::new(id),
            text: text.to_owned(),
            input_code: "code".to_owned(),
            source: CandidateSource::SystemDictionary,
            base_frequency: frequency,
            match_kind,
        }
    }

    #[test]
    fn exact_match_precedes_prefix_even_with_extreme_frequency() {
        let ranked = RankingEngine
            .rank(
                vec![
                    proposal(1, "prefix", u64::MAX, MatchKind::Prefix),
                    proposal(2, "exact", 0, MatchKind::Exact),
                ],
                10,
            )
            .expect("ranking succeeds");
        assert_eq!(ranked[0].proposal.text, "exact");
    }

    #[test]
    fn frequency_and_lexeme_id_are_deterministic_ties() {
        let ranked = RankingEngine
            .rank(
                vec![
                    proposal(3, "same", 10, MatchKind::Exact),
                    proposal(1, "same", 10, MatchKind::Exact),
                    proposal(2, "higher", 11, MatchKind::Exact),
                ],
                10,
            )
            .expect("ranking succeeds");
        assert_eq!(ranked[0].proposal.text, "higher");
        assert_eq!(ranked[1].proposal.lexeme_id, LexemeId::new(1));
        assert_eq!(ranked[2].proposal.lexeme_id, LexemeId::new(3));
    }

    #[test]
    fn ranking_is_bounded() {
        let ranked = RankingEngine
            .rank(
                vec![
                    proposal(1, "a", 1, MatchKind::Exact),
                    proposal(2, "b", 2, MatchKind::Exact),
                ],
                1,
            )
            .expect("ranking succeeds");
        assert_eq!(ranked.len(), 1);
        assert_eq!(ranked[0].proposal.text, "b");
    }
}
