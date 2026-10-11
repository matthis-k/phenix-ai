use phenix_sdk::{operation, Portable};

#[derive(Debug, Clone, PartialEq, Eq, Portable)]
pub enum AgentError {
    InvalidReply,
    InvalidTranscript,
    ModelFailed,
    None,
    ToolFailed,
    TurnLimit,
    UnknownTool,
}
#[derive(Debug, Clone, PartialEq, Eq, Portable)]
pub enum FeedbackStatus {
    Empty,
    Error,
    Success,
}
#[derive(Debug, Clone, PartialEq, Eq, Portable)]
pub struct Feedback {
    pub status: FeedbackStatus,
    pub call_id: String,
    pub text: String,
    pub error: AgentError,
}
impl Default for Feedback {
    fn default() -> Self {
        Self {
            status: FeedbackStatus::Empty,
            call_id: String::new(),
            text: String::new(),
            error: AgentError::None,
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Portable)]
pub struct ModelRequest {
    pub prompt: String,
    pub turn: u64,
    pub available_tool: String,
    pub feedback: Feedback,
}
#[derive(Debug, Clone, PartialEq, Eq, Portable)]
pub enum ReplyKind {
    Final,
    Tool,
}
#[derive(Debug, Clone, PartialEq, Eq, Portable)]
pub struct ModelReply {
    pub kind: ReplyKind,
    pub call_id: String,
    pub tool: String,
    pub text: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Portable)]
pub struct ToolRequest {
    pub call_id: String,
    pub text: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Portable)]
pub struct RunRequest {
    pub prompt: String,
    pub max_turns: u64,
}
#[derive(Debug, Clone, PartialEq, Eq, Portable)]
pub struct RunResponse {
    pub answer: String,
    pub model_turns: u64,
    pub tool_calls: u64,
}
#[operation(interface = "example.agent@1", operation = "run")]
pub type Run = (RunRequest, RunResponse, AgentError);
#[operation(interface = "example.model@1", operation = "complete")]
pub type Complete = (ModelRequest, ModelReply, AgentError);
#[operation(interface = "example.tool@1", operation = "execute")]
pub type Execute = (ToolRequest, String, AgentError);
#[operation(interface = "example.tool@1", operation = "count")]
pub type ToolCount = (String, u64, AgentError);
