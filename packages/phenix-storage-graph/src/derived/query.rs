use super::*;
use phenix_graph::{
    BoundDeclaration, CaptureHandle, Failure, IngressHandle, InputKey, Leased, OutputKey,
    network::CompiledNetwork,
    process::{ProcessDispatcher, ProcessExecutionGuard},
    runtime::{RequestFailurePolicy, RequestStatus, SessionOptions, SessionState},
};
use phenix_kernel::{BuildOptions, DriverError, Kernel, ProducerEvent, WakeHandle};
use phenix_plugin::PluginSnapshot;
use std::{cell::Cell, marker::PhantomData};

thread_local! { static DISPATCHING: Cell<bool> = const { Cell::new(false) }; }
struct DispatchGuard(bool);
impl Drop for DispatchGuard {
    fn drop(&mut self) {
        DISPATCHING.set(self.0);
    }
}
pub(super) struct HostOnlyGuard;
impl ProcessExecutionGuard for HostOnlyGuard {
    fn enter(&self) -> Result<Box<dyn std::any::Any>, Failure> {
        let guard = DispatchGuard(DISPATCHING.replace(true));
        Ok(Box::new(guard))
    }
}

pub struct DerivedRuntime {
    kernel: Kernel,
}
impl DerivedRuntime {
    pub fn build(
        name: impl Into<String>,
        snapshots: impl IntoIterator<Item = PluginSnapshot>,
        dispatcher: Arc<dyn ProcessDispatcher>,
    ) -> Result<Self, Error> {
        Self::build_with_options(
            name,
            snapshots,
            BuildOptions::default().with_dispatcher(dispatcher),
        )
    }
    pub fn build_with_options(
        name: impl Into<String>,
        snapshots: impl IntoIterator<Item = PluginSnapshot>,
        mut options: BuildOptions,
    ) -> Result<Self, Error> {
        Self::guard_options(&mut options.execution);
        Ok(Self {
            kernel: Kernel::build_with_options(name, snapshots, options)
                .map_err(|e| Error::Kernel(Box::new(e)))?,
        })
    }
    pub(super) fn guard_options(options: &mut SessionOptions) {
        options.execution_guards.insert(0, Arc::new(HostOnlyGuard));
        options.request_failure_policy = RequestFailurePolicy::RestartAffectedProcess;
    }
    pub fn kernel(&self) -> &Kernel {
        &self.kernel
    }
    pub fn drain(&self, max_polls: usize) -> Result<Vec<ProducerEvent>, DriverError> {
        self.kernel.drain(max_polls)
    }
    pub fn run_until(
        &mut self,
        wake: &WakeHandle,
        on_event: impl FnMut(ProducerEvent),
    ) -> Result<(), Error> {
        self.kernel
            .run_until(wake, on_event)
            .map_err(|e| Error::Kernel(Box::new(e)))
    }
    pub fn start(&mut self) -> Result<(), Error> {
        self.kernel.start().map_err(|e| Error::Kernel(Box::new(e)))
    }
    pub fn bind_query<T: Clone + Send + Sync + 'static>(
        &self,
        spec: &QuerySpec<T>,
    ) -> Result<QueryHandle<'_, T>, Error> {
        spec.bind_kernel(&self.kernel)
    }
}

/// Final-declaration identity and refresh lineage. Reusing a declaration in two
/// physical instances is ambiguous even when their shapes and names agree.
#[derive(Clone)]
pub struct QuerySpec<T: Clone + Send + Sync + 'static> {
    definition: DefinitionKey,
    capture: String,
    value: DerivedRef<T>,
}
impl<T: Clone + Send + Sync + 'static> QuerySpec<T> {
    pub fn bind_kernel<'a>(&self, kernel: &'a Kernel) -> Result<QueryHandle<'a, T>, Error> {
        self.bind_preparation(&kernel.binding())?.bind_kernel(kernel)
    }
    /// Resolve immutable authority during preparation, without admission or reads.
    /// The result can only execute with this exact compiled generation's Kernel.
    pub fn bind_preparation(
        &self,
        binding: &phenix_kernel::host::Binding<'_>,
    ) -> Result<QueryBinding<T>, Error> {
        // Definition errors precede route errors, including ambiguous occurrences.
        let tap = binding.bind_definition(self.definition)?;
        let root = binding.bind_definition(self.value.root.definition.definition_key())?;
        let refresh = binding
            .bind_ingress(self.value.refresh_route(), InputKey::new("run"))
            .map_err(|_| Error::InvalidRefreshRoute(self.value.refresh_route().into()))?;
        if !refresh.matches_input(&root.input_key(InputKey::new("run"))?)? {
            return Err(Error::InvalidRefreshRoute(
                self.value.refresh_route().into(),
            ));
        }
        let selected = binding
            .bind_capture(&self.capture, OutputKey::<T>::new("value"))
            .map_err(|_| Error::InvalidPublication)?;
        if !selected.matches_output(&tap.output_key(OutputKey::<T>::new("value"))?)? {
            return Err(Error::InvalidPublication);
        }
        let root_value = binding
            .capture(&self.value.root.capture)
            .map_err(|_| Error::InvalidPublication)?;
        if !(self.value.root.matches_value)(&root, &root_value)? {
            return Err(Error::InvalidPublication);
        }
        let errors = binding
            .bind_capture(&self.value.root.errors, OutputKey::<Failure>::new("error"))
            .map_err(|_| Error::InvalidPublication)?;
        if !errors.matches_output(&root.output_key(OutputKey::<Failure>::new("error"))?)? {
            return Err(Error::InvalidPublication);
        }
        let mut captures = Vec::new();
        // Enumerate aliases only at binding. Erased captures include unrelated
        // heterogeneous observations; every export can backpressure publication.
        for name in binding.declaration("")?.interface().outputs().keys() {
            let handle = if name == selected.name() {
                selected.clone()
            } else if name == root_value.name() {
                root_value.clone()
            } else if name == errors.name() {
                errors.clone()
            } else {
                binding.capture(name)?
            };
            captures.push(QueryCapture {
                selected: name == selected.name(),
                root: name == root_value.name(),
                errors: name == errors.name(),
                handle,
            });
        }
        Ok(QueryBinding {
            tap,
            root,
            refresh,
            captures,
            marker: PhantomData,
        })
    }

    pub fn validate_binding(
        &self,
        binding: &phenix_kernel::host::Binding<'_>,
    ) -> Result<(), Error> {
        self.bind_preparation(binding).map(drop)
    }
}

/// Immutable routes for one resolved generation. Cloning never rebinds authority
/// or retains a candidate, session, queued value, or authored Storage context.
#[derive(Clone)]
pub struct QueryBinding<T: Clone + Send + Sync + 'static> {
    tap: BoundDeclaration,
    root: BoundDeclaration,
    refresh: IngressHandle<RefreshCause>,
    captures: Vec<QueryCapture>,
    marker: PhantomData<fn() -> T>,
}
#[derive(Clone)]
struct QueryCapture {
    handle: CaptureHandle,
    selected: bool,
    root: bool,
    errors: bool,
}
impl<T: Clone + Send + Sync + 'static> QueryBinding<T> {
    fn validate(&self, compiled: &CompiledNetwork) -> Result<(), Error> {
        self.tap.validate(compiled)?;
        self.root.validate(compiled)?;
        self.refresh.validate(compiled)?;
        for capture in &self.captures {
            capture.handle.validate(compiled)?;
        }
        Ok(())
    }
    /// Adopt previously resolved authority, never a same-named successor route.
    pub fn bind_kernel<'a>(&self, kernel: &'a Kernel) -> Result<QueryHandle<'a, T>, Error> {
        self.validate(kernel.compiled())?;
        Ok(QueryHandle {
            kernel,
            binding: self.clone(),
        })
    }
}

pub struct QueryHandle<'a, T: Clone + Send + Sync + 'static> {
    kernel: &'a Kernel,
    binding: QueryBinding<T>,
}
impl<T: Clone + Send + Sync + 'static> QueryHandle<'_, T> {
    pub fn query(&self) -> Result<Leased<T>, Error> {
        self.query_with_budget(65_536)
    }
    /// Bounded synchronous driver. A deferred dispatcher must complete within the
    /// budget; unresolved work cancels only this request. Runner-owned invalidated
    /// jobs are returned in `Error::RequestInterrupted`, never silently forgotten.
    pub fn query_with_budget(&self, budget: usize) -> Result<Leased<T>, Error> {
        if DISPATCHING.get() {
            return Err(Error::NestedQuery);
        }
        self.binding.validate(self.kernel.compiled())?;
        self.kernel
            .with_session(|session| {
                let request = session.begin_bound_request(None)?;
                let result = (|| {
                    let mut remaining = budget;
                    let mut value = self.binding.refresh.value(RefreshCause::Demand);
                    loop {
                        match session.inject_bound_request_value(
                            request,
                            &self.binding.refresh,
                            value,
                        ) {
                            Ok(()) => break,
                            Err(error)
                                if error.reason
                                    == phenix_graph::runtime::RequestError::Transport(
                                        phenix_graph::connection::ConnectionError::Full,
                                    ) =>
                            {
                                value = error.value;
                                let report = session.run_until_idle(remaining.min(64));
                                remaining = remaining.saturating_sub(report.steps);
                                let mut drained = false;
                                for output in &self.binding.captures {
                                    while session
                                        .take_bound_unscoped_output(&output.handle)
                                        .map_err(Error::Transport)?
                                        .is_some()
                                    {
                                        drained = true;
                                    }
                                }
                                if report.state == SessionState::Failed {
                                    return Err(session
                                        .failure()
                                        .cloned()
                                        .map(Error::Execution)
                                        .unwrap_or(Error::InvalidPublication));
                                }
                                if remaining == 0 || (report.steps == 0 && !drained) {
                                    return Err(Error::Unsettled);
                                }
                            }
                            Err(error) => return Err(Error::Request(error.reason)),
                        }
                    }
                    session.close_request(request)?;
                    let mut selected = None;
                    let mut count = 0;
                    let mut roots = 0;
                    let mut errors = Vec::new();
                    loop {
                        let report = session.run_until_idle(remaining.min(64));
                        remaining = remaining.saturating_sub(report.steps);
                        match session.request_status(request)? {
                            RequestStatus::Failed => {
                                return Err(session
                                    .request_failure(request)?
                                    .cloned()
                                    .map(Error::Execution)
                                    .unwrap_or(Error::InvalidPublication));
                            }
                            RequestStatus::Cancelled => return Err(Error::InvalidPublication),
                            _ => {}
                        }
                        if report.state == SessionState::Failed {
                            return Err(session
                                .failure()
                                .cloned()
                                .map(Error::Execution)
                                .unwrap_or(Error::InvalidPublication));
                        }
                        let mut drained = false;
                        for output in &self.binding.captures {
                            // Only unscoped observations are expendable. Another
                            // live request's capture must remain under its owner.
                            loop {
                                let packet = match session
                                    .take_bound_request_output(request, &output.handle)?
                                {
                                    Some(packet) => packet,
                                    None => match session
                                        .take_bound_unscoped_output(&output.handle)
                                        .map_err(Error::Transport)?
                                    {
                                        Some(packet) => packet,
                                        None => break,
                                    },
                                };
                                drained = true;
                                if packet.metadata().and_then(|m| m.request()) != Some(request) {
                                    continue;
                                }
                                session.validate_bound_capture(request, &output.handle, &packet)?;
                                if output.root {
                                    roots += 1;
                                }
                                if output.errors {
                                    errors.push(
                                        Failure::from_value(packet.value())
                                            .map_err(Error::Execution)?,
                                    );
                                }
                                if output.selected {
                                    count += 1;
                                    selected = Some(
                                        packet
                                            .value()
                                            .leased_shared::<T>()
                                            .map_err(|_| Error::InvalidPublication)?,
                                    );
                                }
                            }
                        }
                        match session.request_status(request)? {
                            RequestStatus::Settled { .. } => {
                                if !errors.is_empty() {
                                    return Err(Error::Wave(errors));
                                }
                                if roots != 1 {
                                    return Err(Error::InvalidPublication);
                                }
                                if count != 1 {
                                    return Err(Error::PublicationCount(count));
                                }
                                return selected.ok_or(Error::InvalidPublication);
                            }
                            RequestStatus::Cancelled => return Err(Error::InvalidPublication),
                            RequestStatus::Failed => {
                                return Err(session
                                    .request_failure(request)?
                                    .cloned()
                                    .map(Error::Execution)
                                    .unwrap_or(Error::InvalidPublication));
                            }
                            _ => {}
                        }
                        // Removing a finite buffered capture can unblock a retained
                        // publication even when the preceding drive took no steps.
                        if remaining == 0 || (report.steps == 0 && !drained) {
                            return Err(Error::Unsettled);
                        }
                    }
                })();
                let before_failure = session.request_failure(request).ok().flatten().cloned();
                let mut inventory = Vec::new();
                match session.request_status(request) {
                    Ok(RequestStatus::Open | RequestStatus::Pending) => {
                        if let Err(error) = session.close_request(request) {
                            inventory.push((QueryCleanupStage::Close, error));
                        }
                    }
                    Ok(_) => {}
                    Err(error) => inventory.push((QueryCleanupStage::Close, error)),
                }
                // Classify cancellation before moving the non-Clone initiating error.
                let unsettled = matches!(&result, Err(Error::Unsettled));
                let (mut result, termination_error) = match result {
                    Err(error) if unsettled => (Err(error), session.cancel_request(request).err()),
                    Err(error) => {
                        let failure = match error {
                            Error::Execution(failure) => failure,
                            error => Failure::external("derived query failed", error),
                        };
                        let failure = self.kernel.lease_failure(failure);
                        let mut context = failure.context().clone();
                        context.session = Some(request.session());
                        context.generation = Some(request.generation());
                        context.request = Some(request);
                        let failure = failure.with_context(context);
                        let termination = session.fail_request(request, failure.clone()).err();
                        (Err(Error::Execution(failure)), termination)
                    }
                    Ok(value) => (Ok(value), None),
                };
                // Termination can enrich the primary via COW, or install a new
                // recovery failure. Snapshot it while the authenticated scope
                // still exists, without mistaking an older unrelated failure
                // for this query's initiating cause.
                let termination_failure = session
                    .request_failure(request)
                    .ok()
                    .flatten()
                    .cloned()
                    .and_then(|failure| {
                        if let Err(Error::Execution(primary)) = &mut result
                            && primary.same_origin(&failure)
                        {
                            let mut context = failure.context().clone();
                            context.session = Some(request.session());
                            context.generation = Some(request.generation());
                            context.request = Some(request);
                            *primary = self.kernel.lease_failure(failure.with_context(context));
                            return None;
                        }
                        let is_new = before_failure.as_ref().is_none_or(|before| {
                            !before.same_origin(&failure)
                                || failure.cleanup().len() > before.cleanup().len()
                        });
                        is_new.then(|| self.kernel.lease_failure(failure))
                    });
                // Automatic failure recovery may also invalidate runner-owned jobs.
                // Scope release destroys this inventory, so transfer it to the caller.
                let cancelled_jobs = match session.request_cancelled_jobs(request) {
                    Ok(jobs) => jobs.to_vec(),
                    Err(error) => {
                        inventory.push((QueryCleanupStage::CancelledJobs, error));
                        Vec::new()
                    }
                };
                let disposal = match session.request_disposal(request) {
                    Ok(disposal) => disposal,
                    Err(error) => {
                        inventory.push((QueryCleanupStage::Disposal, error));
                        Default::default()
                    }
                };
                let release_error = session.release_request(request).err();
                let result = match (result, termination_error, release_error) {
                    (Err(cause), termination, release)
                        if termination.is_some()
                            || termination_failure.is_some()
                            || release.is_some()
                            || !inventory.is_empty() =>
                    {
                        Err(Error::Cleanup {
                            cause: Box::new(cause),
                            request,
                            termination,
                            termination_failure,
                            release,
                            inventory,
                        })
                    }
                    (Ok(_), termination, Some(error)) => Err(Error::Cleanup {
                        cause: Box::new(Error::Request(error.clone())),
                        request,
                        termination,
                        termination_failure,
                        release: Some(error),
                        inventory,
                    }),
                    (Ok(_), termination, release) if !inventory.is_empty() => Err(Error::Cleanup {
                        cause: Box::new(Error::Request(inventory[0].1.clone())),
                        request,
                        termination,
                        termination_failure,
                        release,
                        inventory,
                    }),
                    (result, _, _) => result,
                };
                result.map_err(|cause| {
                    if cancelled_jobs.is_empty() {
                        cause
                    } else {
                        Error::RequestInterrupted {
                            cause: Box::new(cause),
                            cancelled_jobs,
                            disposal,
                        }
                    }
                })
            })
            .map_err(Error::Access)?
    }
}

pub(crate) fn lower<T: Clone + Send + Sync + 'static>(
    graph: &mut NetworkBuilder,
    name: &str,
    value: &DerivedRef<T>,
) -> Result<QuerySpec<T>, Error> {
    value.validate(graph)?;
    let definition = source::function(
        [phenix_graph::PortDef::local::<T>("run")],
        phenix_graph::PortDef::local::<T>("value"),
        |ctx| ctx.take("run").ok_or_else(|| source::missing_input("run")),
    )?;
    let spec = QuerySpec {
        definition: definition.definition_key(),
        capture: name.into(),
        value: value.clone(),
    };
    let tap = graph.add(name, definition)?;
    graph.connect(value.output(), tap.input("run")?, capacity())?;
    graph.expose_output(name, tap.output::<T>("value")?)?;
    Ok(spec)
}
