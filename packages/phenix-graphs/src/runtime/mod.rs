//! Persistent cooperative execution of compiled networks.
//!
//! Captures participate in atomic publication: callers must drain them to let
//! producers progress. Idle with an open input is not end of stream.
//! Per-emission destination selection narrows existing process connections only.
//! Exported captures observe every committed emission, even an empty process
//! selection; use separate output ports when different observations are desired.
use crate::connection::{
    ConnectionError, ConnectionQueue, ConnectionSet, FanoutTarget, Packet, PreparedFanout,
};
use crate::network::CompiledNetwork;
use crate::process::{
    ActivationJob, CompletedStep, DispatchResult, HandoffError, HandoffState,
    LocalProcessDispatcher, ProcessContext, ProcessDispatcher, ProcessExecutionGuard,
    ProcessHandoff, ResourceGrant, Step, StepTicket,
};
use crate::{GraphOrigin, Value};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::num::NonZeroUsize;
use std::sync::{Arc, atomic::Ordering};
use std::time::{Duration, Instant};
mod closure;
mod diagnostics;
mod readiness;
mod recovery;
mod request;
mod wake;
pub use diagnostics::{BlockedCycle, CorrelationBlock};
pub use readiness::{InputHead, Readiness, ReadinessPolicy, ReadinessSnapshot};
pub use request::{
    CaptureMetadata, RequestDisposal, RequestError, RequestId, RequestInjectionError,
    RequestStatus, RequestWork,
};
pub use wake::{SessionNotifier, WakeHandle, WakeListener};
use wake::{WakeSignal, WakeSlot};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RequestFailurePolicy {
    #[default]
    FailSession,
    /// Spawn fresh state only for processes active in the terminated request.
    /// Shared resources and already performed external effects are not reset.
    RestartAffectedProcess,
}

struct UnavailableProcess;
impl crate::process::Process for UnavailableProcess {
    fn step(&mut self, _: &mut ProcessContext) -> Result<Step, String> {
        Err("process factory unavailable".into())
    }
}

#[derive(Clone)]
pub struct SessionOptions {
    /// Capacity of each exported-output FIFO (including aliases).
    pub capture_capacity: NonZeroUsize,
    /// Physical compiled process paths, not definition-scoped authoring handles.
    pub readiness: BTreeMap<String, Readiness>,
    /// Explicit permission to discard emissions with no connection or capture.
    /// Entries are (physical process path, declared output name).
    pub discard_outputs: BTreeSet<(String, String)>,
    pub resources: BTreeMap<String, Vec<ResourceGrant>>,
    pub max_requests: NonZeroUsize,
    pub request_failure_policy: RequestFailurePolicy,
    /// Optional host-supervised age limit for outstanding owned jobs.
    /// No background timer exists: a driver or explicit supervise_deferred
    /// call checks this bound. Late completions return StaleCompletion.
    pub deferred_timeout: Option<Duration>,
    /// Disable for request-driven source starts; only compiled sources are eligible.
    pub auto_start_sources: bool,
    pub dispatcher: Arc<dyn ProcessDispatcher>,
    pub execution_guard: Option<Arc<dyn ProcessExecutionGuard>>,
    /// Keeps native code/resources alive through jobs and queued/captured packets.
    pub lease: Option<Arc<dyn Send + Sync>>,
}
impl std::fmt::Debug for SessionOptions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SessionOptions")
            .field("capture_capacity", &self.capture_capacity)
            .field("readiness", &self.readiness)
            .field("discard_outputs", &self.discard_outputs)
            .field("resources", &self.resources)
            .field("max_requests", &self.max_requests)
            .field("request_failure_policy", &self.request_failure_policy)
            .field("deferred_timeout", &self.deferred_timeout)
            .finish_non_exhaustive()
    }
}
impl Default for SessionOptions {
    fn default() -> Self {
        Self {
            capture_capacity: NonZeroUsize::new(16).unwrap(),
            readiness: BTreeMap::new(),
            discard_outputs: BTreeSet::new(),
            resources: BTreeMap::new(),
            max_requests: NonZeroUsize::new(64).unwrap(),
            request_failure_policy: RequestFailurePolicy::FailSession,
            deferred_timeout: None,
            auto_start_sources: true,
            dispatcher: Arc::new(LocalProcessDispatcher),
            execution_guard: None,
            lease: None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SessionState {
    /// A cooperative step or retained publication can progress now.
    Runnable,
    /// No outstanding activation or buffered input; ingress/wakes may arrive.
    Quiescent,
    /// A job, read reservation, readiness threshold, or publication is blocked.
    Waiting,
    /// All public inputs closed, all queues drained, every process ended,
    /// and no dispatched jobs or retained publications remain.
    Completed,
    Cancelled,
    Failed,
}

#[derive(Debug)]
pub struct InjectionError {
    pub reason: ConnectionError,
    pub value: Value,
}
impl std::fmt::Display for InjectionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.reason.fmt(f)
    }
}
impl std::error::Error for InjectionError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.reason)
    }
}

#[derive(Clone, Debug)]
pub struct RunReport {
    pub state: SessionState,
    /// Dispatch attempts, including deferred or rejected owned jobs.
    pub steps: usize,
    pub publications: usize,
    /// Lifetime total of accepted queue packets disposed by failure/cancellation.
    pub disposed_packets: usize,
    /// Lifetime total of emissions committed to explicitly permitted discard.
    pub discarded_emissions: usize,
    pub budget_exhausted: bool,
}

/// One dispatcher protocol violation, retaining the refused object unchanged.
/// Taking this transfers its ownership back to the host; the session stays failed.
#[derive(Debug)]
pub enum DispatchReturn {
    Completed {
        error: HandoffError,
        completion: Box<CompletedStep>,
    },
    Rejected {
        error: HandoffError,
        job: ActivationJob,
    },
}

#[derive(Clone, Debug)]
pub enum QueueEndpoint {
    Process { index: usize, port: String },
    Input(String),
    Capture(String),
}
#[derive(Clone, Debug)]
pub struct QueueWait {
    pub queue: usize,
    pub producer: QueueEndpoint,
    pub consumer: QueueEndpoint,
    pub occupancy: usize,
    pub capacity: usize,
}

#[derive(Clone, Debug)]
/// One outstanding owned worker job beyond its configured age limit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OverdueJob {
    pub process: usize,
    pub ticket: StepTicket,
}

pub struct ActivationMetadata {
    pub session: u64,
    pub generation: u64,
    pub request: Option<RequestId>,
    pub process: usize,
    pub path: String,
    pub activation: u64,
    pub step: u64,
    pub output: Option<String>,
    pub origin: Option<GraphOrigin>,
}

struct InputPort {
    name: String,
    queues: Vec<usize>,
    cursor: usize,
    eos_delivered: bool,
}
struct RuntimeProcess {
    handoff: ProcessHandoff,
    inputs: Vec<InputPort>,
    outputs: BTreeMap<String, Vec<usize>>,
    pending: Option<PreparedFanout>,
    pending_targets: Vec<usize>,
    waiting: Option<String>,
    waiting_policy: Option<Readiness>,
    policy: Readiness,
    cursor: usize,
    queued: bool,
    done: bool,
    source: bool,
    wake: Arc<WakeSlot>,
    request: Option<RequestId>,
    resources: Vec<ResourceGrant>,
    closed_in_activation: BTreeSet<usize>,
}

pub struct NetworkSession {
    compiled: CompiledNetwork,
    transport: ConnectionSet,
    processes: Vec<RuntimeProcess>,
    ingress: BTreeMap<String, Vec<usize>>,
    captures: BTreeMap<String, usize>,
    endpoints: Vec<(QueueEndpoint, QueueEndpoint)>,
    ready: VecDeque<usize>,
    terminal: Option<SessionState>,
    failure: Option<String>,
    publications: usize,
    disposed_packets: usize,
    discarded_emissions: usize,
    input_failure: Vec<QueueWait>,
    discard_outputs: BTreeSet<(String, String)>,
    last_step: Option<ActivationMetadata>,
    session_id: u64,
    generation: u64,
    next_request: u64,
    max_requests: usize,
    request_failure_policy: RequestFailurePolicy,
    deferred_timeout: Option<Duration>,
    dispatched_at: BTreeMap<usize, Instant>,
    requests: BTreeMap<RequestId, request::RequestScope>,
    signal: Arc<WakeSignal>,
    dispatcher: Arc<dyn ProcessDispatcher>,
    dispatch_return: Option<DispatchReturn>,
    execution_guard: Option<Arc<dyn ProcessExecutionGuard>>,
    // Last field: compiled factories, process state, transports, dispatchers,
    // grants and policy trait objects must be destroyed before the code lease.
    lease: Option<Arc<dyn Send + Sync>>,
}

impl CompiledNetwork {
    pub fn start(&self) -> NetworkSession {
        self.start_with_options(SessionOptions::default())
    }
    pub fn start_with_options(&self, options: SessionOptions) -> NetworkSession {
        NetworkSession::new(self.clone(), options)
    }
}

impl NetworkSession {
    fn new(compiled: CompiledNetwork, mut options: SessionOptions) -> Self {
        if let Some(lease) = compiled.execution_lease() {
            options.lease = Some(Arc::new((options.lease.take(), lease)));
        }
        let invalid_path = options
            .readiness
            .keys()
            .chain(options.resources.keys())
            .find(|path| {
                !compiled
                    .processes()
                    .iter()
                    .any(|p| p.path() == path.as_str())
            })
            .cloned();
        // Reject static configuration before invoking any plugin-owned factory.
        // Readiness policy callbacks are never evaluated during this preflight.
        let mut initial_failure = invalid_path
            .map(|path| format!("unknown process configuration: {path}"))
            .or_else(|| {
                options
                    .discard_outputs
                    .iter()
                    .find(|(path, output)| {
                        !compiled.processes().iter().any(|p| {
                            p.path() == path && p.interface().outputs().contains_key(output)
                        })
                    })
                    .map(|(path, output)| format!("invalid discard output: {path}.{output}"))
            })
            .or_else(|| {
                compiled.processes().iter().find_map(|process| {
                    options.readiness.get(process.path()).and_then(|policy| {
                        readiness::validate_policy_ports(
                            process.interface().inputs().keys().map(String::as_str),
                            policy,
                        )
                        .err()
                        .map(|error| format!("process {}: {error}", process.path()))
                    })
                })
            });
        let mut processes: Vec<_> = compiled
            .processes()
            .iter()
            .enumerate()
            .map(|(index, p)| RuntimeProcess {
                handoff: {
                    let process = if initial_failure.is_some() {
                        Box::new(UnavailableProcess) as Box<dyn crate::process::Process>
                    } else {
                        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                            p.factory().spawn()
                        })) {
                            Ok(process) => process,
                            Err(_) => {
                                initial_failure =
                                    Some(format!("process factory panicked: {}", p.path()));
                                Box::new(UnavailableProcess)
                            }
                        }
                    };
                    let handoff = ProcessHandoff::new(process);
                    match &options.lease {
                        Some(lease) => handoff.with_lease(lease.clone()),
                        None => handoff,
                    }
                },
                inputs: p
                    .interface()
                    .inputs()
                    .keys()
                    .map(|name| InputPort {
                        name: name.clone(),
                        queues: vec![],
                        cursor: 0,
                        eos_delivered: false,
                    })
                    .collect(),
                outputs: p
                    .interface()
                    .outputs()
                    .keys()
                    .map(|name| (name.clone(), vec![]))
                    .collect(),
                pending: None,
                pending_targets: vec![],
                waiting: None,
                waiting_policy: None,
                policy: options.readiness.get(p.path()).cloned().unwrap_or_default(),
                cursor: 0,
                queued: false,
                done: false,
                source: compiled.source_starts().contains(&index),
                wake: Arc::new(WakeSlot::default()),
                request: None,
                resources: options.resources.get(p.path()).cloned().unwrap_or_default(),
                closed_in_activation: BTreeSet::new(),
            })
            .collect();
        let mut queues = vec![];
        let mut endpoints = vec![];
        let mut ingress = BTreeMap::new();
        let mut captures = BTreeMap::new();
        for edge in compiled.connections() {
            let (from, output) = edge.from();
            let (to, input) = edge.to();
            let index = queues.len();
            queues.push(ConnectionQueue::new(
                edge.capacity(),
                compiled.processes()[from].interface().outputs()[output]
                    .value
                    .clone(),
            ));
            processes[from].outputs.get_mut(output).unwrap().push(index);
            processes[to]
                .inputs
                .iter_mut()
                .find(|p| p.name == input)
                .unwrap()
                .queues
                .push(index);
            endpoints.push((
                QueueEndpoint::Process {
                    index: from,
                    port: output.into(),
                },
                QueueEndpoint::Process {
                    index: to,
                    port: input.into(),
                },
            ));
        }
        for (name, targets) in compiled.inputs() {
            let mut indices = vec![];
            for (to, input) in targets {
                let index = queues.len();
                queues.push(ConnectionQueue::new(
                    compiled.ingress_capacities()[name],
                    compiled.interface().inputs()[name].value.clone(),
                ));
                processes[*to]
                    .inputs
                    .iter_mut()
                    .find(|p| p.name == *input)
                    .unwrap()
                    .queues
                    .push(index);
                endpoints.push((
                    QueueEndpoint::Input(name.clone()),
                    QueueEndpoint::Process {
                        index: *to,
                        port: input.clone(),
                    },
                ));
                indices.push(index);
            }
            ingress.insert(name.clone(), indices);
        }
        for (name, (from, output)) in compiled.outputs() {
            let index = queues.len();
            queues.push(ConnectionQueue::new(
                options.capture_capacity,
                compiled.interface().outputs()[name].value.clone(),
            ));
            processes[*from]
                .outputs
                .get_mut(output)
                .unwrap()
                .push(index);
            endpoints.push((
                QueueEndpoint::Process {
                    index: *from,
                    port: output.clone(),
                },
                QueueEndpoint::Capture(name.clone()),
            ));
            captures.insert(name.clone(), index);
        }
        let mut session = Self {
            compiled,
            transport: ConnectionSet::new(queues),
            processes,
            ingress,
            captures,
            endpoints,
            ready: VecDeque::new(),
            terminal: None,
            failure: None,
            publications: 0,
            disposed_packets: 0,
            discarded_emissions: 0,
            input_failure: vec![],
            discard_outputs: options.discard_outputs,
            last_step: None,
            session_id: request::session_id(),
            generation: 0,
            next_request: 0,
            max_requests: options.max_requests.get(),
            request_failure_policy: options.request_failure_policy,
            deferred_timeout: options.deferred_timeout,
            dispatched_at: BTreeMap::new(),
            requests: BTreeMap::new(),
            signal: WakeSignal::new(),
            dispatcher: options.dispatcher,
            dispatch_return: None,
            execution_guard: options.execution_guard,
            lease: options.lease,
        };
        if let Some(error) = initial_failure {
            session.fail(error);
            return session;
        }
        for index in 0..session.processes.len() {
            if options.auto_start_sources && session.processes[index].source {
                let resources = session.processes[index].resources.clone();
                session.processes[index]
                    .handoff
                    .admit(ProcessContext::default().with_resources(resources))
                    .unwrap();
            }
        }
        session.refresh();
        session
    }

    pub fn inject_local<T: Send + Sync + 'static>(
        &mut self,
        name: &str,
        value: T,
    ) -> Result<(), InjectionError> {
        self.inject(name, Value::local(value))
    }
    pub fn inject(&mut self, name: &str, value: Value) -> Result<(), InjectionError> {
        self.inject_scoped(name, value, None)
    }
    fn inject_scoped(
        &mut self,
        name: &str,
        value: Value,
        request: Option<RequestId>,
    ) -> Result<(), InjectionError> {
        let attempt = (|| {
            if self.terminal.is_some() {
                return Err(ConnectionError::Cancelled);
            }
            let indices = self
                .ingress
                .get(name)
                .ok_or(ConnectionError::InvalidDestination)?;
            if value.identity() != self.compiled.interface().inputs()[name].value {
                return Err(ConnectionError::WrongType);
            }
            let targets: Vec<_> = indices.iter().map(|i| FanoutTarget::identity(*i)).collect();
            let mut packet = Packet::new(value.clone());
            if let Some(lease) = &self.lease {
                packet = packet.with_lease(lease.clone());
            }
            packet.metadata = Some(Arc::new(CaptureMetadata {
                session: self.session_id,
                generation: self.generation,
                process: None,
                output: name.into(),
                activation: 0,
                request,
                capture: None,
            }));
            let mut transaction =
                self.transport
                    .prepare(&packet, &targets)
                    .map_err(|e| match e {
                        crate::connection::PrepareError::Connection(e) => e,
                        _ => ConnectionError::WrongType,
                    })?;
            self.transport.commit(&mut transaction)
        })();
        match attempt {
            Ok(()) => {
                if let Some(request) = request {
                    self.note_request_ingress(request, name);
                }
                self.refresh();
                Ok(())
            }
            Err(reason) => Err(InjectionError { reason, value }),
        }
    }
    pub fn close_input(&mut self, name: &str) -> Result<(), ConnectionError> {
        for index in self
            .ingress
            .get(name)
            .ok_or(ConnectionError::InvalidDestination)?
        {
            self.transport.close_writer(*index)?;
        }
        self.refresh();
        Ok(())
    }
    pub fn take_output(&mut self, name: &str) -> Result<Option<Packet>, ConnectionError> {
        let index = *self
            .captures
            .get(name)
            .ok_or(ConnectionError::InvalidDestination)?;
        let packet = self.transport.pop(index);
        if let Some(packet) = &packet {
            self.observe_capture(name, packet);
        }
        self.refresh();
        Ok(packet)
    }
    pub fn output_closed(&self, name: &str) -> Result<bool, ConnectionError> {
        let index = *self
            .captures
            .get(name)
            .ok_or(ConnectionError::InvalidDestination)?;
        Ok(self.transport.queues()[index].is_eos()
            || self.transport.queues()[index].reader_closed())
    }
    pub fn wake_handle(&self, path: &str) -> Option<WakeHandle> {
        let index = self
            .compiled
            .processes()
            .iter()
            .position(|p| p.path() == path)?;
        (!self.processes[index].done && self.terminal.is_none()).then(|| WakeHandle {
            slot: self.processes[index].wake.clone(),
            signal: self.signal.clone(),
        })
    }
    pub fn wake_listener(&self) -> WakeListener {
        WakeListener::new(self.signal.clone())
    }
    pub fn notifier(&self) -> SessionNotifier {
        SessionNotifier(self.signal.clone())
    }
    pub fn process_origin(&self, index: usize) -> Option<&GraphOrigin> {
        self.compiled.processes().get(index)?.origin()
    }
    pub fn failure(&self) -> Option<&str> {
        self.failure.as_deref()
    }
    pub fn take_dispatch_return(&mut self) -> Option<DispatchReturn> {
        self.dispatch_return.take()
    }
    pub fn disposed_packets(&self) -> usize {
        self.disposed_packets
    }
    pub fn discarded_emissions(&self) -> usize {
        self.discarded_emissions
    }
    /// Bounded snapshot of the actual input queues at the first input failure.
    pub fn stranded_inputs(&self) -> &[QueueWait] {
        &self.input_failure
    }
    /// Only the last accepted step is retained, not an execution history.
    pub fn last_step(&self) -> Option<&ActivationMetadata> {
        self.last_step.as_ref()
    }

    fn closed(&self, index: usize, port: usize) -> bool {
        self.processes[index].inputs[port]
            .queues
            .iter()
            .all(|q| self.transport.queues()[*q].is_eos())
    }
    fn finish(&mut self, index: usize) {
        if self.processes[index]
            .inputs
            .iter()
            .flat_map(|p| &p.queues)
            .any(|q| !self.transport.queues()[*q].is_empty())
        {
            self.fail_input(index, "abandoned accepted input on Finished");
            return;
        }
        let p = &mut self.processes[index];
        if p.handoff.state() == HandoffState::Idle {
            p.handoff.finish_idle().unwrap();
        }
        p.done = true;
        p.request = None;
        p.wake.active.store(false, Ordering::Release);
        for queue in p.outputs.values().flatten() {
            self.transport.close_writer(*queue).unwrap();
        }
        for queue in p.inputs.iter().flat_map(|p| &p.queues) {
            let packets = self.transport.close_reader(*queue).unwrap();
            debug_assert!(packets.is_empty());
        }
    }
    fn refresh(&mut self) {
        if self.terminal.is_some() {
            return;
        }
        // Closure can propagate backwards in compiled index order. Repeat only
        // when a process terminates, so this remains bounded by topology size.
        loop {
            let mut finished = false;
            for index in 0..self.processes.len() {
                if self.processes[index].done {
                    continue;
                }
                let mut state = self.processes[index].handoff.state();
                if state == HandoffState::WaitingWake
                    && self.processes[index]
                        .wake
                        .pending
                        .swap(false, Ordering::AcqRel)
                {
                    self.processes[index].handoff.resume_wake().unwrap();
                    state = HandoffState::Ready;
                }
                if matches!(state, HandoffState::Idle | HandoffState::WaitingInput) {
                    let selected = match self.selection(index) {
                        Ok(selected) => selected,
                        Err(error) => {
                            self.fail_process(index, format!("process {index}: {error}"));
                            return;
                        }
                    };
                    let wake = state == HandoffState::Idle
                        && self.processes[index].source
                        && self.processes[index]
                            .wake
                            .pending
                            .swap(false, Ordering::AcqRel);
                    if !selected.ports.is_empty() || wake {
                        let context = self.reserve(index, selected);
                        let result = if state == HandoffState::Idle {
                            self.processes[index].handoff.admit(context)
                        } else {
                            self.processes[index].handoff.resume_input(context)
                        };
                        if let Err((error, _)) = result {
                            self.fail_process(
                                index,
                                format!("process {index} reservation: {error}"),
                            );
                            return;
                        }
                        self.processes[index].waiting = None;
                        self.processes[index].waiting_policy = None;
                    } else if state == HandoffState::Idle
                        && !self.processes[index].source
                        && self.processes[index].inputs.iter().all(|p| p.eos_delivered)
                    {
                        self.finish(index);
                        if self.terminal.is_some() {
                            return;
                        }
                        finished = true;
                        continue;
                    } else if self.stranded_at_eos(index) {
                        self.fail_input(
                            index,
                            "stranded input: all writers closed and no legal reservation",
                        );
                        return;
                    }
                }
                let p = &self.processes[index];
                let runnable = p.handoff.state() == HandoffState::Ready
                    || (p.pending.is_some()
                        && (p.pending_targets.iter().any(|q| {
                            self.transport.queues()[*q].reader_closed()
                                || self.transport.queues()[*q].writer_closed()
                        }) || p
                            .pending_targets
                            .iter()
                            .all(|q| !self.transport.queues()[*q].is_full())));
                if runnable && !p.queued {
                    self.processes[index].queued = true;
                    self.ready.push_back(index);
                }
            }
            if !finished {
                break;
            }
        }
    }

    /// Passive inspection of outstanding jobs whose configured age limit
    /// elapsed. Never mutates a job, invokes a worker or reads wall-clock time.
    pub fn overdue_jobs(&self, now: Instant) -> Vec<OverdueJob> {
        let Some(timeout) = self.deferred_timeout else {
            return Vec::new();
        };
        if self.terminal.is_some() {
            return Vec::new();
        }
        self.dispatched_at
            .iter()
            .filter(|(_, admitted)| now.saturating_duration_since(**admitted) >= timeout)
            .filter_map(|(index, _)| {
                self.processes[*index]
                    .handoff
                    .outstanding()
                    .map(|ticket| OverdueJob {
                        process: *index,
                        ticket,
                    })
            })
            .collect()
    }

    /// Host-driven watchdog. Failing a session invalidates outstanding
    /// tickets but cannot revoke worker ownership or undo external effects.
    pub fn supervise_deferred(&mut self, now: Instant) -> Vec<OverdueJob> {
        let overdue = self.overdue_jobs(now);
        if let Some(first) = overdue.first() {
            self.fail(format!(
                "deferred job timeout for process {} at activation {} step {}",
                first.process,
                first.ticket.activation(),
                first.ticket.sequence()
            ));
        }
        overdue
    }

    /// Ownership stays in the handoff. The driver may run jobs on other threads.
    /// Publication retries happen here, never by re-running a completed step.
    pub fn next_job(&mut self) -> Option<(usize, ActivationJob)> {
        self.supervise_deferred(Instant::now());
        self.refresh();
        while let Some(index) = self.ready.pop_front() {
            self.processes[index].queued = false;
            if self.processes[index].pending.is_some() {
                self.publish(index);
                self.refresh();
                if self.terminal.is_some() {
                    return None;
                }
                continue;
            }
            let mut job = self.processes[index].handoff.next_job().unwrap();
            let ticket = job.ticket();
            if self.deferred_timeout.is_some() {
                self.dispatched_at.insert(index, Instant::now());
            }
            job = job.with_metadata(ActivationMetadata {
                session: self.session_id,
                generation: self.generation,
                request: self.processes[index].request,
                process: index,
                path: self.compiled.processes()[index].path().into(),
                activation: ticket.activation(),
                step: ticket.sequence(),
                output: None,
                origin: self.compiled.processes()[index].origin().cloned(),
            });
            if let Some(guard) = &self.execution_guard {
                job = job.with_guard(guard.clone());
            }
            return Some((index, job));
        }
        None
    }
    pub fn complete(
        &mut self,
        index: usize,
        completion: CompletedStep,
    ) -> Result<(), (HandoffError, Box<CompletedStep>)> {
        let ticket = completion.ticket();
        let Some(p) = self.processes.get_mut(index) else {
            return Err((HandoffError::ForeignCompletion, Box::new(completion)));
        };
        let result = p.handoff.complete(completion)?;
        self.dispatched_at.remove(&index);
        self.last_step = Some(ActivationMetadata {
            session: self.session_id,
            generation: self.generation,
            request: self.processes[index].request,
            process: index,
            path: self.compiled.processes()[index].path().into(),
            activation: ticket.activation(),
            step: ticket.sequence(),
            output: match &result {
                Ok(Step::Emit(emission)) => Some(emission.output.clone()),
                _ => None,
            },
            origin: self.compiled.processes()[index].origin().cloned(),
        });
        match result {
            Ok(Step::Emit(emission)) => {
                let valid = self.compiled.processes()[index]
                    .interface()
                    .outputs()
                    .get(&emission.output)
                    .is_some_and(|p| p.value == emission.value.identity());
                if !valid {
                    self.fail_process(
                        index,
                        format!("invalid emission from process {index}: {}", emission.output),
                    );
                    return Ok(());
                }
                let outgoing = &self.processes[index].outputs[&emission.output];
                let indices = if let Some(destinations) = &emission.destinations {
                    let selection = (|| -> Result<Vec<usize>, String> {
                        let budget = outgoing
                            .iter()
                            .filter(|q| {
                                matches!(self.endpoints[**q].1, QueueEndpoint::Process { .. })
                            })
                            .count();
                        // Reject oversized process-produced lists before allocating
                        // routing maps, copying targets, or preparing any payload.
                        if destinations.len() > budget {
                            return Err(format!(
                                "selection exceeds compiled outgoing connection budget ({budget})"
                            ));
                        }
                        let connected: BTreeMap<_, _> = outgoing
                            .iter()
                            .filter_map(|q| match &self.endpoints[*q].1 {
                                QueueEndpoint::Process { index, port } => {
                                    Some(((*index, port.as_str()), *q))
                                }
                                _ => None,
                            })
                            .collect();
                        let mut selected = BTreeSet::new();
                        for (position, path) in destinations.iter().enumerate() {
                            let resolved = self.compiled.resolve_input(path).ok_or_else(|| {
                                format!("unknown selected input at destination {position}")
                            })?;
                            if resolved.is_empty() {
                                return Err("selected input has no physical destinations".into());
                            }
                            for (target, input) in resolved {
                                let queue =
                                    connected.get(&(*target, input.as_str())).ok_or_else(|| {
                                        format!(
                                            "selected input {}.{} is not connected to this output",
                                            self.compiled.processes()[*target].path(),
                                            input
                                        )
                                    })?;
                                if !selected.insert(*queue) {
                                    return Err(format!(
                                        "duplicate selected input after alias resolution: {}.{}",
                                        self.compiled.processes()[*target].path(),
                                        input
                                    ));
                                }
                            }
                        }
                        Ok(outgoing
                            .iter()
                            .copied()
                            .filter(|q| {
                                selected.contains(q)
                                    || matches!(self.endpoints[*q].1, QueueEndpoint::Capture(_))
                            })
                            .collect())
                    })();
                    match selection {
                        Ok(indices) => indices,
                        Err(error) => {
                            self.fail_process(index, error);
                            return Ok(());
                        }
                    }
                } else {
                    outgoing.clone()
                };
                if indices.is_empty()
                    && emission.destinations.is_none()
                    && !self.discard_outputs.contains(&(
                        self.compiled.processes()[index].path().to_owned(),
                        emission.output.clone(),
                    ))
                {
                    self.fail_process(
                        index,
                        format!(
                            "unconnected output {}.{} requires explicit discard_outputs",
                            self.compiled.processes()[index].path(),
                            emission.output
                        ),
                    );
                    return Ok(());
                }
                let targets: Vec<_> = indices.iter().map(|q| FanoutTarget::identity(*q)).collect();
                let mut packet = Packet::new(emission.value);
                packet.origin = self.compiled.processes()[index].origin().cloned();
                if let Some(lease) = &self.lease {
                    packet = packet.with_lease(lease.clone());
                }
                packet.metadata = Some(Arc::new(CaptureMetadata {
                    session: self.session_id,
                    generation: self.generation,
                    process: Some(index),
                    output: emission.output,
                    activation: ticket.activation(),
                    request: self.processes[index].request,
                    capture: None,
                }));
                match self.transport.prepare(&packet, &targets) {
                    Ok(mut transaction) => {
                        for queue in &indices {
                            if let QueueEndpoint::Capture(name) = &self.endpoints[*queue].1 {
                                transaction.stamp_capture(*queue, name);
                            }
                        }
                        self.processes[index].pending = Some(transaction);
                        self.processes[index].pending_targets = indices;
                    }
                    Err(error) => self.fail_process(index, error.to_string()),
                }
            }
            Ok(Step::WaitInput(name)) => {
                if !self.processes[index].inputs.iter().any(|p| p.name == name) {
                    self.fail_process(index, format!("unknown WaitInput port: {name}"));
                } else {
                    self.processes[index].waiting = Some(name);
                }
            }
            Ok(Step::WaitInputs(policy)) => {
                self.processes[index].waiting_policy = Some(policy);
            }
            Ok(Step::WaitWake) => {}
            Ok(Step::Finished) => self.finish(index),
            Ok(Step::Complete) => {
                self.processes[index].request = None;
            }
            Ok(Step::Yield) => {}
            Err(error) => self.fail_process(index, format!("process {index}: {error}")),
        }
        self.refresh();
        Ok(())
    }
    pub fn reject(
        &mut self,
        index: usize,
        job: ActivationJob,
    ) -> Result<(), (HandoffError, ActivationJob)> {
        let Some(p) = self.processes.get_mut(index) else {
            return Err((HandoffError::ForeignCompletion, job));
        };
        p.handoff.reject(job)?;
        self.dispatched_at.remove(&index);
        self.refresh();
        Ok(())
    }
    pub fn complete_job(
        &mut self,
        completion: CompletedStep,
    ) -> Result<(), (HandoffError, Box<CompletedStep>)> {
        let Some(index) = self
            .processes
            .iter()
            .position(|p| p.handoff.owns(completion.ticket()))
        else {
            return Err((HandoffError::ForeignCompletion, Box::new(completion)));
        };
        self.complete(index, completion)
    }
    fn publish(&mut self, index: usize) {
        let transaction = self.processes[index].pending.as_mut().unwrap();
        match self.transport.commit(transaction) {
            Ok(()) => {
                if self.processes[index].pending_targets.is_empty() {
                    self.discarded_emissions = self.discarded_emissions.saturating_add(1);
                }
                self.processes[index].pending = None;
                self.processes[index].pending_targets.clear();
                self.processes[index]
                    .handoff
                    .publication_committed()
                    .unwrap();
                self.publications = self.publications.saturating_add(1);
            }
            Err(ConnectionError::Full) => {}
            Err(error) => self.fail_process(index, format!("process {index} publication: {error}")),
        }
    }
    fn fail(&mut self, error: String) {
        // Failure is not rollback: retain committed captures for host draining.
        self.stop_processes();
        for index in 0..self.transport.queues().len() {
            self.transport.close_writer(index).unwrap();
            if !matches!(self.endpoints[index].1, QueueEndpoint::Capture(_)) {
                for packet in self.transport.queues()[index].packets() {
                    if let Some(request) = packet.metadata().and_then(|m| m.request())
                        && let Some(scope) = self.requests.get_mut(&request)
                    {
                        scope.disposal.queued_packets =
                            scope.disposal.queued_packets.saturating_add(1);
                    }
                }
                self.disposed_packets = self
                    .disposed_packets
                    .saturating_add(self.transport.close_reader(index).unwrap().len());
            }
        }
        self.failure = Some(error);
        self.terminal = Some(SessionState::Failed);
    }
    fn stop_processes(&mut self) {
        self.dispatched_at.clear();
        for p in &mut self.processes {
            let ticket = p.handoff.cancel();
            if let Some(request) = p.request
                && let Some(scope) = self.requests.get_mut(&request)
            {
                scope.disposal.activations += 1;
                if let Some(pending) = &p.pending {
                    scope.disposal.prepared_packets += pending.retained_packets();
                }
                if let Some(ticket) = ticket {
                    scope.cancelled_jobs.push(ticket);
                    scope.disposal.handed_jobs += 1;
                }
            }
            p.pending = None;
            p.pending_targets.clear();
            p.wake.active.store(false, Ordering::Release);
            p.request = None;
        }
        self.ready.clear();
        self.signal.invalidate();
    }
    pub fn cancel(&mut self) {
        // A terminal result is final. In particular, do not turn a failure
        // into cancellation or dispose captures intentionally retained by fail.
        if self.terminal.is_some() || self.is_completed() {
            return;
        }
        self.stop_processes();
        for queue in self.transport.queues() {
            for packet in queue.packets() {
                if let Some(request) = packet.metadata().and_then(|m| m.request())
                    && let Some(scope) = self.requests.get_mut(&request)
                {
                    scope.disposal.queued_packets = scope.disposal.queued_packets.saturating_add(1);
                }
            }
        }
        self.disposed_packets = self
            .disposed_packets
            .saturating_add(self.transport.cancel());
        self.terminal = Some(SessionState::Cancelled);
    }
    fn is_completed(&self) -> bool {
        self.processes.iter().all(|p| p.done)
            && self
                .transport
                .queues()
                .iter()
                .all(ConnectionQueue::is_empty)
            && self
                .ingress
                .values()
                .flatten()
                .all(|q| self.transport.queues()[*q].writer_closed())
    }

    /// Passive snapshot of already committed scheduler state.
    /// A newly delivered wake is scheduled by the next driver operation,
    /// never by a status query; no user readiness policy runs here.
    pub fn state(&self) -> SessionState {
        if let Some(state) = self.terminal {
            return state;
        }
        if !self.ready.is_empty()
            || self.processes.iter().any(|process| {
                process.wake.pending.load(Ordering::Acquire)
                    && (process.handoff.state() == HandoffState::WaitingWake
                        || (process.source && process.handoff.state() == HandoffState::Idle))
            })
        {
            return SessionState::Runnable;
        }
        let drained = self.transport.queues().iter().all(|q| q.is_empty());
        if self.is_completed() {
            return SessionState::Completed;
        }
        if !drained
            || self.requests.iter().any(|(request, scope)| {
                scope.sealed
                    && (0..self.processes.len())
                        .any(|index| self.closure_obligation(index, *request))
            })
            || self.processes.iter().any(|p| {
                p.pending.is_some()
                    || p.inputs
                        .iter()
                        .flat_map(|p| &p.queues)
                        .any(|q| !self.transport.queues()[*q].is_empty())
                    || matches!(
                        p.handoff.state(),
                        HandoffState::Dispatched
                            | HandoffState::WaitingInput
                            | HandoffState::WaitingWake
                    )
            })
        {
            return SessionState::Waiting;
        }
        SessionState::Quiescent
    }
    /// The budget counts process steps; publication retries do not execute code
    /// and are bounded by the number of retained per-process transactions.
    /// Both drivers stop at quiescence or external backpressure. Neither spins
    /// waiting for an ingress, capture consumer, worker completion, or wake.
    pub fn run_until_idle(&mut self, budget: usize) -> RunReport {
        self.supervise_deferred(Instant::now());
        let before = self.publications;
        let mut steps = 0;
        while steps < budget {
            let Some((index, job)) = self.next_job() else {
                break;
            };
            steps += 1;
            let dispatched = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                self.dispatcher.dispatch(job)
            }));
            let dispatched = match dispatched {
                Ok(dispatched) => dispatched,
                Err(_) => {
                    self.fail_process(index, "process dispatcher panicked".into());
                    break;
                }
            };
            match dispatched {
                DispatchResult::Completed(completion) => {
                    if let Err((error, completion)) = self.complete(index, completion) {
                        self.dispatch_return =
                            Some(DispatchReturn::Completed { error, completion });
                        self.fail(format!("process dispatcher returned an invalid completion for process {index}: {error}"));
                        break;
                    }
                }
                DispatchResult::Deferred => {}
                DispatchResult::Rejected(job) => {
                    if let Err((error, job)) = self.reject(index, job) {
                        self.dispatch_return = Some(DispatchReturn::Rejected { error, job });
                        self.fail(format!("process dispatcher returned an invalid rejected job for process {index}: {error}"));
                    }
                    break;
                }
            }
        }
        self.supervise_deferred(Instant::now());
        let state = self.state();
        RunReport {
            state,
            steps,
            publications: self.publications - before,
            disposed_packets: self.disposed_packets,
            discarded_emissions: self.discarded_emissions,
            budget_exhausted: steps == budget && state == SessionState::Runnable,
        }
    }

    /// Actual full-queue publication dependencies, including external captures.
    /// These are diagnostics, not a proof that a cycle can make progress.
    pub fn blocked_queues(&self) -> Vec<QueueWait> {
        self.processes
            .iter()
            .filter(|p| p.pending.is_some())
            .flat_map(|p| &p.pending_targets)
            .filter(|q| self.transport.queues()[**q].is_full())
            .map(|q| {
                let queue = &self.transport.queues()[*q];
                QueueWait {
                    queue: *q,
                    producer: self.endpoints[*q].0.clone(),
                    consumer: self.endpoints[*q].1.clone(),
                    occupancy: queue.len(),
                    capacity: queue.capacity(),
                }
            })
            .collect()
    }

    /// Empty upstream queues relevant to idle/read-waiting processes. An open
    /// ingress is an external dependency, not a deadlock or synthetic EOS.
    pub fn input_waits(&self) -> Vec<QueueWait> {
        self.processes
            .iter()
            .filter(|p| {
                !p.done
                    && matches!(
                        p.handoff.state(),
                        HandoffState::Idle | HandoffState::WaitingInput
                    )
            })
            .flat_map(|p| {
                p.inputs
                    .iter()
                    .filter(|port| p.waiting.as_ref().is_none_or(|name| name == &port.name))
            })
            .flat_map(|port| &port.queues)
            .filter(|q| {
                self.transport.queues()[**q].is_empty() && !self.transport.queues()[**q].is_eos()
            })
            .map(|q| QueueWait {
                queue: *q,
                producer: self.endpoints[*q].0.clone(),
                consumer: self.endpoints[*q].1.clone(),
                occupancy: 0,
                capacity: self.transport.queues()[*q].capacity(),
            })
            .collect()
    }
}

impl Drop for NetworkSession {
    fn drop(&mut self) {
        self.signal.invalidate();
    }
}
