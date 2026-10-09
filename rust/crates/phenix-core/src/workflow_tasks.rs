//! Host-owned native/pending invocation accounting for structured plan scopes.
//!
//! This is the lifecycle substrate for the pending-capable invocation ABI.
//! It does not fabricate a pending Core service call; dispatch integration
//! must retain the root's existing generation lease while these are live.

use std::collections::{BTreeMap, BTreeSet};

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
                && !state.terminal()
            {
                *state = WorkflowTaskState::Cancelling;
                signalled.push(id.clone());
            }
        }
        signalled
    }

    pub fn cancel_root(&mut self) -> Vec<WorkflowTaskId> {
        self.root_cancelled = true;
        let mut signalled = Vec::new();
        for (id, state) in &mut self.active {
            if !state.terminal() {
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

#[cfg(test)]
mod tests {
    use super::*;

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
