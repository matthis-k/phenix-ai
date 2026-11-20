//! Exclusive, owned cooperative process handoff.
//!
//! A job owns its process and arguments, never a borrow into scheduler queues.
//! Returning an emission does not complete the logical activation. A caller must
//! commit that emission before resuming its continuation, and must explicitly
//! settle `Complete` before admitting another reservation.
use crate::{CallbackPhase, Failure, FailureKind, ResourceError, ValidatedResourceSet, Value};
use std::collections::BTreeMap;
use std::fmt;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

#[cfg(test)]
mod handoff_model;

static NEXT_OWNER: AtomicU64 = AtomicU64::new(1);

/// One process-private state machine. `Sync` is deliberately not required.
/// Implementations must return from each step; cancellation cannot preempt code.
pub trait Process: Send + 'static {
    fn step(&mut self, context: &mut ProcessContext) -> Result<Step, Failure>;

    /// Managed cleanup may be specialized by wrappers that own code leases.
    fn dispose_catching(self: Box<Self>) -> bool {
        drop_catching(self)
    }
}

fn dispose_process_catching(process: Box<dyn Process>) -> bool {
    // Preserve specialized reports, but contain the callback before the caller
    // releases its leases, including while disposing a caught panic payload.
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| process.dispose_catching())) {
        Ok(panicked) => panicked,
        Err(payload) => {
            crate::discard_panic_payload(payload);
            true
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PanicSummary {
    String(String),
    NonString,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CallbackPanicked {
    pub phase: CallbackPhase,
    pub payload: PanicSummary,
}
impl fmt::Display for CallbackPanicked {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "callback panicked during {:?}", self.phase)
    }
}
impl std::error::Error for CallbackPanicked {}

/// Convert and dispose a caught panic while the caller still holds every known
/// code/resource lease. The arbitrary payload itself never enters the failure.
pub fn callback_panicked(phase: CallbackPhase, payload: Box<dyn std::any::Any + Send>) -> Failure {
    let summary = if let Some(message) = payload.downcast_ref::<String>() {
        PanicSummary::String(message.clone())
    } else if let Some(message) = payload.downcast_ref::<&'static str>() {
        PanicSummary::String((*message).into())
    } else {
        PanicSummary::NonString
    };
    let message = match phase {
        CallbackPhase::Step => "process step panicked",
        CallbackPhase::GuardEnter => "process execution guard entry panicked",
        CallbackPhase::GuardExit => "process execution guard teardown panicked",
        CallbackPhase::Construct => "process factory panicked",
        CallbackPhase::RecoveryConstruct => "process factory panicked during request recovery",
        CallbackPhase::FactoryReadiness => "process factory readiness panicked",
        CallbackPhase::CorrelationBound => "process factory correlation bound panicked",
        CallbackPhase::ReadinessRegistration => "readiness notification contract panicked",
        CallbackPhase::Readiness => "readiness policy panicked",
        CallbackPhase::Dispatch => "process dispatcher panicked",
        CallbackPhase::Prepare => "plugin preparation panicked",
        CallbackPhase::ProducerActivate => "producer activation panicked",
        CallbackPhase::ProducerPoll => "producer poll panicked",
        CallbackPhase::GenerationBind => "generation binding panicked",
        CallbackPhase::WakeRegistration => "producer wake registration panicked",
        CallbackPhase::Event => "event callback panicked",
        CallbackPhase::Adapter => "packet preparation panicked",
        _ => "callback panicked",
    };
    let failure = Failure::external(
        message,
        CallbackPanicked {
            phase,
            payload: summary,
        },
    )
    .with_kind(FailureKind::Panic)
    .with_context(crate::FailureContext {
        phase: Some(phase),
        ..Default::default()
    });
    crate::discard_panic_payload(payload);
    failure
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProcessCleanupError {
    GuardObject { guard_index: usize },
    Emission,
    OwnedState,
}
impl fmt::Display for ProcessCleanupError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "process cleanup: {self:?}")
    }
}
impl std::error::Error for ProcessCleanupError {}
impl ProcessCleanupError {
    fn failure(self) -> Failure {
        let summary = match self {
            Self::OwnedState => "owned destructor panicked during completion",
            Self::Emission => "emission destructor panicked",
            Self::GuardObject { .. } => "process execution guard object destructor panicked",
        };
        let guard_index = match self {
            Self::GuardObject { guard_index } => Some(guard_index),
            _ => None,
        };
        Failure::external(summary, self)
            .with_kind(FailureKind::Cleanup)
            .with_context(crate::FailureContext {
                guard_index,
                phase: Some(CallbackPhase::Cleanup),
                ..Default::default()
            })
    }
}

fn dispose_step(result: Result<Step, Failure>) -> bool {
    match result {
        Ok(Step::Emit(emission)) => emission.value.dispose_catching(),
        Err(failure) => failure.dispose_catching(),
        Ok(step) => drop_catching(step),
    }
}

fn enrich_identity(failure: Failure, identity: &crate::FailureContext) -> Failure {
    let mut context = failure.context().clone();
    context.ticket = identity.ticket;
    context.session = identity.session;
    context.generation = identity.generation;
    context.request = identity.request;
    context.process = identity.process;
    context.path = identity.path.clone();
    failure.with_context(context)
}

fn add_failure(result: Result<Step, Failure>, failure: Failure) -> Result<Step, Failure> {
    Err(match result {
        Err(primary) => primary.with_cleanup(failure),
        Ok(step) => {
            let mut leases = match &step {
                Step::Emit(emission) => emission.value.known_leases().to_vec(),
                _ => Vec::new(),
            };
            let identity = failure.context().clone();
            let mut failure = failure;
            if dispose_step(Ok(step)) {
                failure = failure.with_cleanup(enrich_identity(
                    ProcessCleanupError::Emission
                        .failure()
                        .retain_leases(&leases),
                    &identity,
                ));
            }
            // Keep the exact emission leases through payload cleanup, then catch
            // each release too: an independent lease may now have its final owner.
            while let Some(lease) = leases.pop() {
                if drop_catching(lease) {
                    failure = failure.with_cleanup(enrich_identity(
                        ProcessCleanupError::Emission.failure(),
                        &identity,
                    ));
                }
            }
            failure
        }
    })
}

/// Immutable construction code, invoked once per physical instance and session.
/// Captured shared capabilities remain explicitly shared; Rust cannot inspect
/// captures to prove that a factory constructs isolated application state.
pub trait ProcessFactory: Send + Sync + 'static {
    fn spawn(&self) -> Box<dyn Process>;
    fn readiness(&self) -> crate::runtime::Readiness {
        crate::runtime::Readiness::default()
    }
    /// Fixed correlation lane bound, checked before any source starts.
    fn correlation_lanes(&self) -> Option<std::num::NonZeroUsize> {
        None
    }
}
impl<F, P> ProcessFactory for F
where
    F: Fn() -> P + Send + Sync + 'static,
    P: Process,
{
    fn spawn(&self) -> Box<dyn Process> {
        Box::new(self())
    }
}

pub(crate) struct LeasedFactory {
    // Executable objects must drop before the resources containing their code.
    pub(crate) inner: Arc<dyn ProcessFactory>,
    pub(crate) lease: Arc<dyn Send + Sync>,
}
impl ProcessFactory for LeasedFactory {
    fn readiness(&self) -> crate::runtime::Readiness {
        match self.inner.readiness() {
            crate::runtime::Readiness::Custom(inner) => {
                crate::runtime::Readiness::Custom(Arc::new(LeasedReadiness {
                    inner,
                    _lease: self.lease.clone(),
                }))
            }
            policy => policy,
        }
    }
    fn correlation_lanes(&self) -> Option<std::num::NonZeroUsize> {
        self.inner.correlation_lanes()
    }
    fn spawn(&self) -> Box<dyn Process> {
        Box::new(LeasedProcess {
            inner: Some(self.inner.spawn()),
            lease: Some(self.lease.clone()),
        })
    }
}
struct LeasedReadiness {
    inner: Arc<dyn crate::runtime::ReadinessPolicy>,
    _lease: Arc<dyn Send + Sync>,
}
impl crate::runtime::ReadinessPolicy for LeasedReadiness {
    fn select(&self, snapshot: &crate::runtime::ReadinessSnapshot) -> Vec<String> {
        self.inner.select(snapshot)
    }
    fn closure_ports(&self, lane: usize) -> Vec<String> {
        self.inner.closure_ports(lane)
    }
    fn notification_driven(&self) -> bool {
        self.inner.notification_driven()
    }
}
struct LeasedProcess {
    inner: Option<Box<dyn Process>>,
    lease: Option<Arc<dyn Send + Sync>>,
}
impl Process for LeasedProcess {
    fn step(&mut self, context: &mut ProcessContext) -> Result<Step, Failure> {
        let lease = self.lease.as_ref().expect("live leased process");
        let result = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            self.inner
                .as_mut()
                .expect("live leased process")
                .step(context)
        })) {
            Ok(result) => result,
            Err(payload) => Err(callback_panicked(CallbackPhase::Step, payload)),
        };
        Ok(
            match result.map_err(|failure| failure.retain_leases(std::slice::from_ref(lease)))? {
                Step::Emit(mut emission) => {
                    emission.value = emission.value.with_lease(lease.clone());
                    Step::Emit(emission)
                }
                step => step,
            },
        )
    }
    fn dispose_catching(mut self: Box<Self>) -> bool {
        self.dispose_contents()
    }
}
impl LeasedProcess {
    fn dispose_contents(&mut self) -> bool {
        let inner = self.inner.take();
        let lease = self.lease.take();
        let mut panicked = inner.is_some_and(dispose_process_catching);
        panicked |= drop_catching(lease);
        panicked
    }
}
impl Drop for LeasedProcess {
    fn drop(&mut self) {
        let _ = self.dispose_contents();
    }
}

/// An output name and one ordinary value. Routing and type validation belong to
/// the compiler/session, not to the process or an external dispatcher.
#[derive(Debug)]
pub struct ProcessEmission {
    pub output: String,
    pub value: Value,
    /// None broadcasts. Some selects existing process connections only; exported
    /// output captures remain connected observations, including for Some([]).
    pub destinations: Option<Vec<crate::network::PortPath>>,
}
impl ProcessEmission {
    pub fn new(output: impl Into<String>, value: Value) -> Self {
        Self {
            output: output.into(),
            value,
            destinations: None,
        }
    }
    /// Select stable input paths (including module aliases), never process calls
    /// or new edges. Empty selection is intentional discard only without captures.
    /// The session validates the complete selection before any delivery.
    pub fn select(
        output: impl Into<String>,
        value: Value,
        destinations: Vec<crate::network::PortPath>,
    ) -> Self {
        Self {
            output: output.into(),
            value,
            destinations: Some(destinations),
        }
    }
}

/// A single bounded cooperative result, never an unrestricted batch.
#[derive(Debug)]
pub enum Step {
    Emit(ProcessEmission),
    /// Preserve the activation and fairly reschedule its next step.
    Yield,
    /// Preserve the activation until the requested input can be reserved.
    WaitInput(String),
    /// Stage another head-only reservation within this activation. Exhausted
    /// required groups fail; unrelated EOS cannot satisfy their readiness.
    WaitInputs(crate::runtime::Readiness),
    /// Preserve the activation until its registered wake fires.
    WaitWake,
    /// Successful logical activation completion, distinct from publication.
    Complete,
    /// End the process, settling its current activation.
    Finished,
}

/// Owned inputs for exactly one logical reservation. Processes may remove an
/// input but cannot dequeue from, or retain a borrow into, session queues.
#[derive(Debug, Default)]
pub struct ProcessContext {
    inputs: BTreeMap<String, Value>,
    closed: Vec<String>,
    request_closed: Vec<String>,
    grants: ValidatedResourceSet,
    correlation_lane: Option<usize>,
}
impl ProcessContext {
    pub fn new(inputs: BTreeMap<String, Value>, closed: Vec<String>) -> Self {
        Self {
            inputs,
            closed,
            request_closed: vec![],
            grants: ValidatedResourceSet::default(),
            correlation_lane: None,
        }
    }
    pub fn correlation_lane(&self) -> Option<usize> {
        self.correlation_lane
    }
    pub(crate) fn with_correlation_lane(mut self, lane: Option<usize>) -> Self {
        self.correlation_lane = lane;
        self
    }
    pub(crate) fn rename_inputs(&mut self, mut rename: impl FnMut(&str) -> String) {
        self.inputs = std::mem::take(&mut self.inputs)
            .into_iter()
            .map(|(name, value)| (rename(&name), value))
            .collect();
        for name in self.closed.iter_mut().chain(&mut self.request_closed) {
            *name = rename(name);
        }
    }
    pub fn with_resources(mut self, grants: ValidatedResourceSet) -> Self {
        self.grants = grants;
        self
    }
    pub(crate) fn with_request_closed(mut self, ports: Vec<String>) -> Self {
        self.request_closed = ports;
        self
    }
    /// Actual network EOS, distinct from absence proven for this request.
    pub fn is_eos(&self, name: &str) -> bool {
        self.closed.iter().any(|port| port == name)
    }
    pub fn is_request_closed(&self, name: &str) -> bool {
        self.request_closed.iter().any(|port| port == name)
    }
    /// Resolve exactly the host-granted capability type without invoking it.
    pub fn resource<C: Send + Sync + 'static>(
        &self,
        key: &'static str,
    ) -> Result<&C, ResourceError> {
        self.grants.resource(key)
    }
    pub fn input(&self, name: &str) -> Option<&Value> {
        self.inputs.get(name)
    }
    pub fn take(&mut self, name: &str) -> Option<Value> {
        self.inputs.remove(name)
    }
    pub fn is_closed(&self, name: &str) -> bool {
        // Request absence does not end the persistent process; use is_eos when
        // deciding whether to return Finished rather than Complete.
        self.is_eos(name) || self.is_request_closed(name)
    }
    pub fn inputs(&self) -> impl Iterator<Item = (&str, &Value)> {
        self.inputs
            .iter()
            .map(|(name, value)| (name.as_str(), value))
    }

    fn known_leases(&self) -> impl Iterator<Item = &Arc<dyn Send + Sync>> {
        self.inputs
            .values()
            .flat_map(Value::known_leases)
            .chain(self.grants.known_leases())
    }

    fn into_cleanup_parts(self) -> (Vec<Value>, ValidatedResourceSet) {
        let Self { inputs, grants, .. } = self;
        (inputs.into_values().collect(), grants)
    }

    fn dispose_catching(self) -> bool {
        let (values, grants) = self.into_cleanup_parts();
        let mut panicked = false;
        for value in values {
            panicked |= value.dispose_catching();
        }
        panicked |= grants.dispose_catching();
        panicked
    }
}

/// A step identity is not a logical activation identity. Private fields prevent
/// a runner from manufacturing authority for another owner or reservation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StepTicket {
    owner: u64,
    generation: u64,
    activation: u64,
    step: u64,
}
impl StepTicket {
    pub fn activation(self) -> u64 {
        self.activation
    }
    pub fn sequence(self) -> u64 {
        self.step
    }
}

/// The job is Send even when the process is not Sync. It contains no callback
/// that can publish or mutate a live scheduler.
///
/// Jobs cannot be duplicated by a dispatcher:
/// ```compile_fail
/// use phenix_graph::process::ActivationJob;
/// fn duplicate(job: ActivationJob) { let _ = job.clone(); }
/// ```
pub struct ActivationJob {
    ticket: StepTicket,
    execution: OwnedExecution,
    // Keep native code/resources leased until the process and arguments drop.
    lease: Option<Arc<dyn Send + Sync>>,
}
struct OwnedExecution(Option<Box<JobExecution>>);
impl std::ops::Deref for OwnedExecution {
    type Target = JobExecution;
    fn deref(&self) -> &Self::Target {
        self.0.as_deref().expect("live job execution")
    }
}
impl std::ops::DerefMut for OwnedExecution {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.0.as_deref_mut().expect("live job execution")
    }
}
struct JobExecution {
    process: Box<dyn Process>,
    context: ProcessContext,
    metadata: Option<crate::runtime::ActivationMetadata>,
    guards: Vec<Arc<dyn ProcessExecutionGuard>>,
}
impl ActivationJob {
    pub fn metadata(&self) -> Option<&crate::runtime::ActivationMetadata> {
        self.execution.metadata.as_ref()
    }
    pub(crate) fn with_metadata(mut self, metadata: crate::runtime::ActivationMetadata) -> Self {
        self.execution.metadata = Some(metadata);
        self
    }
    /// The guard is entered by `run`, on the actual executing worker thread.
    /// Additional guards compose; they cannot replace previously installed guards.
    pub fn with_guard(mut self, guard: Arc<dyn ProcessExecutionGuard>) -> Self {
        self.execution.guards.push(guard);
        self
    }
    pub fn ticket(&self) -> StepTicket {
        self.ticket
    }
    pub(crate) fn dispose_catching(mut self) -> bool {
        self.dispose_contents()
    }
    fn dispose_contents(&mut self) -> bool {
        let Some(execution) = self.execution.0.take() else {
            return false;
        };
        let lease = self.lease.take();
        let JobExecution {
            process,
            context,
            metadata,
            guards,
        } = *execution;
        let mut panicked = context.dispose_catching();
        panicked |= dispose_process_catching(process);
        for guard in guards.into_iter().rev() {
            panicked |= drop_catching(guard);
        }
        panicked |= drop_catching(metadata);
        // Keep this exact job's code/resources alive through every cleanup above.
        panicked |= drop_catching(lease);
        panicked
    }
    fn identity(&self) -> crate::FailureContext {
        let mut context = crate::FailureContext {
            ticket: Some(self.ticket),
            ..Default::default()
        };
        if let Some(metadata) = self.metadata() {
            context.session = Some(metadata.session);
            context.generation = Some(metadata.generation);
            context.request = metadata.request;
            context.process = Some(metadata.process);
            context.path = Some(metadata.path.clone());
        }
        context
    }
    fn enrich(&self, failure: Failure) -> Failure {
        let mut failure = enrich_identity(failure, &self.identity());
        if let Some(lease) = &self.lease {
            failure = failure.retain_leases(std::slice::from_ref(lease));
        }
        for lease in self.execution.context.known_leases() {
            failure = failure.retain_leases(std::slice::from_ref(lease));
        }
        failure
    }
    pub fn run(mut self) -> CompletedStep {
        let input_leases: Vec<_> = self.execution.context.known_leases().cloned().collect();
        let mut entered = Vec::with_capacity(self.execution.guards.len());
        let mut result = Ok(Step::Complete);
        for (index, guard) in self.execution.guards.iter().enumerate() {
            let entry =
                match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| guard.enter())) {
                    Ok(entry) => entry,
                    Err(payload) => Err(callback_panicked(CallbackPhase::GuardEnter, payload)),
                };
            match entry {
                Ok(token) => entered.push((index, token)),
                Err(failure) => {
                    let mut context = failure.context().clone();
                    context.guard_index = Some(index);
                    context.phase.get_or_insert(CallbackPhase::GuardEnter);
                    result = Err(self.enrich(failure.with_context(context)));
                    break;
                }
            }
        }
        if result.is_ok() {
            result = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let execution = &mut *self.execution;
                execution.process.step(&mut execution.context)
            })) {
                Ok(result) => result,
                Err(payload) => Err(callback_panicked(CallbackPhase::Step, payload)),
            }
            .map_err(|failure| {
                let mut context = failure.context().clone();
                context.phase.get_or_insert(CallbackPhase::Step);
                self.enrich(failure.with_context(context))
            });
            if let Ok(Step::Emit(emission)) = &mut result
                && let Some(lease) = &self.lease
            {
                emission
                    .value
                    .retain_known_leases(std::slice::from_ref(lease));
            }
        }
        while let Some((index, guard)) = entered.pop() {
            if let Err(payload) =
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| drop(guard)))
            {
                let failure = callback_panicked(CallbackPhase::GuardExit, payload);
                let mut context = failure.context().clone();
                context.guard_index = Some(index);
                result = add_failure(result, self.enrich(failure.with_context(context)));
            }
        }
        result = result.map_err(|failure| self.enrich(failure));
        result = result.map_err(|failure| failure.retain_leases(&input_leases));
        if let Ok(Step::Emit(emission)) = &mut result {
            emission.value.retain_known_leases(&input_leases);
        }
        for lease in input_leases.into_iter().rev() {
            if drop_catching(lease) {
                result = add_failure(
                    result,
                    self.enrich(ProcessCleanupError::OwnedState.failure()),
                );
            }
        }
        CompletedStep { job: self, result }
    }
}
impl Drop for ActivationJob {
    fn drop(&mut self) {
        let _ = self.dispose_contents();
    }
}

/// Returned guards need not be Send: creation and destruction happen inside run.
pub trait ProcessExecutionGuard: Send + Sync {
    fn enter(&self) -> Result<Box<dyn std::any::Any>, Failure>;
}
impl fmt::Debug for ActivationJob {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ActivationJob")
            .field("ticket", &self.ticket)
            .finish_non_exhaustive()
    }
}

/// A completion owns the mutated process as well as its one step result.
/// It is not cloneable. A runner cannot substitute an independently constructed
/// process or attach a result to a copied ticket.
///
/// ```compile_fail
/// use phenix_graph::process::CompletedStep;
/// fn duplicate(completion: CompletedStep) { let _ = completion.clone(); }
/// ```
pub struct CompletedStep {
    // Drop an emitted application value before the job releases its code lease.
    result: Result<Step, Failure>,
    job: ActivationJob,
}
impl CompletedStep {
    pub fn metadata(&self) -> Option<&crate::runtime::ActivationMetadata> {
        self.job.metadata()
    }
    pub fn ticket(&self) -> StepTicket {
        self.job.ticket
    }
    pub(crate) fn dispose_catching(mut self) -> bool {
        self.dispose_contents()
    }
    fn dispose_contents(&mut self) -> bool {
        let result = std::mem::replace(&mut self.result, Ok(Step::Finished));
        let mut panicked = dispose_step(result);
        panicked |= self.job.dispose_contents();
        panicked
    }
}
impl Drop for CompletedStep {
    fn drop(&mut self) {
        let _ = self.dispose_contents();
    }
}
impl fmt::Debug for CompletedStep {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CompletedStep")
            .field("ticket", &self.job.ticket)
            .field("result", &self.result)
            .finish()
    }
}

/// Dispatch customization transfers ownership. Rejection must return the job
/// unchanged; accepted jobs must eventually be returned or explicitly abandoned
/// by the runner. Side effects cannot be rolled back by cancellation.
pub trait ProcessDispatcher: Send + Sync {
    fn dispatch(&self, job: ActivationJob) -> DispatchResult;
}
#[derive(Debug)]
pub enum DispatchResult {
    Completed(CompletedStep),
    Deferred,
    Rejected(ActivationJob),
}
pub struct LocalProcessDispatcher;
impl ProcessDispatcher for LocalProcessDispatcher {
    fn dispatch(&self, job: ActivationJob) -> DispatchResult {
        DispatchResult::Completed(job.run())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HandoffState {
    Idle,
    Ready,
    Dispatched,
    PublicationPending,
    WaitingInput,
    WaitingWake,
    Finished,
    Failed,
    Cancelled,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HandoffError {
    InvalidState(HandoffState),
    ForeignCompletion,
    StaleCompletion,
    CounterExhausted,
    DestructorPanicked,
}
impl fmt::Display for HandoffError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "process handoff: {self:?}")
    }
}
impl std::error::Error for HandoffError {}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HandoffCancelError {
    DestructorPanicked { ticket: Option<StepTicket> },
}
impl fmt::Display for HandoffCancelError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "process handoff cancellation: {self:?}")
    }
}
impl std::error::Error for HandoffCancelError {}

/// Exclusive ownership and activation accounting for one physical process.
/// This is a handoff primitive, not a second graph scheduler. Queue reservation,
/// readiness, routing, and publication are deliberately absent.
pub struct ProcessHandoff {
    owner: u64,
    generation: u64,
    activation: u64,
    sequence: u64,
    state: HandoffState,
    process: Option<Box<dyn Process>>,
    context: Option<ProcessContext>,
    outstanding: Option<StepTicket>,
    lease: Option<Arc<dyn Send + Sync>>,
    // Weak deduplication only, never authority or a code owner. Repeated bound
    // source starts must not grow a lease chain once per request.
    attached_leases: Vec<std::sync::Weak<dyn Send + Sync>>,
}

pub(crate) struct CancelledProcess {
    ticket: Option<StepTicket>,
    context: Option<ProcessContext>,
    process: Option<Box<dyn Process>>,
    lease: Option<Arc<dyn Send + Sync>>,
}

impl CancelledProcess {
    pub(crate) fn ticket(&self) -> Option<StepTicket> {
        self.ticket
    }

    pub(crate) fn dispose(self) -> Result<Option<StepTicket>, HandoffCancelError> {
        let Self {
            ticket,
            context,
            process,
            lease,
        } = self;
        let mut panicked = false;
        if let Some(context) = context {
            panicked |= context.dispose_catching();
        }
        if let Some(process) = process {
            panicked |= dispose_process_catching(process);
        }
        if let Some(lease) = lease {
            panicked |= drop_catching(lease);
        }
        if panicked {
            Err(HandoffCancelError::DestructorPanicked { ticket })
        } else {
            Ok(ticket)
        }
    }
}

impl ProcessHandoff {
    pub fn new(process: Box<dyn Process>) -> Self {
        let owner = NEXT_OWNER
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |id| id.checked_add(1))
            .expect("process owner identity exhausted");
        Self {
            owner,
            generation: 0,
            activation: 0,
            sequence: 0,
            state: HandoffState::Idle,
            process: Some(process),
            context: None,
            outstanding: None,
            lease: None,
            attached_leases: Vec::new(),
        }
    }
    pub fn with_lease(mut self, lease: Arc<dyn Send + Sync>) -> Self {
        self.attached_leases.clear();
        self.lease = Some(lease);
        self
    }
    pub(crate) fn retain_leases(&mut self, leases: &[Arc<dyn Send + Sync>]) {
        let mut added = Vec::new();
        for lease in leases {
            let weak = Arc::downgrade(lease);
            if !self.attached_leases.iter().any(|held| held.ptr_eq(&weak)) {
                self.attached_leases.push(weak);
                if !self
                    .lease
                    .as_ref()
                    .is_some_and(|held| Arc::ptr_eq(held, lease))
                {
                    added.push(lease.clone());
                }
            }
        }
        if !added.is_empty() {
            self.lease = Some(crate::network::HandleLeases::append(
                self.lease.take(),
                added,
            ));
        }
    }
    pub fn state(&self) -> HandoffState {
        self.state
    }
    pub fn outstanding(&self) -> Option<StepTicket> {
        self.outstanding
    }
    pub(crate) fn owns(&self, ticket: StepTicket) -> bool {
        self.owner == ticket.owner
    }
    /// New ownership authority and application state, preserving only monotonic
    /// physical-process activation/step accounting across an explicit restart.
    pub(crate) fn replacement(&self, process: Box<dyn Process>) -> Self {
        debug_assert_eq!(self.state, HandoffState::Cancelled);
        let mut replacement = Self::new(process);
        replacement.activation = self.activation;
        replacement.sequence = self.sequence;
        replacement
    }
    pub fn admit(&mut self, context: ProcessContext) -> Result<(), (HandoffError, ProcessContext)> {
        if let Err(error) = self.require(HandoffState::Idle) {
            return Err((error, context));
        }
        let Some(activation) = self.activation.checked_add(1) else {
            return Err((HandoffError::CounterExhausted, context));
        };
        self.activation = activation;
        self.context = Some(context);
        self.state = HandoffState::Ready;
        Ok(())
    }
    pub fn next_job(&mut self) -> Result<ActivationJob, HandoffError> {
        self.require(HandoffState::Ready)?;
        self.sequence = self
            .sequence
            .checked_add(1)
            .ok_or(HandoffError::CounterExhausted)?;
        let ticket = StepTicket {
            owner: self.owner,
            generation: self.generation,
            activation: self.activation,
            step: self.sequence,
        };
        self.outstanding = Some(ticket);
        self.state = HandoffState::Dispatched;
        Ok(ActivationJob {
            ticket,
            execution: OwnedExecution(Some(Box::new(JobExecution {
                process: self.process.take().expect("ready process"),
                context: self.context.take().expect("ready context"),
                metadata: None,
                guards: Vec::new(),
            }))),
            lease: self.lease.clone(),
        })
    }
    /// Rejecting an unexecuted job does not consume the reservation or mutate
    /// the process. The next dispatch receives a new step ticket.
    pub fn reject(
        &mut self,
        mut job: ActivationJob,
    ) -> Result<Result<(), Failure>, (HandoffError, ActivationJob)> {
        if let Err(error) = self.validate(job.ticket) {
            return Err((error, job));
        }
        let identity = job.identity();
        let cleanup = self.restore(&mut job);
        if let Some(mut failure) = cleanup {
            self.state = HandoffState::Failed;
            let leases: Vec<_> = self
                .lease
                .iter()
                .chain(self.context.iter().flat_map(ProcessContext::known_leases))
                .cloned()
                .collect();
            let mut panicked = self
                .context
                .take()
                .is_some_and(|context| context.dispose_catching());
            panicked |= self.process.take().is_some_and(dispose_process_catching);
            panicked |= drop_catching(self.lease.take());
            if panicked {
                failure = failure.with_cleanup(
                    enrich_identity(ProcessCleanupError::OwnedState.failure(), &identity)
                        .retain_leases(&leases),
                );
            }
            Ok(Err(failure))
        } else {
            self.state = HandoffState::Ready;
            Ok(Ok(()))
        }
    }
    /// Foreign/stale completions are returned intact. In particular, probing a
    /// wrong owner cannot accidentally destroy another owner's running process.
    pub fn complete(
        &mut self,
        mut completion: CompletedStep,
    ) -> Result<Result<Step, Failure>, (HandoffError, Box<CompletedStep>)> {
        if let Err(error) = self.validate(completion.job.ticket) {
            return Err((error, Box::new(completion)));
        }
        let result = std::mem::replace(&mut completion.result, Ok(Step::Finished));
        let job = &mut completion.job;
        let identity = job.identity();
        let leases: Vec<_> = job
            .lease
            .iter()
            .chain(self.lease.iter())
            .chain(job.execution.context.known_leases())
            .cloned()
            .collect();
        let mut result = result
            .map_err(|failure| job.enrich(failure))
            .map(|step| match step {
                Step::Emit(mut emission) => {
                    for lease in job.lease.iter().chain(self.lease.iter()) {
                        emission.value = emission.value.with_lease(lease.clone());
                    }
                    Step::Emit(emission)
                }
                step => step,
            });
        if let Some(failure) = self.restore(job) {
            result = add_failure(result, failure);
        }
        let mut cleanup_panicked = false;
        self.state = match &result {
            Ok(Step::Emit(_)) => HandoffState::PublicationPending,
            Ok(Step::Yield) => HandoffState::Ready,
            Ok(Step::WaitInput(_)) => HandoffState::WaitingInput,
            Ok(Step::WaitInputs(_)) => HandoffState::WaitingInput,
            Ok(Step::WaitWake) => HandoffState::WaitingWake,
            Ok(Step::Complete) => HandoffState::Idle,
            Ok(Step::Finished) => HandoffState::Finished,
            Err(_) => HandoffState::Failed,
        };
        if matches!(
            self.state,
            HandoffState::Idle | HandoffState::Finished | HandoffState::Failed
        ) && let Some(context) = self.context.take()
        {
            cleanup_panicked |= context.dispose_catching();
        }
        if (matches!(self.state, HandoffState::Finished | HandoffState::Failed) || cleanup_panicked)
            && let Some(process) = self.process.take()
        {
            cleanup_panicked |= dispose_process_catching(process);
        }
        if (matches!(self.state, HandoffState::Finished | HandoffState::Failed) || cleanup_panicked)
            && let Some(lease) = self.lease.take()
        {
            cleanup_panicked |= drop_catching(lease);
        }
        if cleanup_panicked {
            self.state = HandoffState::Failed;
            if let Some(process) = self.process.take() {
                let _ = dispose_process_catching(process);
            }
            result = add_failure(
                result,
                enrich_identity(ProcessCleanupError::OwnedState.failure(), &identity),
            );
        }
        result = result.map_err(|failure| failure.retain_leases(&leases));
        let mut lease_panicked = false;
        for lease in leases.into_iter().rev() {
            lease_panicked |= drop_catching(lease);
        }
        if lease_panicked {
            self.state = HandoffState::Failed;
            let mut failure = enrich_identity(ProcessCleanupError::OwnedState.failure(), &identity);
            if let Some(lease) = &self.lease {
                failure = failure.retain_leases(std::slice::from_ref(lease));
            }
            result = add_failure(result, failure);
        }
        Ok(result)
    }
    /// Call only after the retained emission commits. A blocked publication
    /// must leave this handoff untouched, preventing action re-execution.
    pub fn publication_committed(&mut self) -> Result<(), HandoffError> {
        self.require(HandoffState::PublicationPending)?;
        self.state = HandoffState::Ready;
        Ok(())
    }
    pub fn resume_wake(&mut self) -> Result<(), HandoffError> {
        self.require(HandoffState::WaitingWake)?;
        self.state = HandoffState::Ready;
        Ok(())
    }
    pub fn publication_failed(&mut self) -> Result<(), HandoffError> {
        self.require(HandoffState::PublicationPending)?;
        self.state = HandoffState::Failed;
        let context_panicked = self
            .context
            .take()
            .is_some_and(|context| context.dispose_catching());
        let process_panicked = self.process.take().is_some_and(dispose_process_catching);
        let lease_panicked = self.lease.take().is_some_and(drop_catching);
        if context_panicked || process_panicked || lease_panicked {
            Err(HandoffError::DestructorPanicked)
        } else {
            Ok(())
        }
    }
    /// Release an idle process after the session has delivered all input EOS.
    pub fn finish_idle(&mut self) -> Result<(), HandoffError> {
        self.require(HandoffState::Idle)?;
        self.state = HandoffState::Finished;
        let context_panicked = self
            .context
            .take()
            .is_some_and(|context| context.dispose_catching());
        let process_panicked = self.process.take().is_some_and(dispose_process_catching);
        let lease_panicked = self.lease.take().is_some_and(drop_catching);
        if context_panicked || process_panicked || lease_panicked {
            Err(HandoffError::DestructorPanicked)
        } else {
            Ok(())
        }
    }
    /// Scheduler-owned admission after an exact additional reservation commits.
    pub fn resume_input(
        &mut self,
        inputs: ProcessContext,
    ) -> Result<(), (HandoffError, ProcessContext)> {
        if let Err(error) = self.require(HandoffState::WaitingInput) {
            return Err((error, inputs));
        }
        let context = self.context.as_mut().expect("waiting context");
        if inputs
            .inputs
            .keys()
            .any(|name| context.inputs.contains_key(name))
        {
            return Err((HandoffError::InvalidState(self.state), inputs));
        }
        context.inputs.extend(inputs.inputs);
        for name in inputs.request_closed {
            if !context.request_closed.contains(&name) {
                context.request_closed.push(name);
            }
        }
        for name in inputs.closed {
            if !context.closed.contains(&name) {
                context.closed.push(name);
            }
        }
        self.state = HandoffState::Ready;
        Ok(())
    }
    /// Invalidates authority before releasing owned state. The returned ticket
    /// remains runner inventory: it does not imply that external work stopped.
    /// A destructor panic is returned only after all handoff-owned fields have
    /// been detached and each cleanup drop has been attempted.
    pub fn cancel(&mut self) -> Result<Option<StepTicket>, HandoffCancelError> {
        self.cancel_detach().dispose()
    }

    /// Invalidate the handoff now and return plugin-owned state for destruction
    /// after the enclosing request/session transition is fully visible.
    pub(crate) fn cancel_detach(&mut self) -> CancelledProcess {
        self.state = HandoffState::Cancelled;
        // Cancellation is terminal; no future admission can reuse this owner.
        self.generation = self.generation.saturating_add(1);
        CancelledProcess {
            ticket: self.outstanding.take(),
            context: self.context.take(),
            process: self.process.take(),
            lease: self.lease.take(),
        }
    }
    fn require(&self, state: HandoffState) -> Result<(), HandoffError> {
        if self.state == state {
            Ok(())
        } else {
            Err(HandoffError::InvalidState(self.state))
        }
    }
    fn validate(&self, ticket: StepTicket) -> Result<(), HandoffError> {
        if ticket.owner != self.owner {
            return Err(HandoffError::ForeignCompletion);
        }
        if self.state != HandoffState::Dispatched || self.outstanding != Some(ticket) {
            return Err(HandoffError::StaleCompletion);
        }
        Ok(())
    }
    fn restore(&mut self, job: &mut ActivationJob) -> Option<Failure> {
        let mut cleanup = None;
        // Guard objects are executable owners too, and must be attempted separately.
        let guards = std::mem::take(&mut job.execution.guards);
        for (index, guard) in guards.into_iter().enumerate().rev() {
            if drop_catching(guard) {
                let failure =
                    job.enrich(ProcessCleanupError::GuardObject { guard_index: index }.failure());
                cleanup = Some(match cleanup {
                    Some(primary) => Failure::with_cleanup(primary, failure),
                    None => failure,
                });
            }
        }
        let JobExecution {
            process,
            context,
            metadata: _,
            guards,
        } = *job.execution.0.take().expect("accepted job execution");
        self.outstanding = None;
        self.process = Some(process);
        self.context = Some(context);
        // The job's lease survives until after process/context transfer.
        debug_assert!(guards.is_empty());
        let _ = drop_catching(job.lease.take());
        cleanup
    }
}
impl Drop for ProcessHandoff {
    fn drop(&mut self) {
        let _ = self.cancel_detach().dispose();
    }
}

fn drop_catching<T>(value: T) -> bool {
    if let Err(payload) = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| drop(value))) {
        crate::discard_panic_payload(payload);
        true
    } else {
        false
    }
}
