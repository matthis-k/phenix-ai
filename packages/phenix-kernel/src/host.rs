//! Host-controlled publication of complete kernel generations.
//!
//! Preparation can acquire external producer resources, but never polls them.
//! Dropping a candidate releases its owned resources; this is not rollback of
//! arbitrary external effects performed by plugin factories or composers.
//!
//! # Host publication
//!
//! `KernelHost::prepare(stages, options, name, bind)` builds the exact
//! candidate inventory, invokes the nonadmitting binding hook, then activates producers
//! without polling. The hook receives only:
//!
//! ```rust,ignore
//! Binding::export::<T>(&Key) -> Option<&T>
//! Binding::inspect(&str) -> Result<ComponentSnapshot, NetworkError>
//! Binding::lookup_definition(DefinitionKey) -> Result<PhysicalDeclaration, NetworkError>
//! Binding::input_targets(&str) -> Option<&[(usize, String)]>
//! Binding::output_source(&str) -> Option<(usize, &str)>
//! ```
//!
//! There is no session or startable compiled-network getter on `Binding`.
//! Use opaque `DefinitionKey` identities, not factory/action pointer equality.
//! `publish(candidate)` rejects foreign/stale hosts and epochs, commits the plugin
//! batch, retires old owned producers and replaces the generation. Publication waits
//! only for producer drains, never old explicit session work. Old `Generation` handles
//! retain `inject`, `with_session`, `run_steps`, `compiled`, `with_kernel` and exports;
//! their retired producer drains remain empty. Candidate drop releases artifacts and
//! producers before their plugin leases. Native callback external effects are not
//! sandboxed or rolled back. Typed callback errors preserve their exact source.
//!
//! # Host wake hints
//!
//! `KernelHost::with_wake(plugins, wake.clone())` opts into producer/publication
//! notifications; `new` remains manual. Preparation builds, binds, and starts before
//! registering producer wakers, with shutdown checks on either side of registration.
//! It never polls or runs candidate process steps or installs a session notifier.
//! Unsupported/panicking registration and shutdown return `LifecycleError::Driver`.
//! Activation/subscription may perform I/O, and failed candidates may signal hints;
//! these external effects and wake versions are not rolled back. Shutdown checks
//! are not atomic publication revocation of an already prepared candidate.
//!
//! `WakeHandle::version()` snapshots a wrapping change hint. Snapshot before work
//! inspection, then `wait_since(version)` waits for a different version or sticky
//! shutdown, looping through spurious wakes. `is_shutdown()` must also be checked
//! during busy iterations. Wakes notify all waiters before forwarding to the native
//! driver notifier, without holding the epoch mutex during forwarding. Versions
//! are neither event counts nor generation IDs. Publication signals after the new
//! active pointer is visible and the old drain gate is released, before old producer
//! destructors, including on first publication.
//!
//! This is **source/publication-driven only**, not a general scheduler. An empty
//! bounded drain is not idle: finish a sweep using an application-known producer
//! bound, execute bounded steps, drain captures/backpressure, and retry after
//! progress or exhausted budgets. Graph process wakes, worker completion, deadlines,
//! explicit injection, and session-lease release do not automatically advance the
//! host version. Keep native `run_until` or coordinate those sources independently.
//! Never hold a host mutex while processing or waiting. Mixing host-reactor and
//! static-driver modes with different wake handles, including handover back to the
//! host after static registration, is unsupported: producer wakers are not restored.
//!
//! `DerivedHost::with_wake` forwards this opt-in without changing mandatory storage
//! guards. Wake hints are advisory; publication and session ownership remain independent of readiness notification.
use crate::{
    BuildOptions, CallbackError, DriverError, Kernel, KernelLockScope, Key, ProducerEvent,
    ResolutionError, StartError, WakeHandle,
};
use phenix_graph::{
    BoundDeclaration, CaptureHandle, IngressHandle, InputKey, OutputKey, SourceHandle, Value,
    component::DefinitionKey,
    network::{CompiledNetwork, ComponentSnapshot, NetworkError},
    runtime::{
        InjectionError, NetworkSession, ProcessNotifier, RequestError, RequestId,
        RequestInjectionError, RunReport,
    },
};
use phenix_plugin::{Failure, PluginSet, StagedPlugin};
use std::sync::Arc;

#[derive(Debug)]
pub enum LifecycleError {
    Plugins(Failure),
    Build(Box<ResolutionError>),
    Start(StartError),
    Binding(CallbackError),
    Driver(DriverError),
    StaleCandidate,
    EpochExhausted,
    ReentrantDrain,
}

impl std::fmt::Display for LifecycleError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Plugins(error) => write!(f, "plugin batch: {error}"),
            Self::Build(error) => write!(f, "generation build: {error}"),
            Self::Start(error) => write!(f, "producer start: {error}"),
            Self::Binding(error) => write!(f, "generation binding: {error}"),
            Self::Driver(error) => write!(f, "generation wake registration: {error}"),
            Self::StaleCandidate => f.write_str("candidate belongs to another host or epoch"),
            Self::EpochExhausted => f.write_str("host epoch exhausted"),
            Self::ReentrantDrain => {
                f.write_str("publication attempted during its own producer drain")
            }
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
            Self::Driver(error) => Some(error),
            _ => None,
        }
    }
}

/// An immutable generation identity with serialized producer access. No mutable
/// kernel or producer access escapes. Explicit session calls remain valid after
/// replacement. Publication waits for an admitted whole drain; self-retirement
/// inside a callback rejects its late event. An admission that won the retirement
/// boundary may finish, including its eager session callbacks.
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
    ) -> Result<R, crate::SessionAccessError> {
        self.kernel.with_session(use_session)
    }

    pub fn inject(&self, input: &str, value: Value) -> Result<(), InjectionError> {
        self.kernel.inject(input, value)
    }

    /// Ordinary typed admission through this generation's Kernel. Keys are names,
    /// not generation authority; conversion precedes session acquisition.
    pub fn inject_key<T: Send + Sync + 'static>(
        &self,
        key: phenix_graph::InputKey<T>,
        payload: T,
    ) -> Result<(), InjectionError> {
        self.kernel.inject_key(key, payload)
    }

    pub fn inject_bound<T: Send + Sync + 'static>(
        &self,
        ingress: &IngressHandle<T>,
        payload: T,
    ) -> Result<(), InjectionError> {
        self.kernel.inject_bound(ingress, payload)
    }

    pub fn inject_bound_value<T: 'static>(
        &self,
        ingress: &IngressHandle<T>,
        value: Value,
    ) -> Result<(), InjectionError> {
        self.kernel.inject_bound_value(ingress, value)
    }

    pub fn inject_bound_request<T: Send + Sync + 'static>(
        &self,
        id: RequestId,
        ingress: &IngressHandle<T>,
        payload: T,
    ) -> Result<(), RequestInjectionError> {
        self.kernel.inject_bound_request(id, ingress, payload)
    }

    pub fn inject_bound_request_value<T: 'static>(
        &self,
        id: RequestId,
        ingress: &IngressHandle<T>,
        value: Value,
    ) -> Result<(), RequestInjectionError> {
        self.kernel.inject_bound_request_value(id, ingress, value)
    }

    pub fn begin_bound_request<'a>(
        &self,
        exact_output: impl Into<Option<&'a CaptureHandle>>,
    ) -> Result<Result<RequestId, RequestError>, crate::SessionAccessError> {
        self.kernel.begin_bound_request(exact_output)
    }

    pub fn close_bound_input<T>(
        &self,
        ingress: &IngressHandle<T>,
    ) -> Result<Result<(), phenix_graph::connection::ConnectionError>, crate::SessionAccessError>
    {
        self.kernel.close_bound_input(ingress)
    }

    pub fn take_bound_output(
        &self,
        capture: &CaptureHandle,
    ) -> Result<
        Result<Option<phenix_graph::connection::Packet>, phenix_graph::connection::ConnectionError>,
        crate::SessionAccessError,
    > {
        self.kernel.take_bound_output(capture)
    }

    pub fn take_bound_request_output(
        &self,
        id: RequestId,
        capture: &CaptureHandle,
    ) -> Result<
        Result<Option<phenix_graph::connection::Packet>, RequestError>,
        crate::SessionAccessError,
    > {
        self.kernel.take_bound_request_output(id, capture)
    }

    pub fn validate_bound_capture(
        &self,
        id: RequestId,
        capture: &CaptureHandle,
        packet: &phenix_graph::connection::Packet,
    ) -> Result<Result<(), RequestError>, crate::SessionAccessError> {
        self.kernel.validate_bound_capture(id, capture, packet)
    }

    pub fn bound_output_closed(
        &self,
        capture: &CaptureHandle,
    ) -> Result<Result<bool, phenix_graph::connection::ConnectionError>, crate::SessionAccessError>
    {
        self.kernel.bound_output_closed(capture)
    }

    pub fn start_bound_request_source(
        &self,
        id: RequestId,
        source: &SourceHandle,
    ) -> Result<Result<(), RequestError>, crate::SessionAccessError> {
        self.kernel.start_bound_request_source(id, source)
    }

    pub fn bound_wake_handle(
        &self,
        declaration: &BoundDeclaration,
    ) -> Result<
        Result<Option<phenix_graph::runtime::WakeHandle>, NetworkError>,
        crate::SessionAccessError,
    > {
        self.kernel.bound_wake_handle(declaration)
    }

    pub fn bound_process_notifier(
        &self,
        declaration: &BoundDeclaration,
    ) -> Result<Result<Option<ProcessNotifier>, NetworkError>, crate::SessionAccessError> {
        self.kernel.bound_process_notifier(declaration)
    }

    pub fn run_steps(&self, budget: usize) -> Result<RunReport, crate::SessionAccessError> {
        self.kernel.run_steps(budget)
    }

    pub fn drain(&self, max_polls: usize) -> Result<Vec<ProducerEvent>, crate::DriverError> {
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

/// Read-only binding capability for a compiled kernel. It exposes exports and
/// deployment identities, but no kernel, session admission, or producer access.
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

impl Kernel {
    /// The same restricted binding view used during host preparation.
    pub fn binding(&self) -> Binding<'_> {
        Binding { kernel: self }
    }
}

impl Binding<'_> {
    /// Final-composition authority only; no executable owner escapes this view.
    pub fn declaration(&self, path: &str) -> Result<BoundDeclaration, NetworkError> {
        self.kernel.compiled().declaration(path)
    }

    pub fn bind_definition(&self, key: DefinitionKey) -> Result<BoundDeclaration, NetworkError> {
        self.kernel.compiled().bind_definition(key)
    }

    pub fn declarations(&self, key: DefinitionKey) -> Vec<BoundDeclaration> {
        self.kernel.compiled().declarations(key)
    }

    pub fn bind_ingress<T: 'static>(
        &self,
        name: &str,
        key: InputKey<T>,
    ) -> Result<IngressHandle<T>, NetworkError> {
        self.kernel.compiled().bind_ingress(name, key)
    }

    pub fn bind_capture<T: 'static>(
        &self,
        name: &str,
        key: OutputKey<T>,
    ) -> Result<CaptureHandle, NetworkError> {
        self.kernel.compiled().bind_capture(name, key)
    }

    pub fn capture(&self, name: &str) -> Result<CaptureHandle, NetworkError> {
        self.kernel.compiled().capture(name)
    }

    pub fn source(&self, declaration: &BoundDeclaration) -> Result<SourceHandle, NetworkError> {
        self.kernel.compiled().source(declaration)
    }

    pub fn export<T: Send + Sync + 'static>(&self, key: &Key) -> Option<&T> {
        self.kernel.export(key)
    }

    pub fn inspect(&self, path: &str) -> Result<ComponentSnapshot, NetworkError> {
        self.kernel.compiled().inspect(path)
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

    /// Opt in to source and publication hints for an application-owned reactor.
    /// This does not install a native session notifier or drive a candidate.
    pub fn with_wake(plugins: PluginSet, wake: WakeHandle) -> Self {
        let mut host = Self::new(plugins);
        host.wake = Some(wake);
        host
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
        let mut kernel = Kernel::build_with_options(name, snapshots, options)
            .map_err(|error| LifecycleError::Build(Box::new(error)))?;
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            bind(&kernel.binding())
        }))
        .unwrap_or_else(|payload| {
            Err(kernel
                .lease_failure(phenix_graph::process::callback_panicked(
                    phenix_graph::CallbackPhase::GenerationBind,
                    payload,
                ))
                .into())
        })
        .map_err(|cause| {
            LifecycleError::Binding(phenix_plugin::lease_callback_error(
                cause,
                kernel.code_lease.clone(),
            ))
        })?;
        kernel.start().map_err(LifecycleError::Start)?;
        if let Some(wake) = &self.wake {
            if wake.is_shutdown() {
                return Err(LifecycleError::Driver(DriverError::WakeShutdown));
            }
            kernel
                .register_producer_wakers(wake)
                .map_err(LifecycleError::Driver)?;
            if wake.is_shutdown() {
                return Err(LifecycleError::Driver(DriverError::WakeShutdown));
            }
        }
        Ok(Candidate {
            host: self.identity.clone(),
            expected_epoch: self.epoch,
            stages,
            generation: Arc::new(Generation { epoch, kernel }),
        })
    }

    /// After validation, commit and pointer replacement have no fallible step.
    /// Retirement waits for an admitted whole producer drain, but never for
    /// explicit session work. No new drain can poll a retired generation.
    /// Producer destructors run after the transition and outside the state lock.
    pub fn publish(&mut self, candidate: Candidate) -> Result<Arc<Generation>, LifecycleError> {
        if !Arc::ptr_eq(&self.identity, &candidate.host) || self.epoch != candidate.expected_epoch {
            return Err(LifecycleError::StaleCandidate);
        }
        // Own an Arc separately from the mutable active pointer. It keeps
        // the old kernel and plugin code leased through retirement and Drops.
        let retiring = self.active.clone();
        // Reject same-thread publication from a producer callback before
        // blocking on the whole-drain gate. Other threads still wait normally.
        let drain_scope = retiring
            .as_ref()
            .map(|generation| {
                KernelLockScope::enter(&generation.kernel.producer_drain)
                    .ok_or(LifecycleError::ReentrantDrain)
            })
            .transpose()?;
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
        drop(drain_scope);
        if let Some(wake) = &self.wake {
            wake.wake();
        }
        if let Some(generation) = &retiring {
            generation.kernel.notify_driver_retirement();
        }
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
            // A producer callback may synchronously destroy its host while
            // holding this generation's drain gate. Do not deadlock on the
            // same mutex or detach its temporarily checked-out producer.
            // The enclosing drain/registration restores the slot and retires
            // all producers after releasing its gate and inventory locks.
            if KernelLockScope::held(&generation.kernel.producer_drain) {
                generation
                    .kernel
                    .producers
                    .lock()
                    .unwrap_or_else(|error| error.into_inner())
                    .retired = true;
                generation.kernel.notify_driver_retirement();
                return;
            }
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
            generation.kernel.notify_driver_retirement();
            drop(producers);
        }
    }
}

#[cfg(test)]
#[path = "h3b_closure_tests.rs"]
mod h3b_closure_tests;

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
            Contribution::new("source", NetworkPatch::new([]))
                .prepare(move |_| {
                    calls.fetch_add(1, Ordering::SeqCst);
                    let calls = calls.clone();
                    let mut artifacts = CandidateArtifacts::default();
                    artifacts.activate(move || {
                        calls.fetch_add(1, Ordering::SeqCst);
                        Ok(Box::new(Source))
                    })?;
                    Ok(artifacts)
                })
                .register(into)
        }
    }

    #[test]
    fn host_drop_during_producer_callbacks_defers_retirement_and_destructors() {
        use std::cell::RefCell;
        use std::sync::Weak;

        thread_local! {
            static OWNER: RefCell<Option<KernelHost>> = const { RefCell::new(None) };
        }

        #[derive(Clone, Copy)]
        enum Trigger {
            Poll,
            Wake,
            WakeUnsupported,
            WakePanics,
            Never,
        }

        struct RetireSource {
            generation: Weak<Generation>,
            trigger: Trigger,
            calls: Arc<AtomicUsize>,
            drops: Arc<AtomicUsize>,
        }

        impl Producer for RetireSource {
            fn poll(&mut self) -> Result<Option<InputEvent>, CallbackError> {
                self.calls.fetch_add(1, Ordering::SeqCst);
                if matches!(self.trigger, Trigger::Poll) {
                    OWNER.with(|owner| drop(owner.borrow_mut().take()));
                    // The in-flight producer cannot be destroyed until its
                    // callback returns and the whole drain gate is released.
                    assert_eq!(self.drops.load(Ordering::SeqCst), 0);
                    return Ok(Some(InputEvent {
                        input: "in".into(),
                        value: Value::local(7_i64),
                    }));
                }
                Ok(None)
            }

            fn set_waker(&mut self, _: crate::WakeHandle) -> bool {
                self.calls.fetch_add(1, Ordering::SeqCst);
                if matches!(
                    self.trigger,
                    Trigger::Wake | Trigger::WakeUnsupported | Trigger::WakePanics
                ) {
                    OWNER.with(|owner| drop(owner.borrow_mut().take()));
                    assert_eq!(self.drops.load(Ordering::SeqCst), 0);
                    if matches!(self.trigger, Trigger::WakeUnsupported) {
                        return false;
                    }
                    if matches!(self.trigger, Trigger::WakePanics) {
                        panic!("intentional wake-registration failure");
                    }
                }
                true
            }
        }

        impl Drop for RetireSource {
            fn drop(&mut self) {
                let generation = self.generation.upgrade().unwrap();
                let _gate = generation
                    .kernel
                    .producer_drain
                    .try_lock()
                    .expect("producer destroyed while drain gate is locked");
                let _inventory = generation
                    .kernel
                    .producers
                    .try_lock()
                    .expect("producer destroyed while inventory is locked");
                self.drops.fetch_add(1, Ordering::SeqCst);
            }
        }

        for trigger in [
            Trigger::Poll,
            Trigger::Wake,
            Trigger::WakeUnsupported,
            Trigger::WakePanics,
        ] {
            let calls = Arc::new(AtomicUsize::new(0));
            let other_calls = Arc::new(AtomicUsize::new(0));
            let drops = Arc::new(AtomicUsize::new(0));
            let generation = Arc::new_cyclic(|weak| {
                let mut kernel =
                    Kernel::build("reentrant-host-drop", [], Arc::new(LocalProcessDispatcher))
                        .unwrap();
                kernel.start().unwrap();

                if matches!(
                    trigger,
                    Trigger::Poll | Trigger::Wake | Trigger::WakeUnsupported | Trigger::WakePanics
                ) {
                    // Both producer admission and wake registration need live
                    // ingress to prove no packet is admitted after retirement.
                    // Construct a live ingress so the callback really runs.
                    let mut builder = phenix_graph::network::NetworkBuilder::new();
                    let sink = builder
                        .add(
                            "sink",
                            Arc::new(phenix_graph::ComponentDef::sink(|_: &i64| {})),
                        )
                        .unwrap();
                    builder
                        .expose_input(
                            "in",
                            sink.input::<i64>("input").unwrap(),
                            std::num::NonZeroUsize::new(1).unwrap(),
                        )
                        .unwrap();
                    kernel.session = crate::ExclusiveSlot::new(
                        builder.build().unwrap().compile().unwrap().start(),
                    );
                }

                kernel.routes.insert(
                    Key::new("test", "destroy-host"),
                    std::collections::BTreeSet::from(["in".to_owned()]),
                );
                kernel.producers.get_mut().unwrap().producers = vec![
                    (
                        Key::new("test", "destroy-host"),
                        Some(Box::new(RetireSource {
                            generation: weak.clone(),
                            trigger,
                            calls: calls.clone(),
                            drops: drops.clone(),
                        }) as Box<dyn Producer>),
                        None,
                    ),
                    (
                        Key::new("test", "must-not-run"),
                        Some(Box::new(RetireSource {
                            generation: weak.clone(),
                            trigger: Trigger::Never,
                            calls: other_calls.clone(),
                            drops: drops.clone(),
                        }) as Box<dyn Producer>),
                        None,
                    ),
                ];
                Generation { epoch: 1, kernel }
            });

            let mut host = KernelHost::new(PluginCatalog::new().instance());
            host.epoch = 1;
            host.active = Some(generation.clone());
            OWNER.with(|owner| {
                assert!(owner.borrow().is_none());
                *owner.borrow_mut() = Some(host);
            });

            match trigger {
                Trigger::Poll => assert!(generation.drain(3).unwrap().is_empty()),
                Trigger::Wake | Trigger::WakeUnsupported | Trigger::WakePanics => {
                    let wake = crate::WakeHandle::default();
                    wake.shutdown();
                    let outcome = generation.kernel.run_until(&wake, |_| {});
                    match outcome {
                        Err(crate::DriverError::Retired) => {
                            assert!(matches!(trigger, Trigger::Wake));
                        }
                        Err(crate::DriverError::UnsupportedWake { .. }) => {
                            assert!(matches!(trigger, Trigger::WakeUnsupported));
                        }
                        Err(crate::DriverError::WakeRegistrationPanicked { .. }) => {
                            assert!(matches!(trigger, Trigger::WakePanics));
                        }
                        other => panic!("unexpected wake-registration result: {other:?}"),
                    }
                }
                Trigger::Never => unreachable!(),
            }
            assert_eq!(calls.load(Ordering::SeqCst), 1);
            assert_eq!(other_calls.load(Ordering::SeqCst), 0);
            assert_eq!(drops.load(Ordering::SeqCst), 2);
            assert!(
                generation.kernel.driver_wake.lock().unwrap().is_none(),
                "driver wake registration must be released on every exit"
            );
            assert_eq!(
                generation.with_session(|session| session.state()).unwrap(),
                phenix_graph::runtime::SessionState::Quiescent,
                "an event returned after self-retirement must never enter the session"
            );
            let state = generation.kernel.producers.lock().unwrap();
            assert!(state.retired);
            assert!(state.producers.is_empty());
            drop(state);

            // Keeping a retired generation handle alive must not resurrect
            // callbacks or prevent immediate producer cleanup.
            assert!(generation.drain(2).unwrap().is_empty());
            assert_eq!(calls.load(Ordering::SeqCst), 1);
        }
    }

    #[test]
    fn externally_retired_live_driver_exits_without_shutdown_and_preserves_session() {
        use std::sync::mpsc;
        use std::time::Duration;

        struct Idle {
            registered: mpsc::Sender<()>,
            polled: mpsc::Sender<()>,
        }

        impl Producer for Idle {
            fn poll(&mut self) -> Result<Option<InputEvent>, CallbackError> {
                self.polled.send(()).unwrap();
                Ok(None)
            }

            fn set_waker(&mut self, _: crate::WakeHandle) -> bool {
                self.registered.send(()).unwrap();
                true
            }
        }

        for replace in [false, true] {
            let (registered, registration) = mpsc::channel();
            let (polled, poll) = mpsc::channel();
            let mut kernel =
                Kernel::build("live-retirement", [], Arc::new(LocalProcessDispatcher)).unwrap();
            kernel.start().unwrap();

            let mut builder = phenix_graph::network::NetworkBuilder::new();
            let sink = builder
                .add(
                    "sink",
                    Arc::new(phenix_graph::ComponentDef::sink(|_: &i64| {})),
                )
                .unwrap();
            builder
                .expose_input(
                    "input",
                    sink.input::<i64>("input").unwrap(),
                    std::num::NonZeroUsize::new(1).unwrap(),
                )
                .unwrap();
            kernel.session =
                crate::ExclusiveSlot::new(builder.build().unwrap().compile().unwrap().start());
            kernel.producers.get_mut().unwrap().producers.push((
                Key::new("test", "idle"),
                Some(Box::new(Idle { registered, polled })),
                None,
            ));

            let generation = Arc::new(Generation { epoch: 1, kernel });
            let mut host = KernelHost::new(PluginCatalog::new().instance());
            host.epoch = 1;
            host.active = Some(generation.clone());

            let wake = crate::WakeHandle::default();
            let worker_wake = wake.clone();
            let worker_generation = generation.clone();
            let (result_tx, result_rx) = mpsc::channel();
            let driver = std::thread::spawn(move || {
                let outcome = worker_generation.kernel.run_until(&worker_wake, |_| {});
                result_tx.send(outcome).unwrap();
            });
            registration.recv_timeout(Duration::from_secs(2)).unwrap();
            poll.recv_timeout(Duration::from_secs(2)).unwrap();

            if replace {
                let candidate = host
                    .prepare(
                        vec![],
                        BuildOptions::default().with_dispatcher(Arc::new(LocalProcessDispatcher)),
                        "replacement",
                        |_| Ok(()),
                    )
                    .unwrap();
                host.publish(candidate).unwrap();
            } else {
                drop(host);
            }

            // A regression cannot hang CI for the driver wait interval:
            // shutdown is a fallback only, never part of the success path.
            let outcome = result_rx.recv_timeout(Duration::from_secs(2));
            if outcome.is_err() {
                wake.shutdown();
            }
            driver.join().unwrap();
            assert!(matches!(outcome.unwrap(), Err(crate::DriverError::Retired)));
            assert!(generation.kernel.driver_wake.lock().unwrap().is_none());
            assert!(generation.kernel.producers.lock().unwrap().retired);

            // Retired producer admission is revoked, but the retained
            // generation's explicit native session remains operational.
            generation.inject("input", Value::local(7_i64)).unwrap();
            generation.run_steps(32).unwrap();
            assert!(
                generation
                    .with_session(|session| session.failure().is_none())
                    .unwrap()
            );
        }
    }

    #[test]
    fn publication_rejects_same_thread_reentry_into_a_held_producer_drain() {
        let mut host = KernelHost::new(PluginCatalog::new().instance());
        let first = host
            .prepare(
                vec![],
                BuildOptions::default().with_dispatcher(Arc::new(LocalProcessDispatcher)),
                "initial",
                |_| Ok(()),
            )
            .unwrap();
        let active = host.publish(first).unwrap();
        let replacement = host
            .prepare(
                vec![],
                BuildOptions::default().with_dispatcher(Arc::new(LocalProcessDispatcher)),
                "replacement",
                |_| Ok(()),
            )
            .unwrap();

        // A producer callback holds exactly these two guards during a drain.
        let scope = crate::KernelLockScope::enter(&active.kernel.producer_drain).unwrap();
        let gate = active.kernel.producer_drain.lock().unwrap();
        let error = match host.publish(replacement) {
            Ok(_) => panic!("recursive publication must be rejected"),
            Err(error) => error,
        };
        assert!(matches!(error, LifecycleError::ReentrantDrain));
        assert!(Arc::ptr_eq(&active, &host.active().unwrap()));
        assert_eq!(active.epoch(), 1);
        drop(gate);
        drop(scope);

        // Failed publication did not consume the epoch or poison the gate.
        let next = host
            .prepare(
                vec![],
                BuildOptions::default().with_dispatcher(Arc::new(LocalProcessDispatcher)),
                "next",
                |_| Ok(()),
            )
            .unwrap();
        assert_eq!(host.publish(next).unwrap().epoch(), 2);
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
                BuildOptions::default().with_dispatcher(Arc::new(LocalProcessDispatcher)),
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
                BuildOptions::default().with_dispatcher(Arc::new(LocalProcessDispatcher)),
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
            assert!(active.drain(1).unwrap().is_empty());
        }
    }
}
