use super::{
    BudgetActual, ContextCandidate, ContextDemand, ContextInvocationPreparation,
    DelegatedWorkerTaskRecord, RouteDecision, RouteSelectionPolicy, StepAttemptRecord,
    TaskRequirements, UsageAttemptKind, UsageAttribution, UsagePolicy,
};
use phenix_core::{
    Bytes, CallableId, ComponentInterface, InterfaceId, ModelToolCall, ModelToolDescriptor,
    ModelToolTurn, RoutingProfileId, ServiceId, SessionId, SkillId,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub const INVOCATION_SERVICE: &str = "phenix.invocation@1";
pub const DEFAULT_INVOCATION_SERVICE: &str = "phenix.invocation.default@1";
pub const HELPER_INVOCATION_SERVICE: &str = "phenix.invocation.helper@1";
pub const INVOCATION_DEFAULTS_SERVICE: &str = "phenix.invocation.defaults@1";
pub const INVOCATION_CLOCK_SERVICE: &str = "phenix.invocation.clock@1";
pub const STEP_RUNNER_SERVICE: &str = "phenix.step-runner@1";
pub const DELEGATED_WORKER_SERVICE: &str = "phenix.delegated-worker@1";

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(deny_unknown_fields)]
pub struct InvocationRequest {
    pub execution_id: String,
    pub session_id: Option<SessionId>,
    pub parent_attempt_id: Option<String>,
    pub callable_id: Option<CallableId>,
    pub input: Bytes,
    #[serde(default)]
    pub tools: Vec<ModelToolDescriptor>,
    #[serde(default)]
    pub continuation: Vec<ModelToolTurn>,
}

#[derive(
    Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue,
)]
#[serde(rename_all = "snake_case")]
pub enum HelperInvocationKind {
    Helper,
    Verification,
    RecoveryClassifier,
}

impl HelperInvocationKind {
    #[must_use]
    pub const fn usage_kind(self) -> UsageAttemptKind {
        match self {
            Self::Helper => UsageAttemptKind::Helper,
            Self::Verification => UsageAttemptKind::Verification,
            Self::RecoveryClassifier => UsageAttemptKind::RecoveryClassifier,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(deny_unknown_fields)]
pub struct HelperInvocationRequest {
    pub execution_id: String,
    pub parent_attempt_id: String,
    pub profile_id: RoutingProfileId,
    pub kind: HelperInvocationKind,
    pub callable_id: CallableId,
    pub input: Bytes,
    #[serde(default)]
    pub tools: Vec<ModelToolDescriptor>,
}

impl HelperInvocationRequest {
    #[must_use]
    pub fn as_invocation_request(&self) -> InvocationRequest {
        InvocationRequest {
            execution_id: self.execution_id.clone(),
            session_id: None,
            parent_attempt_id: Some(self.parent_attempt_id.clone()),
            callable_id: Some(self.callable_id.clone()),
            input: self.input.clone(),
            tools: self.tools.clone(),
            continuation: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(deny_unknown_fields)]
pub struct InvocationIntent {
    pub output_reserve_tokens: u64,
    #[serde(default)]
    pub required_context_capabilities: BTreeSet<String>,
    #[serde(default)]
    pub required_capabilities: BTreeSet<String>,
    #[serde(default)]
    pub required_tools: BTreeSet<CallableId>,
    #[serde(default)]
    pub optional_tools: BTreeSet<CallableId>,
    #[serde(default)]
    pub required_skills: BTreeSet<SkillId>,
    #[serde(default)]
    pub optional_skills: BTreeSet<SkillId>,
    pub requested_reasoning: Option<String>,
    pub deadline_at_ms: Option<u64>,
}

impl InvocationIntent {
    #[must_use]
    pub fn derive_task(&self, preparation: &ContextInvocationPreparation) -> TaskRequirements {
        let mandatory_input_tokens = preparation
            .candidates
            .iter()
            .filter(|candidate| candidate.mandatory)
            .fold(0_u64, |total, candidate| {
                total.saturating_add(candidate.estimated_tokens)
            });
        let reducible_input_tokens = preparation
            .candidates
            .iter()
            .filter(|candidate| !candidate.mandatory)
            .fold(0_u64, |total, candidate| {
                total.saturating_add(candidate.estimated_tokens)
            });
        TaskRequirements {
            request_input_tokens: preparation.request_input_tokens,
            context: ContextDemand {
                mandatory_input_tokens,
                reducible_input_tokens,
                output_reserve_tokens: self.output_reserve_tokens,
                required_capabilities: self.required_context_capabilities.clone(),
            },
            required_capabilities: self.required_capabilities.clone(),
            required_tools: self.required_tools.clone(),
            optional_tools: self.optional_tools.clone(),
            required_skills: self.required_skills.clone(),
            optional_skills: self.optional_skills.clone(),
            requested_reasoning: self.requested_reasoning.clone(),
            deadline_at_ms: self.deadline_at_ms,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(deny_unknown_fields)]
pub struct InvocationParams {
    pub profile_id: RoutingProfileId,
    pub policy: UsagePolicy,
    pub intent: InvocationIntent,
    pub route_policy: RouteSelectionPolicy,
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
pub enum HelperInvocationCommand {
    Invoke { request: HelperInvocationRequest },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub enum InvocationDefaultsCommand {
    Resolve { request: InvocationRequest },
    ResolveHelper { request: HelperInvocationRequest },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(tag = "response", rename_all = "snake_case", deny_unknown_fields)]
pub enum InvocationDefaultsResponse {
    Params { params: InvocationParams },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub enum InvocationClockCommand {
    Now,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(tag = "response", rename_all = "snake_case", deny_unknown_fields)]
pub enum InvocationClockResponse {
    Time { now_ms: u64 },
}

#[derive(
    Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue,
)]
#[serde(rename_all = "snake_case")]
pub enum StepSettlementBasis {
    ReservedMaximum,
    ProviderReportedOutput,
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
    #[serde(default)]
    pub continuation: Vec<ModelToolTurn>,
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
    Run {
        request: PlannedStepRequest,
    },
    RunResolved {
        request: PlannedStepRequest,
        decision: RouteDecision,
    },
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

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub enum DelegatedWorkerCommand {
    RunNext { now_ms: u64 },
    RunTask { task_id: String, now_ms: u64 },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(tag = "response", rename_all = "snake_case", deny_unknown_fields)]
pub enum DelegatedWorkerResponse {
    Idle,
    Processed {
        task: Box<DelegatedWorkerTaskRecord>,
        parent_admitted: bool,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(deny_unknown_fields)]
pub struct HelperInvocationResponse {
    pub output: Bytes,
    #[serde(default)]
    pub tool_calls: Vec<ModelToolCall>,
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

pub struct HelperInvocationInterface;

impl ComponentInterface for HelperInvocationInterface {
    fn interface_id() -> InterfaceId {
        InterfaceId::parse(HELPER_INVOCATION_SERVICE)
            .expect("static helper invocation interface id is valid")
    }

    fn schema() -> phenix_core::InterfaceSchema {
        phenix_core::InterfaceSchema::of::<HelperInvocationCommand, HelperInvocationResponse>()
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

pub struct InvocationClockInterface;

impl ComponentInterface for InvocationClockInterface {
    fn interface_id() -> InterfaceId {
        InterfaceId::parse(INVOCATION_CLOCK_SERVICE)
            .expect("static invocation clock interface id is valid")
    }

    fn schema() -> phenix_core::InterfaceSchema {
        phenix_core::InterfaceSchema::of::<InvocationClockCommand, InvocationClockResponse>()
    }
}

pub struct DelegatedWorkerInterface;

impl ComponentInterface for DelegatedWorkerInterface {
    fn interface_id() -> InterfaceId {
        InterfaceId::parse(DELEGATED_WORKER_SERVICE)
            .expect("static delegated worker interface id is valid")
    }

    fn schema() -> phenix_core::InterfaceSchema {
        phenix_core::InterfaceSchema::of::<DelegatedWorkerCommand, DelegatedWorkerResponse>()
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
pub fn helper_invocation_service() -> ServiceId {
    ServiceId::parse(HELPER_INVOCATION_SERVICE)
        .expect("static helper invocation service id is valid")
}

#[must_use]
pub fn invocation_defaults_service() -> ServiceId {
    ServiceId::parse(INVOCATION_DEFAULTS_SERVICE)
        .expect("static invocation defaults service id is valid")
}

#[must_use]
pub fn invocation_clock_service() -> ServiceId {
    ServiceId::parse(INVOCATION_CLOCK_SERVICE).expect("static invocation clock service id is valid")
}

#[must_use]
pub fn step_runner_service() -> ServiceId {
    ServiceId::parse(STEP_RUNNER_SERVICE).expect("static step runner service id is valid")
}

#[must_use]
pub fn delegated_worker_service() -> ServiceId {
    ServiceId::parse(DELEGATED_WORKER_SERVICE).expect("static delegated worker service id is valid")
}
