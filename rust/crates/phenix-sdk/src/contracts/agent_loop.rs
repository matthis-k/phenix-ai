//! Provider-neutral agent execution, lifecycle and tool delegation contracts.
//!
//! These types are shared by first-party and third-party agent implementations.
//! Their interface identity does not imply any particular agent-loop provider.
use super::tool_observation::ToolObservation;
use phenix_core::{
    Bytes, CallableId, ComponentInterface, InterfaceId, ModelToolCall, ModelToolDescriptor,
    ModelToolResult, ModelToolTurn, ServiceId, SessionId,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    num::{NonZeroU32, NonZeroU64},
};

pub const AGENT_LOOP_SERVICE: &str = "phenix.agent-loop@1";
pub const AGENT_TOOL_EXECUTION_SERVICE: &str = "phenix.agent-tool-execution@1";
pub const AGENT_LOOP_PROGRESS_SERVICE: &str = "phenix.agent-loop-progress@1";
pub const AGENT_LOOP_CONTROL_SERVICE: &str = "phenix.agent-loop-control@1";
pub const AGENT_TURN_STEP_SERVICE: &str = "phenix.agent-turn-step@1";
pub const AGENT_TOOL_BATCH_SERVICE: &str = "phenix.agent-tool-batch@1";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AgentLoopPolicy {
    max_model_turns: Option<NonZeroU32>,
    max_tool_calls_per_turn: Option<NonZeroU32>,
    max_tool_observation_model_bytes: Option<NonZeroU64>,
    result_reduction: bool,
}

impl AgentLoopPolicy {
    #[must_use]
    pub const fn new(max_model_turns: NonZeroU32, max_tool_calls_per_turn: NonZeroU32) -> Self {
        Self {
            max_model_turns: Some(max_model_turns),
            max_tool_calls_per_turn: Some(max_tool_calls_per_turn),
            max_tool_observation_model_bytes: None,
            result_reduction: true,
        }
    }

    #[must_use]
    pub const fn max_model_turns(self) -> Option<NonZeroU32> {
        self.max_model_turns
    }

    #[must_use]
    pub const fn max_tool_calls_per_turn(self) -> Option<NonZeroU32> {
        self.max_tool_calls_per_turn
    }

    #[must_use]
    pub const fn max_tool_observation_model_bytes(self) -> Option<NonZeroU64> {
        self.max_tool_observation_model_bytes
    }

    #[must_use]
    pub const fn with_tool_observation_model_bytes(mut self, limit: NonZeroU64) -> Self {
        self.max_tool_observation_model_bytes = Some(limit);
        self
    }

    #[must_use]
    pub const fn with_result_reduction(mut self, enabled: bool) -> Self {
        self.result_reduction = enabled;
        self
    }

    #[must_use]
    pub const fn result_reduction(self) -> bool {
        self.result_reduction
    }
}

impl Default for AgentLoopPolicy {
    fn default() -> Self {
        Self {
            max_model_turns: None,
            max_tool_calls_per_turn: None,
            max_tool_observation_model_bytes: None,
            result_reduction: true,
        }
    }
}

pub struct AgentLoopInterface;

impl ComponentInterface for AgentLoopInterface {
    fn interface_id() -> InterfaceId {
        InterfaceId::parse(AGENT_LOOP_SERVICE).expect("static agent loop interface id is valid")
    }

    fn schema() -> phenix_core::InterfaceSchema {
        phenix_core::InterfaceSchema::of::<AgentLoopCommand, AgentLoopResponse>()
    }
}

pub struct AgentLoopControlInterface;

impl ComponentInterface for AgentLoopControlInterface {
    fn interface_id() -> InterfaceId {
        InterfaceId::parse(AGENT_LOOP_CONTROL_SERVICE)
            .expect("static agent loop control interface id is valid")
    }

    fn schema() -> phenix_core::InterfaceSchema {
        phenix_core::InterfaceSchema::of::<AgentLoopControlRequest, AgentLoopControlResponse>()
    }
}

pub struct AgentToolExecutionInterface;

impl ComponentInterface for AgentToolExecutionInterface {
    fn interface_id() -> InterfaceId {
        InterfaceId::parse(AGENT_TOOL_EXECUTION_SERVICE)
            .expect("static agent tool execution interface id is valid")
    }

    fn schema() -> phenix_core::InterfaceSchema {
        phenix_core::InterfaceSchema::of::<AgentToolExecutionRequest, AgentToolExecutionResponse>()
    }
}

pub struct AgentLoopProgressInterface;

impl ComponentInterface for AgentLoopProgressInterface {
    fn interface_id() -> InterfaceId {
        InterfaceId::parse(AGENT_LOOP_PROGRESS_SERVICE)
            .expect("static agent loop progress interface id is valid")
    }

    fn schema() -> phenix_core::InterfaceSchema {
        phenix_core::InterfaceSchema::of::<AgentLoopProgressRecord, AgentLoopProgressResponse>()
    }
}

/// A single model turn. The provider owns admission, invocation and validation.
pub struct AgentTurnStepInterface;

impl ComponentInterface for AgentTurnStepInterface {
    fn interface_id() -> InterfaceId {
        InterfaceId::parse(AGENT_TURN_STEP_SERVICE).expect("static turn interface")
    }

    fn schema() -> phenix_core::InterfaceSchema {
        phenix_core::InterfaceSchema::of::<AgentTurnStepRequest, AgentTurnStepResponse>()
    }
}

/// Runs a batch of model tool calls and prepares the next continuation turn.
pub struct AgentToolBatchInterface;

impl ComponentInterface for AgentToolBatchInterface {
    fn interface_id() -> InterfaceId {
        InterfaceId::parse(AGENT_TOOL_BATCH_SERVICE).expect("static batch interface")
    }

    fn schema() -> phenix_core::InterfaceSchema {
        phenix_core::InterfaceSchema::of::<AgentToolBatchRequest, AgentToolBatchResponse>()
    }
}

/// State passed between providers. The generic kernel does not own agent state.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
pub struct AgentTurnState {
    pub execution_id: String,
    pub session_id: Option<SessionId>,
    pub parent_attempt_id: Option<String>,
    pub callable_id: Option<CallableId>,
    pub input: Bytes,
    pub tools: Vec<ModelToolDescriptor>,
    #[serde(default)]
    pub continuation: Vec<ModelToolTurn>,
    #[serde(default)]
    pub seen_tool_call_ids: Vec<String>,
    #[serde(default)]
    pub observations: BTreeMap<CallableId, ToolObservation>,
    pub usage: AgentLoopUsage,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
pub struct AgentTurnStepRequest {
    pub state: AgentTurnState,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(tag = "outcome", rename_all = "snake_case", deny_unknown_fields)]
pub enum AgentTurnStepResponse {
    ToolCalls {
        state: AgentTurnState,
        assistant_output: Bytes,
        tool_calls: Vec<ModelToolCall>,
    },
    Final {
        state: AgentTurnState,
        output: Bytes,
    },
    Cancelled {
        state: AgentTurnState,
    },
    Failed {
        state: AgentTurnState,
        failure: AgentLoopFailure,
    },
}

impl AgentTurnStepResponse {
    /// The branch declared by the topology for this typed result.
    #[must_use]
    pub const fn workflow_outcome(&self) -> &'static str {
        match self {
            Self::ToolCalls { .. } => "tool_calls",
            Self::Final { .. } => "final",
            Self::Cancelled { .. } => "cancelled",
            Self::Failed { .. } => "failed",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
pub struct AgentToolBatchRequest {
    pub state: AgentTurnState,
    pub assistant_output: Bytes,
    pub tool_calls: Vec<ModelToolCall>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(tag = "outcome", rename_all = "snake_case", deny_unknown_fields)]
pub enum AgentToolBatchResponse {
    Continue { state: AgentTurnState },
    Cancelled { state: AgentTurnState },
}

impl AgentToolBatchResponse {
    /// The branch declared by the topology for this typed result.
    #[must_use]
    pub const fn workflow_outcome(&self) -> &'static str {
        match self {
            Self::Continue { .. } => "continue",
            Self::Cancelled { .. } => "cancelled",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub enum AgentLoopCommand {
    Run {
        execution_id: String,
        session_id: Option<SessionId>,
        parent_attempt_id: Option<String>,
        callable_id: Option<CallableId>,
        input: Bytes,
        #[serde(default)]
        tools: Vec<ModelToolDescriptor>,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
pub struct AgentLoopUsage {
    pub model_calls: u32,
    pub tool_calls: u32,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(tag = "failure", rename_all = "snake_case", deny_unknown_fields)]
pub enum AgentLoopFailure {
    ModelTurnLimitExceeded { limit: u32 },
    ToolCallLimitExceeded { limit: u32, actual: u32 },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(tag = "response", rename_all = "snake_case", deny_unknown_fields)]
pub enum AgentLoopResponse {
    Completed {
        output: Bytes,
        usage: AgentLoopUsage,
    },
    Cancelled {
        usage: AgentLoopUsage,
    },
    Failed {
        failure: AgentLoopFailure,
        usage: AgentLoopUsage,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
pub struct AgentLoopControlRequest {
    pub execution_id: String,
    pub session_id: Option<SessionId>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(tag = "response", rename_all = "snake_case", deny_unknown_fields)]
pub enum AgentLoopControlResponse {
    Continue,
    Cancelled,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
pub struct AgentToolExecutionRequest {
    pub execution_id: String,
    pub session_id: Option<SessionId>,
    pub call: ModelToolCall,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(tag = "response", rename_all = "snake_case", deny_unknown_fields)]
pub enum AgentToolExecutionResponse {
    Completed {
        result: ModelToolResult,
        #[serde(default)]
        activated_tools: Vec<ModelToolDescriptor>,
        #[serde(default)]
        observation: Option<Box<ToolObservation>>,
    },
    Cancelled,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(tag = "progress", rename_all = "snake_case", deny_unknown_fields)]
pub enum AgentLoopProgress {
    ToolCall { call: ModelToolCall },
    ToolResult { result: ModelToolResult },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
pub struct AgentLoopProgressRecord {
    pub execution_id: String,
    pub session_id: Option<SessionId>,
    pub progress: AgentLoopProgress,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(tag = "response", rename_all = "snake_case", deny_unknown_fields)]
pub enum AgentLoopProgressResponse {
    Recorded,
}

#[must_use]
pub fn agent_turn_step_service() -> ServiceId {
    ServiceId::parse(AGENT_TURN_STEP_SERVICE).expect("static turn service")
}

#[must_use]
pub fn agent_tool_batch_service() -> ServiceId {
    ServiceId::parse(AGENT_TOOL_BATCH_SERVICE).expect("static batch service")
}

#[must_use]
pub fn agent_loop_service() -> ServiceId {
    ServiceId::parse(AGENT_LOOP_SERVICE).expect("static agent loop service id is valid")
}

#[must_use]
pub fn agent_tool_execution_service() -> ServiceId {
    ServiceId::parse(AGENT_TOOL_EXECUTION_SERVICE)
        .expect("static agent tool execution service id is valid")
}

#[must_use]
pub fn agent_loop_progress_service() -> ServiceId {
    ServiceId::parse(AGENT_LOOP_PROGRESS_SERVICE)
        .expect("static agent loop progress service id is valid")
}

#[must_use]
pub fn agent_loop_control_service() -> ServiceId {
    ServiceId::parse(AGENT_LOOP_CONTROL_SERVICE)
        .expect("static agent loop control service id is valid")
}

pub fn validate_model_tool_calls(
    calls: &[ModelToolCall],
    seen: &mut std::collections::BTreeSet<String>,
) -> Result<(), String> {
    let mut current = std::collections::BTreeSet::new();
    for call in calls {
        if call.call_id.trim().is_empty() {
            return Err("model returned a tool call with an empty call id".to_owned());
        }
        if seen.contains(&call.call_id) || !current.insert(call.call_id.clone()) {
            return Err(format!(
                "model returned duplicate tool call id {}",
                call.call_id
            ));
        }
    }
    seen.extend(current);
    Ok(())
}

pub fn validate_initial_tools(tools: &[ModelToolDescriptor]) -> Result<(), String> {
    let mut ids = std::collections::BTreeSet::new();
    for tool in tools {
        if !ids.insert(tool.id.clone()) {
            return Err(format!(
                "agent loop received duplicate tool descriptor {}",
                tool.id
            ));
        }
    }
    Ok(())
}

pub fn activate_tools(
    active: &mut Vec<ModelToolDescriptor>,
    activated: Vec<ModelToolDescriptor>,
) -> Result<(), String> {
    let mut additions = Vec::new();
    for tool in activated {
        if let Some(existing) = active.iter().find(|existing| existing.id == tool.id) {
            if existing != &tool {
                return Err(format!(
                    "tool executor attempted to change active descriptor {}",
                    tool.id
                ));
            }
            continue;
        }
        if let Some(existing) = additions
            .iter()
            .find(|existing: &&ModelToolDescriptor| existing.id == tool.id)
        {
            if *existing != tool {
                return Err(format!(
                    "tool executor returned conflicting activated descriptors {}",
                    tool.id
                ));
            }
            continue;
        }
        additions.push(tool);
    }
    active.extend(additions);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agent_execution_contract_identity_does_not_name_an_implementation() {
        assert_eq!(
            AgentLoopInterface::interface_id().as_str(),
            AGENT_LOOP_SERVICE
        );
        assert_eq!(
            AgentLoopControlInterface::interface_id().as_str(),
            AGENT_LOOP_CONTROL_SERVICE
        );
        assert_eq!(
            AgentToolExecutionInterface::interface_id().as_str(),
            AGENT_TOOL_EXECUTION_SERVICE
        );
        assert_eq!(
            AgentLoopProgressInterface::interface_id().as_str(),
            AGENT_LOOP_PROGRESS_SERVICE
        );
    }

    #[test]
    fn node_requests_and_outcomes_roundtrip_without_implementation_code() {
        let state = AgentTurnState {
            execution_id: "run-1".into(),
            session_id: None,
            parent_attempt_id: None,
            callable_id: None,
            input: Bytes::from(b"prompt".to_vec()),
            tools: Vec::new(),
            continuation: Vec::new(),
            seen_tool_call_ids: Vec::new(),
            observations: BTreeMap::new(),
            usage: AgentLoopUsage {
                model_calls: 0,
                tool_calls: 0,
            },
        };
        let request = AgentTurnStepRequest {
            state: state.clone(),
        };
        let json = serde_json::to_value(&request).unwrap();
        let decoded: AgentTurnStepRequest = serde_json::from_value(json).unwrap();
        assert_eq!(request, decoded);

        let turn_response = AgentTurnStepResponse::ToolCalls {
            state: state.clone(),
            assistant_output: Bytes::from(b"tool".to_vec()),
            tool_calls: Vec::new(),
        };
        assert_eq!(turn_response.workflow_outcome(), "tool_calls");
        let json = serde_json::to_value(&turn_response).unwrap();
        assert_eq!(json["outcome"], "tool_calls");
        let decoded: AgentTurnStepResponse = serde_json::from_value(json).unwrap();
        assert_eq!(decoded, turn_response);

        let batch_request = AgentToolBatchRequest {
            state: state.clone(),
            assistant_output: Bytes::from(b"tool".to_vec()),
            tool_calls: Vec::new(),
        };
        let json = serde_json::to_value(&batch_request).unwrap();
        let decoded: AgentToolBatchRequest = serde_json::from_value(json).unwrap();
        assert_eq!(decoded, batch_request);

        let batch_response = AgentToolBatchResponse::Continue { state };
        assert_eq!(batch_response.workflow_outcome(), "continue");
        let json = serde_json::to_value(&batch_response).unwrap();
        assert_eq!(json["outcome"], "continue");
        let decoded: AgentToolBatchResponse = serde_json::from_value(json).unwrap();
        assert_eq!(decoded, batch_response);
    }

    #[test]
    fn agent_execution_wire_shape_is_unchanged() {
        let request = AgentLoopCommand::Run {
            execution_id: "execution-1".into(),
            session_id: None,
            parent_attempt_id: None,
            callable_id: None,
            input: Bytes::from("hello".as_bytes().to_vec()),
            tools: vec![],
        };
        let json = serde_json::to_value(&request).expect("serialize agent request");
        assert_eq!(json["operation"], "run");
        assert_eq!(json["execution_id"], "execution-1");
        assert_eq!(json["tools"], serde_json::json!([]));
        let decoded: AgentLoopCommand =
            serde_json::from_value(json).expect("deserialize agent request");
        assert_eq!(decoded, request);
    }
}
