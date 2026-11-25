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
mod bootstrap;
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
    lane_allocator: request::LaneAllocator,
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

impl CompiledNetwork {
    pub fn start(&self) -> NetworkSession {
        self.start_with_options(SessionOptions::default())
    }
    pub fn start_with_options(&self, options: SessionOptions) -> NetworkSession {
        NetworkSession::new(self.clone(), options)
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
