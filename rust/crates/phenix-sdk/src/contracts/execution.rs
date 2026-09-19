use super::{DelegatedWorkerResult, DelegationTaskBinding};
use phenix_core::{ComponentInterface, InterfaceId, ServiceId};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub const EXECUTION_SERVICE: &str = "phenix.execution@1";

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
pub struct ExecutionAuthority {
    #[serde(default)]
    pub capabilities: BTreeSet<String>,
}

impl ExecutionAuthority {
    #[must_use]
    pub fn new(capabilities: impl IntoIterator<Item = impl Into<String>>) -> Self {
        Self {
            capabilities: capabilities.into_iter().map(Into::into).collect(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionState {
    Active,
    Completed,
    Failed,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
pub struct ExecutionRecord {
    pub id: String,
    pub parent_execution: Option<String>,
    pub graph_generation: String,
    pub authority: ExecutionAuthority,
    pub state: ExecutionState,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
pub struct CallableRecord {
    pub id: String,
    pub service: String,
    pub required_authority: ExecutionAuthority,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum WorkerTaskState {
    Pending,
    Running {
        execution_id: String,
    },
    Completed {
        execution_id: String,
        result_refs: Vec<String>,
    },
    Failed {
        execution_id: String,
        cause: String,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
pub struct WorkerTaskRecord {
    pub id: String,
    pub parent_execution: String,
    pub graph_generation: String,
    pub description: String,
    #[serde(default)]
    pub depends_on: BTreeSet<String>,
    pub delegated_authority: ExecutionAuthority,
    pub state: WorkerTaskState,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(deny_unknown_fields)]
pub struct DelegatedWorkerTaskRecord {
    pub task: WorkerTaskRecord,
    pub binding: DelegationTaskBinding,
    pub result: Option<DelegatedWorkerResult>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(tag = "operation", rename_all = "snake_case")]
pub enum ExecutionCommand {
    AllocateExecution {
        prefix: String,
        requested_authority: ExecutionAuthority,
    },
    CreateExecution {
        id: String,
        requested_authority: ExecutionAuthority,
    },
    DelegateExecution {
        parent_execution: String,
        id: String,
        requested_authority: ExecutionAuthority,
    },
    GetExecution {
        id: String,
    },
    FinishExecution {
        id: String,
        success: bool,
    },
    RegisterCallable {
        id: String,
        service: String,
        required_authority: ExecutionAuthority,
    },
    InvokeCallable {
        execution_id: String,
        callable_id: String,
        input: Vec<u8>,
    },
    CreateTask {
        id: String,
        parent_execution: String,
        description: String,
        depends_on: BTreeSet<String>,
        requested_authority: ExecutionAuthority,
    },
    CreateDelegatedTask {
        id: String,
        parent_execution: String,
        description: String,
        depends_on: BTreeSet<String>,
        binding: DelegationTaskBinding,
    },
    RunnableTasks,
    StartTask {
        task_id: String,
        execution_id: String,
    },
    CompleteTask {
        task_id: String,
        execution_id: String,
        result_refs: Vec<String>,
    },
    CompleteDelegatedTask {
        task_id: String,
        execution_id: String,
        result: DelegatedWorkerResult,
    },
    FailTask {
        task_id: String,
        execution_id: String,
        cause: String,
    },
    GetTask {
        id: String,
    },
    GetDelegatedTask {
        id: String,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(tag = "response", rename_all = "snake_case")]
pub enum ExecutionResponse {
    Execution {
        execution: ExecutionRecord,
    },
    ExecutionLookup {
        execution: Option<ExecutionRecord>,
    },
    Callable {
        callable: CallableRecord,
    },
    Invocation {
        output: Vec<u8>,
    },
    Task {
        task: WorkerTaskRecord,
    },
    TaskLookup {
        task: Option<WorkerTaskRecord>,
    },
    DelegatedTask {
        task: DelegatedWorkerTaskRecord,
    },
    DelegatedTaskLookup {
        task: Option<DelegatedWorkerTaskRecord>,
    },
    RunnableTasks {
        task_ids: Vec<String>,
    },
}

pub struct ExecutionInterface;

impl ComponentInterface for ExecutionInterface {
    fn interface_id() -> InterfaceId {
        InterfaceId::parse(EXECUTION_SERVICE).expect("static execution interface id is valid")
    }

    fn schema() -> phenix_core::InterfaceSchema {
        phenix_core::InterfaceSchema::of::<ExecutionCommand, ExecutionResponse>()
    }
}

#[must_use]
pub fn execution_service() -> ServiceId {
    ServiceId::parse(EXECUTION_SERVICE).expect("static execution service id is valid")
}
