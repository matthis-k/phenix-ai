use phenix_sdk::Portable;

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
#[phenix_sdk::contract(id = "example.agent@1")]
pub trait RunContract { fn run(&self, request: RunRequest) -> Result<RunResponse, AgentError>; }
pub type Run = RunContract::run;
#[phenix_sdk::contract(id = "example.model@1")]
pub trait CompleteContract { fn complete(&self, request: ModelRequest) -> Result<ModelReply, AgentError>; }
pub type Complete = CompleteContract::complete;
#[phenix_sdk::contract(id = "example.tool.execute@1")]
pub trait ExecuteContract { fn execute(&self, request: ToolRequest) -> Result<String, AgentError>; }
pub type Execute = ExecuteContract::execute;
#[phenix_sdk::contract(id = "example.tool.count@1")]
pub trait ToolCountContract { fn count(&self, request: String) -> Result<u64, AgentError>; }
pub type ToolCount = ToolCountContract::count;
