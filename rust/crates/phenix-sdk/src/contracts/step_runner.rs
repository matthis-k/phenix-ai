use super::{
    BudgetActual, ContextCandidate, RouteSelectionPolicy, StepAttemptRecord, TaskRequirements,
    UsageAttribution, UsagePolicy,
};
use phenix_core::{
    Bytes, CallableId, ComponentInterface, InterfaceId, ModelToolCall, ModelToolDescriptor,
    RoutingProfileId, ServiceId,
};
use serde::{Deserialize, Serialize};

pub const STEP_RUNNER_SERVICE: &str = "phenix.step-runner@1";

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(deny_unknown_fields)]
pub struct PlannedStepRequest {
    pub attribution: UsageAttribution,
    pub profile_id: RoutingProfileId,
    pub callable_id: Option<CallableId>,
    pub input: Bytes,
    #[serde(default)]
    pub tools: Vec<ModelToolDescriptor>,
    pub policy: UsagePolicy,
    pub task: TaskRequirements,
    #[serde(default)]
    pub context_candidates: Vec<ContextCandidate>,
    pub cache_epoch: u64,
    pub route_policy: RouteSelectionPolicy,
    pub now_ms: u64,
}

#[derive(
    Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue,
)]
#[serde(rename_all = "snake_case")]
pub enum StepSettlementBasis {
    ReservedMaximum,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub enum StepRunnerCommand {
    Run { request: PlannedStepRequest },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(tag = "response", rename_all = "snake_case", deny_unknown_fields)]
pub enum StepRunnerResponse {
    Completed {
        attempt: StepAttemptRecord,
        output: Bytes,
        tool_calls: Vec<ModelToolCall>,
        settled: BudgetActual,
        settlement_basis: StepSettlementBasis,
    },
}

pub struct StepRunnerInterface;

impl ComponentInterface for StepRunnerInterface {
    fn interface_id() -> InterfaceId {
        InterfaceId::parse(STEP_RUNNER_SERVICE).expect("static step runner interface id is valid")
    }

    fn schema() -> phenix_core::InterfaceSchema {
        phenix_core::InterfaceSchema::of::<StepRunnerCommand, StepRunnerResponse>()
    }
}

#[must_use]
pub fn step_runner_service() -> ServiceId {
    ServiceId::parse(STEP_RUNNER_SERVICE).expect("static step runner service id is valid")
}
