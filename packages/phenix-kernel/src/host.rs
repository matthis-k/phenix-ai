//! Host-controlled publication of complete kernel generations.
//!
//! Preparation can acquire external producer resources, but never polls them.
//! Dropping a candidate releases its owned resources; this is not rollback of
//! arbitrary external effects performed by plugin factories or composers.
use crate::{
    BuildOptions, CallbackError, DriverError, Kernel, Key, ProducerEvent, ResolutionError,
    StartError, WakeHandle,
};
use phenix_graph::{Engine, Executor, InputAddr, NodeAction, TriggerSelector, Value, WaveReport};
use phenix_plugin::{Failure, PluginSet, StagedPlugin};
use std::sync::Arc;

#[derive(Debug)]
pub enum LifecycleError {
    Plugins(Failure),
    Build(Box<ResolutionError>),
    Start(StartError),
    Driver(DriverError),
    Binding(CallbackError),
    StaleCandidate,
    EpochExhausted,
}

impl std::fmt::Display for LifecycleError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Plugins(error) => write!(f, "plugin batch: {error}"),
            Self::Build(error) => write!(f, "generation build: {error}"),
            Self::Start(error) => write!(f, "producer start: {error}"),
            Self::Driver(error) => write!(f, "producer wake registration: {error}"),
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
            Self::Start(error) => Some(error),
            Self::Driver(error) => Some(error),
            Self::Binding(error) => Some(error.as_ref()),
            _ => None,
        }
    }
}

/// An immutable generation identity with serialized producer access. No mutable
/// kernel or producer access escapes. Explicit engine calls remain valid after
/// replacement, but retired producers can never admit another notice.
pub struct Generation {
    epoch: u64,
    kernel: Kernel,
}

impl Generation {
    pub fn epoch(&self) -> u64 {
        self.epoch
    }

    /// Scoped read-only access includes generation-owned exports and the engine.
    /// Explicit reads and engine calls do not acquire the producer lock.
    pub fn with_kernel<R>(&self, read: impl FnOnce(&Kernel) -> R) -> R {
        read(&self.kernel)
    }

    /// Engine-only view, independent of producer polling and retirement.
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
        self.kernel.drain(max_polls)
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

/// Preparation-only inspection capability. It exposes exports and deployment
/// identities, but no kernel, engine, wave admission, or producer access.
/// Exports are arbitrary application values; their external effects are not
/// sandboxed or rolled back by the host.
///
/// ```compile_fail
/// use phenix_kernel::host::Binding;
/// fn admit(binding: &Binding<'_>) {
///     binding.engine().fire("refresh", vec![]);
/// }
/// ```
/// ```compile_fail
/// use phenix_kernel::host::Binding;
/// fn admit(binding: &Binding<'_>) {
///     binding.fire("refresh", vec![]);
/// }
/// ```
pub struct Binding<'a> {
    kernel: &'a Kernel,
}

impl Binding<'_> {
    pub fn export<T: Send + Sync + 'static>(&self, key: &Key) -> Option<&T> {
        self.kernel.export(key)
    }

    pub fn node_ids_for_action(&self, action: &Arc<dyn NodeAction>) -> Vec<u16> {
        self.kernel.engine().node_ids_for_action(action)
    }

    pub fn trigger_targets(&self, name: &str) -> Option<&[Vec<InputAddr>]> {
        self.kernel.engine().trigger_targets(name)
    }
}

/// Sole owner of the mutable plugin inventory and the coherent active pointer.
/// Share the returned Arc handles with readers; serialize host mutations in the
/// embedding application. There is no public mutable PluginSet access.
pub struct KernelHost {
    identity: Arc<()>,
    epoch: u64,
    plugins: PluginSet,
    active: Option<Arc<Generation>>,
    wake: Option<WakeHandle>,
}

impl KernelHost {
    pub fn new(plugins: PluginSet) -> Self {
        Self {
            identity: Arc::new(()),
            epoch: 0,
            plugins,
            active: None,
            wake: None,
        }
    }

    /// Opt in to a wake-driven published-generation host. Producers must support
    /// wake registration; the embedding application owns the event loop and thread.
    pub fn with_wake(plugins: PluginSet, wake: WakeHandle) -> Self {
        Self {
            wake: Some(wake),
            ..Self::new(plugins)
        }
    }

    /// Shared access permits staging, never committing the host's inventory.
    pub fn plugins(&self) -> &PluginSet {
        &self.plugins
    }

    pub fn active(&self) -> Option<Arc<Generation>> {
        self.active.clone()
    }

    /// Build the exact candidate inventory, inspect it through a nonadmitting
    /// binding capability, then start producers without polling them. Failure
    /// drops candidate-owned resources without changing the active generation;
    /// arbitrary callback external effects are not rolled back.
    pub fn prepare(
        &self,
        stages: Vec<StagedPlugin>,
        options: BuildOptions,
        executor: Arc<dyn Executor>,
        name: impl Into<String>,
        bind: impl FnOnce(&Binding<'_>) -> Result<(), CallbackError>,
    ) -> Result<Candidate, LifecycleError> {
        let epoch = self
            .epoch
            .checked_add(1)
            .ok_or(LifecycleError::EpochExhausted)?;
        let snapshots = self
            .plugins
            .candidate_snapshots(&stages)
            .map_err(LifecycleError::Plugins)?;
        let mut kernel = Kernel::build_with_options(name, snapshots, executor, options)
            .map_err(|error| LifecycleError::Build(Box::new(error)))?;
        bind(&Binding { kernel: &kernel }).map_err(LifecycleError::Binding)?;
        kernel.start().map_err(LifecycleError::Start)?;
        if let Some(wake) = &self.wake {
            kernel
                .register_wake(wake.clone())
                .map_err(LifecycleError::Driver)?;
        }
        Ok(Candidate {
            host: self.identity.clone(),
            expected_epoch: self.epoch,
            stages,
            generation: Arc::new(Generation { epoch, kernel }),
        })
    }

    /// After validation, commit and pointer replacement have no fallible step.
    /// Retirement waits only for producer drains, never explicit waves or queries.
    /// Producer destructors run after the transition.
    pub fn publish(&mut self, candidate: Candidate) -> Result<Arc<Generation>, LifecycleError> {
        if !Arc::ptr_eq(&self.identity, &candidate.host) || self.epoch != candidate.expected_epoch {
            return Err(LifecycleError::StaleCandidate);
        }
        let mut old = self.active.as_ref().map(|generation| {
            generation
                .kernel
                .producers
                .lock()
                .unwrap_or_else(|error| error.into_inner())
        });
        self.plugins
            .commit_batch(candidate.stages)
            .map_err(LifecycleError::Plugins)?;
        let producers = old.as_mut().map(|state| {
            state.retired = true;
            std::mem::take(&mut state.producers)
        });
        drop(old);
        self.epoch = candidate.generation.epoch;
        let previous = self.active.replace(candidate.generation.clone());
        // Wake the reactor after the new active pointer becomes visible but
        // before running arbitrary old-producer destructors.
        if let Some(wake) = &self.wake {
            wake.wake();
        }
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
                .kernel
                .producers
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            state.retired = true;
            let producers = std::mem::take(&mut state.producers);
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
        fn poll(&mut self) -> Result<Option<Notice>, CallbackError> {
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
                BuildOptions::default(),
                Arc::new(LocalExecutor),
                "last",
                |_| Ok(()),
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
            let result = host.prepare(
                stages,
                BuildOptions::default(),
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
