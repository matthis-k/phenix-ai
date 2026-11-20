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
    ActivationJob, CancelledProcess, CompletedStep, DispatchResult, HandoffError, HandoffState,
    LocalProcessDispatcher, ProcessContext, ProcessDispatcher, ProcessExecutionGuard,
    ProcessHandoff, Step, StepTicket,
};
use crate::{CallbackPhase, Failure, FailureKind, GraphOrigin, ValidatedResourceSet, Value};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::num::NonZeroUsize;
use std::sync::{Arc, atomic::Ordering};
use std::time::{Duration, Instant};
mod admission;
mod bound;
pub use bound::BoundSessionOptions;
mod closure;
mod diagnostics;
mod errors;
mod execution;
mod observation;
mod readiness;
mod recovery;
mod request;
mod scheduling;
// The external Loom verifier recompiles wake.rs with instrumented sync types.
// Production builds retain std types and the unchanged public runtime API.
#[allow(unexpected_cfgs)]
mod wake;
pub use diagnostics::*;
pub use errors::{CleanupStage, InputFailureReason, RuntimeError};
pub(crate) use readiness::validate_policy;
pub use readiness::{InputHead, Readiness, ReadinessError, ReadinessPolicy, ReadinessSnapshot};
pub use request::{
    CaptureMetadata, InvalidClosurePortNames, RequestDisposal, RequestError, RequestId,
    RequestInjectionError, RequestStatus, RequestWork,
};
pub use wake::{ProcessNotifier, SessionNotifier, WakeHandle, WakeListener};
use wake::{WakeSignal, WakeSlot};

pub(super) fn drop_packets_catching(packets: impl IntoIterator<Item = Packet>) -> bool {
    let mut panicked = false;
    for packet in packets {
        panicked |= packet.dispose_catching();
    }
    panicked
}

pub(super) fn drop_prepared_catching(pending: Vec<PreparedFanout>) -> bool {
    let mut panicked = false;
    for fanout in pending {
        panicked |= drop_packets_catching(fanout.into_packets());
    }
    panicked
}

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
    fn step(&mut self, _: &mut ProcessContext) -> Result<Step, Failure> {
        Err(RuntimeError::UnavailableProcess.failure())
    }
}

#[derive(Clone)]
pub struct SessionOptions {
    /// Owned, generation-checked selectors, lowered only after start preflight.
    pub bound: BoundSessionOptions,
    /// Capacity of each exported-output FIFO (including aliases).
    pub capture_capacity: NonZeroUsize,
    /// Physical compiled process paths, not definition-scoped authoring handles.
    pub readiness: BTreeMap<String, Readiness>,
    /// Explicit permission to discard emissions with no connection or capture.
    /// Entries are (physical process path, declared output name).
    pub discard_outputs: BTreeSet<(String, String)>,
    /// Validated host capabilities for each physical process path.
    pub resources: BTreeMap<String, ValidatedResourceSet>,
    pub max_requests: NonZeroUsize,
    pub request_failure_policy: RequestFailurePolicy,
    /// Queue-level isolation for request traffic; does not isolate process state.
    pub request_queue_policy: RequestQueuePolicy,
    /// Optional host-supervised age limit for externally owned jobs.
    /// No background timer exists: a driver or explicit supervise_deferred
    /// call checks this bound. Expired tickets cannot publish; a restarted
    /// process has new ownership authority. Synchronous dispatch is not timed.
    pub deferred_timeout: Option<Duration>,
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
            .field("deferred_timeout", &self.deferred_timeout)
            .finish_non_exhaustive()
    }
}
impl Default for SessionOptions {
    fn default() -> Self {
        Self {
            bound: BoundSessionOptions::default(),
            capture_capacity: NonZeroUsize::new(16).unwrap(),
            readiness: BTreeMap::new(),
            discard_outputs: BTreeSet::new(),
            resources: BTreeMap::new(),
            max_requests: NonZeroUsize::new(64).unwrap(),
            request_failure_policy: RequestFailurePolicy::FailSession,
            request_queue_policy: RequestQueuePolicy::SharedFifo,
            deferred_timeout: None,
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

/// One outstanding owned worker job beyond its configured age limit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OverdueJob {
    pub process: usize,
    pub ticket: StepTicket,
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
    // Stable opt-in snapshot from the policy at session startup.
    notification_driven: bool,
    cursor: usize,
    queued: bool,
    done: bool,
    source: bool,
    wake: Arc<WakeSlot>,
    request: Option<RequestId>,
    resources: ValidatedResourceSet,
    closed_in_activation: BTreeSet<usize>,
}

struct RequestLane {
    inputs: BTreeMap<String, Vec<usize>>,
    input_ports: BTreeMap<(usize, usize), Vec<usize>>,
    outputs: BTreeMap<(usize, String), Vec<usize>>,
    captures: BTreeMap<String, usize>,
    queues: Vec<usize>,
}

pub struct NetworkSession {
    compiled: CompiledNetwork,
    // Static topology for request-root reachability. Constructed once at
    // start, avoiding an O(V*E) scan for each admitted request.
    downstream: Vec<Vec<usize>>,
    transport: ConnectionSet,
    processes: Vec<RuntimeProcess>,
    // Process IDs and base policies are fixed at construction; staged waits use waiting_policy.
    custom_readiness: Vec<usize>,
    ingress: BTreeMap<String, Vec<usize>>,
    captures: BTreeMap<String, usize>,
    request_lanes: BTreeMap<usize, RequestLane>,
    request_cursors: BTreeMap<usize, usize>,
    queue_lanes: Vec<Option<usize>>,
    endpoints: Vec<(QueueEndpoint, QueueEndpoint)>,
    ready: VecDeque<usize>,
    // Sources, legacy custom readiness and staged waits require unconditional
    // polling. Opt-in notified Custom and Optional policies are polled while
    // authenticated sealed scopes can reach them.
    always_poll: BTreeSet<usize>,
    scoped_readiness_poll: BTreeMap<usize, usize>,
    dirty_processes: BTreeSet<usize>,
    force_full_refresh: bool,
    observed_global_wake_epoch: u64,
    terminal: Option<SessionState>,
    failure: Option<Failure>,
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
    request_queue_policy: RequestQueuePolicy,
    deferred_timeout: Option<Duration>,
    dispatched_at: BTreeMap<usize, Instant>,
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
) -> Result<usize, Failure> {
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
        .ok_or_else(|| RuntimeError::LaneCapacityOverflow.failure())
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
            options.lease = Some(crate::network::HandleLeases::append(
                options.lease.take(),
                vec![lease],
            ));
        }
        if let Some(lease) = options.bound.execution_lease() {
            options.lease = Some(crate::network::HandleLeases::append(
                options.lease.take(),
                vec![lease],
            ));
        }
        let bound_failure = options
            .bound
            .apply(
                &compiled,
                &mut options.readiness,
                &mut options.resources,
                &mut options.discard_outputs,
            )
            .err();
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
        let mut initial_failure = bound_failure
            .or_else(|| {
                invalid_path
                    .map(|path| RuntimeError::UnknownConfiguration { path }.failure())
                    .or_else(|| {
                        options
                            .discard_outputs
                            .iter()
                            .find(|(path, output)| {
                                !compiled.processes().iter().any(|p| {
                                    p.path() == path && p.interface().outputs().contains_key(output)
                                })
                            })
                            .map(|(path, output)| {
                                RuntimeError::InvalidDiscard {
                                    path: path.clone(),
                                    output: output.clone(),
                                }
                                .failure()
                            })
                    })
            })
            .or_else(|| {
                compiled.processes().iter().find_map(|process| {
                    options.readiness.get(process.path()).and_then(|policy| {
                        readiness::validate_policy_ports(
                            process.interface().inputs().keys().map(String::as_str),
                            policy,
                        )
                        .err()
                        .map(|error| {
                            Failure::external(format!("process {}: {error}", process.path()), error)
                                .with_kind(FailureKind::Framework)
                                .with_context(crate::FailureContext {
                                    path: Some(process.path().into()),
                                    ..Default::default()
                                })
                        })
                    })
                })
            })
            .or_else(|| {
                (options.request_queue_policy == RequestQueuePolicy::BoundedLanes)
                    .then(|| request_lane_packet_bound(&compiled, &options).err())
                    .flatten()
            });
        for process in compiled.processes() {
            if initial_failure.is_some() {
                break;
            }
            match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                process.factory().correlation_lanes()
            })) {
                Ok(Some(lanes)) if options.max_requests > lanes => {
                    initial_failure = Some(
                        RuntimeError::CorrelationBound {
                            path: process.path().into(),
                            lanes: lanes.get(),
                            requests: options.max_requests.get(),
                        }
                        .failure(),
                    );
                }
                Err(payload) => {
                    initial_failure = Some(
                        crate::process::callback_panicked(CallbackPhase::CorrelationBound, payload)
                            .with_context(crate::FailureContext {
                                path: Some(process.path().into()),
                                phase: Some(CallbackPhase::CorrelationBound),
                                ..Default::default()
                            }),
                    );
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
                                initial_failure = Some(
                                    crate::process::callback_panicked(
                                        CallbackPhase::Construct,
                                        payload,
                                    )
                                    .with_context(
                                        crate::FailureContext {
                                            process: Some(index),
                                            path: Some(p.path().into()),
                                            phase: Some(CallbackPhase::Construct),
                                            ..Default::default()
                                        },
                                    ),
                                );
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
                            initial_failure.get_or_insert_with(|| {
                                crate::process::callback_panicked(
                                    CallbackPhase::FactoryReadiness,
                                    payload,
                                )
                                .with_context(
                                    crate::FailureContext {
                                        process: Some(index),
                                        path: Some(p.path().into()),
                                        phase: Some(CallbackPhase::FactoryReadiness),
                                        ..Default::default()
                                    },
                                )
                            });
                            Readiness::default()
                        }
                    },
                };
                if initial_failure.is_none()
                    && let Err(error) = readiness::validate_policy_ports(
                        p.interface().inputs().keys().map(String::as_str),
                        &policy,
                    )
                {
                    initial_failure = Some(
                        Failure::external(format!("process {}: {error}", p.path()), error)
                            .with_kind(FailureKind::Framework)
                            .with_context(crate::FailureContext {
                                process: Some(index),
                                path: Some(p.path().into()),
                                ..Default::default()
                            }),
                    );
                }
                RuntimeProcess {
                    handoff,
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
                    policy,
                    notification_driven: false,
                    cursor: 0,
                    queued: false,
                    done: false,
                    source: compiled.source_starts().contains(&index),
                    wake: Arc::new(WakeSlot::default()),
                    request: None,
                    resources: options.resources.get(p.path()).cloned().unwrap_or_default(),
                    closed_in_activation: BTreeSet::new(),
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
        let queue_lanes = if options.request_queue_policy == RequestQueuePolicy::BoundedLanes {
            vec![None; queues.len()]
        } else {
            Vec::new()
        };
        let mut downstream = vec![Vec::new(); compiled.processes().len()];
        for edge in compiled.connections() {
            downstream[edge.from().0].push(edge.to().0);
        }
        // A user policy must explicitly promise to send process-targeted
        // notifications for external selection changes. The default stays
        // polled. Query once and contain plugin panics as session failure.
        let mut always_poll = BTreeSet::new();
        let mut custom_readiness = Vec::new();
        for (index, process) in processes.iter_mut().enumerate() {
            if initial_failure.is_none()
                && let Readiness::Custom(policy) = &process.policy
            {
                custom_readiness.push(index);
                match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    policy.notification_driven()
                })) {
                    Ok(value) => process.notification_driven = value,
                    Err(payload) => {
                        let failure = crate::process::callback_panicked(
                            CallbackPhase::ReadinessRegistration,
                            payload,
                        )
                        .with_context(crate::FailureContext {
                            process: Some(index),
                            path: Some(compiled.processes()[index].path().into()),
                            phase: Some(CallbackPhase::ReadinessRegistration),
                            ..Default::default()
                        });
                        if initial_failure.is_none() {
                            initial_failure = Some(failure);
                        }
                    }
                }
            }
            if process.source
                || process.inputs.is_empty()
                || matches!(&process.policy, Readiness::Custom(_)) && !process.notification_driven
            {
                always_poll.insert(index);
            }
        }
        let mut session = Self {
            compiled,
            downstream,
            transport: ConnectionSet::new(queues),
            processes,
            custom_readiness,
            ingress,
            captures,
            request_lanes: BTreeMap::new(),
            request_cursors: BTreeMap::new(),
            queue_lanes,
            endpoints,
            ready: VecDeque::new(),
            always_poll,
            scoped_readiness_poll: BTreeMap::new(),
            dirty_processes: BTreeSet::new(),
            force_full_refresh: true,
            observed_global_wake_epoch: 0,
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
            request_queue_policy: options.request_queue_policy,
            deferred_timeout: options.deferred_timeout,
            dispatched_at: BTreeMap::new(),
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
}

impl Drop for NetworkSession {
    fn drop(&mut self) {
        self.signal.invalidate();
        for guard in std::mem::take(&mut self.execution_guards).into_iter().rev() {
            if let Err(payload) =
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| drop(guard)))
            {
                crate::discard_panic_payload(payload);
            }
        }
    }
}
