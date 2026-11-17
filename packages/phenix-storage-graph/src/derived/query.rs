use super::*;
use phenix_graph::{
    Failure, Value,
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

/// A typed value retaining the actual native code lease, including across clones.
/// Borrowing cannot escape this wrapper's lifetime. No bare owned extraction is
/// offered: T may itself contain plugin-defined code or destruction behavior.
#[derive(Clone, Debug)]
pub struct QueryValue<T: Send + Sync + 'static> {
    value: Value,
    marker: PhantomData<T>,
}
impl<T: Send + Sync + 'static> std::ops::Deref for QueryValue<T> {
    type Target = T;
    fn deref(&self) -> &T {
        self.value.borrow().expect("validated query value")
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
        Self::build_with_options(name, snapshots, dispatcher, BuildOptions::default())
    }
    pub fn build_with_options(
        name: impl Into<String>,
        snapshots: impl IntoIterator<Item = PluginSnapshot>,
        dispatcher: Arc<dyn ProcessDispatcher>,
        mut options: BuildOptions,
    ) -> Result<Self, Error> {
        Self::guard_options(&mut options.execution);
        Ok(Self {
            kernel: Kernel::build_with_options(name, snapshots, dispatcher, options)
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
        self.validate_compiled(kernel.compiled())?;
        Ok(QueryHandle {
            kernel,
            spec: self.clone(),
        })
    }
    fn validate_compiled(&self, compiled: &CompiledNetwork) -> Result<(), Error> {
        let tap = compiled.lookup_definition(self.definition)?;
        let root = compiled.lookup_definition(self.value.root.definition.definition_key())?;
        self.validate_targets(
            root.index,
            compiled
                .inputs()
                .get(self.value.refresh_route())
                .map(Vec::as_slice),
        )?;
        if compiled.outputs().get(&self.capture) != Some(&(tap.index, "value".into())) {
            return Err(Error::InvalidPublication);
        }
        if compiled.outputs().get(&self.value.root.capture) != Some(&(root.index, "value".into()))
            || compiled.outputs().get(&self.value.root.errors)
                != Some(&(root.index, "error".into()))
        {
            return Err(Error::InvalidPublication);
        }
        Ok(())
    }
    pub fn validate_binding(
        &self,
        binding: &phenix_kernel::host::Binding<'_>,
    ) -> Result<(), Error> {
        let tap = binding.lookup_definition(self.definition)?;
        let root = binding.lookup_definition(self.value.root.definition.definition_key())?;
        self.validate_targets(
            root.index,
            binding.input_targets(self.value.refresh_route()),
        )?;
        if binding.output_source(&self.capture) != Some((tap.index, "value"))
            || binding.output_source(&self.value.root.capture) != Some((root.index, "value"))
            || binding.output_source(&self.value.root.errors) != Some((root.index, "error"))
        {
            return Err(Error::InvalidPublication);
        }
        Ok(())
    }
    fn validate_targets(
        &self,
        root: usize,
        targets: Option<&[(usize, String)]>,
    ) -> Result<(), Error> {
        if targets != Some(&[(root, "run".into())][..]) {
            return Err(Error::InvalidRefreshRoute(
                self.value.refresh_route().into(),
            ));
        }
        Ok(())
    }
}

pub struct QueryHandle<'a, T: Clone + Send + Sync + 'static> {
    kernel: &'a Kernel,
    spec: QuerySpec<T>,
}
impl<T: Clone + Send + Sync + 'static> QueryHandle<'_, T> {
    pub fn query(&self) -> Result<QueryValue<T>, Error> {
        self.query_with_budget(65_536)
    }
    /// Bounded synchronous driver. A deferred dispatcher must complete within the
    /// budget; unresolved work cancels only this request. Runner-owned invalidated
    /// jobs are returned in `Error::RequestInterrupted`, never silently forgotten.
    pub fn query_with_budget(&self, budget: usize) -> Result<QueryValue<T>, Error> {
        if DISPATCHING.get() {
            return Err(Error::NestedQuery);
        }
        self.spec.validate_compiled(self.kernel.compiled())?;
        let outputs: Vec<_> = self.kernel.compiled().outputs().keys().cloned().collect();
        self.kernel
            .with_session(|session| {
                let request = session.begin_request(None)?;
                let result = (|| {
                    let mut remaining = budget;
                    let mut value = Value::local(RefreshCause::Demand);
                    loop {
                        match session.inject_request(
                            request,
                            self.spec.value.refresh_route(),
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
                                for output in &outputs {
                                    while session
                                        .take_output(output)
                                        .map_err(Error::Transport)?
                                        .is_some()
                                    {}
                                }
                                if report.state == SessionState::Failed {
                                    return Err(session
                                        .failure()
                                        .cloned()
                                        .map(Error::Execution)
                                        .unwrap_or(Error::InvalidPublication));
                                }
                                if remaining == 0 || report.steps == 0 {
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
                        for output in &outputs {
                            // Drain older observation captures too: they cannot satisfy
                            // this request, and retaining them would block its fanout.
                            loop {
                                let packet = match session.take_request_output(request, output)? {
                                    Some(packet) => packet,
                                    None => match session
                                        .take_output(output)
                                        .map_err(Error::Transport)?
                                    {
                                        Some(packet) => packet,
                                        None => break,
                                    },
                                };
                                if packet.metadata().and_then(|m| m.request()) != Some(request) {
                                    continue;
                                }
                                session.validate_capture(request, output, &packet)?;
                                if output == &self.spec.value.root.capture {
                                    roots += 1;
                                }
                                if output == &self.spec.value.root.errors {
                                    errors.push(
                                        Failure::from_value(packet.value())
                                            .map_err(Error::Execution)?,
                                    );
                                }
                                if output == &self.spec.capture {
                                    count += 1;
                                    if packet.value().borrow::<T>().is_none() {
                                        return Err(Error::InvalidPublication);
                                    }
                                    selected = Some(QueryValue {
                                        value: packet.into_value(),
                                        marker: PhantomData,
                                    });
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
                        if remaining == 0 || report.steps == 0 {
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
