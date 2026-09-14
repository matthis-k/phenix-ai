use super::{
    BudgetActual, ContextCandidate, RouteSelectionPolicy, StepAttemptRecord, TaskRequirements,
    UsageAttribution, UsagePolicy,
};
use phenix_core::{
    Bytes, CallableId, ComponentInterface, InterfaceId, ModelToolCall, ModelToolDescriptor,
    RoutingProfileId, ServiceId,
};
use serde::{Deserialize, Serialize};

pub const INVOCATION_SERVICE: &str = "phenix.invocation@1";
pub const DEFAULT_INVOCATION_SERVICE: &str = "phenix.invocation.default@1";
pub const INVOCATION_DEFAULTS_SERVICE: &str = "phenix.invocation.defaults@1";
pub const STEP_RUNNER_SERVICE: &str = "phenix.step-runner@1";

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(deny_unknown_fields)]
pub struct InvocationRequest {
    pub attribution: UsageAttribution,
    pub callable_id: Option<CallableId>,
    pub input: Bytes,
    #[serde(default)]
    pub tools: Vec<ModelToolDescriptor>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(deny_unknown_fields)]
pub struct InvocationParams {
    pub profile_id: RoutingProfileId,
    pub policy: UsagePolicy,
    pub task: TaskRequirements,
    #[serde(default)]
    pub context_candidates: Vec<ContextCandidate>,
    pub cache_epoch: u64,
    pub route_policy: RouteSelectionPolicy,
    pub now_ms: u64,
}

impl InvocationRequest {
    #[must_use]
    pub fn into_planned_step(self, params: InvocationParams) -> PlannedStepRequest {
        let Self {
            attribution,
            callable_id,
            input,
            tools,
        } = self;
        let InvocationParams {
            profile_id,
            policy,
            task,
            context_candidates,
            cache_epoch,
            route_policy,
            now_ms,
        } = params;
        PlannedStepRequest {
            attribution,
            profile_id,
            callable_id,
            input,
            tools,
            policy,
            task,
            context_candidates,
            cache_epoch,
            route_policy,
            now_ms,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub enum InvocationCommand {
    Invoke {
        request: InvocationRequest,
        params: InvocationParams,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub enum DefaultInvocationCommand {
    Invoke { request: InvocationRequest },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub enum InvocationDefaultsCommand {
    Resolve { request: InvocationRequest },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(tag = "response", rename_all = "snake_case", deny_unknown_fields)]
pub enum InvocationDefaultsResponse {
    Params { params: InvocationParams },
}

#[derive(
    Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue,
)]
#[serde(rename_all = "snake_case")]
pub enum StepSettlementBasis {
    ReservedMaximum,
}

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

pub type InvocationResponse = StepRunnerResponse;
pub type DefaultInvocationResponse = InvocationResponse;

pub struct InvocationInterface;

impl ComponentInterface for InvocationInterface {
    fn interface_id() -> InterfaceId {
        InterfaceId::parse(INVOCATION_SERVICE).expect("static invocation interface id is valid")
    }

    fn schema() -> phenix_core::InterfaceSchema {
        phenix_core::InterfaceSchema::of::<InvocationCommand, InvocationResponse>()
    }
}

pub struct DefaultInvocationInterface;

impl ComponentInterface for DefaultInvocationInterface {
    fn interface_id() -> InterfaceId {
        InterfaceId::parse(DEFAULT_INVOCATION_SERVICE)
            .expect("static default invocation interface id is valid")
    }

    fn schema() -> phenix_core::InterfaceSchema {
        phenix_core::InterfaceSchema::of::<DefaultInvocationCommand, DefaultInvocationResponse>()
    }
}

pub struct InvocationDefaultsInterface;

impl ComponentInterface for InvocationDefaultsInterface {
    fn interface_id() -> InterfaceId {
        InterfaceId::parse(INVOCATION_DEFAULTS_SERVICE)
            .expect("static invocation defaults interface id is valid")
    }

    fn schema() -> phenix_core::InterfaceSchema {
        phenix_core::InterfaceSchema::of::<InvocationDefaultsCommand, InvocationDefaultsResponse>()
    }
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
pub fn invocation_service() -> ServiceId {
    ServiceId::parse(INVOCATION_SERVICE).expect("static invocation service id is valid")
}

#[must_use]
pub fn default_invocation_service() -> ServiceId {
    ServiceId::parse(DEFAULT_INVOCATION_SERVICE)
        .expect("static default invocation service id is valid")
}

#[must_use]
pub fn invocation_defaults_service() -> ServiceId {
    ServiceId::parse(INVOCATION_DEFAULTS_SERVICE)
        .expect("static invocation defaults service id is valid")
}

#[must_use]
pub fn step_runner_service() -> ServiceId {
    ServiceId::parse(STEP_RUNNER_SERVICE).expect("static step runner service id is valid")
}
