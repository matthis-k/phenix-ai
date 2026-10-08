//! Provider-neutral agent execution, lifecycle and tool delegation contracts.
//!
//! These types are shared by first-party and third-party agent implementations.
//! Their interface identity does not imply any particular agent-loop provider.
use super::tool_observation::ToolObservation;
use phenix_core::{
    Bytes, CallableId, ComponentInterface, InterfaceId, ModelToolCall, ModelToolDescriptor,
    ModelToolResult, ServiceId, SessionId,
};
use serde::{Deserialize, Serialize};

pub const AGENT_LOOP_SERVICE: &str = "phenix.agent-loop@1";
pub const AGENT_TOOL_EXECUTION_SERVICE: &str = "phenix.agent-tool-execution@1";
pub const AGENT_LOOP_PROGRESS_SERVICE: &str = "phenix.agent-loop-progress@1";
pub const AGENT_LOOP_CONTROL_SERVICE: &str = "phenix.agent-loop-control@1";

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agent_execution_contract_identity_does_not_name_an_implementation() {
        assert_eq!(AgentLoopInterface::interface_id().as_str(), AGENT_LOOP_SERVICE);
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
