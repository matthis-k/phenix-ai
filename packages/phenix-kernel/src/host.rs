//! Host-controlled publication of complete kernel generations.
//!
//! Preparation can acquire external producer resources, but never polls them.
//! Dropping a candidate releases its owned resources; this is not rollback of
//! arbitrary external effects performed by plugin factories or composers.
use crate::{BuildOptions, CallbackError, Kernel, Key, ProducerEvent, ResolutionError, StartError};
use phenix_graph::{
    Value,
    component::DefinitionKey,
    network::{CompiledNetwork, ComponentSnapshot, NetworkError, PhysicalDeclaration},
    process::ProcessDispatcher,
    runtime::{InjectionError, NetworkSession, RunReport},
};
use phenix_plugin::{Failure, PluginSet, StagedPlugin};
use std::sync::Arc;

#[derive(Debug)]
pub enum LifecycleError {
    Plugins(Failure),
    Build(Box<ResolutionError>),
    Start(StartError),
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
            Self::Binding(error) => Some(error.as_ref()),
            _ => None,
        }
    }
}

/// An immutable generation identity with serialized producer access. No mutable
/// kernel or producer access escapes. Explicit session calls remain valid after
/// replacement, but retired producers can never admit another event.
pub struct Generation {
    epoch: u64,
    kernel: Kernel,
}

impl Generation {
    pub fn epoch(&self) -> u64 {
        self.epoch
    }

    /// Scoped access includes generation-owned exports and explicit session calls.
    /// Explicit reads and session calls do not acquire the producer lock.
    pub fn with_kernel<R>(&self, read: impl FnOnce(&Kernel) -> R) -> R {
        read(&self.kernel)
    }

    pub fn compiled(&self) -> &CompiledNetwork {
        self.kernel.compiled()
    }

    pub fn with_session<R>(
        &self,
        use_session: impl FnOnce(&mut NetworkSession) -> R,
    ) -> Result<R, String> {
        self.kernel.with_session(use_session)
    }

    pub fn inject(&self, input: &str, value: Value) -> Result<(), InjectionError> {
        self.kernel.inject(input, value)
    }

    pub fn run_steps(&self, budget: usize) -> Result<RunReport, String> {
        self.kernel.run_steps(budget)
    }

    pub fn drain(&self, max_polls: usize) -> Vec<ProducerEvent> {
        self.kernel.drain(max_polls)
    }
}

/// Private execution state: there is intentionally no generation/session getter.
/// A candidate is bound to both the preparing host and its publication epoch.
pub struct Candidate {
    host: Arc<()>,
    expected_epoch: u64,
    stages: Vec<StagedPlugin>,
    generation: Arc<Generation>,
}

/// Preparation-only inspection capability. It exposes exports and deployment
/// identities, but no kernel, session admission, or producer access.
/// Exports are arbitrary application values; their external effects are not
/// sandboxed or rolled back by the host.
///
/// ```compile_fail
/// use phenix_kernel::host::Binding;
/// fn admit(binding: &Binding<'_>) {
///     binding.with_session(|session| session.inject_local("refresh", 1_i64));
/// }
/// ```
/// ```compile_fail
/// use phenix_kernel::host::Binding;
/// fn admit(binding: &Binding<'_>) {
///     binding.compiled().start();
/// }
/// ```
pub struct Binding<'a> {
    kernel: &'a Kernel,
}

impl Binding<'_> {
    pub fn export<T: Send + Sync + 'static>(&self, key: &Key) -> Option<&T> {
        self.kernel.export(key)
    }

    pub fn inspect(&self, path: &str) -> Result<ComponentSnapshot, NetworkError> {
        self.kernel.compiled().inspect(path)
    }

    pub fn lookup_definition(
        &self,
        key: DefinitionKey,
    ) -> Result<PhysicalDeclaration, NetworkError> {
        self.kernel.compiled().lookup_definition(key)
    }

    pub fn input_targets(&self, name: &str) -> Option<&[(usize, String)]> {
        self.kernel.compiled().inputs().get(name).map(Vec::as_slice)
    }

    /// Inspect the exact compiled capture source without exposing session admission.
    pub fn output_source(&self, output: &str) -> Option<(usize, &str)> {
        self.kernel
            .compiled()
            .outputs()
            .get(output)
            .map(|(index, port)| (*index, port.as_str()))
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

    /// Build the exact candidate inventory, inspect it through a nonadmitting
    /// binding capability, then start producers without polling them. Failure
    /// drops candidate-owned resources without changing the active generation;
    /// arbitrary callback external effects are not rolled back.
    pub fn prepare(
        &self,
        stages: Vec<StagedPlugin>,
        options: BuildOptions,
        dispatcher: Arc<dyn ProcessDispatcher>,
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
        let mut kernel = Kernel::build_with_options(name, snapshots, dispatcher, options)
            .map_err(|error| LifecycleError::Build(Box::new(error)))?;
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            bind(&Binding { kernel: &kernel })
        }))
        .unwrap_or_else(|_| Err("generation binding panicked".into()))
        .map_err(|cause| {
            LifecycleError::Binding(phenix_plugin::lease_callback_error(
                cause,
                kernel.code_lease.clone(),
            ))
        })?;
        kernel.start().map_err(LifecycleError::Start)?;
        Ok(Candidate {
            host: self.identity.clone(),
            expected_epoch: self.epoch,
            stages,
            generation: Arc::new(Generation { epoch, kernel }),
        })
    }

    /// After validation, commit and pointer replacement have no fallible step.
    /// Retirement waits only for producer drains, never explicit session work or queries.
    /// Producer destructors run after the transition.
    pub fn publish(&mut self, candidate: Candidate) -> Result<Arc<Generation>, LifecycleError> {
        if !Arc::ptr_eq(&self.identity, &candidate.host) || self.epoch != candidate.expected_epoch {
            return Err(LifecycleError::StaleCandidate);
        }
        // Own an Arc separately from the mutable active pointer. It keeps
        // the old kernel and plugin code leased through retirement and Drops.
        let retiring = self.active.clone();
        // The whole-drain gate is acquired before the inventory lock. A
        // running drain finishes all of its polls before retirement begins.
        let drain = retiring.as_ref().map(|generation| {
            generation
                .kernel
                .producer_drain
                .lock()
                .unwrap_or_else(|error| error.into_inner())
        });
        let mut old = retiring.as_ref().map(|generation| {
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
        drop(drain);
        drop(producers);
        // Detached producers may execute plugin code in Drop. Retain the old
        // kernel and its leases until all producer destructors have completed.
        drop(previous);
        drop(retiring);
        Ok(candidate.generation)
    }
}

impl Drop for KernelHost {
    fn drop(&mut self) {
        if let Some(generation) = &self.active {
            let drain = generation
                .kernel
                .producer_drain
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            let mut state = generation
                .kernel
                .producers
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            state.retired = true;
            let producers = std::mem::take(&mut state.producers);
            drop(state);
            drop(drain);
            drop(producers);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{CandidateArtifacts, Contribution, InputEvent, Producer};
    use phenix_graph::{compose::NetworkPatch, process::LocalProcessDispatcher};
    use phenix_plugin::{PluginCatalog, PluginInstance, Registrar};
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct Source;
    impl Producer for Source {
        fn poll(&mut self) -> Result<Option<InputEvent>, CallbackError> {
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
                patch: NetworkPatch::new([]),
                prepare: Some(Box::new(move |_| {
                    calls.fetch_add(1, Ordering::SeqCst);
                    let calls = calls.clone();
                    let mut artifacts = CandidateArtifacts::default();
                    artifacts.activate(move || {
                        calls.fetch_add(1, Ordering::SeqCst);
                        Ok(Box::new(Source))
                    })?;
                    Ok(artifacts)
                })),
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
                Arc::new(LocalProcessDispatcher),
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
                Arc::new(LocalProcessDispatcher),
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
