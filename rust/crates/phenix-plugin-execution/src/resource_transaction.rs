use crate::delegated_task_state::{DelegatedTaskStore, DelegatedTaskStoreError};
use phenix_sdk::{
    BudgetActual, BudgetLedgerError, BudgetReservationPurpose, BudgetReservationRequest,
    DelegatedWorkerResult, DelegatedWorkerTaskRecord, DelegationResourcePolicy,
    DelegationTaskBinding, ExecutionAuthority, RemainingBudget, RootBudgetLedger, WorkerTaskRecord,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub(crate) struct TaskReservationBinding {
    pub root_execution_id: String,
    pub reservation_id: String,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub(crate) struct ExecutionResourceState {
    ledgers: BTreeMap<String, RootBudgetLedger>,
    delegated: DelegatedTaskStore,
    task_reservations: BTreeMap<String, TaskReservationBinding>,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum ExecutionResourceError {
    DuplicateRootBudget {
        root_execution_id: String,
    },
    UnknownRootBudget {
        root_execution_id: String,
    },
    DuplicateTaskReservation {
        task_id: String,
    },
    UnknownTaskReservation {
        task_id: String,
    },
    ChildLimitExceeded {
        parent_execution: String,
        allowed: u32,
    },
    ReservationPurposeMismatch,
    ReservationPolicyMismatch,
    ReservationBudgetMismatch,
    ReservationAttemptMismatch,
    Budget(BudgetLedgerError),
    Task(DelegatedTaskStoreError),
}

impl ExecutionResourceState {
    pub(crate) fn register_root_budget(
        &mut self,
        ledger: RootBudgetLedger,
    ) -> Result<RootBudgetLedger, ExecutionResourceError> {
        let root_execution_id = ledger.root_execution_id.clone();
        if self.ledgers.contains_key(&root_execution_id) {
            return Err(ExecutionResourceError::DuplicateRootBudget { root_execution_id });
        }
        self.ledgers.insert(root_execution_id, ledger.clone());
        Ok(ledger)
    }

    pub(crate) fn reserve(
        &mut self,
        root_execution_id: &str,
        reservation: BudgetReservationRequest,
    ) -> Result<RootBudgetLedger, ExecutionResourceError> {
        let ledger = self.ledgers.get_mut(root_execution_id).ok_or_else(|| {
            ExecutionResourceError::UnknownRootBudget {
                root_execution_id: root_execution_id.to_owned(),
            }
        })?;
        ledger
            .reserve(reservation)
            .map_err(ExecutionResourceError::Budget)?;
        Ok(ledger.clone())
    }

    pub(crate) fn settle_reservation(
        &mut self,
        root_execution_id: &str,
        reservation_id: &str,
        actual: BudgetActual,
    ) -> Result<RootBudgetLedger, ExecutionResourceError> {
        let ledger = self.ledgers.get_mut(root_execution_id).ok_or_else(|| {
            ExecutionResourceError::UnknownRootBudget {
                root_execution_id: root_execution_id.to_owned(),
            }
        })?;
        ledger
            .settle(reservation_id, actual)
            .map_err(ExecutionResourceError::Budget)?;
        Ok(ledger.clone())
    }

    pub(crate) fn release_reservation(
        &mut self,
        root_execution_id: &str,
        reservation_id: &str,
    ) -> Result<RootBudgetLedger, ExecutionResourceError> {
        let ledger = self.ledgers.get_mut(root_execution_id).ok_or_else(|| {
            ExecutionResourceError::UnknownRootBudget {
                root_execution_id: root_execution_id.to_owned(),
            }
        })?;
        ledger
            .release(reservation_id)
            .map_err(ExecutionResourceError::Budget)?;
        Ok(ledger.clone())
    }

    pub(crate) fn delegated_task(&self, task_id: &str) -> Option<&DelegatedWorkerTaskRecord> {
        self.delegated.get(task_id)
    }

    pub(crate) fn remaining(
        &self,
        root_execution_id: &str,
    ) -> Result<RemainingBudget, ExecutionResourceError> {
        self.ledger(root_execution_id)
            .map(RootBudgetLedger::remaining)
    }

    pub(crate) fn remaining_within(
        &self,
        root_execution_id: &str,
        reservation_id: &str,
    ) -> Result<RemainingBudget, ExecutionResourceError> {
        self.ledger(root_execution_id)?
            .remaining_within(reservation_id)
            .map_err(ExecutionResourceError::Budget)
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn admit_delegated(
        &mut self,
        root_execution_id: &str,
        reservation: BudgetReservationRequest,
        task: WorkerTaskRecord,
        binding: DelegationTaskBinding,
        parent_authority: &ExecutionAuthority,
        policy: &DelegationResourcePolicy,
        now_ms: u64,
    ) -> Result<DelegatedWorkerTaskRecord, ExecutionResourceError> {
        validate_reservation_binding(&reservation, &binding)?;
        if self.task_reservations.contains_key(&task.id) {
            return Err(ExecutionResourceError::DuplicateTaskReservation { task_id: task.id });
        }
        if self.delegated.child_count(&task.parent_execution) >= policy.max_children as usize {
            return Err(ExecutionResourceError::ChildLimitExceeded {
                parent_execution: task.parent_execution,
                allowed: policy.max_children,
            });
        }
        let task_id = task.id.clone();
        let reservation_id = reservation.reservation_id.clone();
        let mut ledger = self.ledger(root_execution_id)?.clone();
        let mut delegated = self.delegated.clone();
        ledger
            .reserve(reservation)
            .map_err(ExecutionResourceError::Budget)?;
        let record = delegated
            .create(task, binding, parent_authority, policy, now_ms)
            .map_err(ExecutionResourceError::Task)?
            .clone();
        self.ledgers.insert(root_execution_id.to_owned(), ledger);
        self.delegated = delegated;
        self.task_reservations.insert(
            task_id,
            TaskReservationBinding {
                root_execution_id: root_execution_id.to_owned(),
                reservation_id,
            },
        );
        Ok(record)
    }

    pub(crate) fn start_delegated(
        &mut self,
        task_id: &str,
        execution_id: String,
        now_ms: u64,
    ) -> Result<DelegatedWorkerTaskRecord, ExecutionResourceError> {
        self.delegated
            .start(task_id, execution_id, now_ms)
            .cloned()
            .map_err(ExecutionResourceError::Task)
    }

    pub(crate) fn complete_delegated(
        &mut self,
        task_id: &str,
        execution_id: &str,
        result: DelegatedWorkerResult,
        actual: BudgetActual,
    ) -> Result<DelegatedWorkerTaskRecord, ExecutionResourceError> {
        let reservation = self.task_reservation(task_id)?.clone();
        let mut ledger = self.ledger(&reservation.root_execution_id)?.clone();
        let mut delegated = self.delegated.clone();
        let record = delegated
            .complete(task_id, execution_id, result)
            .map_err(ExecutionResourceError::Task)?
            .clone();
        ledger
            .settle(&reservation.reservation_id, actual)
            .map_err(ExecutionResourceError::Budget)?;
        self.ledgers
            .insert(reservation.root_execution_id.clone(), ledger);
        self.delegated = delegated;
        Ok(record)
    }

    pub(crate) fn fail_delegated(
        &mut self,
        task_id: &str,
        execution_id: &str,
        cause: String,
        actual: BudgetActual,
    ) -> Result<DelegatedWorkerTaskRecord, ExecutionResourceError> {
        let reservation = self.task_reservation(task_id)?.clone();
        let mut ledger = self.ledger(&reservation.root_execution_id)?.clone();
        let mut delegated = self.delegated.clone();
        let record = delegated
            .fail(task_id, execution_id, cause)
            .map_err(ExecutionResourceError::Task)?
            .clone();
        ledger
            .settle(&reservation.reservation_id, actual)
            .map_err(ExecutionResourceError::Budget)?;
        self.ledgers
            .insert(reservation.root_execution_id.clone(), ledger);
        self.delegated = delegated;
        Ok(record)
    }

    fn ledger(&self, root_execution_id: &str) -> Result<&RootBudgetLedger, ExecutionResourceError> {
        self.ledgers.get(root_execution_id).ok_or_else(|| {
            ExecutionResourceError::UnknownRootBudget {
                root_execution_id: root_execution_id.to_owned(),
            }
        })
    }

    fn task_reservation(
        &self,
        task_id: &str,
    ) -> Result<&TaskReservationBinding, ExecutionResourceError> {
        self.task_reservations.get(task_id).ok_or_else(|| {
            ExecutionResourceError::UnknownTaskReservation {
                task_id: task_id.to_owned(),
            }
        })
    }
}

fn validate_reservation_binding(
    reservation: &BudgetReservationRequest,
    binding: &DelegationTaskBinding,
) -> Result<(), ExecutionResourceError> {
    if reservation.purpose != BudgetReservationPurpose::Delegation {
        return Err(ExecutionResourceError::ReservationPurposeMismatch);
    }
    if reservation.policy_revision != binding.parent_policy_revision {
        return Err(ExecutionResourceError::ReservationPolicyMismatch);
    }
    if reservation.budget != binding.resources.budget {
        return Err(ExecutionResourceError::ReservationBudgetMismatch);
    }
    if reservation.attempts != binding.resources.attempts {
        return Err(ExecutionResourceError::ReservationAttemptMismatch);
    }
    Ok(())
}
