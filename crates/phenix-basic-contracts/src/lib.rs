//! Typed Basic plugin contracts. The kernel depends on none of these types.
use phenix_sdk::*;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Portable, Serialize, Deserialize)]
pub enum ErrorCode {
    Conflict,
    Denied,
    Indeterminate,
    InvalidCursor,
    InvalidInput,
    InvalidReply,
    ModelFailed,
    NotFound,
    StorageFailed,
    ToolFailed,
    TurnLimit,
}
#[derive(Debug, Clone, PartialEq, Eq, Portable, Serialize, Deserialize)]
pub struct BasicError {
    pub code: ErrorCode,
    pub message: String,
}
pub fn error(code: ErrorCode, message: impl Into<String>) -> BasicError {
    BasicError {
        code,
        message: message.into(),
    }
}
pub fn session_resource(session: &str) -> Result<String, BasicError> {
    if !identifier(session) {
        return Err(error(ErrorCode::InvalidInput, "invalid session identity"));
    }
    Ok(format!("phenix.session.{session}"))
}
pub fn check_session(host: &Host<'_>, session: &str) -> CallResult<(), BasicError> {
    let resource = session_resource(session).map_err(CallError::Domain)?;
    if host.resource()?.as_deref() != Some(&resource) {
        return Err(CallError::Domain(error(
            ErrorCode::Denied,
            "session resource is not authorized",
        )));
    }
    Ok(())
}
pub fn encode<T: Portable>(value: &T) -> Result<Blob, Failure> {
    Ok(Blob(value.to_value().encode()?))
}
pub fn decode<T: Portable>(value: &Blob) -> Result<T, Failure> {
    T::from_value(Value::decode(&value.0)?)
}

#[derive(Debug, Clone, PartialEq, Eq, Portable, Serialize, Deserialize)]
pub struct Scope {
    pub session: String,
}
#[derive(Debug, Clone, Default, PartialEq, Eq, Portable, Serialize, Deserialize)]
#[serde(default)]
pub struct Limits {
    pub model_turns: Option<u64>,
    pub tool_calls: Option<u64>,
    pub model_tokens: Option<u64>,
    pub http_timeout_ms: Option<u64>,
}
#[event(id = "phenix.basic.prompt")]
#[derive(Debug, Clone, PartialEq, Eq, Portable, Serialize, Deserialize)]
pub struct PromptSubmitted {
    pub session: String,
    pub text: String,
    pub limits: Limits,
}
#[action(
    id = "phenix.basic.submit",
    event = "phenix.basic.prompt",
    description = "Submit or continue a Basic agent session",
    requires = "phenix.basic.input@1/submit"
)]
pub struct SubmitPrompt;

#[derive(Debug, Clone, PartialEq, Eq, Portable, Serialize, Deserialize)]
pub enum Role {
    User,
    Assistant,
    Tool,
}
#[derive(Debug, Clone, PartialEq, Eq, Portable, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    pub tool: String,
    pub arguments: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Portable, Serialize, Deserialize)]
pub struct Message {
    pub role: Role,
    pub text: String,
    pub calls: Vec<ToolCall>,
    pub call_id: String,
    pub error: Option<String>,
}
#[derive(Debug, Clone, Default, PartialEq, Eq, Portable, Serialize, Deserialize)]
pub struct Usage {
    pub input_tokens: u64,
    pub output_tokens: u64,
}
#[derive(Debug, Clone, PartialEq, Eq, Portable, Serialize, Deserialize)]
pub struct ToolInfo {
    pub id: String,
    pub description: String,
    pub arguments_schema: String,
    pub idempotent: bool,
}
#[derive(Debug, Clone, PartialEq, Eq, Portable, Serialize, Deserialize)]
pub struct Skill {
    pub id: String,
    pub description: String,
    pub instructions: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Portable, Serialize, Deserialize)]
pub struct ContextItem {
    pub id: String,
    pub text: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Portable, Serialize, Deserialize)]
pub struct ModelRequest {
    pub session: String,
    pub run: String,
    pub history: Vec<Message>,
    pub tools: Vec<ToolInfo>,
    pub skills: Vec<Skill>,
    pub context: Vec<ContextItem>,
    pub limits: Limits,
}
#[derive(Debug, Clone, PartialEq, Eq, Portable, Serialize, Deserialize)]
pub struct ModelReply {
    pub text: String,
    pub calls: Vec<ToolCall>,
    pub usage: Usage,
}
#[derive(Debug, Clone, PartialEq, Eq, Portable, Serialize, Deserialize)]
pub struct RunResult {
    pub session: String,
    pub run: String,
    pub answer: String,
    pub model_turns: u64,
    pub tool_calls: u64,
    pub usage: Usage,
}

#[derive(Debug, Clone, PartialEq, Eq, Portable, Serialize, Deserialize)]
pub struct CatalogCursor {
    pub query: String,
    pub revision: u64,
    pub offset: u64,
}
#[derive(Debug, Clone, PartialEq, Eq, Portable, Serialize, Deserialize)]
pub struct ToolQuery {
    pub session: String,
    pub query: String,
    pub cursor: Option<CatalogCursor>,
    pub limit: u64,
}
#[derive(Debug, Clone, PartialEq, Eq, Portable, Serialize, Deserialize)]
pub struct ToolPage {
    pub tools: Vec<ToolInfo>,
    pub revision: u64,
    pub next: Option<CatalogCursor>,
}
#[derive(Debug, Clone, PartialEq, Eq, Portable, Serialize, Deserialize)]
pub struct ToolRequest {
    pub session: String,
    pub run: String,
    pub call: ToolCall,
}
#[derive(Debug, Clone, PartialEq, Eq, Portable, Serialize, Deserialize)]
pub struct ToolResult {
    pub text: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Portable, Serialize, Deserialize)]
pub struct SkillSet {
    pub session: String,
    pub skills: Vec<Skill>,
}
#[derive(Debug, Clone, PartialEq, Eq, Portable, Serialize, Deserialize)]
pub struct ContextSet {
    pub session: String,
    pub items: Vec<ContextItem>,
}

/// Typed payload carried by ToolFinished events in Event::data.
/// Older, payload-free journal entries represent successful completion.
#[derive(Debug, Clone, PartialEq, Eq, Portable, Serialize, Deserialize)]
pub struct ToolCompletion {
    pub failed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Portable, Serialize, Deserialize)]
pub enum EventKind {
    PromptAccepted,
    ModelStarted,
    TextDelta,
    AssistantReply,
    ToolStarted,
    ToolFinished,
    Answer,
    Failure,
    Cancelled,
}
#[derive(Debug, Clone, PartialEq, Eq, Portable, Serialize, Deserialize)]
pub struct Event {
    pub sequence: u64,
    pub session: String,
    pub run: String,
    pub kind: EventKind,
    pub call_id: String,
    pub text: String,
    pub data: Blob,
}
#[derive(Debug, Clone, PartialEq, Eq, Portable, Serialize, Deserialize)]
pub struct JournalAppend {
    pub session: String,
    pub events: Vec<Event>,
}
#[derive(Debug, Clone, PartialEq, Eq, Portable, Serialize, Deserialize)]
pub struct JournalQuery {
    pub session: String,
    pub after: u64,
    pub limit: u64,
}
#[derive(Debug, Clone, PartialEq, Eq, Portable, Serialize, Deserialize)]
pub struct EventPage {
    pub events: Vec<Event>,
    pub next: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Portable, Serialize, Deserialize)]
pub struct ObserverRequest {
    pub session: String,
    pub observer: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Portable, Serialize, Deserialize)]
pub struct ObserverCheckpoint {
    pub session: String,
    pub observer: String,
    pub after: u64,
    pub revision: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Portable, Serialize, Deserialize)]
pub enum RunStatus {
    Idle,
    Running,
    Completed,
    Failed,
    Cancelled,
    Indeterminate,
}
#[derive(Debug, Clone, PartialEq, Eq, Portable, Serialize, Deserialize)]
pub struct SessionSnapshot {
    pub session: String,
    pub history: Vec<Message>,
    pub status: RunStatus,
    pub run: String,
    pub checkpoint: u64,
    pub revision: u64,
    pub pending: Vec<ToolCall>,
    pub usage: Usage,
}
#[derive(Debug, Clone, PartialEq, Eq, Portable, Serialize, Deserialize)]
pub struct ReconcileTool {
    pub session: String,
    pub call_id: String,
    pub text: String,
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Portable, Serialize, Deserialize)]
pub struct RecordKey {
    pub session: String,
    pub key: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Portable, Serialize, Deserialize)]
pub struct StoredRecord {
    pub revision: u64,
    pub data: Blob,
}
#[derive(Debug, Clone, PartialEq, Eq, Portable, Serialize, Deserialize)]
pub struct StorageEdit {
    pub key: String,
    pub expected: u64,
    pub data: Blob,
}
#[derive(Debug, Clone, PartialEq, Eq, Portable, Serialize, Deserialize)]
pub struct StorageTransaction {
    pub session: String,
    pub edits: Vec<StorageEdit>,
}

#[derive(Debug, Clone, PartialEq, Eq, Portable, Serialize, Deserialize)]
pub struct ClientInput {
    pub session: String,
    pub line: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Portable, Serialize, Deserialize)]
pub struct Submission {
    pub action: String,
    pub payload: Blob,
    pub cancel: bool,
}
#[derive(Debug, Clone, PartialEq, Eq, Portable, Serialize, Deserialize)]
pub struct RenderRequest {
    pub session: String,
    pub events: Vec<Event>,
}

#[contract(id = "phenix.basic.model@1")]
pub trait Model {
    fn complete(&self, request: ModelRequest) -> Result<ModelReply, BasicError>;
}
#[contract(id = "phenix.basic.catalog@1")]
pub trait ToolCatalog {
    fn list(&self, request: ToolQuery) -> Result<ToolPage, BasicError>;
}
#[contract(id = "phenix.basic.tool@1")]
pub trait ToolExecute {
    fn execute(&self, request: ToolRequest) -> Result<ToolResult, BasicError>;
    fn count(&self, request: Scope) -> Result<u64, BasicError>;
}
#[contract(id = "phenix.basic.skills@1")]
pub trait Skills {
    fn list(&self, request: Scope) -> Result<Vec<Skill>, BasicError>;
    fn set(&self, request: SkillSet) -> Result<(), BasicError>;
}
#[contract(id = "phenix.basic.context@1")]
pub trait Context {
    fn list(&self, request: Scope) -> Result<Vec<ContextItem>, BasicError>;
    fn set(&self, request: ContextSet) -> Result<(), BasicError>;
}
#[contract(id = "phenix.basic.session@1")]
pub trait Session {
    fn open(&self, request: Scope) -> Result<SessionSnapshot, BasicError>;
    fn checkpoint(&self, request: SessionSnapshot) -> Result<SessionSnapshot, BasicError>;
    fn reconcile(&self, request: ReconcileTool) -> Result<SessionSnapshot, BasicError>;
}
#[contract(id = "phenix.basic.journal@1")]
pub trait EventJournal {
    fn append(&self, request: JournalAppend) -> Result<u64, BasicError>;
    fn read(&self, request: JournalQuery) -> Result<EventPage, BasicError>;
    fn observer(&self, request: ObserverRequest) -> Result<ObserverCheckpoint, BasicError>;
    fn checkpoint_observer(
        &self,
        request: ObserverCheckpoint,
    ) -> Result<ObserverCheckpoint, BasicError>;
}
#[contract(id = "phenix.basic.storage@1")]
pub trait Storage {
    fn read(&self, request: RecordKey) -> Result<StoredRecord, BasicError>;
    fn transact(&self, request: StorageTransaction) -> Result<Vec<StoredRecord>, BasicError>;
}
#[contract(id = "phenix.basic.input@1")]
pub trait BasicInput {
    fn submit(&self, request: PromptSubmitted) -> Result<RunResult, BasicError>;
}
/// Opt-in graph preparation stage. Produces exactly one authoritative
/// ModelRequest and publishes the watched Basic request cell. It never
/// performs Model::complete or a tool effect.
#[contract(id = "phenix.basic.turn-prepare@1")]
pub trait BasicTurnPrepare {
    fn prepare(&self, request: PromptSubmitted) -> Result<ModelRequest, BasicError>;
}

#[property(owner = "phenix.basic.agent", id = "pending_model_request", get, watch)]
pub type BasicPendingModelRequest = Option<ModelRequest>;

#[contract(id = "phenix.basic.output@1")]
pub trait BasicOutput {
    fn read(&self, request: JournalQuery) -> Result<EventPage, BasicError>;
    fn observer(&self, request: ObserverRequest) -> Result<ObserverCheckpoint, BasicError>;
    fn checkpoint_observer(
        &self,
        request: ObserverCheckpoint,
    ) -> Result<ObserverCheckpoint, BasicError>;
}
#[contract(id = "phenix.basic.frontend@1")]
pub trait Frontend {
    fn decode(&self, request: ClientInput) -> Result<Submission, BasicError>;
    fn render(&self, request: RenderRequest) -> Result<Vec<String>, BasicError>;
    fn cancel(&self, request: Scope) -> Result<(), BasicError>;
}

const GRAPH: StaticGraphContribution = StaticGraphContribution {
    plugin: "phenix.basic-contracts",
    events: &[PromptSubmitted::EVENT],
    actions: &[SubmitPrompt::ACTION],
    nodes: &[],
};
#[plugin(
    id = "phenix.basic-contracts",
    version = "1.0.0",
    profile = "definitions",
    define(Model),
    define(ToolCatalog),
    define(ToolExecute),
    define(Skills),
    define(Context),
    define(Session),
    define(EventJournal),
    define(Storage),
    define(BasicInput),
    define(BasicTurnPrepare),
    define(BasicOutput),
    define(Frontend),
    graph(super::GRAPH)
)]
pub mod definitions {
    use super::*;
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn complete_transcript_and_definition_package_use_canonical_types() {
        let request = ModelRequest {
            session: "test".into(),
            run: "r1".into(),
            history: vec![
                Message {
                    role: Role::User,
                    text: "hello".into(),
                    calls: vec![],
                    call_id: String::new(),
                    error: None,
                },
                Message {
                    role: Role::Assistant,
                    text: String::new(),
                    calls: vec![ToolCall {
                        id: "c1".into(),
                        tool: "uppercase".into(),
                        arguments: "{}".into(),
                    }],
                    call_id: String::new(),
                    error: None,
                },
                Message {
                    role: Role::Tool,
                    text: "HELLO".into(),
                    calls: vec![],
                    call_id: "c1".into(),
                    error: None,
                },
            ],
            tools: vec![],
            skills: vec![],
            context: vec![],
            limits: Limits::default(),
        };
        assert_eq!(
            decode::<ModelRequest>(&encode(&request).unwrap()).unwrap(),
            request
        );
        let metadata = ContractMetadata::decode(definitions::METADATA.bytes()).unwrap();
        assert_eq!(metadata.execution, MetadataExecution::None);
        assert_eq!(metadata.definitions.len(), 12);
        assert!(metadata
            .definitions
            .iter()
            .any(|definition| definition.contract == "phenix.basic.turn-prepare@1"));
        let graph = GraphContribution::decode(definitions::GRAPH_METADATA.bytes()).unwrap();
        assert_eq!(graph.actions[0].id, "phenix.basic.submit");
        assert_eq!(
            graph.events[0].payload,
            PromptSubmitted::SCHEMA.owned().unwrap()
        );
        assert!(Blob::from_value(Value::List(vec![])).is_err());
    }
}
