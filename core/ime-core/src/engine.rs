use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

use crate::{
    CoreLimits, EngineId, ImeError, ImeSession, PlatformCapabilities, ResourceGeneration,
    SessionId,
    candidate::{CandidateEngine, CandidateProvider, DictionaryCandidateProvider},
    dictionary::InMemoryDictionary,
    language::{LanguageEngine, ReferenceLanguageEngine},
    ranking::RankingEngine,
};

static NEXT_ENGINE_ID: AtomicU64 = AtomicU64::new(1);
static NEXT_SESSION_ID: AtomicU64 = AtomicU64::new(1);

/// Immutable configuration shared by Sessions created from one Engine.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct EngineConfig {
    pub limits: CoreLimits,
    pub capabilities: PlatformCapabilities,
}

/// Shared owner of immutable Core configuration and resource identity.
#[derive(Clone, Debug)]
pub struct ImeEngine {
    pub(crate) inner: Arc<EngineInner>,
}

#[derive(Debug)]
pub(crate) struct EngineInner {
    pub(crate) engine_id: EngineId,
    pub(crate) resource_generation: ResourceGeneration,
    pub(crate) config: EngineConfig,
    pub(crate) candidate_engine: CandidateEngine,
}

impl ImeEngine {
    /// Creates an Engine after validating all hard limits.
    pub fn new(config: EngineConfig) -> Result<Self, ImeError> {
        Self::with_reference_dictionary(config, InMemoryDictionary::default())
    }

    /// Creates an Engine using the Phase 1B reference language and a read-only in-memory dictionary.
    pub fn with_reference_dictionary(
        config: EngineConfig,
        dictionary: InMemoryDictionary,
    ) -> Result<Self, ImeError> {
        let provider: Arc<dyn CandidateProvider> =
            Arc::new(DictionaryCandidateProvider::system(Arc::new(dictionary)));
        Self::with_candidate_pipeline(
            config,
            Arc::new(ReferenceLanguageEngine),
            vec![provider],
            RankingEngine,
        )
    }

    /// Creates an Engine from explicit Phase 1B language and provider components.
    pub fn with_candidate_pipeline(
        config: EngineConfig,
        language: Arc<dyn LanguageEngine>,
        providers: Vec<Arc<dyn CandidateProvider>>,
        ranking: RankingEngine,
    ) -> Result<Self, ImeError> {
        config.limits.validate()?;
        let engine_id = EngineId::new(next_monotonic_id(&NEXT_ENGINE_ID, "engine_id")?);
        Ok(Self {
            inner: Arc::new(EngineInner {
                engine_id,
                resource_generation: ResourceGeneration::new(0),
                config,
                candidate_engine: CandidateEngine::new(language, providers, ranking),
            }),
        })
    }

    /// Returns the Engine's process-local identity.
    pub fn id(&self) -> EngineId {
        self.inner.engine_id
    }

    /// Returns the immutable resource generation used by Phase 1A.
    pub fn resource_generation(&self) -> ResourceGeneration {
        self.inner.resource_generation
    }

    /// Returns the immutable Engine configuration.
    pub fn config(&self) -> &EngineConfig {
        &self.inner.config
    }

    /// Creates an isolated Session that keeps the Engine internals alive.
    pub fn new_session(&self) -> Result<ImeSession, ImeError> {
        let session_id = SessionId::new(next_monotonic_id(&NEXT_SESSION_ID, "session_id")?);
        Ok(ImeSession::new(Arc::clone(&self.inner), session_id))
    }
}

fn next_monotonic_id(counter: &AtomicU64, name: &'static str) -> Result<u64, ImeError> {
    counter
        .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |current| {
            current.checked_add(1)
        })
        .map_err(|_| ImeError::CounterExhausted(name))
}
