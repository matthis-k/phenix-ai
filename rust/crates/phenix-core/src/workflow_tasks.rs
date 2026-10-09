//! Root-owned pending native calls, cancellation, and completion wakeups.
//!
//! Workers use the existing kernel task runtime and retain the selected root's
//! generation lease through actual settlement. A bound component import can
//! run on this path. The compiled workflow scheduler does not yet suspend or
//! resume its Fork children from these tickets; plugin ABI parity is separate.

use crate::{
    Authority, CancellationToken, KernelError, ResolvedImportHandle, RootExecutionHandle,
    TaskCancellationHandle, TaskHandle,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
        mpsc::{self, Receiver, Sender},
    },
    thread,
};

/// Correlated completion identity. The generation component prevents a late
/// callback into a different resident plan after a promotion.
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct WorkflowTaskId {
    pub generation: String,
    pub scope: String,
    pub call: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WorkflowTaskState {
    Pending,
    Cancelling,
    Completed,
    Failed,
    Cancelled,
}

impl WorkflowTaskState {
    fn terminal(self) -> bool {
        matches!(self, Self::Completed | Self::Failed | Self::Cancelled)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum WorkflowTaskError {
    WrongGeneration(WorkflowTaskId),
    DuplicateAdmission(WorkflowTaskId),
    UnknownTask(WorkflowTaskId),
    DuplicateSettlement(WorkflowTaskId),
    InvalidSettlement(WorkflowTaskId),
    RootNotAdmitting,
    OutstandingTasks(usize),
    MissingGeneration,
    CallCounterOverflow,
}

#[derive(Clone, Debug)]
pub struct WorkflowPendingTasks {
    generation: String,
    active: BTreeMap<WorkflowTaskId, WorkflowTaskState>,
    cancelled_scopes: BTreeSet<String>,
    root_cancelled: bool,
    closed: bool,
}

impl WorkflowPendingTasks {
    pub fn new(generation: impl Into<String>) -> Self {
        Self {
            generation: generation.into(),
            active: BTreeMap::new(),
            cancelled_scopes: BTreeSet::new(),
            root_cancelled: false,
            closed: false,
        }
    }

    fn scoped(&self, scope: &str) -> bool {
        self.cancelled_scopes.iter().any(|cancelled| {
            scope == cancelled
                || scope
                    .strip_prefix(cancelled)
                    .is_some_and(|remainder| remainder.starts_with('/'))
        })
    }

    /// Admit only while the pinned root is open and the enclosing scope has
    /// not been cancelled. A ticket is unique for the root lifetime.
    pub fn admit(&mut self, id: WorkflowTaskId) -> Result<(), WorkflowTaskError> {
        if id.generation != self.generation {
            return Err(WorkflowTaskError::WrongGeneration(id));
        }
        if self.closed || self.root_cancelled || self.scoped(&id.scope) {
            return Err(WorkflowTaskError::RootNotAdmitting);
        }
        if self.active.contains_key(&id) {
            return Err(WorkflowTaskError::DuplicateAdmission(id));
        }
        self.active.insert(id, WorkflowTaskState::Pending);
        Ok(())
    }

    /// Return the exact outstanding tickets that the provider boundary needs
    /// to signal. This only requests cancellation: **none** are settled here.
    pub fn cancel_scope(&mut self, scope: &str) -> Vec<WorkflowTaskId> {
        self.cancelled_scopes.insert(scope.into());
        let mut signalled = Vec::new();
        for (id, state) in &mut self.active {
            if (id.scope == scope
                || id
                    .scope
                    .strip_prefix(scope)
                    .is_some_and(|rest| rest.starts_with('/')))
                && *state == WorkflowTaskState::Pending
            {
                *state = WorkflowTaskState::Cancelling;
                signalled.push(id.clone());
            }
        }
        signalled
    }

    /// Cancel a single ticket. Repeated requests never signal twice.
    pub fn cancel_ticket(&mut self, id: &WorkflowTaskId) -> Result<bool, WorkflowTaskError> {
        if id.generation != self.generation {
            return Err(WorkflowTaskError::WrongGeneration(id.clone()));
        }
        let state = self
            .active
            .get_mut(id)
            .ok_or_else(|| WorkflowTaskError::UnknownTask(id.clone()))?;
        if *state != WorkflowTaskState::Pending {
            return Ok(false);
        }
        *state = WorkflowTaskState::Cancelling;
        Ok(true)
    }

    pub fn cancel_root(&mut self) -> Vec<WorkflowTaskId> {
        self.root_cancelled = true;
        let mut signalled = Vec::new();
        for (id, state) in &mut self.active {
            if *state == WorkflowTaskState::Pending {
                *state = WorkflowTaskState::Cancelling;
                signalled.push(id.clone());
            }
        }
        signalled
    }

    /// A pending invocation can complete after cancellation was requested.
    /// The actual provider settlement determines its terminal status.
    pub fn settle(
        &mut self,
        id: &WorkflowTaskId,
        outcome: WorkflowTaskState,
    ) -> Result<(), WorkflowTaskError> {
        if id.generation != self.generation {
            return Err(WorkflowTaskError::WrongGeneration(id.clone()));
        }
        if !outcome.terminal() {
            return Err(WorkflowTaskError::InvalidSettlement(id.clone()));
        }
        let Some(state) = self.active.get_mut(id) else {
            return Err(WorkflowTaskError::UnknownTask(id.clone()));
        };
        if state.terminal() {
            return Err(WorkflowTaskError::DuplicateSettlement(id.clone()));
        }
        *state = outcome;
        Ok(())
    }

    /// Generation retirement and root settlement cannot infer completion
    /// from a cancelled future, disconnected channel, or requested timeout.
    pub fn close_root(&mut self) -> Result<(), WorkflowTaskError> {
        let outstanding = self
            .active
            .values()
            .filter(|state| !state.terminal())
            .count();
        if outstanding != 0 {
            return Err(WorkflowTaskError::OutstandingTasks(outstanding));
        }
        self.closed = true;
        Ok(())
    }

    pub fn outstanding(&self) -> usize {
        self.active
            .values()
            .filter(|state| !state.terminal())
            .count()
    }

    pub fn state(&self, id: &WorkflowTaskId) -> Option<WorkflowTaskState> {
        self.active.get(id).copied()
    }
}

/// A single workflow root's host-side native tasks. Every admitted worker
/// owns a clone of the pinned root lease until its callback settles, even if
/// the caller drops its task result or cancels the enclosing group.
/// Errors from a pending bound provider call never become normal plan
/// outcomes. The caller resumes the original compiled Invoke with this
/// result, or settles the root with a typed invocation failure.
#[derive(Debug)]
pub enum WorkflowNativeDispatchError {
    Cancelled,
    Invoke(KernelError),
}

/// Pending result of one generation-pinned component import invocation.
pub type WorkflowPendingImport = WorkflowNativeTask<Result<Vec<u8>, WorkflowNativeDispatchError>>;

// A callback from another root may share generation and scope names. Unique
// call IDs prevent two independent roots from ever producing the same ticket.
static NEXT_NATIVE_CALL: AtomicU64 = AtomicU64::new(0);

pub struct WorkflowNativeTaskGroup {
    root: RootExecutionHandle,
    shared: Arc<Mutex<NativeTaskLedger>>,
    completion_tx: Sender<WorkflowTaskId>,
    completion_rx: Mutex<Receiver<WorkflowTaskId>>,
}

struct NativeTaskLedger {
    pending: WorkflowPendingTasks,
    signals: BTreeMap<WorkflowTaskId, TaskCancellationHandle>,
}

struct NativeTaskSettlement {
    id: WorkflowTaskId,
    shared: Arc<Mutex<NativeTaskLedger>>,
    terminal: WorkflowTaskState,
    completion_tx: Sender<WorkflowTaskId>,
}

impl NativeTaskSettlement {
    fn settle_as(&mut self, terminal: WorkflowTaskState) {
        debug_assert!(terminal.terminal());
        self.terminal = terminal;
    }
}

impl Drop for NativeTaskSettlement {
    fn drop(&mut self) {
        let mut shared = self
            .shared
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        shared.signals.remove(&self.id);
        // A provider panic is a failed terminal settlement, not a
        // completed normal result. Cancellation alone is not settlement.
        let result = shared.pending.settle(&self.id, self.terminal);
        debug_assert!(result.is_ok(), "a worker must settle its ticket once");
        drop(shared);
        // The scheduler receives a wakeup only after actual settlement.
        let _ = self.completion_tx.send(self.id.clone());
    }
}

/// A pollable result for one native task. The actual root lease belongs to
/// the worker until its settlement, not to this result receiver.
pub struct WorkflowNativeTask<T> {
    id: WorkflowTaskId,
    task: TaskHandle<T>,
    shared: Arc<Mutex<NativeTaskLedger>>,
}

impl<T> WorkflowNativeTask<T> {
    pub fn id(&self) -> &WorkflowTaskId {
        &self.id
    }

    pub fn is_finished(&self) -> bool {
        self.task.is_finished()
    }

    pub fn join(self) -> thread::Result<T> {
        self.task.join()
    }

    pub fn cancel(&self) -> Result<bool, WorkflowTaskError> {
        let mut shared = self
            .shared
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if shared.pending.cancel_ticket(&self.id)? {
            self.task.cancel();
            Ok(true)
        } else {
            Ok(false)
        }
    }
}

impl WorkflowNativeTaskGroup {
    pub fn new(root: RootExecutionHandle) -> Result<Self, WorkflowTaskError> {
        let generation = root
            .generation()
            .ok_or(WorkflowTaskError::MissingGeneration)?
            .as_str()
            .to_owned();
        let (completion_tx, completion_rx) = mpsc::channel();
        Ok(Self {
            root,
            shared: Arc::new(Mutex::new(NativeTaskLedger {
                pending: WorkflowPendingTasks::new(generation),
                signals: BTreeMap::new(),
            })),
            completion_tx,
            completion_rx: Mutex::new(completion_rx),
        })
    }

    pub fn generation(&self) -> &crate::GenerationId {
        self.root
            .generation()
            .expect("native root generation was checked")
    }

    pub fn spawn<T, F>(
        &self,
        scope: &str,
        requested_authority: &Authority,
        worker: F,
    ) -> Result<WorkflowNativeTask<T>, WorkflowTaskError>
    where
        T: Send + 'static,
        F: FnOnce(CancellationToken) -> T + Send + 'static,
    {
        self.spawn_classified(scope, requested_authority, worker, |_| {
            WorkflowTaskState::Completed
        })
    }

    /// Admit a real selected provider call into a native worker. The existing
    /// kernel performs all import, authority, and Layer checks against this
    /// group's pinned generation. The callback result is never fabricated
    /// as a normal plan edge and failure never triggers provider fallback.
    pub fn dispatch_import_pending(
        &self,
        scope: &str,
        import: ResolvedImportHandle,
        request: Vec<u8>,
    ) -> Result<WorkflowPendingImport, WorkflowTaskError> {
        let bound_root = self.root.clone();
        self.spawn_classified(
            scope,
            self.root.authority(),
            move |token| {
                if token.is_cancelled() {
                    return Err(WorkflowNativeDispatchError::Cancelled);
                }
                let response = bound_root
                    .invoke_import(&import, &request)
                    .map_err(WorkflowNativeDispatchError::Invoke)?;
                if token.is_cancelled() {
                    return Err(WorkflowNativeDispatchError::Cancelled);
                }
                Ok(response)
            },
            |result| match result {
                Ok(_) => WorkflowTaskState::Completed,
                Err(WorkflowNativeDispatchError::Cancelled) => WorkflowTaskState::Cancelled,
                Err(WorkflowNativeDispatchError::Invoke(_)) => WorkflowTaskState::Failed,
            },
        )
    }

    fn spawn_classified<T, F, C>(
        &self,
        scope: &str,
        requested_authority: &Authority,
        worker: F,
        classify: C,
    ) -> Result<WorkflowNativeTask<T>, WorkflowTaskError>
    where
        T: Send + 'static,
        F: FnOnce(CancellationToken) -> T + Send + 'static,
        C: FnOnce(&T) -> WorkflowTaskState + Send + 'static,
    {
        let call = NEXT_NATIVE_CALL
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |current| {
                current.checked_add(1)
            })
            .map_err(|_| WorkflowTaskError::CallCounterOverflow)?
            + 1;
        let id = WorkflowTaskId {
            generation: self.generation().as_str().to_owned(),
            scope: scope.to_owned(),
            call,
        };
        // Admission and signal registration are one transaction. A concurrent
        // cancel can never observe a ticket without a cancellable worker.
        let mut shared = self
            .shared
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        shared.pending.admit(id.clone())?;
        let lease = self.root.clone();
        let settlement = NativeTaskSettlement {
            id: id.clone(),
            shared: Arc::clone(&self.shared),
            terminal: WorkflowTaskState::Failed,
            completion_tx: self.completion_tx.clone(),
        };
        let task = self
            .root
            .spawn_native_workflow_task(requested_authority, move |token| {
                // The lease is deliberately retained until after the actual
                // worker result, including late results after cancellation.
                let _lease = lease;
                let mut settlement = settlement;
                let result = worker(token);
                settlement.settle_as(classify(&result));
                result
            });
        shared
            .signals
            .insert(id.clone(), task.cancellation_handle());
        Ok(WorkflowNativeTask {
            id,
            task,
            shared: Arc::clone(&self.shared),
        })
    }

    fn signal(shared: &mut NativeTaskLedger, ids: Vec<WorkflowTaskId>) -> Vec<WorkflowTaskId> {
        for id in &ids {
            // The same mutex guards admission, settlement, and signaling:
            // cancellation can never race a worker's signal deregistration.
            shared
                .signals
                .get(id)
                .expect("an outstanding ticket has a registered cancellation signal")
                .cancel();
        }
        ids
    }

    pub fn cancel_scope(&self, scope: &str) -> Vec<WorkflowTaskId> {
        let mut shared = self
            .shared
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let ids = shared.pending.cancel_scope(scope);
        Self::signal(&mut shared, ids)
    }

    pub fn cancel_root(&self) -> Vec<WorkflowTaskId> {
        let mut shared = self
            .shared
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let ids = shared.pending.cancel_root();
        Self::signal(&mut shared, ids)
    }

    /// Drain one real callback settlement without waiting. Each ticket is
    /// delivered once and names its original scope and pinned generation.
    pub fn poll_settlement(&self) -> Option<WorkflowTaskId> {
        self.completion_rx
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .try_recv()
            .ok()
    }

    /// Sleep until a native worker settles. This explicit wakeup does not
    /// create another plan scheduler or poll a blocking provider in a loop.
    pub fn wait_settlement(&self) -> Option<WorkflowTaskId> {
        self.completion_rx
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .recv()
            .ok()
    }

    pub fn outstanding(&self) -> usize {
        self.shared
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .pending
            .outstanding()
    }

    pub fn state(&self, id: &WorkflowTaskId) -> Option<WorkflowTaskState> {
        self.shared
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .pending
            .state(id)
    }

    /// Close only after callbacks have actually settled. The group retains
    /// its root lease until the owner drops it, while individual workers keep
    /// their own leases until their callbacks finish.
    pub fn close(&self) -> Result<(), WorkflowTaskError> {
        self.shared
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .pending
            .close_root()
    }
}

impl Drop for WorkflowNativeTaskGroup {
    fn drop(&mut self) {
        self.cancel_root();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        ComponentManifest, ConfigContribution, Kernel, KernelError, PermissionId, PluginExecution,
        PluginId, PluginManifest, ResolvedGeneration, ResolvedGenerationActivation,
    };
    use std::sync::mpsc;

    fn empty_generation_with(plugin: Option<&str>) -> ResolvedGeneration {
        let plugins: Vec<PluginManifest> = plugin
            .map(|name| PluginManifest {
                id: PluginId::parse(name).unwrap(),
                version: 1,
                execution: PluginExecution::ResourceOnly,
                dependencies: Vec::new(),
                services: Vec::new(),
                resource_namespaces: Vec::new(),
                maximum_authority: Authority::default(),
            })
            .into_iter()
            .collect();
        ResolvedGeneration::resolve(
            plugins,
            Vec::<ComponentManifest>::new(),
            Vec::<ConfigContribution>::new(),
            &Authority::default(),
        )
        .unwrap()
    }

    #[test]
    fn native_worker_retains_physical_generation_lease_after_root_abandonment() {
        let first = empty_generation_with(None);
        let second = empty_generation_with(Some("fixture.next-generation"));
        assert_ne!(first.generation(), second.generation());
        let old_generation = first.generation().clone();
        let mut kernel = Kernel::new(first.kernel_config().clone());
        kernel.activate_resolved_generation(&first).unwrap();
        kernel.activate_all().unwrap();
        let group = kernel
            .root_execution_handle(&Authority::default())
            .native_workflow_tasks()
            .unwrap();
        assert_eq!(group.generation(), &old_generation);
        let (ready_tx, ready_rx) = mpsc::channel();
        let (finish_tx, finish_rx) = mpsc::channel::<()>();
        let task = group
            .spawn("root/map/0", &Authority::default(), move |token| {
                ready_tx.send(()).unwrap();
                finish_rx.recv().unwrap();
                token.is_cancelled()
            })
            .unwrap();
        ready_rx.recv().unwrap();
        let ticket = task.id().clone();
        assert_eq!(group.outstanding(), 1);
        assert!(matches!(
            kernel.reconcile_resolved_generation(&second, &BTreeSet::new()),
            Err(KernelError::GenerationInUse { .. })
        ));
        assert!(!task.is_finished(), "blocked worker cannot be ready");
        assert_eq!(group.cancel_root(), vec![ticket.clone()]);
        assert!(group.cancel_root().is_empty());
        assert_eq!(group.state(&ticket), Some(WorkflowTaskState::Cancelling));
        assert_eq!(group.close(), Err(WorkflowTaskError::OutstandingTasks(1)));
        drop(group);
        // The client and the task group are gone, but the native callback
        // still pins the old runtime generation until the worker settles.
        assert!(matches!(
            kernel.reconcile_resolved_generation(&second, &BTreeSet::new()),
            Err(KernelError::GenerationInUse {
                generation,
                active_roots: 1,
            }) if generation == old_generation
        ));
        finish_tx.send(()).unwrap();
        assert!(task.join().unwrap());
        kernel
            .reconcile_resolved_generation(&second, &BTreeSet::new())
            .unwrap();
        assert_eq!(kernel.graph_generation(), Some(second.generation()));
    }

    #[test]
    fn native_scope_cancellation_is_exact_and_late_completion_settles_once() {
        let first = empty_generation_with(None);
        let mut kernel = Kernel::new(first.kernel_config().clone());
        kernel.activate_resolved_generation(&first).unwrap();
        kernel.activate_all().unwrap();
        let group = kernel
            .root_execution_handle(&Authority::default())
            .native_workflow_tasks()
            .unwrap();
        let (a_tx, a_rx) = mpsc::channel::<()>();
        let (b_tx, b_rx) = mpsc::channel::<()>();
        let (ready_tx, ready_rx) = mpsc::channel::<()>();
        let ready_b = ready_tx.clone();
        let a = group
            .spawn("root/fork/a", &Authority::default(), move |token| {
                ready_tx.send(()).unwrap();
                a_rx.recv().unwrap();
                token.is_cancelled()
            })
            .unwrap();
        let b = group
            .spawn("root/fork/ab", &Authority::default(), move |token| {
                ready_b.send(()).unwrap();
                b_rx.recv().unwrap();
                token.is_cancelled()
            })
            .unwrap();
        ready_rx.recv().unwrap();
        ready_rx.recv().unwrap();
        assert_eq!(group.cancel_scope("root/fork/a"), vec![a.id().clone()]);
        assert!(group.cancel_scope("root/fork/a").is_empty());
        assert_eq!(group.state(b.id()), Some(WorkflowTaskState::Pending));
        assert_eq!(group.cancel_root(), vec![b.id().clone()]);
        assert_eq!(group.outstanding(), 2);
        assert!(group.poll_settlement().is_none());
        assert_eq!(group.close(), Err(WorkflowTaskError::OutstandingTasks(2)));
        let a_id = a.id().clone();
        let b_id = b.id().clone();
        a_tx.send(()).unwrap();
        b_tx.send(()).unwrap();
        assert!(a.join().unwrap());
        assert!(b.join().unwrap());
        assert_eq!(group.outstanding(), 0);
        let first_wakeup = group.wait_settlement().unwrap();
        let second_wakeup = group.wait_settlement().unwrap();
        assert_ne!(first_wakeup, second_wakeup);
        assert_eq!(
            BTreeSet::from([first_wakeup, second_wakeup]),
            BTreeSet::from([a_id, b_id]),
        );
        assert!(group.poll_settlement().is_none());
        group.close().unwrap();
        assert!(matches!(
            group.spawn("root/late", &Authority::default(), |_| 1_u64),
            Err(WorkflowTaskError::RootNotAdmitting)
        ));
    }

    #[test]
    fn independent_roots_never_reuse_native_ticket_identity() {
        let selected = empty_generation_with(None);
        let mut kernel = Kernel::new(selected.kernel_config().clone());
        kernel.activate_resolved_generation(&selected).unwrap();
        kernel.activate_all().unwrap();
        let left = kernel
            .root_execution_handle(&Authority::default())
            .native_workflow_tasks()
            .unwrap();
        let right = kernel
            .root_execution_handle(&Authority::default())
            .native_workflow_tasks()
            .unwrap();
        let a = left
            .spawn("root/same", &Authority::default(), |_| 1_u64)
            .unwrap();
        let b = right
            .spawn("root/same", &Authority::default(), |_| 2_u64)
            .unwrap();
        assert_eq!(a.id().generation, b.id().generation);
        assert_eq!(a.id().scope, b.id().scope);
        assert_ne!(a.id(), b.id());
        assert_eq!(a.join().unwrap(), 1);
        assert_eq!(b.join().unwrap(), 2);
        left.close().unwrap();
        right.close().unwrap();
    }

    #[test]
    fn native_worker_receives_only_root_attenuated_authority_and_generation() {
        let read = PermissionId::parse("fixture.permission.read").unwrap();
        let write = PermissionId::parse("fixture.permission.write").unwrap();
        let ceiling = Authority::new([read.clone()]);
        let selected = ResolvedGeneration::resolve(
            Vec::<PluginManifest>::new(),
            Vec::<ComponentManifest>::new(),
            Vec::<ConfigContribution>::new(),
            &ceiling,
        )
        .unwrap();
        let mut kernel = Kernel::new(selected.kernel_config().clone());
        kernel.activate_resolved_generation(&selected).unwrap();
        kernel.activate_all().unwrap();
        let caller = Authority::new([read.clone(), write.clone()]);
        let group = kernel
            .root_execution_handle(&caller)
            .native_workflow_tasks()
            .unwrap();
        let generation = group.generation().clone();
        let task = group
            .spawn("root/scoped", &caller, move |token| {
                (
                    token.graph_generation().clone(),
                    token.authority().permits(&read),
                    token.authority().permits(&write),
                )
            })
            .unwrap();
        assert_eq!(task.join().unwrap(), (generation, true, false));
        group.close().unwrap();
    }

    #[test]
    fn panicking_native_worker_records_failure_and_releases_lease() {
        let first = empty_generation_with(None);
        let mut kernel = Kernel::new(first.kernel_config().clone());
        kernel.activate_resolved_generation(&first).unwrap();
        kernel.activate_all().unwrap();
        let group = kernel
            .root_execution_handle(&Authority::default())
            .native_workflow_tasks()
            .unwrap();
        let task = group
            .spawn("root/panic", &Authority::default(), |_| -> () {
                panic!("deliberately failed native invocation");
            })
            .unwrap();
        let id = task.id().clone();
        assert!(task.join().is_err());
        assert_eq!(group.state(&id), Some(WorkflowTaskState::Failed));
        group.close().unwrap();
    }

    fn id(scope: &str, call: u64) -> WorkflowTaskId {
        WorkflowTaskId {
            generation: "gen-a".into(),
            scope: scope.into(),
            call,
        }
    }

    #[test]
    fn root_cancellation_never_releases_outstanding_native_work() {
        let mut tasks = WorkflowPendingTasks::new("gen-a");
        let a = id("root/fork/alpha", 1);
        let b = id("root/fork/beta", 2);
        tasks.admit(a.clone()).unwrap();
        tasks.admit(b.clone()).unwrap();
        assert_eq!(tasks.cancel_root(), vec![a.clone(), b.clone()]);
        assert_eq!(
            tasks.close_root(),
            Err(WorkflowTaskError::OutstandingTasks(2))
        );
        assert_eq!(
            tasks.admit(id("root/late", 3)),
            Err(WorkflowTaskError::RootNotAdmitting)
        );
        tasks.settle(&a, WorkflowTaskState::Cancelled).unwrap();
        assert_eq!(
            tasks.close_root(),
            Err(WorkflowTaskError::OutstandingTasks(1))
        );
        // A non-preemptible native call can report completion after cancellation.
        tasks.settle(&b, WorkflowTaskState::Completed).unwrap();
        tasks.close_root().unwrap();
        assert_eq!(tasks.outstanding(), 0);
        assert_eq!(
            tasks.settle(&b, WorkflowTaskState::Failed),
            Err(WorkflowTaskError::DuplicateSettlement(b))
        );
    }

    #[test]
    fn sibling_cancel_does_not_affect_other_scope_or_misclassify_prefixes() {
        let mut tasks = WorkflowPendingTasks::new("gen-a");
        let a = id("root/fork/a", 1);
        let b = id("root/fork/ab", 2);
        tasks.admit(a.clone()).unwrap();
        tasks.admit(b.clone()).unwrap();
        assert_eq!(tasks.cancel_scope("root/fork/a"), vec![a.clone()]);
        assert_eq!(tasks.state(&b), Some(WorkflowTaskState::Pending));
        assert_eq!(
            tasks.admit(id("root/fork/a/inner", 3)),
            Err(WorkflowTaskError::RootNotAdmitting)
        );
        tasks.admit(id("root/fork/ab/inner", 4)).unwrap();
    }

    #[test]
    fn cancellation_signals_once_and_late_success_still_retains_lease_until_settlement() {
        let mut tasks = WorkflowPendingTasks::new("gen-a");
        let a = id("root/fork/alpha", 1);
        let b = id("root/fork/beta", 2);
        tasks.admit(a.clone()).unwrap();
        tasks.admit(b.clone()).unwrap();
        assert_eq!(tasks.cancel_scope("root/fork/alpha"), vec![a.clone()]);
        assert!(tasks.cancel_scope("root/fork/alpha").is_empty());
        assert_eq!(tasks.cancel_root(), vec![b.clone()]);
        assert!(tasks.cancel_root().is_empty());
        assert!(tasks.cancel_scope("root").is_empty());
        assert_eq!(tasks.outstanding(), 2);
        tasks.settle(&a, WorkflowTaskState::Completed).unwrap();
        assert_eq!(tasks.outstanding(), 1);
        assert!(matches!(
            tasks.close_root(),
            Err(WorkflowTaskError::OutstandingTasks(1))
        ));
        tasks.settle(&b, WorkflowTaskState::Cancelled).unwrap();
        tasks.close_root().unwrap();
    }

    #[test]
    fn stale_callbacks_from_other_generations_cannot_settle_a_root() {
        let mut tasks = WorkflowPendingTasks::new("gen-a");
        let valid = id("root/child", 1);
        tasks.admit(valid.clone()).unwrap();
        let foreign = WorkflowTaskId {
            generation: "gen-b".into(),
            ..valid.clone()
        };
        assert_eq!(
            tasks.settle(&foreign, WorkflowTaskState::Completed),
            Err(WorkflowTaskError::WrongGeneration(foreign))
        );
        assert_eq!(tasks.outstanding(), 1);
        tasks.settle(&valid, WorkflowTaskState::Completed).unwrap();
        tasks.close_root().unwrap();
    }
}
