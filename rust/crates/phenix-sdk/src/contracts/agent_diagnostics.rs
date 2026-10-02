use phenix_core::{CallableId, EventTypeId, SessionId};
use serde::{Deserialize, Serialize};

pub const AGENT_DIAGNOSTIC_EVENT: &str = "phenix.agent.diagnostic";
pub const AGENT_DIAGNOSTIC_EVENT_VERSION: u32 = 1;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum AgentDiagnosticEvent {
    RunStarted {
        execution_id: String,
        session_id: Option<SessionId>,
        callable_id: Option<CallableId>,
    },
    ModelTurnStarted {
        execution_id: String,
        session_id: Option<SessionId>,
        turn: u32,
    },
    ModelTurnCompleted {
        execution_id: String,
        session_id: Option<SessionId>,
        turn: u32,
        tool_calls: u32,
    },
    ModelTurnFailed {
        execution_id: String,
        session_id: Option<SessionId>,
        turn: u32,
        reason: String,
    },
    ToolInvocationStarted {
        execution_id: String,
        session_id: Option<SessionId>,
        call_id: String,
        callable_id: CallableId,
    },
    ToolInvocationCompleted {
        execution_id: String,
        session_id: Option<SessionId>,
        call_id: String,
        callable_id: CallableId,
    },
    ToolInvocationFailed {
        execution_id: String,
        session_id: Option<SessionId>,
        call_id: String,
        callable_id: CallableId,
        reason: String,
    },
    RunCompleted {
        execution_id: String,
        session_id: Option<SessionId>,
        model_calls: u32,
        tool_calls: u32,
    },
    RunCancelled {
        execution_id: String,
        session_id: Option<SessionId>,
        model_calls: u32,
        tool_calls: u32,
    },
    RunFailed {
        execution_id: String,
        session_id: Option<SessionId>,
        reason: String,
        model_calls: u32,
        tool_calls: u32,
    },
}

#[must_use]
pub fn agent_diagnostic_event_type() -> EventTypeId {
    EventTypeId::parse(AGENT_DIAGNOSTIC_EVENT).expect("static agent diagnostic event type is valid")
}
