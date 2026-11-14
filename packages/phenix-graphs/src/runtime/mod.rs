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
pub use diagnostics::BlockedCycle;
pub(crate) use readiness::validate_policy;
pub use readiness::{InputHead, Readiness, ReadinessPolicy, ReadinessSnapshot};
pub use request::{
    CaptureMetadata, InvalidClosurePortNames, RequestDisposal, RequestError, RequestId,
    RequestInjectionError, RequestStatus, RequestWork,
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

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RequestQueuePolicy {
    /// Preserve shared connection and capture FIFO ordering across requests.
    #[default]
    SharedFifo,
    /// Give each active request bounded FIFO queues on every compiled edge.
    /// Process state remains shared and single-flight.
    BoundedLanes,
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
    /// Queue-level isolation for request traffic; does not isolate process state.
    pub request_queue_policy: RequestQueuePolicy,
    /// Opt-in timeout for one externally owned activation handoff. It invalidates
    /// completion authority; it cannot stop worker code or external effects.
    pub deferred_job_timeout: Option<Duration>,
    /// Disable for request-driven source starts; only compiled sources are eligible.
    pub auto_start_sources: bool,
    pub dispatcher: Arc<dyn ProcessDispatcher>,
    /// Entered in order on the actual worker, torn down in reverse order.
    pub execution_guards: Vec<Arc<dyn ProcessExecutionGuard>>,
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
            .field("request_queue_policy", &self.request_queue_policy)
            .field("deferred_job_timeout", &self.deferred_job_timeout)
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
            request_queue_policy: RequestQueuePolicy::SharedFifo,
            deferred_job_timeout: None,
            auto_start_sources: true,
            dispatcher: Arc::new(LocalProcessDispatcher),
            execution_guards: Vec::new(),
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
    /// Time until the earliest supervised job deadline, if one is active.
    pub next_deferred_timeout: Option<Duration>,
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
    shared_queues: Vec<usize>,
    lane_queues: BTreeMap<usize, Vec<usize>>,
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
    request_cursor: usize,
    queued: bool,
    done: bool,
    source: bool,
    wake: Arc<WakeSlot>,
    request: Option<RequestId>,
    resources: Vec<ResourceGrant>,
    closed_in_activation: BTreeSet<usize>,
    deferred_ticket: Option<(StepTicket, Instant)>,
}

struct RequestLane {
    inputs: BTreeMap<String, Vec<usize>>,
    outputs: BTreeMap<(usize, String), Vec<usize>>,
    captures: BTreeMap<String, usize>,
    queues: Vec<usize>,
}

pub struct NetworkSession {
    compiled: CompiledNetwork,
    transport: ConnectionSet,
    processes: Vec<RuntimeProcess>,
    ingress: BTreeMap<String, Vec<usize>>,
    captures: BTreeMap<String, usize>,
    request_lanes: BTreeMap<usize, RequestLane>,
    queue_lanes: Vec<Option<usize>>,
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
    deferred_job_timeout: Option<Duration>,
    request_queue_policy: RequestQueuePolicy,
    requests: BTreeMap<RequestId, request::RequestScope>,
    signal: Arc<WakeSignal>,
    dispatcher: Arc<dyn ProcessDispatcher>,
    dispatch_return: Option<DispatchReturn>,
    execution_guards: Vec<Arc<dyn ProcessExecutionGuard>>,
    // Last field: compiled factories, process state, transports, dispatchers,
    // grants and policy trait objects must be destroyed before the code lease.
    lease: Option<Arc<dyn Send + Sync>>,
}

fn request_lane_packet_bound(
    compiled: &CompiledNetwork,
    options: &SessionOptions,
) -> Result<usize, String> {
    let connections = compiled
        .connections()
        .iter()
        .try_fold(0usize, |total, edge| {
            total.checked_add(edge.capacity().get())
        });
    let ingress = compiled
        .inputs()
        .iter()
        .try_fold(0usize, |total, (name, targets)| {
            compiled.ingress_capacities()[name]
                .get()
                .checked_mul(targets.len())
                .and_then(|capacity| total.checked_add(capacity))
        });
    let captures = options
        .capture_capacity
        .get()
        .checked_mul(compiled.outputs().len());
    connections
        .and_then(|connections| ingress.and_then(|ingress| connections.checked_add(ingress)))
        .and_then(|capacity| captures.and_then(|captures| capacity.checked_add(captures)))
        .and_then(|capacity| capacity.checked_mul(options.max_requests.get()))
        .ok_or_else(|| "bounded request lane packet capacity overflows usize".into())
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
        let mut initial_failure =
            if options.request_queue_policy == RequestQueuePolicy::BoundedLanes {
                request_lane_packet_bound(&compiled, &options).err()
            } else {
                None
            };
        for process in compiled.processes() {
            if initial_failure.is_some() {
                break;
            }
            match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                process.factory().correlation_lanes()
            })) {
                Ok(Some(lanes)) if options.max_requests > lanes => {
                    initial_failure = Some(format!(
                        "insufficient correlation bound: {} has {} lanes for {} inflight requests; head blocking is not supported beyond this bound",
                        process.path(),
                        lanes,
                        options.max_requests
                    ));
                    break;
                }
                Err(payload) => {
                    crate::discard_panic_payload(payload);
                    initial_failure = Some(format!(
                        "process factory correlation bound panicked: {}",
                        process.path()
                    ));
                    break;
                }
                _ => {}
            }
        }
        let mut processes: Vec<_> = compiled
            .processes()
            .iter()
            .enumerate()
            .map(|(index, p)| {
                let handoff = {
                    let process = if initial_failure.is_some() {
                        Box::new(UnavailableProcess) as Box<dyn crate::process::Process>
                    } else {
                        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                            p.factory().spawn()
                        })) {
                            Ok(process) => process,
                            Err(payload) => {
                                crate::discard_panic_payload(payload);
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
                };
                let policy = match options.readiness.get(p.path()) {
                    Some(policy) => policy.clone(),
                    None if initial_failure.is_some() => Readiness::default(),
                    None => match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        p.factory().readiness()
                    })) {
                        Ok(policy) => policy,
                        Err(payload) => {
                            crate::discard_panic_payload(payload);
                            initial_failure.get_or_insert_with(|| {
                                format!("process factory readiness panicked: {}", p.path())
                            });
                            Readiness::default()
                        }
                    },
                };
                RuntimeProcess {
                    handoff,
                    inputs: p
                        .interface()
                        .inputs()
                        .keys()
                        .map(|name| InputPort {
                            name: name.clone(),
                            queues: vec![],
                            shared_queues: vec![],
                            lane_queues: BTreeMap::new(),
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
                    policy,
                    cursor: 0,
                    request_cursor: 0,
                    queued: false,
                    done: false,
                    source: compiled.source_starts().contains(&index),
                    wake: Arc::new(WakeSlot::default()),
                    request: None,
                    resources: options.resources.get(p.path()).cloned().unwrap_or_default(),
                    closed_in_activation: BTreeSet::new(),
                    deferred_ticket: None,
                }
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
            processes[to]
                .inputs
                .iter_mut()
                .find(|p| p.name == input)
                .unwrap()
                .shared_queues
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
                processes[*to]
                    .inputs
                    .iter_mut()
                    .find(|p| p.name == *input)
                    .unwrap()
                    .shared_queues
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
        let queue_lanes = if options.request_queue_policy == RequestQueuePolicy::BoundedLanes {
            vec![None; queues.len()]
        } else {
            Vec::new()
        };
        let mut session = Self {
            compiled,
            transport: ConnectionSet::new(queues),
            processes,
            ingress,
            captures,
            request_lanes: BTreeMap::new(),
            queue_lanes,
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
            deferred_job_timeout: options.deferred_job_timeout,
            request_queue_policy: options.request_queue_policy,
            requests: BTreeMap::new(),
            signal: WakeSignal::new(),
            dispatcher: options.dispatcher,
            dispatch_return: None,
            execution_guards: options.execution_guards,
            lease: options.lease,
        };
        if let Some(error) = initial_failure {
            session.fail(error);
            return session;
        }
        if let Some(path) = invalid_path {
            session.fail(format!("unknown process configuration: {path}"));
            return session;
        }
        if let Some((path, output)) = session.discard_outputs.iter().find(|(path, output)| {
            !session
                .compiled
                .processes()
                .iter()
                .any(|p| p.path() == path && p.interface().outputs().contains_key(output))
        }) {
            session.fail(format!("invalid discard output: {path}.{output}"));
            return session;
        }
        for index in 0..session.processes.len() {
            if let Err(error) = session.validate_policy(index, &session.processes[index].policy) {
                session.fail(error);
                return session;
            }
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

    /// Invalidate overdue externally owned jobs. This is cooperative supervision:
    /// callers must drive the session, and external code is not preempted.
    pub fn supervise_deferred_jobs(&mut self) {
        let Some(timeout) = self.deferred_job_timeout else {
            return;
        };
        let now = Instant::now();
        let expired: Vec<_> = self
            .processes
            .iter()
            .enumerate()
            .filter_map(|(index, process)| {
                process.deferred_ticket.and_then(|(ticket, since)| {
                    (process.handoff.outstanding() == Some(ticket)
                        && now.saturating_duration_since(since) >= timeout)
                        .then_some((index, ticket))
                })
            })
            .collect();
        for (index, ticket) in expired {
            if self.processes[index]
                .deferred_ticket
                .is_none_or(|(current, _)| current != ticket)
                || self.processes[index].handoff.outstanding() != Some(ticket)
            {
                continue;
            }
            self.processes[index].deferred_ticket = None;
            let request = self.processes[index].request;
            if let Some(request) = request
                && let Some(scope) = self.requests.get_mut(&request)
            {
                scope.timed_out = true;
            }
            let path = self.compiled.processes()[index].path();
            self.fail_process(
                index,
                format!(
                    "deferred job timed out after {timeout:?}: process={path}, request={request:?}, activation={}, step={}",
                    ticket.activation(),
                    ticket.sequence()
                ),
            );
        }
    }

    fn next_deferred_timeout(&self) -> Option<Duration> {
        let timeout = self.deferred_job_timeout?;
        let now = Instant::now();
        self.processes
            .iter()
            .filter_map(|process| {
                process.deferred_ticket.and_then(|(ticket, since)| {
                    (process.handoff.outstanding() == Some(ticket))
                        .then(|| timeout.saturating_sub(now.saturating_duration_since(since)))
                })
            })
            .min()
    }

    fn timeout_invalidated(&self, ticket: StepTicket) -> bool {
        self.requests
            .values()
            .any(|scope| scope.timed_out && scope.cancelled_jobs.contains(&ticket))
    }

    pub(super) fn prepare_request_lane(&mut self, lane: usize) -> Result<(), RequestError> {
        if self.request_queue_policy == RequestQueuePolicy::SharedFifo {
            return Ok(());
        }
        if let Some(existing) = self.request_lanes.get(&lane) {
            if existing
                .queues
                .iter()
                .any(|queue| !self.transport.queues()[*queue].is_empty())
            {
                return Err(RequestError::Busy);
            }
            return Ok(());
        }

        let mut request_lane = RequestLane {
            inputs: BTreeMap::new(),
            outputs: BTreeMap::new(),
            captures: BTreeMap::new(),
            queues: Vec::new(),
        };
        let canonical_count = self
            .queue_lanes
            .iter()
            .take_while(|lane| lane.is_none())
            .count();
        for canonical in 0..canonical_count {
            let queue = self
                .transport
                .add_queue(self.transport.queues()[canonical].empty_like());
            let endpoints = self.endpoints[canonical].clone();
            match &endpoints.0 {
                QueueEndpoint::Process { index, port } => {
                    request_lane
                        .outputs
                        .entry((*index, port.clone()))
                        .or_default()
                        .push(queue);
                }
                QueueEndpoint::Input(name) => {
                    request_lane
                        .inputs
                        .entry(name.clone())
                        .or_default()
                        .push(queue);
                }
                QueueEndpoint::Capture(_) => unreachable!("capture cannot be a writer"),
            }
            match &endpoints.1 {
                QueueEndpoint::Process { index, port } => {
                    let input = self.processes[*index]
                        .inputs
                        .iter_mut()
                        .find(|input| &input.name == port)
                        .unwrap();
                    input.queues.push(queue);
                    input.lane_queues.entry(lane).or_default().push(queue);
                }
                QueueEndpoint::Capture(name) => {
                    request_lane.captures.insert(name.clone(), queue);
                }
                QueueEndpoint::Input(_) => unreachable!("ingress cannot be a reader"),
            }
            self.endpoints.push(endpoints);
            self.queue_lanes.push(Some(lane));
            request_lane.queues.push(queue);
        }
        self.request_lanes.insert(lane, request_lane);
        Ok(())
    }

    pub fn inject_local<T: Send + Sync + 'static>(
        &mut self,
        name: &str,
        value: T,
    ) -> Result<(), InjectionError> {
        self.inject(name, Value::local(value))
    }
    pub fn inject(&mut self, name: &str, value: Value) -> Result<(), InjectionError> {
        self.supervise_deferred_jobs();
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
            let indices = match request {
                Some(request) if self.request_queue_policy == RequestQueuePolicy::BoundedLanes => {
                    self.requests
                        .get(&request)
                        .and_then(|scope| self.request_lanes.get(&scope.lane))
                        .and_then(|lane| lane.inputs.get(name))
                }
                _ => self.ingress.get(name),
            }
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
        self.supervise_deferred_jobs();
        for index in self
            .ingress
            .get(name)
            .ok_or(ConnectionError::InvalidDestination)?
        {
            self.transport.close_writer(*index)?;
        }
        for lane in self.request_lanes.values() {
            for queue in &lane.inputs[name] {
                self.transport.close_writer(*queue)?;
            }
        }
        self.refresh();
        Ok(())
    }
    pub fn take_output(&mut self, name: &str) -> Result<Option<Packet>, ConnectionError> {
        self.supervise_deferred_jobs();
        let index = *self
            .captures
            .get(name)
            .ok_or(ConnectionError::InvalidDestination)?;
        self.take_capture(name, index)
    }

    pub(super) fn take_capture(
        &mut self,
        name: &str,
        index: usize,
    ) -> Result<Option<Packet>, ConnectionError> {
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
        Ok(std::iter::once(index)
            .chain(
                self.request_lanes
                    .values()
                    .filter_map(|lane| lane.captures.get(name).copied()),
            )
            .all(|index| {
                self.transport.queues()[index].is_eos()
                    || self.transport.queues()[index].reader_closed()
            }))
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
        for lane in self.request_lanes.values() {
            for ((producer, _), queues) in &lane.outputs {
                if *producer == index {
                    for queue in queues {
                        self.transport.close_writer(*queue).unwrap();
                    }
                }
            }
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

    /// Ownership stays in the handoff. The driver may run jobs on other threads.
    /// Publication retries happen here, never by re-running a completed step.
    pub fn next_job(&mut self) -> Option<(usize, ActivationJob)> {
        self.supervise_deferred_jobs();
        let (index, job) = self.next_job_inner()?;
        if self.deferred_job_timeout.is_some() {
            self.processes[index].deferred_ticket = Some((job.ticket(), Instant::now()));
        }
        Some((index, job))
    }

    fn next_job_inner(&mut self) -> Option<(usize, ActivationJob)> {
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
            for guard in &self.execution_guards {
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
        self.supervise_deferred_jobs();
        let ticket = completion.ticket();
        let Some(p) = self.processes.get_mut(index) else {
            return Err((HandoffError::ForeignCompletion, Box::new(completion)));
        };
        let result = p.handoff.complete(completion)?;
        self.processes[index].deferred_ticket = None;
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
                let outgoing = match (self.processes[index].request, self.request_queue_policy) {
                    (Some(request), RequestQueuePolicy::BoundedLanes) => {
                        let output_key = (index, emission.output.clone());
                        let lane = self.requests[&request].lane;
                        self.request_lanes
                            .get(&lane)
                            .and_then(|lane| lane.outputs.get(&output_key))
                            .map(Vec::as_slice)
                            .unwrap_or(&[])
                    }
                    _ => self.processes[index].outputs[&emission.output].as_slice(),
                };
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
                    outgoing.to_vec()
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
        self.supervise_deferred_jobs();
        let Some(p) = self.processes.get_mut(index) else {
            return Err((HandoffError::ForeignCompletion, job));
        };
        p.handoff.reject(job)?;
        self.processes[index].deferred_ticket = None;
        self.refresh();
        Ok(())
    }
    pub fn complete_job(
        &mut self,
        completion: CompletedStep,
    ) -> Result<(), (HandoffError, Box<CompletedStep>)> {
        self.supervise_deferred_jobs();
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
            p.deferred_ticket = None;
        }
        self.ready.clear();
        self.signal.invalidate();
    }
    pub fn cancel(&mut self) {
        // Explicit cancellation wins if it is serialized before timeout supervision.
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
    pub fn state(&mut self) -> SessionState {
        self.supervise_deferred_jobs();
        self.refresh();
        if let Some(state) = self.terminal {
            return state;
        }
        if !self.ready.is_empty() {
            return SessionState::Runnable;
        }
        let drained = self.transport.queues().iter().all(|q| q.is_empty());
        let ingress_closed = self
            .ingress
            .values()
            .flatten()
            .all(|q| self.transport.queues()[*q].writer_closed());
        if self.processes.iter().all(|p| p.done) && drained && ingress_closed {
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
        let before = self.publications;
        let mut steps = 0;
        while steps < budget {
            self.supervise_deferred_jobs();
            let Some((index, job)) = self.next_job_inner() else {
                break;
            };
            let ticket = job.ticket();
            steps += 1;
            let dispatched = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                self.dispatcher.dispatch(job)
            }));
            self.supervise_deferred_jobs();
            let dispatched = match dispatched {
                Ok(dispatched) => dispatched,
                Err(payload) => {
                    crate::discard_panic_payload(payload);
                    if self.processes[index].handoff.outstanding() == Some(ticket) {
                        self.fail_process(index, "process dispatcher panicked".into());
                    }
                    break;
                }
            };
            match dispatched {
                DispatchResult::Completed(completion) => {
                    if let Err((error, completion)) = self.complete(index, completion) {
                        let ticket = completion.ticket();
                        if self.timeout_invalidated(ticket) || self.terminal.is_some() {
                            drop(completion);
                            break;
                        }
                        self.dispatch_return =
                            Some(DispatchReturn::Completed { error, completion });
                        self.fail(format!("process dispatcher returned an invalid completion for process {index}: {error}"));
                        break;
                    }
                }
                DispatchResult::Deferred => {
                    if self.deferred_job_timeout.is_some()
                        && self.processes[index].handoff.outstanding() == Some(ticket)
                    {
                        self.processes[index].deferred_ticket = Some((ticket, Instant::now()));
                    }
                }
                DispatchResult::Rejected(job) => {
                    if let Err((error, job)) = self.reject(index, job) {
                        let ticket = job.ticket();
                        if self.timeout_invalidated(ticket) || self.terminal.is_some() {
                            drop(job);
                            break;
                        }
                        self.dispatch_return = Some(DispatchReturn::Rejected { error, job });
                        self.fail(format!("process dispatcher returned an invalid rejected job for process {index}: {error}"));
                    }
                    break;
                }
            }
        }
        let state = self.state();
        RunReport {
            state,
            steps,
            publications: self.publications - before,
            disposed_packets: self.disposed_packets,
            discarded_emissions: self.discarded_emissions,
            budget_exhausted: steps == budget && state == SessionState::Runnable,
            next_deferred_timeout: self.next_deferred_timeout(),
        }
    }
    pub fn run_until_complete(&mut self, budget: usize) -> RunReport {
        self.run_until_idle(budget)
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
        let live_lanes: BTreeSet<_> =
            if self.request_queue_policy == RequestQueuePolicy::BoundedLanes {
                self.requests.values().map(|scope| scope.lane).collect()
            } else {
                BTreeSet::new()
            };
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
            .filter(|queue| {
                self.request_queue_policy == RequestQueuePolicy::SharedFifo
                    || self.queue_lanes[**queue].is_none_or(|lane| live_lanes.contains(&lane))
            })
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
