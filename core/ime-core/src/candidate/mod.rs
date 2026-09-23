//! Bounded candidate generation, deduplication, ranking, and revision-scoped snapshots.

mod provider;

use std::{collections::BTreeMap, sync::Arc};

use crate::{
    CandidateId, CoreLimits, LexemeId, SessionId, StateRevision,
    language::{CandidateIdentity, LanguageEngine},
    ranking::{RankedCandidate, RankingEngine},
    state::{CandidateView, DegradedComponent},
};

pub use provider::{CandidateProvider, DictionaryCandidateProvider};

/// Origin of an unranked candidate proposal.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum CandidateSource {
    SystemDictionary,
}

impl CandidateSource {
    pub(crate) const fn priority(self) -> i64 {
        match self {
            Self::SystemDictionary => 100,
        }
    }
}

/// Relationship between the composition and a dictionary input code.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum MatchKind {
    Prefix,
    Exact,
}

/// Unranked output from a candidate provider.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CandidateProposal {
    /// Stable identity from the source dictionary.
    pub lexeme_id: LexemeId,
    /// UTF-8 text proposed for commit.
    pub text: String,
    /// Normalized code matched by the provider.
    pub input_code: String,
    /// Provider category used for ranking priority.
    pub source: CandidateSource,
    /// Non-negative source frequency; duplicate proposals use the maximum, never the sum.
    pub base_frequency: u64,
    /// Whether the input code matched exactly or by prefix.
    pub match_kind: MatchKind,
}

/// Recoverable pipeline failure. The Session retains raw composition on failure.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct CandidatePipelineFailure {
    pub(crate) component: DegradedComponent,
}

/// Full Phase 1B candidate pipeline.
#[derive(Debug)]
pub(crate) struct CandidateEngine {
    language: Arc<dyn LanguageEngine>,
    providers: Vec<Arc<dyn CandidateProvider>>,
    ranking: RankingEngine,
}

impl CandidateEngine {
    pub(crate) fn new(
        language: Arc<dyn LanguageEngine>,
        providers: Vec<Arc<dyn CandidateProvider>>,
        ranking: RankingEngine,
    ) -> Self {
        Self {
            language,
            providers,
            ranking,
        }
    }

    pub(crate) fn generate(
        &self,
        session_id: SessionId,
        revision: StateRevision,
        composition: &str,
        limits: &CoreLimits,
    ) -> Result<CandidateSnapshot, CandidatePipelineFailure> {
        if composition.is_empty() {
            return Ok(CandidateSnapshot::empty(session_id, revision));
        }
        let normalized =
            self.language
                .normalize_input(composition)
                .map_err(|_| CandidatePipelineFailure {
                    component: DegradedComponent::Language,
                })?;
        let parsed =
            self.language
                .parse_input(&normalized)
                .map_err(|_| CandidatePipelineFailure {
                    component: DegradedComponent::Language,
                })?;

        let mut proposals = Vec::with_capacity(limits.max_generated_candidates.min(64));
        for provider in &self.providers {
            let remaining = limits
                .max_generated_candidates
                .saturating_sub(proposals.len());
            if remaining == 0 {
                break;
            }
            let generated =
                provider
                    .generate(&parsed, remaining)
                    .map_err(|_| CandidatePipelineFailure {
                        component: provider.degraded_component(),
                    })?;
            proposals.extend(generated.into_iter().take(remaining));
        }

        let deduplicated = self.deduplicate(proposals);
        let ranked = self
            .ranking
            .rank(deduplicated, limits.max_visible_candidates)
            .map_err(|_| CandidatePipelineFailure {
                component: DegradedComponent::Ranking,
            })?;
        Ok(CandidateSnapshot::from_ranked(session_id, revision, ranked))
    }

    fn deduplicate(&self, proposals: Vec<CandidateProposal>) -> Vec<CandidateProposal> {
        let mut merged: BTreeMap<CandidateIdentity, CandidateProposal> = BTreeMap::new();
        for proposal in proposals {
            let identity = self.language.candidate_identity(&proposal.text);
            match merged.get_mut(&identity) {
                Some(existing) => merge_proposal(existing, proposal),
                None => {
                    merged.insert(identity, proposal);
                }
            }
        }
        merged.into_values().collect()
    }
}

fn merge_proposal(existing: &mut CandidateProposal, incoming: CandidateProposal) {
    let maximum_frequency = existing.base_frequency.max(incoming.base_frequency);
    if canonical_precedes(&incoming, existing) {
        *existing = incoming;
    }
    existing.base_frequency = maximum_frequency;
}

fn canonical_precedes(left: &CandidateProposal, right: &CandidateProposal) -> bool {
    right
        .match_kind
        .cmp(&left.match_kind)
        .then_with(|| right.base_frequency.cmp(&left.base_frequency))
        .then_with(|| left.lexeme_id.cmp(&right.lexeme_id))
        .is_lt()
}

#[derive(Clone, Debug)]
struct SnapshotEntry {
    candidate_id: CandidateId,
    ranked: RankedCandidate,
}

/// Candidates valid only for one Session revision.
#[derive(Clone, Debug)]
pub(crate) struct CandidateSnapshot {
    session_id: SessionId,
    revision: StateRevision,
    entries: Vec<SnapshotEntry>,
    selected_index: Option<usize>,
}

impl CandidateSnapshot {
    pub(crate) fn empty(session_id: SessionId, revision: StateRevision) -> Self {
        Self {
            session_id,
            revision,
            entries: Vec::new(),
            selected_index: None,
        }
    }

    fn from_ranked(
        session_id: SessionId,
        revision: StateRevision,
        ranked: Vec<RankedCandidate>,
    ) -> Self {
        let entries: Vec<_> = ranked
            .into_iter()
            .enumerate()
            .map(|(index, ranked)| SnapshotEntry {
                candidate_id: CandidateId::new(
                    u32::try_from(index.saturating_add(1)).unwrap_or(u32::MAX),
                ),
                ranked,
            })
            .collect();
        let selected_index = (!entries.is_empty()).then_some(0);
        Self {
            session_id,
            revision,
            entries,
            selected_index,
        }
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub(crate) fn is_bound_to(&self, session_id: SessionId, revision: StateRevision) -> bool {
        self.session_id == session_id && self.revision == revision
    }

    pub(crate) const fn revision(&self) -> StateRevision {
        self.revision
    }

    pub(crate) fn rebind_revision(&mut self, revision: StateRevision) {
        self.revision = revision;
    }

    pub(crate) fn views(&self) -> Vec<CandidateView> {
        self.entries
            .iter()
            .map(|entry| CandidateView {
                candidate_id: entry.candidate_id,
                text: entry.ranked.proposal.text.clone(),
            })
            .collect()
    }

    pub(crate) fn selected_candidate_id(&self) -> Option<CandidateId> {
        self.selected_index
            .and_then(|index| self.entries.get(index))
            .map(|entry| entry.candidate_id)
    }

    pub(crate) fn candidate_text(&self, candidate_id: CandidateId) -> Option<&str> {
        self.entries
            .iter()
            .find(|entry| entry.candidate_id == candidate_id)
            .map(|entry| entry.ranked.proposal.text.as_str())
    }

    pub(crate) fn move_selection(&mut self, delta: i32) -> bool {
        let Some(current) = self.selected_index else {
            return false;
        };
        if delta == 0 {
            return false;
        }
        let maximum = self.entries.len().saturating_sub(1) as i64;
        let next = (current as i64 + i64::from(delta)).clamp(0, maximum) as usize;
        if next == current {
            return false;
        }
        self.selected_index = Some(next);
        true
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::{CandidateEngine, CandidateProvider, DictionaryCandidateProvider, MatchKind};
    use crate::{
        CoreLimits, LexemeId, SessionId, StateRevision,
        dictionary::{DictionaryEntry, InMemoryDictionary},
        language::ReferenceLanguageEngine,
        ranking::RankingEngine,
    };

    fn engine() -> CandidateEngine {
        let provider: Arc<dyn CandidateProvider> = Arc::new(DictionaryCandidateProvider::system(
            Arc::new(InMemoryDictionary::new([
                DictionaryEntry::new(LexemeId::new(1), "nihao", "你好", 3000),
                DictionaryEntry::new(LexemeId::new(2), "nihao", "你好", 4500),
                DictionaryEntry::new(LexemeId::new(3), "nihaoma", "你好吗", 100),
                DictionaryEntry::new(LexemeId::new(4), "nihao", "你号", 100),
            ])),
        ));
        CandidateEngine::new(
            Arc::new(ReferenceLanguageEngine),
            vec![provider],
            RankingEngine,
        )
    }

    #[test]
    fn deduplicates_without_summing_frequency_and_assigns_ids() {
        let snapshot = engine()
            .generate(
                SessionId::new(7),
                StateRevision::new(3),
                "NIHAO",
                &CoreLimits::default(),
            )
            .expect("pipeline succeeds");
        assert_eq!(snapshot.entries.len(), 3);
        assert_eq!(snapshot.entries[0].candidate_id.get(), 1);
        assert_eq!(snapshot.entries[0].ranked.proposal.text, "你好");
        assert_eq!(snapshot.entries[0].ranked.proposal.base_frequency, 4500);
        assert_eq!(
            snapshot.entries[0].ranked.proposal.match_kind,
            MatchKind::Exact
        );
        assert_eq!(snapshot.entries[1].ranked.proposal.text, "你号");
        assert_eq!(snapshot.entries[2].ranked.proposal.text, "你好吗");
    }

    #[test]
    fn empty_input_and_visible_limit_are_bounded() {
        let limits = CoreLimits {
            max_visible_candidates: 1,
            ..CoreLimits::default()
        };
        let empty = engine()
            .generate(SessionId::new(1), StateRevision::new(1), "", &limits)
            .expect("pipeline succeeds");
        assert!(empty.is_empty());

        let bounded = engine()
            .generate(SessionId::new(1), StateRevision::new(2), "nihao", &limits)
            .expect("pipeline succeeds");
        assert_eq!(bounded.entries.len(), 1);
    }

    #[test]
    fn repeated_generation_is_deterministic() {
        let first = engine()
            .generate(
                SessionId::new(1),
                StateRevision::new(1),
                "nihao",
                &CoreLimits::default(),
            )
            .expect("pipeline succeeds")
            .views();
        let second = engine()
            .generate(
                SessionId::new(1),
                StateRevision::new(1),
                "nihao",
                &CoreLimits::default(),
            )
            .expect("pipeline succeeds")
            .views();
        assert_eq!(first, second);
    }
}
