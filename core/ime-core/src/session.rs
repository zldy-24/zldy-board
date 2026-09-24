use std::{collections::BTreeMap, sync::Arc};

use crate::{
    ActionId, CandidateId, ContextSnapshot, EngineId, ImeError, InputEvent, MarkedTextSupport,
    ModeId, SessionId, StateRevision,
    candidate::CandidateSnapshot,
    composer::{Composer, ComposerSnapshot},
    engine::EngineInner,
    input::{InputNormalizer, LogicalInput, NormalizedHardwareEvent},
    state::{
        AckOutcome, ActionDisposition, CommitKind, DegradedComponent, EventHandling, ImeAction,
        ImeActionKind, ImeResult, ImeState, ImeStatus, PlatformResetReason, SessionPhase,
    },
};

/// Per-input-context state. Calls on one Session must be serialized by the host.
#[derive(Debug)]
pub struct ImeSession {
    engine: Arc<EngineInner>,
    session_id: SessionId,
    state_revision: StateRevision,
    phase: SessionPhase,
    composer: Composer,
    candidate_snapshot: CandidateSnapshot,
    degraded_components: Vec<DegradedComponent>,
    context: ContextSnapshot,
    mode: ModeId,
    next_action_id: u64,
    input_sequence: u64,
    pending_actions: BTreeMap<ActionId, PendingAction>,
    resolved_actions: BTreeMap<ActionId, ActionDisposition>,
    platform_composition_active: bool,
    reconciliation_required: bool,
}

#[derive(Debug)]
struct PendingAction {
    action: ImeAction,
    recovery: ActionRecovery,
}

#[derive(Debug)]
enum ActionRecovery {
    None,
    Commit(CommitRecovery),
}

#[derive(Debug)]
struct CommitRecovery {
    composer: ComposerSnapshot,
    post_commit_revision: StateRevision,
    context_epoch: u64,
    input_sequence: u64,
}

impl ImeSession {
    pub(crate) fn new(engine: Arc<EngineInner>, session_id: SessionId) -> Self {
        Self {
            engine,
            session_id,
            state_revision: StateRevision::new(0),
            phase: SessionPhase::Idle,
            composer: Composer::new(),
            candidate_snapshot: CandidateSnapshot::empty(session_id, StateRevision::new(0)),
            degraded_components: Vec::new(),
            context: ContextSnapshot::default(),
            mode: ModeId::default(),
            next_action_id: 1,
            input_sequence: 0,
            pending_actions: BTreeMap::new(),
            resolved_actions: BTreeMap::new(),
            platform_composition_active: false,
            reconciliation_required: false,
        }
    }

    /// Returns the Session identity.
    pub const fn id(&self) -> SessionId {
        self.session_id
    }

    /// Returns the shared Engine identity.
    pub fn engine_id(&self) -> EngineId {
        self.engine.engine_id
    }

    /// Returns the immutable resources generation shared with this Session.
    pub fn resource_generation(&self) -> crate::ResourceGeneration {
        self.engine.resource_generation
    }

    /// Returns the current state revision.
    pub const fn state_revision(&self) -> StateRevision {
        self.state_revision
    }

    /// Returns whether a stale failed action requires host/Core reconciliation.
    pub const fn reconciliation_required(&self) -> bool {
        self.reconciliation_required
    }

    /// Returns components currently degraded for candidate generation.
    pub fn degraded_components(&self) -> &[DegradedComponent] {
        &self.degraded_components
    }

    /// Returns a renderable snapshot without processing an event.
    pub fn snapshot(&self) -> ImeState {
        ImeState {
            phase: self.phase,
            composition_utf8: self.composer.text().to_owned(),
            composition_cursor_grapheme: self.composer.cursor_grapheme(),
            composition_cursor_utf8_byte_offset: self.composer.cursor_utf8_byte_offset(),
            candidates: self.candidate_snapshot.views(),
            selected_candidate: self.candidate_snapshot.selected_candidate_id(),
            mode: self.mode,
        }
    }

    /// Processes one portable input event synchronously.
    pub fn process_event(&mut self, event: InputEvent) -> Result<ImeResult, ImeError> {
        self.bump_input_sequence()?;
        self.invalidate_commit_recoveries();

        let outcome = match event {
            InputEvent::InsertText(text) => self.process_logical(LogicalInput::InsertText(text))?,
            InputEvent::Backspace => self.process_logical(LogicalInput::Backspace { count: 1 })?,
            InputEvent::DeleteForward => {
                self.process_logical(LogicalInput::DeleteForward { count: 1 })?
            }
            InputEvent::MoveCompositionCursor { grapheme_delta } => {
                self.process_logical(LogicalInput::MoveCompositionCursor { grapheme_delta })?
            }
            InputEvent::HardwareKey(event) => self.process_hardware_key(event)?,
            InputEvent::Commit => self.process_logical(LogicalInput::Commit)?,
            InputEvent::Cancel => self.process_logical(LogicalInput::Cancel)?,
            InputEvent::SelectCandidate {
                candidate_id,
                state_revision,
            } => self.select_candidate(candidate_id, state_revision)?,
            InputEvent::MoveCandidateSelection { delta } => self.move_candidate_selection(delta)?,
            InputEvent::ContextChanged(context) => self.process_context_changed(context)?,
            InputEvent::Reset => self.process_reset()?,
            InputEvent::NextCandidatePage
            | InputEvent::PreviousCandidatePage
            | InputEvent::SwitchMode(_)
            | InputEvent::FocusChanged(_) => ProcessOutcome::unsupported(),
        };

        Ok(self.result(outcome))
    }

    /// Records a weak platform acknowledgement.
    ///
    /// A failed commit may advance the state revision when its composition is safely restored.
    pub fn acknowledge_action(
        &mut self,
        action_id: ActionId,
        disposition: ActionDisposition,
    ) -> Result<AckOutcome, ImeError> {
        if let Some(pending) = self.pending_actions.remove(&action_id) {
            self.update_platform_composition_state(&pending.action.kind, disposition);
            self.handle_action_recovery(pending, disposition)?;
            self.record_resolved(action_id, disposition);
            return Ok(AckOutcome::Recorded {
                action_id,
                disposition,
            });
        }

        if let Some(existing) = self.resolved_actions.get(&action_id).copied() {
            return Ok(AckOutcome::AlreadyResolved {
                action_id,
                disposition: existing,
            });
        }

        Err(ImeError::UnknownAction(action_id))
    }

    /// Returns the number of actions still waiting for a disposition.
    pub fn outstanding_action_count(&self) -> usize {
        self.pending_actions.len()
    }

    fn process_hardware_key(
        &mut self,
        event: crate::HardwareKeyEvent,
    ) -> Result<ProcessOutcome, ImeError> {
        if !self.engine.config.capabilities.hardware_key_support {
            return Ok(ProcessOutcome::pass_through());
        }

        match InputNormalizer.normalize(event, self.phase != SessionPhase::Idle) {
            NormalizedHardwareEvent::Logical(logical) => self.process_logical(logical),
            NormalizedHardwareEvent::PassThrough => Ok(ProcessOutcome::pass_through()),
        }
    }

    fn process_logical(&mut self, input: LogicalInput) -> Result<ProcessOutcome, ImeError> {
        match input {
            LogicalInput::InsertText(text) => self.insert_text(text),
            LogicalInput::Backspace { count } => self.backspace(count),
            LogicalInput::DeleteForward { count } => self.delete_forward(count),
            LogicalInput::MoveCompositionCursor { grapheme_delta } => {
                self.move_composition_cursor(grapheme_delta)
            }
            LogicalInput::Commit => self.commit(),
            LogicalInput::Cancel => self.cancel(),
        }
    }

    fn insert_text(&mut self, text: String) -> Result<ProcessOutcome, ImeError> {
        if text.is_empty() {
            return Ok(ProcessOutcome::no_op());
        }
        self.validate_insert(&text)?;
        self.prepare_composition_update_action()?;

        self.composer.insert_text(&text);
        self.bump_revision()?;
        self.regenerate_candidates();

        let actions = self.composition_update_action()?;
        Ok(ProcessOutcome::ok(actions))
    }

    fn backspace(&mut self, count: u16) -> Result<ProcessOutcome, ImeError> {
        if self.phase == SessionPhase::Idle {
            self.ensure_action_capacity()?;
            let action =
                self.allocate_action(ImeActionKind::DeleteBackward { operation_count: 1 })?;
            return Ok(ProcessOutcome::ok(vec![action]));
        }

        if self.composer.cursor_grapheme() == 0 {
            return Ok(ProcessOutcome::no_op());
        }

        self.prepare_composition_update_action()?;
        let mut changed = false;
        for _ in 0..count.max(1) {
            changed |= self.composer.backspace();
            if self.composer.cursor_grapheme() == 0 {
                break;
            }
        }
        if !changed {
            return Ok(ProcessOutcome::no_op());
        }

        self.bump_revision()?;
        self.regenerate_candidates();
        let actions = self.composition_update_action()?;
        Ok(ProcessOutcome::ok(actions))
    }

    fn delete_forward(&mut self, count: u16) -> Result<ProcessOutcome, ImeError> {
        if self.phase == SessionPhase::Idle {
            return Ok(ProcessOutcome::no_op());
        }

        if self.composer.cursor_grapheme() >= self.composer.grapheme_count() {
            return Ok(ProcessOutcome::no_op());
        }

        self.prepare_composition_update_action()?;
        let mut changed = false;
        for _ in 0..count.max(1) {
            changed |= self.composer.delete_forward();
            if self.composer.cursor_grapheme() >= self.composer.grapheme_count() {
                break;
            }
        }
        if !changed {
            return Ok(ProcessOutcome::no_op());
        }

        self.bump_revision()?;
        self.regenerate_candidates();
        let actions = self.composition_update_action()?;
        Ok(ProcessOutcome::ok(actions))
    }

    fn move_composition_cursor(&mut self, grapheme_delta: i32) -> Result<ProcessOutcome, ImeError> {
        if self.phase == SessionPhase::Idle || grapheme_delta == 0 {
            return Ok(ProcessOutcome::no_op());
        }

        let current = self.composer.cursor_grapheme() as i64;
        let maximum = self.composer.grapheme_count() as i64;
        let next = (current + i64::from(grapheme_delta)).clamp(0, maximum);
        if next == current {
            return Ok(ProcessOutcome::no_op());
        }

        self.prepare_composition_update_action()?;
        self.composer.move_cursor(grapheme_delta);

        self.bump_revision()?;
        self.regenerate_candidates();
        let actions = self.composition_update_action()?;
        Ok(ProcessOutcome::ok(actions))
    }

    fn commit(&mut self) -> Result<ProcessOutcome, ImeError> {
        if self.phase == SessionPhase::Idle {
            return Ok(ProcessOutcome::no_op());
        }

        let text_utf8 = self.composer.text().to_owned();
        self.commit_text(text_utf8, CommitKind::DirectInput)
    }

    fn select_candidate(
        &mut self,
        candidate_id: CandidateId,
        state_revision: StateRevision,
    ) -> Result<ProcessOutcome, ImeError> {
        if state_revision != self.state_revision {
            return Err(ImeError::StaleRevision {
                expected: self.state_revision,
                actual: state_revision,
            });
        }
        if self.phase != SessionPhase::CandidateSelecting {
            return Ok(ProcessOutcome::unsupported());
        }
        if !self
            .candidate_snapshot
            .is_bound_to(self.session_id, self.state_revision)
        {
            return Err(ImeError::StaleRevision {
                expected: self.state_revision,
                actual: self.candidate_snapshot.revision(),
            });
        }
        let text_utf8 = self
            .candidate_snapshot
            .candidate_text(candidate_id)
            .ok_or(ImeError::UnknownCandidate(candidate_id))?
            .to_owned();
        self.commit_text(text_utf8, CommitKind::Candidate)
    }

    fn move_candidate_selection(&mut self, delta: i32) -> Result<ProcessOutcome, ImeError> {
        if self.phase != SessionPhase::CandidateSelecting {
            return Ok(ProcessOutcome::unsupported());
        }
        if !self.candidate_snapshot.move_selection(delta) {
            return Ok(ProcessOutcome::no_op());
        }
        self.bump_revision()?;
        Ok(ProcessOutcome::ok(Vec::new()))
    }

    fn commit_text(
        &mut self,
        text_utf8: String,
        commit_kind: CommitKind,
    ) -> Result<ProcessOutcome, ImeError> {
        self.supersede_pending_composition_updates();
        self.ensure_action_capacity()?;
        let composer = self.composer.recovery_snapshot();
        self.composer.clear();
        self.clear_candidates();
        self.phase = SessionPhase::Idle;
        self.bump_revision()?;
        let recovery = ActionRecovery::Commit(CommitRecovery {
            composer,
            post_commit_revision: self.state_revision,
            context_epoch: self.context.context_epoch(),
            input_sequence: self.input_sequence,
        });
        let action = self.allocate_action_with_recovery(
            ImeActionKind::CommitText {
                text_utf8,
                commit_kind,
            },
            recovery,
        )?;
        Ok(ProcessOutcome::ok(vec![action]))
    }

    fn cancel(&mut self) -> Result<ProcessOutcome, ImeError> {
        if self.phase == SessionPhase::Idle {
            return Ok(ProcessOutcome::no_op());
        }

        let host_may_have_composition =
            self.platform_composition_active || self.has_pending_composition_update();
        self.supersede_pending_composition_updates();
        if host_may_have_composition {
            self.ensure_action_capacity()?;
        }

        self.composer.clear();
        self.clear_candidates();
        self.phase = SessionPhase::Idle;
        self.bump_revision()?;

        let actions = if self.supports_marked_text() && host_may_have_composition {
            vec![self.allocate_action(ImeActionKind::CancelComposition)?]
        } else {
            Vec::new()
        };
        Ok(ProcessOutcome::ok(actions))
    }

    fn process_context_changed(
        &mut self,
        context: ContextSnapshot,
    ) -> Result<ProcessOutcome, ImeError> {
        let context =
            context.sanitized(&self.engine.config.limits, self.engine.config.capabilities)?;
        if context == self.context {
            return Ok(ProcessOutcome::no_op());
        }
        self.context = context;
        self.reconciliation_required = false;
        self.bump_revision()?;
        Ok(ProcessOutcome::ok(Vec::new()))
    }

    fn process_reset(&mut self) -> Result<ProcessOutcome, ImeError> {
        let host_may_have_composition =
            self.platform_composition_active || self.has_pending_composition_update();
        let state_changed = self.phase != SessionPhase::Idle
            || !self.composer.is_empty()
            || self.context != ContextSnapshot::default()
            || self.reconciliation_required;

        self.supersede_all_pending();
        self.composer.clear();
        self.clear_candidates();
        self.phase = SessionPhase::Idle;
        self.context = ContextSnapshot::default();
        self.reconciliation_required = false;

        if state_changed {
            self.bump_revision()?;
        }

        let actions = if host_may_have_composition {
            self.ensure_action_capacity()?;
            vec![self.allocate_action(ImeActionKind::RequestPlatformReset {
                reason: PlatformResetReason::ExplicitReset,
            })?]
        } else {
            Vec::new()
        };

        if state_changed || !actions.is_empty() {
            Ok(ProcessOutcome::ok(actions))
        } else {
            Ok(ProcessOutcome::no_op())
        }
    }

    fn validate_insert(&self, text: &str) -> Result<(), ImeError> {
        let limits = &self.engine.config.limits;
        if text.len() > limits.max_event_text_bytes {
            return Err(ImeError::EventTextTooLong {
                actual: text.len(),
                max: limits.max_event_text_bytes,
            });
        }
        let composition_size = self.composer.text().len().saturating_add(text.len());
        if composition_size > limits.max_composition_text_bytes {
            return Err(ImeError::CompositionTooLong {
                actual: composition_size,
                max: limits.max_composition_text_bytes,
            });
        }
        Ok(())
    }

    fn regenerate_candidates(&mut self) {
        if self.composer.is_empty() {
            self.clear_candidates();
            self.phase = SessionPhase::Idle;
            return;
        }

        match self.engine.candidate_engine.generate(
            self.session_id,
            self.state_revision,
            self.composer.text(),
            &self.engine.config.limits,
        ) {
            Ok(snapshot) => {
                self.phase = if snapshot.is_empty() {
                    SessionPhase::Composing
                } else {
                    SessionPhase::CandidateSelecting
                };
                self.candidate_snapshot = snapshot;
                self.degraded_components.clear();
            }
            Err(failure) => {
                self.candidate_snapshot =
                    CandidateSnapshot::empty(self.session_id, self.state_revision);
                self.phase = SessionPhase::Composing;
                self.degraded_components = vec![failure.component];
            }
        }
    }

    fn clear_candidates(&mut self) {
        self.candidate_snapshot = CandidateSnapshot::empty(self.session_id, self.state_revision);
        self.degraded_components.clear();
    }

    fn composition_update_action(&mut self) -> Result<Vec<ImeAction>, ImeError> {
        if !self.supports_marked_text() {
            return Ok(Vec::new());
        }

        let kind = if self.composer.is_empty() {
            ImeActionKind::CancelComposition
        } else {
            ImeActionKind::SetCompositionText {
                text_utf8: self.composer.text().to_owned(),
                cursor_utf8_byte_offset: self.composer.cursor_utf8_byte_offset(),
            }
        };
        Ok(vec![self.allocate_action(kind)?])
    }

    fn prepare_composition_update_action(&mut self) -> Result<(), ImeError> {
        if self.supports_marked_text() {
            self.supersede_pending_composition_updates();
            self.ensure_action_capacity()?;
        }
        Ok(())
    }

    fn supports_marked_text(&self) -> bool {
        self.engine.config.capabilities.marked_text_support == MarkedTextSupport::Basic
    }

    fn has_pending_composition_update(&self) -> bool {
        self.pending_actions.values().any(|pending| {
            matches!(
                &pending.action.kind,
                ImeActionKind::SetCompositionText { .. }
            )
        })
    }

    fn supersede_pending_composition_updates(&mut self) {
        let action_ids: Vec<_> = self
            .pending_actions
            .iter()
            .filter_map(|(action_id, pending)| {
                matches!(
                    &pending.action.kind,
                    ImeActionKind::SetCompositionText { .. }
                )
                .then_some(*action_id)
            })
            .collect();
        for action_id in action_ids {
            self.pending_actions.remove(&action_id);
            self.record_resolved(action_id, ActionDisposition::Superseded);
        }
    }

    fn supersede_all_pending(&mut self) {
        let pending = std::mem::take(&mut self.pending_actions);
        for (action_id, _) in pending {
            self.record_resolved(action_id, ActionDisposition::Superseded);
        }
    }

    fn ensure_action_capacity(&self) -> Result<(), ImeError> {
        let max = self.engine.config.limits.max_outstanding_actions;
        if self.pending_actions.len() >= max {
            return Err(ImeError::TooManyOutstandingActions { max });
        }
        Ok(())
    }

    fn allocate_action(&mut self, kind: ImeActionKind) -> Result<ImeAction, ImeError> {
        self.allocate_action_with_recovery(kind, ActionRecovery::None)
    }

    fn allocate_action_with_recovery(
        &mut self,
        kind: ImeActionKind,
        recovery: ActionRecovery,
    ) -> Result<ImeAction, ImeError> {
        self.ensure_action_capacity()?;
        let action_id = ActionId::new(self.next_action_id);
        self.next_action_id = self
            .next_action_id
            .checked_add(1)
            .ok_or(ImeError::CounterExhausted("action_id"))?;
        let action = ImeAction {
            action_id,
            session_id: self.session_id,
            originating_revision: self.state_revision,
            kind,
        };
        self.pending_actions.insert(
            action_id,
            PendingAction {
                action: action.clone(),
                recovery,
            },
        );
        Ok(action)
    }

    fn record_resolved(&mut self, action_id: ActionId, disposition: ActionDisposition) {
        self.resolved_actions.insert(action_id, disposition);
        let max = self.engine.config.limits.max_resolved_action_history;
        while self.resolved_actions.len() > max {
            self.resolved_actions.pop_first();
        }
    }

    fn handle_action_recovery(
        &mut self,
        pending: PendingAction,
        disposition: ActionDisposition,
    ) -> Result<(), ImeError> {
        if !matches!(pending.action.kind, ImeActionKind::CommitText { .. }) {
            return Ok(());
        }

        match disposition {
            ActionDisposition::Applied | ActionDisposition::Superseded => Ok(()),
            ActionDisposition::Rejected
            | ActionDisposition::Unavailable
            | ActionDisposition::Failed => match pending.recovery {
                ActionRecovery::Commit(recovery) if self.can_restore_commit(&recovery) => {
                    self.bump_revision()?;
                    self.composer.restore(recovery.composer);
                    self.regenerate_candidates();
                    Ok(())
                }
                ActionRecovery::None | ActionRecovery::Commit(_) => {
                    self.reconciliation_required = true;
                    Ok(())
                }
            },
        }
    }

    fn can_restore_commit(&self, recovery: &CommitRecovery) -> bool {
        !self.reconciliation_required
            && self.input_sequence == recovery.input_sequence
            && self.state_revision == recovery.post_commit_revision
            && self.context.context_epoch() == recovery.context_epoch
            && self.phase == SessionPhase::Idle
            && self.composer.is_empty()
    }

    fn invalidate_commit_recoveries(&mut self) {
        for pending in self.pending_actions.values_mut() {
            if matches!(&pending.recovery, ActionRecovery::Commit(_)) {
                pending.recovery = ActionRecovery::None;
            }
        }
    }

    fn update_platform_composition_state(
        &mut self,
        kind: &ImeActionKind,
        disposition: ActionDisposition,
    ) {
        if disposition != ActionDisposition::Applied {
            return;
        }
        match kind {
            ImeActionKind::SetCompositionText { .. } => {
                self.platform_composition_active = true;
            }
            ImeActionKind::FinishComposition
            | ImeActionKind::CancelComposition
            | ImeActionKind::CommitText { .. }
            | ImeActionKind::RequestPlatformReset { .. } => {
                self.platform_composition_active = false;
                if matches!(kind, ImeActionKind::RequestPlatformReset { .. }) {
                    self.reconciliation_required = false;
                }
            }
            ImeActionKind::DeleteBackward { .. } => {}
        }
    }

    fn bump_input_sequence(&mut self) -> Result<(), ImeError> {
        self.input_sequence = self
            .input_sequence
            .checked_add(1)
            .ok_or(ImeError::CounterExhausted("input_sequence"))?;
        Ok(())
    }

    fn bump_revision(&mut self) -> Result<(), ImeError> {
        let next = self
            .state_revision
            .get()
            .checked_add(1)
            .ok_or(ImeError::CounterExhausted("state_revision"))?;
        self.state_revision = StateRevision::new(next);
        self.candidate_snapshot.rebind_revision(self.state_revision);
        Ok(())
    }

    fn result(&self, outcome: ProcessOutcome) -> ImeResult {
        ImeResult {
            status: outcome.status,
            event_handling: outcome.event_handling,
            session_id: self.session_id,
            state_revision: self.state_revision,
            resource_generation: self.engine.resource_generation,
            state: self.snapshot(),
            actions: outcome.actions,
            degraded_components: self.degraded_components.clone(),
            result_flags: Vec::new(),
        }
    }
}

#[derive(Debug)]
struct ProcessOutcome {
    status: ImeStatus,
    event_handling: EventHandling,
    actions: Vec<ImeAction>,
}

impl ProcessOutcome {
    fn ok(actions: Vec<ImeAction>) -> Self {
        Self {
            status: ImeStatus::Ok,
            event_handling: EventHandling::Consumed,
            actions,
        }
    }

    fn no_op() -> Self {
        Self {
            status: ImeStatus::NoOp,
            event_handling: EventHandling::Consumed,
            actions: Vec::new(),
        }
    }

    fn unsupported() -> Self {
        Self {
            status: ImeStatus::Unsupported,
            event_handling: EventHandling::Consumed,
            actions: Vec::new(),
        }
    }

    fn pass_through() -> Self {
        Self {
            status: ImeStatus::NoOp,
            event_handling: EventHandling::PassThrough,
            actions: Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use crate::{
        ActionDisposition, CandidateId, ContextSnapshot, CoreLimits, EngineConfig, EventHandling,
        FocusChange, HardwareKeyEvent, ImeActionKind, ImeEngine, ImeError, ImeStatus, InputEvent,
        InputScope, KeyPhase, LexemeId, LogicalKey, MarkedTextSupport, Modifiers,
        PlatformCapabilities, SessionPhase, SurroundingTextSupport,
        candidate::{CandidateProposal, CandidateProvider},
        dictionary::{DictionaryEntry, InMemoryDictionary},
        language::{ParsedInput, ReferenceLanguageEngine},
        ranking::RankingEngine,
        state::DegradedComponent,
    };

    fn engine_with_hardware() -> ImeEngine {
        ImeEngine::new(EngineConfig {
            capabilities: PlatformCapabilities {
                hardware_key_support: true,
                ..PlatformCapabilities::default()
            },
            ..EngineConfig::default()
        })
        .expect("test config is valid")
    }

    fn default_engine() -> ImeEngine {
        ImeEngine::new(EngineConfig::default()).expect("default test config is valid")
    }

    fn engine_with_candidates() -> ImeEngine {
        ImeEngine::with_reference_dictionary(
            EngineConfig::default(),
            InMemoryDictionary::new([
                DictionaryEntry::new(LexemeId::new(1), "ni", "你", 1000),
                DictionaryEntry::new(LexemeId::new(2), "nih", "你好", 100),
                DictionaryEntry::new(LexemeId::new(3), "nihao", "你好", 3000),
                DictionaryEntry::new(LexemeId::new(4), "nihao", "你号", 100),
                DictionaryEntry::new(LexemeId::new(5), "nihaoma", "你好吗", 500),
            ]),
        )
        .expect("candidate test config is valid")
    }

    #[derive(Debug)]
    struct FailingProvider;

    impl CandidateProvider for FailingProvider {
        fn generate(
            &self,
            _input: &ParsedInput,
            _limit: usize,
        ) -> Result<Vec<CandidateProposal>, ImeError> {
            Err(ImeError::InvalidConfig("injected candidate failure"))
        }

        fn degraded_component(&self) -> DegradedComponent {
            DegradedComponent::Candidate
        }
    }

    fn engine_with_marked_text() -> ImeEngine {
        ImeEngine::new(EngineConfig {
            capabilities: PlatformCapabilities {
                marked_text_support: MarkedTextSupport::Basic,
                ..PlatformCapabilities::default()
            },
            ..EngineConfig::default()
        })
        .expect("test config is valid")
    }

    fn hardware(logical_key: LogicalKey) -> HardwareKeyEvent {
        HardwareKeyEvent {
            physical_code: 0,
            logical_key,
            modifiers: Modifiers::empty(),
            phase: KeyPhase::Down,
            repeat_count: 1,
        }
    }

    #[test]
    fn state_machine_insert_commit_and_cancel() {
        let engine = default_engine();
        let mut session = engine.new_session().expect("session id is available");

        let inserted = session
            .process_event(InputEvent::InsertText("abc".into()))
            .expect("insert succeeds");
        assert_eq!(inserted.state.phase, SessionPhase::Composing);
        assert_eq!(inserted.state.composition_utf8, "abc");

        let committed = session
            .process_event(InputEvent::Commit)
            .expect("commit succeeds");
        assert_eq!(committed.state.phase, SessionPhase::Idle);
        assert!(matches!(
            committed.actions.as_slice(),
            [crate::ImeAction {
                kind: ImeActionKind::CommitText { .. },
                ..
            }]
        ));

        session
            .process_event(InputEvent::InsertText("x".into()))
            .expect("insert succeeds");
        let cancelled = session
            .process_event(InputEvent::Cancel)
            .expect("cancel succeeds");
        assert_eq!(cancelled.state.phase, SessionPhase::Idle);
        assert!(cancelled.state.composition_utf8.is_empty());
    }

    #[test]
    fn deleting_last_grapheme_returns_to_idle() {
        let engine = default_engine();
        let mut session = engine.new_session().expect("session id is available");
        session
            .process_event(InputEvent::InsertText("😀".into()))
            .expect("insert succeeds");

        let result = session
            .process_event(InputEvent::Backspace)
            .expect("backspace succeeds");
        assert_eq!(result.state.phase, SessionPhase::Idle);
        assert!(result.state.composition_utf8.is_empty());
    }

    #[test]
    fn reset_clears_state() {
        let engine = default_engine();
        let mut session = engine.new_session().expect("session id is available");
        session
            .process_event(InputEvent::InsertText("abc".into()))
            .expect("insert succeeds");

        let result = session
            .process_event(InputEvent::Reset)
            .expect("reset succeeds");
        assert_eq!(result.state.phase, SessionPhase::Idle);
        assert!(result.state.composition_utf8.is_empty());
    }

    #[test]
    fn hardware_event_handling_obeys_composition_state() {
        let engine = engine_with_hardware();
        let mut session = engine.new_session().expect("session id is available");

        for key in [
            LogicalKey::Unknown,
            LogicalKey::Enter,
            LogicalKey::Backspace,
        ] {
            let result = session
                .process_event(InputEvent::HardwareKey(hardware(key)))
                .expect("hardware event succeeds");
            assert_eq!(result.event_handling, EventHandling::PassThrough);
        }

        let inserted = session
            .process_event(InputEvent::HardwareKey(hardware(LogicalKey::Character(
                'a',
            ))))
            .expect("character succeeds");
        assert_eq!(inserted.event_handling, EventHandling::Consumed);

        let committed = session
            .process_event(InputEvent::HardwareKey(hardware(LogicalKey::Enter)))
            .expect("enter succeeds");
        assert_eq!(committed.event_handling, EventHandling::Consumed);
        assert!(matches!(
            committed.actions[0].kind,
            ImeActionKind::CommitText { .. }
        ));
    }

    #[test]
    fn logical_backspace_is_consumed_when_idle() {
        let engine = default_engine();
        let mut session = engine.new_session().expect("session id is available");
        let result = session
            .process_event(InputEvent::Backspace)
            .expect("logical backspace succeeds");

        assert_eq!(result.event_handling, EventHandling::Consumed);
        assert!(matches!(
            result.actions[0].kind,
            ImeActionKind::DeleteBackward { operation_count: 1 }
        ));
    }

    #[test]
    fn revision_changes_only_when_session_state_changes() {
        let engine = engine_with_hardware();
        let mut session = engine.new_session().expect("session id is available");

        let pass = session
            .process_event(InputEvent::HardwareKey(hardware(LogicalKey::Unknown)))
            .expect("unknown key succeeds");
        assert_eq!(pass.state_revision.get(), 0);

        let insert = session
            .process_event(InputEvent::InsertText("a".into()))
            .expect("insert succeeds");
        assert_eq!(insert.state_revision.get(), 1);

        let no_op = session
            .process_event(InputEvent::MoveCompositionCursor { grapheme_delta: 9 })
            .expect("clamped movement succeeds");
        assert_eq!(no_op.status, ImeStatus::NoOp);
        assert_eq!(no_op.state_revision.get(), 1);
    }

    #[test]
    fn action_ids_are_monotonic_and_duplicate_ack_is_idempotent() {
        let engine = default_engine();
        let mut session = engine.new_session().expect("session id is available");

        let first = session
            .process_event(InputEvent::Backspace)
            .expect("first action succeeds")
            .actions
            .remove(0);
        let second = session
            .process_event(InputEvent::Backspace)
            .expect("second action succeeds")
            .actions
            .remove(0);
        assert_eq!(first.action_id.get(), 1);
        assert_eq!(second.action_id.get(), 2);

        let recorded = session
            .acknowledge_action(first.action_id, ActionDisposition::Applied)
            .expect("ack succeeds");
        let duplicate = session
            .acknowledge_action(first.action_id, ActionDisposition::Failed)
            .expect("duplicate ack is safe");
        assert!(matches!(recorded, crate::AckOutcome::Recorded { .. }));
        assert!(matches!(
            duplicate,
            crate::AckOutcome::AlreadyResolved {
                disposition: ActionDisposition::Applied,
                ..
            }
        ));
    }

    #[test]
    fn applied_commit_discards_recovery_and_stays_idle() {
        let engine = default_engine();
        let mut session = engine.new_session().expect("session id is available");
        session
            .process_event(InputEvent::InsertText("nihao".into()))
            .expect("insert succeeds");
        let action = session
            .process_event(InputEvent::Commit)
            .expect("commit succeeds")
            .actions
            .remove(0);
        let committed_revision = session.state_revision();

        let outcome = session
            .acknowledge_action(action.action_id, ActionDisposition::Applied)
            .expect("commit acknowledgement succeeds");
        assert!(matches!(outcome, crate::AckOutcome::Recorded { .. }));
        assert_eq!(session.snapshot().phase, SessionPhase::Idle);
        assert!(session.snapshot().composition_utf8.is_empty());
        assert_eq!(session.outstanding_action_count(), 0);
        assert_eq!(session.state_revision(), committed_revision);

        let duplicate = session
            .acknowledge_action(action.action_id, ActionDisposition::Applied)
            .expect("duplicate acknowledgement is idempotent");
        assert!(matches!(
            duplicate,
            crate::AckOutcome::AlreadyResolved {
                disposition: ActionDisposition::Applied,
                ..
            }
        ));
        assert_eq!(session.state_revision(), committed_revision);
    }

    #[test]
    fn failed_commit_restores_text_and_grapheme_cursor() {
        let engine = default_engine();
        let mut session = engine.new_session().expect("session id is available");
        session
            .process_event(InputEvent::InsertText("nihao".into()))
            .expect("insert succeeds");
        session
            .process_event(InputEvent::MoveCompositionCursor { grapheme_delta: -2 })
            .expect("cursor move succeeds");
        let action = session
            .process_event(InputEvent::Commit)
            .expect("commit succeeds")
            .actions
            .remove(0);
        let post_commit_revision = action.originating_revision;
        assert_eq!(session.state_revision(), post_commit_revision);

        session
            .acknowledge_action(action.action_id, ActionDisposition::Failed)
            .expect("failed acknowledgement is recorded");
        let state = session.snapshot();
        assert_eq!(state.phase, SessionPhase::Composing);
        assert_eq!(state.composition_utf8, "nihao");
        assert_eq!(state.composition_cursor_grapheme, 3);
        assert_eq!(
            session.state_revision().get(),
            post_commit_revision.get() + 1
        );
        assert!(!session.reconciliation_required());
    }

    #[test]
    fn rejected_commit_restores_composition() {
        let engine = default_engine();
        let mut session = engine.new_session().expect("session id is available");
        session
            .process_event(InputEvent::InsertText("nihao".into()))
            .expect("insert succeeds");
        let action = session
            .process_event(InputEvent::Commit)
            .expect("commit succeeds")
            .actions
            .remove(0);

        session
            .acknowledge_action(action.action_id, ActionDisposition::Rejected)
            .expect("rejected acknowledgement is recorded");
        assert_eq!(session.snapshot().phase, SessionPhase::Composing);
        assert_eq!(session.snapshot().composition_utf8, "nihao");
    }

    #[test]
    fn unavailable_commit_restores_composition_without_forcing_reset() {
        let engine = default_engine();
        let mut session = engine.new_session().expect("session id is available");
        session
            .process_event(InputEvent::InsertText("nihao".into()))
            .expect("insert succeeds");
        let action = session
            .process_event(InputEvent::Commit)
            .expect("commit succeeds")
            .actions
            .remove(0);

        session
            .acknowledge_action(action.action_id, ActionDisposition::Unavailable)
            .expect("unavailable acknowledgement is recorded");
        assert_eq!(session.snapshot().phase, SessionPhase::Composing);
        assert_eq!(session.snapshot().composition_utf8, "nihao");
        assert!(!session.reconciliation_required());
    }

    #[test]
    fn superseded_commit_does_not_restore_composition() {
        let engine = default_engine();
        let mut session = engine.new_session().expect("session id is available");
        session
            .process_event(InputEvent::InsertText("nihao".into()))
            .expect("insert succeeds");
        let action = session
            .process_event(InputEvent::Commit)
            .expect("commit succeeds")
            .actions
            .remove(0);

        session
            .acknowledge_action(action.action_id, ActionDisposition::Superseded)
            .expect("superseded acknowledgement is recorded");
        assert_eq!(session.snapshot().phase, SessionPhase::Idle);
        assert!(session.snapshot().composition_utf8.is_empty());
        assert!(!session.reconciliation_required());
    }

    #[test]
    fn stale_commit_failure_never_overwrites_new_input() {
        let engine = default_engine();
        let mut session = engine.new_session().expect("session id is available");
        session
            .process_event(InputEvent::InsertText("abc".into()))
            .expect("first insert succeeds");
        let action = session
            .process_event(InputEvent::Commit)
            .expect("commit succeeds")
            .actions
            .remove(0);
        session
            .process_event(InputEvent::InsertText("x".into()))
            .expect("new input succeeds");

        session
            .acknowledge_action(action.action_id, ActionDisposition::Failed)
            .expect("stale failure is recorded");
        let state = session.snapshot();
        assert_eq!(state.phase, SessionPhase::Composing);
        assert_eq!(state.composition_utf8, "x");
        assert!(session.reconciliation_required());
    }

    #[test]
    fn focus_change_makes_commit_recovery_stale() {
        let engine = default_engine();
        let mut session = engine.new_session().expect("session id is available");
        session
            .process_event(InputEvent::InsertText("abc".into()))
            .expect("insert succeeds");
        let action = session
            .process_event(InputEvent::Commit)
            .expect("commit succeeds")
            .actions
            .remove(0);
        session
            .process_event(InputEvent::FocusChanged(FocusChange {
                focused: false,
                context_epoch: 1,
            }))
            .expect("focus notification is handled structurally");

        session
            .acknowledge_action(action.action_id, ActionDisposition::Failed)
            .expect("stale failure is recorded");
        assert_eq!(session.snapshot().phase, SessionPhase::Idle);
        assert!(session.snapshot().composition_utf8.is_empty());
        assert!(session.reconciliation_required());

        let stale_revision = session.state_revision();
        session
            .process_event(InputEvent::Reset)
            .expect("reset clears reconciliation marker");
        assert!(!session.reconciliation_required());
        assert_eq!(session.state_revision().get(), stale_revision.get() + 1);
    }

    #[test]
    fn context_change_makes_commit_recovery_stale() {
        let engine = default_engine();
        let mut session = engine.new_session().expect("session id is available");
        session
            .process_event(InputEvent::InsertText("abc".into()))
            .expect("insert succeeds");
        let action = session
            .process_event(InputEvent::Commit)
            .expect("commit succeeds")
            .actions
            .remove(0);
        session
            .process_event(InputEvent::ContextChanged(ContextSnapshot::empty(
                InputScope::Normal,
                1,
            )))
            .expect("context change succeeds");

        session
            .acknowledge_action(action.action_id, ActionDisposition::Rejected)
            .expect("stale rejection is recorded");
        assert_eq!(session.snapshot().phase, SessionPhase::Idle);
        assert!(session.snapshot().composition_utf8.is_empty());
        assert!(session.reconciliation_required());
    }

    #[test]
    fn reset_supersedes_pending_commit_recovery() {
        let engine = default_engine();
        let mut session = engine.new_session().expect("session id is available");
        session
            .process_event(InputEvent::InsertText("abc".into()))
            .expect("insert succeeds");
        let action = session
            .process_event(InputEvent::Commit)
            .expect("commit succeeds")
            .actions
            .remove(0);

        session
            .process_event(InputEvent::Reset)
            .expect("reset succeeds");
        let outcome = session
            .acknowledge_action(action.action_id, ActionDisposition::Failed)
            .expect("old commit remains known as superseded");
        assert!(matches!(
            outcome,
            crate::AckOutcome::AlreadyResolved {
                disposition: ActionDisposition::Superseded,
                ..
            }
        ));
        assert_eq!(session.snapshot().phase, SessionPhase::Idle);
        assert!(session.snapshot().composition_utf8.is_empty());
        assert!(!session.reconciliation_required());
    }

    #[test]
    fn duplicate_failed_ack_restores_only_once() {
        let engine = default_engine();
        let mut session = engine.new_session().expect("session id is available");
        session
            .process_event(InputEvent::InsertText("abc".into()))
            .expect("insert succeeds");
        let action = session
            .process_event(InputEvent::Commit)
            .expect("commit succeeds")
            .actions
            .remove(0);
        session
            .acknowledge_action(action.action_id, ActionDisposition::Failed)
            .expect("first failure is recorded");
        let restored_revision = session.state_revision();

        let duplicate = session
            .acknowledge_action(action.action_id, ActionDisposition::Failed)
            .expect("duplicate failure is idempotent");
        assert!(matches!(
            duplicate,
            crate::AckOutcome::AlreadyResolved {
                disposition: ActionDisposition::Failed,
                ..
            }
        ));
        assert_eq!(session.state_revision(), restored_revision);
        assert_eq!(session.snapshot().composition_utf8, "abc");
    }

    #[test]
    fn pending_commit_recovery_obeys_outstanding_action_limit() {
        let engine = ImeEngine::new(EngineConfig {
            limits: CoreLimits {
                max_outstanding_actions: 2,
                ..CoreLimits::default()
            },
            ..EngineConfig::default()
        })
        .expect("test config is valid");
        let mut session = engine.new_session().expect("session id is available");

        for text in ["a", "b"] {
            session
                .process_event(InputEvent::InsertText(text.into()))
                .expect("insert succeeds");
            session
                .process_event(InputEvent::Commit)
                .expect("bounded commit succeeds");
        }
        session
            .process_event(InputEvent::InsertText("c".into()))
            .expect("third insert succeeds");

        assert_eq!(
            session.process_event(InputEvent::Commit),
            Err(ImeError::TooManyOutstandingActions { max: 2 })
        );
        assert_eq!(session.outstanding_action_count(), 2);
        assert_eq!(session.snapshot().composition_utf8, "c");
    }

    #[test]
    fn failed_set_composition_ack_does_not_mutate_composer() {
        let engine = engine_with_marked_text();
        let mut session = engine.new_session().expect("session id is available");
        let action = session
            .process_event(InputEvent::InsertText("a".into()))
            .expect("insert succeeds")
            .actions
            .remove(0);

        session
            .acknowledge_action(action.action_id, ActionDisposition::Failed)
            .expect("failure is recorded");
        assert_eq!(session.snapshot().composition_utf8, "a");
        assert_eq!(session.outstanding_action_count(), 0);

        let duplicate = session
            .acknowledge_action(action.action_id, ActionDisposition::Applied)
            .expect("duplicate acknowledgement is idempotent");
        assert!(matches!(
            duplicate,
            crate::AckOutcome::AlreadyResolved {
                disposition: ActionDisposition::Failed,
                ..
            }
        ));
        assert_eq!(session.snapshot().composition_utf8, "a");
    }

    #[test]
    fn marked_text_action_uses_utf8_byte_cursor() {
        let engine = engine_with_marked_text();
        let mut session = engine.new_session().expect("session id is available");
        let result = session
            .process_event(InputEvent::InsertText("😀".into()))
            .expect("insert succeeds");

        assert!(matches!(
            result.actions.as_slice(),
            [crate::ImeAction {
                kind: ImeActionKind::SetCompositionText {
                    text_utf8,
                    cursor_utf8_byte_offset: 4,
                },
                ..
            }] if text_utf8 == "😀"
        ));
    }

    #[test]
    fn acknowledged_platform_composition_is_cancelled_explicitly() {
        let engine = engine_with_marked_text();
        let mut session = engine.new_session().expect("session id is available");
        let set_action = session
            .process_event(InputEvent::InsertText("a".into()))
            .expect("insert succeeds")
            .actions
            .remove(0);
        session
            .acknowledge_action(set_action.action_id, ActionDisposition::Applied)
            .expect("set composition ack succeeds");

        let cancelled = session
            .process_event(InputEvent::Cancel)
            .expect("cancel succeeds");
        assert!(matches!(
            cancelled.actions.as_slice(),
            [crate::ImeAction {
                kind: ImeActionKind::CancelComposition,
                ..
            }]
        ));
    }

    #[test]
    fn event_and_composition_limits_are_structured_errors() {
        let engine = ImeEngine::new(EngineConfig {
            limits: CoreLimits {
                max_event_text_bytes: 4,
                max_composition_text_bytes: 4,
                ..CoreLimits::default()
            },
            ..EngineConfig::default()
        })
        .expect("test config is valid");
        let mut session = engine.new_session().expect("session id is available");

        assert_eq!(
            session.process_event(InputEvent::InsertText("12345".into())),
            Err(ImeError::EventTextTooLong { actual: 5, max: 4 })
        );
        session
            .process_event(InputEvent::InsertText("1234".into()))
            .expect("maximum-size event succeeds");
        assert_eq!(
            session.process_event(InputEvent::InsertText("x".into())),
            Err(ImeError::CompositionTooLong { actual: 5, max: 4 })
        );
        assert_eq!(session.state_revision().get(), 1);
    }

    #[test]
    fn context_capabilities_and_limits_are_enforced() {
        let engine = ImeEngine::new(EngineConfig {
            limits: CoreLimits {
                max_context_text_bytes: 3,
                ..CoreLimits::default()
            },
            capabilities: PlatformCapabilities {
                surrounding_text_support: SurroundingTextSupport::Bidirectional,
                selected_text_read_support: true,
                ..PlatformCapabilities::default()
            },
        })
        .expect("test config is valid");
        let mut session = engine.new_session().expect("session id is available");
        let context = ContextSnapshot::new("1234", "", "", InputScope::Normal, 1);

        assert_eq!(
            session.process_event(InputEvent::ContextChanged(context)),
            Err(ImeError::ContextTextTooLong {
                field: "before_cursor_utf8",
                actual: 4,
                max: 3,
            })
        );
        assert_eq!(session.state_revision().get(), 0);
    }

    #[test]
    fn command_modified_hardware_character_passes_through() {
        let engine = engine_with_hardware();
        let mut session = engine.new_session().expect("session id is available");
        let mut event = hardware(LogicalKey::Character('c'));
        event.modifiers = Modifiers::CONTROL;

        let result = session
            .process_event(InputEvent::HardwareKey(event))
            .expect("modified key succeeds");
        assert_eq!(result.event_handling, EventHandling::PassThrough);
        assert!(result.actions.is_empty());
        assert_eq!(result.state_revision.get(), 0);
    }

    #[test]
    fn unknown_ack_is_an_error() {
        let engine = default_engine();
        let mut session = engine.new_session().expect("session id is available");

        assert_eq!(
            session.acknowledge_action(crate::ActionId::new(99), ActionDisposition::Applied),
            Err(ImeError::UnknownAction(crate::ActionId::new(99)))
        );
    }

    #[test]
    fn reset_supersedes_pending_action() {
        let engine = engine_with_marked_text();
        let mut session = engine.new_session().expect("session id is available");
        let action = session
            .process_event(InputEvent::InsertText("a".into()))
            .expect("insert succeeds")
            .actions
            .remove(0);

        session
            .process_event(InputEvent::Reset)
            .expect("reset succeeds");
        let outcome = session
            .acknowledge_action(action.action_id, ActionDisposition::Applied)
            .expect("superseded action remains known");
        assert!(matches!(
            outcome,
            crate::AckOutcome::AlreadyResolved {
                disposition: ActionDisposition::Superseded,
                ..
            }
        ));
    }

    #[test]
    fn sensitive_context_is_discarded() {
        for scope in [InputScope::Password, InputScope::Sensitive] {
            let engine = default_engine();
            let mut session = engine.new_session().expect("session id is available");
            let context =
                ContextSnapshot::new("secret-before", "secret-selected", "secret-after", scope, 1);

            session
                .process_event(InputEvent::ContextChanged(context))
                .expect("sensitive context is accepted and discarded");
            assert!(session.context.before_cursor_utf8().is_empty());
            assert!(session.context.selected_text_utf8().is_empty());
            assert!(session.context.after_cursor_utf8().is_empty());
        }
    }

    #[test]
    fn unknown_scope_uses_conservative_policy() {
        assert!(!InputScope::Unknown.permits_persistent_learning());
        assert!(!InputScope::Unknown.permits_private_history_prediction());

        let engine = default_engine();
        let mut session = engine.new_session().expect("session id is available");
        let context =
            ContextSnapshot::new("private", "selection", "context", InputScope::Unknown, 1);
        session
            .process_event(InputEvent::ContextChanged(context))
            .expect("unknown context is accepted conservatively");
        assert!(session.context.before_cursor_utf8().is_empty());
    }

    #[test]
    fn sessions_are_isolated_and_share_engine_identity() {
        let engine = default_engine();
        let mut first = engine.new_session().expect("first session id is available");
        let second = engine
            .new_session()
            .expect("second session id is available");

        first
            .process_event(InputEvent::InsertText("first".into()))
            .expect("insert succeeds");

        assert_ne!(first.id(), second.id());
        assert_eq!(first.engine_id(), second.engine_id());
        assert_eq!(first.snapshot().composition_utf8, "first");
        assert!(second.snapshot().composition_utf8.is_empty());
        assert_eq!(
            engine.resource_generation(),
            crate::ResourceGeneration::new(0)
        );
    }

    #[test]
    fn engine_handle_can_drop_before_session() {
        let mut session = {
            let engine = default_engine();
            engine.new_session().expect("session id is available")
        };

        let result = session
            .process_event(InputEvent::InsertText("still alive".into()))
            .expect("session retains EngineInner");
        assert_eq!(result.state.composition_utf8, "still alive");
    }

    #[test]
    fn typing_generates_deterministic_case_normalized_candidates() {
        let engine = engine_with_candidates();
        let mut lowercase = engine.new_session().expect("session id is available");
        let mut uppercase = engine.new_session().expect("session id is available");

        let lower = lowercase
            .process_event(InputEvent::InsertText("nihao".into()))
            .expect("lowercase insert succeeds");
        let upper = uppercase
            .process_event(InputEvent::InsertText("NIHAO".into()))
            .expect("uppercase insert succeeds");

        assert_eq!(lower.state.phase, SessionPhase::CandidateSelecting);
        assert_eq!(upper.state.phase, SessionPhase::CandidateSelecting);
        let lower_text: Vec<_> = lower
            .state
            .candidates
            .iter()
            .map(|candidate| candidate.text.as_str())
            .collect();
        let upper_text: Vec<_> = upper
            .state
            .candidates
            .iter()
            .map(|candidate| candidate.text.as_str())
            .collect();
        assert_eq!(lower_text, upper_text);
        assert_eq!(lower_text, ["你好", "你号", "你好吗"]);
    }

    #[test]
    fn editing_regenerates_candidates_and_rejects_old_revision() {
        let engine = engine_with_candidates();
        let mut session = engine.new_session().expect("session id is available");
        let first = session
            .process_event(InputEvent::InsertText("ni".into()))
            .expect("first insert succeeds");
        let old_revision = first.state_revision;

        let edited = session
            .process_event(InputEvent::InsertText("hao".into()))
            .expect("editing succeeds");
        assert_ne!(edited.state_revision, old_revision);
        assert_eq!(edited.state.candidates[0].text, "你好");

        let stale = session.process_event(InputEvent::SelectCandidate {
            candidate_id: CandidateId::new(1),
            state_revision: old_revision,
        });
        assert!(matches!(stale, Err(ImeError::StaleRevision { .. })));
    }

    #[test]
    fn candidate_selection_commits_candidate_and_applied_finishes_idle() {
        let engine = engine_with_candidates();
        let mut session = engine.new_session().expect("session id is available");
        let generated = session
            .process_event(InputEvent::InsertText("nihao".into()))
            .expect("insert succeeds");
        let committed = session
            .process_event(InputEvent::SelectCandidate {
                candidate_id: CandidateId::new(1),
                state_revision: generated.state_revision,
            })
            .expect("selection succeeds");

        assert!(matches!(
            &committed.actions[0].kind,
            ImeActionKind::CommitText {
                text_utf8,
                commit_kind: crate::CommitKind::Candidate,
            } if text_utf8 == "你好"
        ));
        assert_eq!(committed.state.phase, SessionPhase::Idle);
        assert!(committed.state.candidates.is_empty());

        session
            .acknowledge_action(committed.actions[0].action_id, ActionDisposition::Applied)
            .expect("candidate commit acknowledgement succeeds");
        assert_eq!(session.snapshot().phase, SessionPhase::Idle);
        assert!(session.snapshot().candidates.is_empty());
    }

    #[test]
    fn failed_candidate_commit_restores_raw_input_with_new_snapshot() {
        let engine = engine_with_candidates();
        let mut session = engine.new_session().expect("session id is available");
        let generated = session
            .process_event(InputEvent::InsertText("nihao".into()))
            .expect("insert succeeds");
        let old_revision = generated.state_revision;
        let action = session
            .process_event(InputEvent::SelectCandidate {
                candidate_id: CandidateId::new(1),
                state_revision: old_revision,
            })
            .expect("selection succeeds")
            .actions
            .remove(0);
        let post_commit_revision = session.state_revision();

        session
            .acknowledge_action(action.action_id, ActionDisposition::Failed)
            .expect("failed commit is recovered");
        let restored = session.snapshot();
        assert_eq!(restored.phase, SessionPhase::CandidateSelecting);
        assert_eq!(restored.composition_utf8, "nihao");
        assert_eq!(restored.candidates[0].candidate_id, CandidateId::new(1));
        assert!(session.state_revision() > post_commit_revision);

        let stale = session.process_event(InputEvent::SelectCandidate {
            candidate_id: CandidateId::new(1),
            state_revision: old_revision,
        });
        assert!(matches!(stale, Err(ImeError::StaleRevision { .. })));
    }

    #[test]
    fn stale_candidate_commit_failure_does_not_overwrite_new_input() {
        let engine = engine_with_candidates();
        let mut session = engine.new_session().expect("session id is available");
        let generated = session
            .process_event(InputEvent::InsertText("nihao".into()))
            .expect("insert succeeds");
        let action = session
            .process_event(InputEvent::SelectCandidate {
                candidate_id: CandidateId::new(1),
                state_revision: generated.state_revision,
            })
            .expect("selection succeeds")
            .actions
            .remove(0);
        session
            .process_event(InputEvent::InsertText("x".into()))
            .expect("new input succeeds");

        session
            .acknowledge_action(action.action_id, ActionDisposition::Failed)
            .expect("stale failure is recorded");
        assert_eq!(session.snapshot().composition_utf8, "x");
        assert!(session.reconciliation_required());
    }

    #[test]
    fn raw_commit_remains_direct_input_when_candidates_exist() {
        let engine = engine_with_candidates();
        let mut session = engine.new_session().expect("session id is available");
        session
            .process_event(InputEvent::InsertText("nihao".into()))
            .expect("insert succeeds");
        let committed = session
            .process_event(InputEvent::Commit)
            .expect("raw commit succeeds");
        assert!(matches!(
            &committed.actions[0].kind,
            ImeActionKind::CommitText {
                text_utf8,
                commit_kind: crate::CommitKind::DirectInput,
            } if text_utf8 == "nihao"
        ));
    }

    #[test]
    fn moving_candidate_selection_clamps_and_invalidates_old_revision() {
        let engine = engine_with_candidates();
        let mut session = engine.new_session().expect("session id is available");
        let generated = session
            .process_event(InputEvent::InsertText("nihao".into()))
            .expect("insert succeeds");
        assert_eq!(
            generated.state.selected_candidate,
            Some(CandidateId::new(1))
        );

        let moved = session
            .process_event(InputEvent::MoveCandidateSelection { delta: 1 })
            .expect("selection movement succeeds");
        assert_eq!(moved.state.selected_candidate, Some(CandidateId::new(2)));
        assert!(moved.state_revision > generated.state_revision);

        let clamped = session
            .process_event(InputEvent::MoveCandidateSelection { delta: 99 })
            .expect("clamped movement succeeds");
        assert_eq!(clamped.state.selected_candidate, Some(CandidateId::new(3)));
        let no_op = session
            .process_event(InputEvent::MoveCandidateSelection { delta: 1 })
            .expect("boundary movement succeeds");
        assert_eq!(no_op.status, ImeStatus::NoOp);

        let stale = session.process_event(InputEvent::SelectCandidate {
            candidate_id: CandidateId::new(1),
            state_revision: generated.state_revision,
        });
        assert!(matches!(stale, Err(ImeError::StaleRevision { .. })));
    }

    #[test]
    fn cancel_and_reset_clear_candidates() {
        let engine = engine_with_candidates();
        let mut session = engine.new_session().expect("session id is available");
        session
            .process_event(InputEvent::InsertText("nihao".into()))
            .expect("insert succeeds");
        let cancelled = session
            .process_event(InputEvent::Cancel)
            .expect("cancel succeeds");
        assert!(cancelled.state.candidates.is_empty());

        session
            .process_event(InputEvent::InsertText("nihao".into()))
            .expect("insert succeeds");
        let reset = session
            .process_event(InputEvent::Reset)
            .expect("reset succeeds");
        assert!(reset.state.candidates.is_empty());
    }

    #[test]
    fn recoverable_pipeline_failure_keeps_direct_input_available() {
        let engine = ImeEngine::with_candidate_pipeline(
            EngineConfig::default(),
            Arc::new(ReferenceLanguageEngine),
            vec![Arc::new(FailingProvider)],
            RankingEngine,
        )
        .expect("failure test engine is valid");
        let mut session = engine.new_session().expect("session id is available");
        let inserted = session
            .process_event(InputEvent::InsertText("raw".into()))
            .expect("raw input survives pipeline failure");
        assert_eq!(inserted.state.phase, SessionPhase::Composing);
        assert!(inserted.state.candidates.is_empty());
        assert_eq!(inserted.degraded_components, [DegradedComponent::Candidate]);

        let committed = session
            .process_event(InputEvent::Commit)
            .expect("direct commit remains available");
        assert!(matches!(
            &committed.actions[0].kind,
            ImeActionKind::CommitText {
                text_utf8,
                commit_kind: crate::CommitKind::DirectInput,
            } if text_utf8 == "raw"
        ));
    }

    #[test]
    fn candidate_events_are_structured_unsupported() {
        let engine = default_engine();
        let mut session = engine.new_session().expect("session id is available");
        let result = session
            .process_event(InputEvent::SelectCandidate {
                candidate_id: CandidateId::new(1),
                state_revision: session.state_revision(),
            })
            .expect("unsupported event does not panic");

        assert_eq!(result.status, ImeStatus::Unsupported);
        assert_eq!(result.event_handling, EventHandling::Consumed);
        assert_eq!(result.state_revision.get(), 0);
    }
}
