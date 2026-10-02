//! Metadata-only diagnostics for agent execution observability.
//!
//! These events carry correlation and lifecycle fields only. Prompt content, tool arguments,
//! tool results, and file contents remain outside this contract and follow explicit content-capture
//! policy instead.

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


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_failure_diagnostic_is_metadata_only() {
        let diagnostic = AgentDiagnosticEvent::ToolInvocationFailed {
            execution_id: "execution-7".into(),
            session_id: None,
            call_id: "call-3".into(),
            callable_id: CallableId::parse("workspace.shell").unwrap(),
            reason: "permission denied".into(),
        };
        let value = serde_json::to_value(diagnostic).unwrap();
        let object = value.as_object().unwrap();

        assert_eq!(object["event"], "tool_invocation_failed");
        assert_eq!(object["execution_id"], "execution-7");
        assert_eq!(object["call_id"], "call-3");
        assert_eq!(object["callable_id"], "workspace.shell");
        assert_eq!(object["reason"], "permission denied");
        assert!(!object.contains_key("input"));
        assert!(!object.contains_key("output"));
        assert!(!object.contains_key("arguments"));
        assert!(!object.contains_key("result"));
    }
}
