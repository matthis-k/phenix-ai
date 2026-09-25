use super::{
    BudgetActual, BudgetReservationRequest, DelegatedWorkerResult, DelegatedWorkerTaskRecord,
    DelegationResourcePolicy, DelegationTaskBinding, ExecutionAuthority, RemainingBudget,
    RootBudgetLedger, WorkerTaskRecord,
};
use phenix_core::{ComponentInterface, InterfaceId, ServiceId};
use serde::{Deserialize, Serialize};

pub const EXECUTION_RESOURCE_SERVICE: &str = "phenix.execution.resources@1";

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub enum ExecutionResourceCommand {
    RegisterRootBudget {
        ledger: RootBudgetLedger,
    },
    Reserve {
        root_execution_id: String,
        reservation: BudgetReservationRequest,
    },
    SettleReservation {
        root_execution_id: String,
        reservation_id: String,
        actual: BudgetActual,
    },
    ReleaseReservation {
        root_execution_id: String,
        reservation_id: String,
    },
    Remaining {
        root_execution_id: String,
    },
    RemainingWithin {
        root_execution_id: String,
        reservation_id: String,
    },
    RunnableDelegated,
    AdmitDelegated {
        root_execution_id: String,
        reservation: BudgetReservationRequest,
        task: WorkerTaskRecord,
        binding: DelegationTaskBinding,
        parent_authority: ExecutionAuthority,
        policy: DelegationResourcePolicy,
        now_ms: u64,
    },
    StartDelegated {
        task_id: String,
        execution_id: String,
        now_ms: u64,
    },
    CancelDelegatedBeforeStart {
        task_id: String,
        cause: String,
    },
    CompleteDelegated {
        task_id: String,
        execution_id: String,
        result: DelegatedWorkerResult,
        actual: BudgetActual,
    },
    FailDelegated {
        task_id: String,
        execution_id: String,
        cause: String,
        actual: BudgetActual,
    },
    GetDelegated {
        task_id: String,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(tag = "response", rename_all = "snake_case", deny_unknown_fields)]
pub enum ExecutionResourceResponse {
    RootBudget {
        ledger: RootBudgetLedger,
    },
    Remaining {
        budget: RemainingBudget,
    },
    DelegatedRunnableTasks {
        task_ids: Vec<String>,
    },
    DelegatedTask {
        task: DelegatedWorkerTaskRecord,
    },
    DelegatedTaskLookup {
        task: Option<DelegatedWorkerTaskRecord>,
    },
}

pub struct ExecutionResourceInterface;

impl ComponentInterface for ExecutionResourceInterface {
    fn interface_id() -> InterfaceId {
        InterfaceId::parse(EXECUTION_RESOURCE_SERVICE)
            .expect("static execution resource interface id is valid")
    }

    fn schema() -> phenix_core::InterfaceSchema {
        phenix_core::InterfaceSchema::of::<ExecutionResourceCommand, ExecutionResourceResponse>()
    }
}

#[must_use]
pub fn execution_resource_service() -> ServiceId {
    ServiceId::parse(EXECUTION_RESOURCE_SERVICE)
        .expect("static execution resource service id is valid")
}
