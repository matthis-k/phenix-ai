//! Exclusive, owned cooperative process handoff.
//!
//! A job owns its process and arguments, never a borrow into scheduler queues.
//! Returning an emission does not complete the logical activation. A caller must
//! commit that emission before resuming its continuation, and must explicitly
//! settle `Complete` before admitting another reservation.
use crate::{Store, Value};
use std::collections::BTreeMap;
use std::fmt;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_OWNER: AtomicU64 = AtomicU64::new(1);

/// One process-private state machine. `Sync` is deliberately not required.
/// Implementations must return from each step; cancellation cannot preempt code.
pub trait Process: Send + 'static {
    fn step(&mut self, context: &mut ProcessContext) -> Result<Step, String>;
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
            inner: self.inner.spawn(),
            lease: self.lease.clone(),
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
    inner: Box<dyn Process>,
    lease: Arc<dyn Send + Sync>,
}
impl Process for LeasedProcess {
    fn step(&mut self, context: &mut ProcessContext) -> Result<Step, String> {
        Ok(match self.inner.step(context)? {
            Step::Emit(mut emission) => {
                emission.value = emission.value.with_lease(self.lease.clone());
                Step::Emit(emission)
            }
            step => step,
        })
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
#[derive(Clone)]
pub struct ResourceGrant {
    store: Arc<Store>,
    key: &'static str,
    writable: bool,
}
impl ResourceGrant {
    pub fn read(store: Arc<Store>, key: &'static str) -> Self {
        Self {
            store,
            key,
            writable: false,
        }
    }
    pub fn write(store: Arc<Store>, key: &'static str) -> Self {
        Self {
            store,
            key,
            writable: true,
        }
    }
}
impl fmt::Debug for ResourceGrant {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ResourceGrant")
            .field("key", &self.key)
            .field("writable", &self.writable)
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Default)]
pub struct ProcessContext {
    inputs: BTreeMap<String, Value>,
    closed: Vec<String>,
    request_closed: Vec<String>,
    grants: Vec<ResourceGrant>,
    correlation_lane: Option<usize>,
}
impl ProcessContext {
    pub fn new(inputs: BTreeMap<String, Value>, closed: Vec<String>) -> Self {
        Self {
            inputs,
            closed,
            request_closed: vec![],
            grants: vec![],
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
    pub fn with_resources(mut self, grants: Vec<ResourceGrant>) -> Self {
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
    pub fn read<T: Send + Sync + 'static, R>(
        &self,
        key: &'static str,
        f: impl FnOnce(&T) -> R,
    ) -> Result<R, String> {
        let grant = self
            .grants
            .iter()
            .find(|g| g.key == key)
            .ok_or_else(|| format!("read of {key} not granted"))?;
        let guard = grant
            .store
            .get::<T>(key)?
            .read()
            .map_err(|e| e.to_string())?;
        Ok(f(&guard))
    }
    pub fn write<T: Send + Sync + 'static, R>(
        &self,
        key: &'static str,
        f: impl FnOnce(&mut T) -> R,
    ) -> Result<R, String> {
        let grant = self
            .grants
            .iter()
            .find(|g| g.key == key && g.writable)
            .ok_or_else(|| format!("write of {key} not granted"))?;
        let mut guard = grant
            .store
            .get::<T>(key)?
            .write()
            .map_err(|e| e.to_string())?;
        Ok(f(&mut guard))
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

    fn into_cleanup_parts(self) -> (Vec<Value>, Vec<ResourceGrant>) {
        let Self { inputs, grants, .. } = self;
        (inputs.into_values().collect(), grants)
    }

    fn dispose_catching(self) -> bool {
        let (values, grants) = self.into_cleanup_parts();
        let mut panicked = false;
        for value in values {
            panicked |= drop_catching(value);
        }
        for grant in grants {
            panicked |= drop_catching(grant);
        }
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
    execution: Box<JobExecution>,
    // Keep native code/resources leased until the process and arguments drop.
    lease: Option<Arc<dyn Send + Sync>>,
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
    pub(crate) fn dispose_catching(self) -> bool {
        let Self {
            execution, lease, ..
        } = self;
        let JobExecution {
            process,
            context,
            metadata,
            guards,
        } = *execution;
        let mut panicked = context.dispose_catching();
        panicked |= drop_catching(process);
        for guard in guards.into_iter().rev() {
            panicked |= drop_catching(guard);
        }
        panicked |= drop_catching(metadata);
        // Keep this exact job's code/resources alive through every cleanup above.
        panicked |= drop_catching(lease);
        panicked
    }
    pub fn run(mut self) -> CompletedStep {
        let mut entered = Vec::with_capacity(self.execution.guards.len());
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let execution = &mut self.execution;
            for guard in &execution.guards {
                entered.push(guard.enter()?);
            }
            execution.process.step(&mut execution.context)
        }));
        let mut result = match result {
            Ok(result) => result,
            Err(payload) => {
                crate::discard_panic_payload(payload);
                Err("process step panicked".into())
            }
        };
        while let Some(guard) = entered.pop() {
            if let Err(payload) =
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| drop(guard)))
            {
                crate::discard_panic_payload(payload);
                result = Err("process execution guard teardown panicked".into());
            }
        }
        CompletedStep { job: self, result }
    }
}

/// Returned guards need not be Send: creation and destruction happen inside run.
pub trait ProcessExecutionGuard: Send + Sync {
    fn enter(&self) -> Result<Box<dyn std::any::Any>, String>;
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
    result: Result<Step, String>,
    job: ActivationJob,
}
impl CompletedStep {
    pub fn metadata(&self) -> Option<&crate::runtime::ActivationMetadata> {
        self.job.metadata()
    }
    pub fn ticket(&self) -> StepTicket {
        self.job.ticket
    }
    pub(crate) fn dispose_catching(self) -> bool {
        let Self { result, job } = self;
        let mut panicked = drop_catching(result);
        panicked |= job.dispose_catching();
        panicked
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
            panicked |= drop_catching(process);
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
        }
    }
    pub fn with_lease(mut self, lease: Arc<dyn Send + Sync>) -> Self {
        self.lease = Some(lease);
        self
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
            execution: Box::new(JobExecution {
                process: self.process.take().expect("ready process"),
                context: self.context.take().expect("ready context"),
                metadata: None,
                guards: Vec::new(),
            }),
            lease: self.lease.clone(),
        })
    }
    /// Rejecting an unexecuted job does not consume the reservation or mutate
    /// the process. The next dispatch receives a new step ticket.
    pub fn reject(&mut self, job: ActivationJob) -> Result<(), (HandoffError, ActivationJob)> {
        if let Err(error) = self.validate(job.ticket) {
            return Err((error, job));
        }
        self.restore(job);
        self.state = HandoffState::Ready;
        Ok(())
    }
    /// Foreign/stale completions are returned intact. In particular, probing a
    /// wrong owner cannot accidentally destroy another owner's running process.
    pub fn complete(
        &mut self,
        completion: CompletedStep,
    ) -> Result<Result<Step, String>, (HandoffError, Box<CompletedStep>)> {
        if let Err(error) = self.validate(completion.job.ticket) {
            return Err((error, Box::new(completion)));
        }
        let CompletedStep { job, result } = completion;
        let mut result = result.map(|step| match step {
            Step::Emit(mut emission) => {
                for lease in job.lease.iter().chain(self.lease.iter()) {
                    emission.value = emission.value.with_lease(lease.clone());
                }
                Step::Emit(emission)
            }
            step => step,
        });
        self.restore(job);
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
            cleanup_panicked |= drop_catching(process);
        }
        if (matches!(self.state, HandoffState::Finished | HandoffState::Failed) || cleanup_panicked)
            && let Some(lease) = self.lease.take()
        {
            cleanup_panicked |= drop_catching(lease);
        }
        if cleanup_panicked {
            let error = match result {
                Err(error) => format!("{error}; owned destructor panicked during completion"),
                Ok(_) => "owned destructor panicked during completion".into(),
            };
            self.state = HandoffState::Failed;
            if let Some(process) = self.process.take() {
                let _ = drop_catching(process);
            }
            result = Err(error);
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
        let process_panicked = self.process.take().is_some_and(drop_catching);
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
        let process_panicked = self.process.take().is_some_and(drop_catching);
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
    fn restore(&mut self, job: ActivationJob) {
        let JobExecution {
            process,
            context,
            metadata: _,
            guards,
        } = *job.execution;
        self.outstanding = None;
        self.process = Some(process);
        self.context = Some(context);
        // The job's lease survives until after process/context transfer.
        drop(guards);
        drop(job.lease);
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
