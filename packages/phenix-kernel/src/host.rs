//! Host-controlled publication of complete kernel generations.
//!
//! Preparation can acquire external producer resources, but never polls them.
//! Dropping a candidate releases its owned resources; this is not rollback of
//! arbitrary external effects performed by plugin factories or composers.
use crate::{ImportPolicy, Kernel, ProducerEvent, ResolutionError};
use phenix_graph::{Engine, Executor, TriggerSelector, Value, WaveReport};
use phenix_plugin::{Failure, PluginSet, StagedPlugin};
use std::sync::{Arc, Mutex};

#[derive(Debug)]
pub enum LifecycleError {
    Plugins(Failure),
    Build(Box<ResolutionError>),
    Start(String),
    Binding(String),
    StaleCandidate,
    EpochExhausted,
}

impl std::fmt::Display for LifecycleError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Plugins(error) => write!(f, "plugin batch: {error}"),
            Self::Build(error) => write!(f, "generation build: {error}"),
            Self::Start(error) => write!(f, "producer start: {error}"),
            Self::Binding(error) => write!(f, "generation binding: {error}"),
            Self::StaleCandidate => f.write_str("candidate belongs to another host or epoch"),
            Self::EpochExhausted => f.write_str("host epoch exhausted"),
        }
    }
}

impl std::error::Error for LifecycleError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Plugins(error) => Some(error),
            Self::Build(error) => Some(error.as_ref()),
            _ => None,
        }
    }
}

struct State {
    kernel: Kernel,
    retired: bool,
}

/// An immutable generation identity with serialized kernel access. No mutable
/// kernel or producer access escapes. Explicit engine calls remain valid after
/// replacement, but retired producers can never admit another notice.
pub struct Generation {
    epoch: u64,
    state: Mutex<State>,
}

impl Generation {
    pub fn epoch(&self) -> u64 {
        self.epoch
    }

    /// Scoped read-only access includes generation-owned exports and the engine.
    /// Do not reenter this generation from the callback.
    /// The lock also serializes publication with an in-flight call.
    pub fn with_kernel<R>(&self, read: impl FnOnce(&Kernel) -> R) -> R {
        let state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        read(&state.kernel)
    }

    /// Engine-only convenience view over the same serialized kernel.
    pub fn with_engine<R>(&self, read: impl FnOnce(&Engine) -> R) -> R {
        self.with_kernel(|kernel| read(kernel.engine()))
    }

    pub fn fire<T: TriggerSelector>(
        &self,
        trigger: T,
        values: Vec<Value>,
    ) -> Result<WaveReport, String> {
        self.with_engine(|engine| engine.fire(trigger, values))
    }

    pub fn drain(&self, max_polls: usize) -> Vec<ProducerEvent> {
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        if state.retired {
            return Vec::new();
        }
        state.kernel.drain(max_polls)
    }
}

/// Private execution state: there is intentionally no generation/engine getter.
/// A candidate is bound to both the preparing host and its publication epoch.
pub struct Candidate {
    host: Arc<()>,
    expected_epoch: u64,
    stages: Vec<StagedPlugin>,
    generation: Arc<Generation>,
}

/// Sole owner of the mutable plugin inventory and the coherent active pointer.
/// Share the returned Arc handles with readers; serialize host mutations in the
/// embedding application. There is no public mutable PluginSet access.
pub struct KernelHost {
    identity: Arc<()>,
    epoch: u64,
    plugins: PluginSet,
    active: Option<Arc<Generation>>,
}

impl KernelHost {
    pub fn new(plugins: PluginSet) -> Self {
        Self {
            identity: Arc::new(()),
            epoch: 0,
            plugins,
            active: None,
        }
    }

    /// Shared access permits staging, never committing the host's inventory.
    pub fn plugins(&self) -> &PluginSet {
        &self.plugins
    }

    pub fn active(&self) -> Option<Arc<Generation>> {
        self.active.clone()
    }

    pub fn prepare(
        &self,
        stages: Vec<StagedPlugin>,
        policy: &ImportPolicy,
        executor: Arc<dyn Executor>,
        name: impl Into<String>,
    ) -> Result<Candidate, LifecycleError> {
        self.prepare_with(stages, policy, executor, name, |_| Ok(()))
    }

    /// The trusted host binds store/query adapters here against this exact
    /// kernel and its exports before producers start. This keeps storage dependencies out of
    /// the kernel. The hook must not admit waves or leak an admission handle;
    /// cleanup of hook-owned external effects remains the caller's obligation.
    pub fn prepare_with(
        &self,
        stages: Vec<StagedPlugin>,
        policy: &ImportPolicy,
        executor: Arc<dyn Executor>,
        name: impl Into<String>,
        bind: impl FnOnce(&Kernel) -> Result<(), String>,
    ) -> Result<Candidate, LifecycleError> {
        let epoch = self
            .epoch
            .checked_add(1)
            .ok_or(LifecycleError::EpochExhausted)?;
        let snapshots = self
            .plugins
            .candidate_snapshots(&stages)
            .map_err(LifecycleError::Plugins)?;
        let mut kernel = Kernel::build_with_policy(name, snapshots, executor, policy)
            .map_err(|error| LifecycleError::Build(Box::new(error)))?;
        bind(&kernel).map_err(LifecycleError::Binding)?;
        kernel.start().map_err(LifecycleError::Start)?;
        Ok(Candidate {
            host: self.identity.clone(),
            expected_epoch: self.epoch,
            stages,
            generation: Arc::new(Generation {
                epoch,
                state: Mutex::new(State {
                    kernel,
                    retired: false,
                }),
            }),
        })
    }

    /// After validation, commit and pointer replacement have no fallible step.
    /// Acquiring the old lock may wait for a wave/query, but old Arc readers do
    /// not prevent publication. Producer destructors run after the transition.
    pub fn publish(&mut self, candidate: Candidate) -> Result<Arc<Generation>, LifecycleError> {
        if !Arc::ptr_eq(&self.identity, &candidate.host) || self.epoch != candidate.expected_epoch {
            return Err(LifecycleError::StaleCandidate);
        }
        let mut old = self.active.as_ref().map(|generation| {
            generation
                .state
                .lock()
                .unwrap_or_else(|error| error.into_inner())
        });
        self.plugins
            .commit_batch(candidate.stages)
            .map_err(LifecycleError::Plugins)?;
        let producers = old.as_mut().map(|state| {
            state.retired = true;
            std::mem::take(&mut state.kernel.producers)
        });
        drop(old);
        self.epoch = candidate.generation.epoch;
        let previous = self.active.replace(candidate.generation.clone());
        drop(producers);
        // Detached producers may execute plugin code in Drop. Retain the old
        // kernel and its leases until all producer destructors have completed.
        drop(previous);
        Ok(candidate.generation)
    }
}

impl Drop for KernelHost {
    fn drop(&mut self) {
        if let Some(generation) = &self.active {
            let mut state = generation
                .state
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            state.retired = true;
            let producers = std::mem::take(&mut state.kernel.producers);
            drop(state);
            drop(producers);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Contribution, Notice, Producer};
    use phenix_graph::LocalExecutor;
    use phenix_plugin::{PluginCatalog, PluginInstance, Registrar};
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct Source;
    impl Producer for Source {
        fn poll(&mut self) -> Result<Option<Notice>, String> {
            Ok(None)
        }
    }

    struct Plugin(Arc<AtomicUsize>);
    impl PluginInstance for Plugin {
        fn contribute(&self, into: &mut Registrar) -> Result<(), Failure> {
            self.0.fetch_add(1, Ordering::SeqCst);
            let calls = self.0.clone();
            into.add(Contribution {
                id: "source".into(),
                requires: vec![],
                routes: vec![],
                compose: Box::new(move |context| {
                    calls.fetch_add(1, Ordering::SeqCst);
                    let calls = calls.clone();
                    context.activate(move || {
                        calls.fetch_add(1, Ordering::SeqCst);
                        Ok(Box::new(Source))
                    })
                }),
            })
        }
    }

    #[test]
    fn exhausted_epoch_rejects_preparation_before_callbacks_without_changing_state() {
        let calls = Arc::new(AtomicUsize::new(0));
        let mut catalog = PluginCatalog::new();
        let key = catalog
            .register("source", |plugin: Plugin| Ok(plugin))
            .unwrap();
        let mut host = KernelHost::new(catalog.instance());
        host.epoch = u64::MAX - 1;
        let stage = host
            .plugins()
            .prepare_load(&key, Plugin(calls.clone()))
            .unwrap();
        let candidate = host
            .prepare(
                vec![stage],
                &ImportPolicy::default(),
                Arc::new(LocalExecutor),
                "last",
            )
            .unwrap();
        let active = host.publish(candidate).unwrap();
        assert_eq!(active.epoch(), u64::MAX);
        let snapshot = host.plugins().snapshot(&key).unwrap();
        let stage = host
            .plugins()
            .prepare_reload(&key, Plugin(calls.clone()))
            .unwrap();
        let before = calls.load(Ordering::SeqCst);
        assert_eq!(before, 4); // Two registrations, one composition and activation.
        for stages in [vec![], vec![stage]] {
            let result = host.prepare_with(
                stages,
                &ImportPolicy::default(),
                Arc::new(LocalExecutor),
                "exhausted",
                |_| panic!("binding must not run after epoch exhaustion"),
            );
            assert!(matches!(result, Err(LifecycleError::EpochExhausted)));
            assert_eq!(calls.load(Ordering::SeqCst), before);
            assert_eq!(host.epoch, u64::MAX);
            assert!(Arc::ptr_eq(&active, &host.active().unwrap()));
            assert_eq!(host.plugins().snapshots().count(), 1);
            assert_eq!(
                host.plugins().snapshot(&key).unwrap().graph_origin(),
                snapshot.graph_origin()
            );
            assert_eq!(
                host.plugins().snapshot(&key).unwrap().revision(),
                snapshot.revision()
            );
            assert!(active.drain(1).is_empty());
        }
    }
}
