pub use phenix_core::SessionId;
use phenix_core::{
    Bytes, CallableId, ComponentInterface, ContractId, InterfaceId, PhenixValue,
    PreparedMutationHandle, ServiceId, Type, ValueCodec, ValueError,
};
use serde::{Deserialize, Serialize};

pub const SESSION_SERVICE: &str = "phenix.sessions@1";
pub const SESSION_MUTATION_SERVICE: &str = "phenix.sessions.mutation@1";

#[must_use]
pub fn session_input_resource(id: &SessionId, sequence: u64) -> String {
    format!("input/{id}/{sequence}")
}

#[must_use]
pub fn session_history_resource(id: &SessionId, sequence: u64) -> String {
    format!("history/{id}/{sequence}")
}

#[derive(
    Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue,
)]
#[serde(rename_all = "snake_case")]
pub enum SessionInputKind {
    User,
    Root,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
pub struct SessionInput {
    pub sequence: u64,
    pub kind: SessionInputKind,
    pub content: Bytes,
}

#[derive(
    Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue,
)]
#[serde(rename_all = "snake_case")]
pub enum SessionHistoryRole {
    User,
    Assistant,
    Tool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum SessionHistoryContentPart {
    Text {
        text: String,
    },
    MediaReference {
        media_type: String,
        resource: String,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SessionHistoryValue(pub PhenixValue);

impl From<PhenixValue> for SessionHistoryValue {
    fn from(value: PhenixValue) -> Self {
        Self(value)
    }
}

impl From<SessionHistoryValue> for PhenixValue {
    fn from(value: SessionHistoryValue) -> Self {
        value.0
    }
}

impl ValueCodec for SessionHistoryValue {
    fn phenix_type() -> Type {
        Type::Any
    }

    fn to_value(&self) -> PhenixValue {
        self.0.clone()
    }

    fn from_value(value: &PhenixValue) -> Result<Self, ValueError> {
        Ok(Self(value.clone()))
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(deny_unknown_fields)]
pub struct SessionHistoryToolCall {
    pub call_id: String,
    pub callable_id: CallableId,
    pub arguments: SessionHistoryValue,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(tag = "outcome", rename_all = "snake_case", deny_unknown_fields)]
pub enum SessionHistoryToolOutcome {
    Success { value: SessionHistoryValue },
    Failure { code: String, message: String },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(deny_unknown_fields)]
pub struct SessionHistoryToolResult {
    pub call_id: String,
    pub callable_id: CallableId,
    pub result: SessionHistoryToolOutcome,
}

#[derive(
    Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue,
)]
#[serde(rename_all = "snake_case")]
pub enum SessionHistoryFinishReason {
    Complete,
    ToolCalls,
    Length,
    ContentFilter,
    Cancelled,
    ProviderError,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(deny_unknown_fields)]
pub struct SessionHistoryUsage {
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(deny_unknown_fields)]
pub struct SessionHistoryEntry {
    pub sequence: u64,
    pub role: SessionHistoryRole,
    pub content: Vec<SessionHistoryContentPart>,
    pub tool_calls: Vec<SessionHistoryToolCall>,
    pub tool_results: Vec<SessionHistoryToolResult>,
    pub finish_reason: Option<SessionHistoryFinishReason>,
    pub usage: Option<SessionHistoryUsage>,
    pub context_revision: String,
    pub instruction_revision: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(deny_unknown_fields)]
pub struct SessionHistoryDraft {
    pub role: SessionHistoryRole,
    pub content: Vec<SessionHistoryContentPart>,
    pub tool_calls: Vec<SessionHistoryToolCall>,
    pub tool_results: Vec<SessionHistoryToolResult>,
    pub finish_reason: Option<SessionHistoryFinishReason>,
    pub usage: Option<SessionHistoryUsage>,
    pub context_revision: String,
    pub instruction_revision: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(deny_unknown_fields)]
pub struct SessionJournalDraft {
    pub stream: ContractId,
    pub payload: PhenixValue,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(deny_unknown_fields)]
pub struct SessionJournalEntry {
    pub sequence: u64,
    pub stream: ContractId,
    pub payload: PhenixValue,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(tag = "transition", rename_all = "snake_case", deny_unknown_fields)]
pub enum SessionTransition {
    Rename { title: String },
    Close,
}

#[derive(
    Clone,
    Copy,
    Debug,
    Default,
    Eq,
    PartialEq,
    Serialize,
    Deserialize,
    phenix_sdk_macros::PhenixValue,
)]
#[serde(rename_all = "snake_case")]
pub enum SessionLifecycle {
    #[default]
    Open,
    Closed,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(deny_unknown_fields)]
pub struct SessionRecord {
    pub id: SessionId,
    #[serde(default)]
    pub working_directory: Option<String>,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub lifecycle: SessionLifecycle,
}

impl SessionRecord {
    #[must_use]
    pub fn new(id: SessionId) -> Self {
        Self {
            id,
            working_directory: None,
            title: None,
            lifecycle: SessionLifecycle::Open,
        }
    }

    #[must_use]
    pub fn application(id: SessionId, working_directory: String, title: Option<String>) -> Self {
        Self {
            id,
            working_directory: Some(working_directory),
            title,
            lifecycle: SessionLifecycle::Open,
        }
    }

    #[must_use]
    pub fn is_open(&self) -> bool {
        self.lifecycle == SessionLifecycle::Open
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub enum SessionMutationCommand {
    PrepareCreate { session: SessionRecord },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(tag = "result", rename_all = "snake_case", deny_unknown_fields)]
pub enum SessionMutationResponse {
    PreparedCreate {
        session: SessionRecord,
        mutation: PreparedMutationHandle,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub enum SessionCommand {
    Create {
        session: SessionRecord,
    },
    Get {
        id: SessionId,
    },
    List,
    Rename {
        id: SessionId,
        title: String,
    },
    Close {
        id: SessionId,
    },
    Transition {
        id: SessionId,
        transition: SessionTransition,
        journal: SessionJournalDraft,
    },
    AppendJournal {
        id: SessionId,
        entry: SessionJournalDraft,
    },
    Journal {
        id: SessionId,
        stream: ContractId,
        after_sequence: Option<u64>,
    },
    Continue {
        id: SessionId,
        kind: SessionInputKind,
        content: Bytes,
    },
    Inputs {
        id: SessionId,
    },
    ResolveInput {
        resource: String,
    },
    AppendHistory {
        id: SessionId,
        entry: SessionHistoryDraft,
    },
    History {
        id: SessionId,
    },
    ResolveHistory {
        resource: String,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(tag = "result", rename_all = "snake_case", deny_unknown_fields)]
pub enum SessionResponse {
    Created {
        session: SessionRecord,
    },
    Session {
        session: Option<SessionRecord>,
    },
    Sessions {
        sessions: Vec<SessionRecord>,
    },
    Updated {
        session: SessionRecord,
    },
    Transitioned {
        session: SessionRecord,
        journal: SessionJournalEntry,
    },
    JournalAppended {
        entry: SessionJournalEntry,
    },
    Journal {
        through_sequence: u64,
        entries: Vec<SessionJournalEntry>,
    },
    Continued {
        session: SessionRecord,
        input: SessionInput,
    },
    Inputs {
        inputs: Vec<SessionInput>,
    },
    Input {
        input: Option<SessionInput>,
    },
    HistoryAppended {
        entry: SessionHistoryEntry,
    },
    History {
        entries: Vec<SessionHistoryEntry>,
    },
    HistoryEntry {
        entry: Option<SessionHistoryEntry>,
    },
}

pub struct SessionInterface;

impl ComponentInterface for SessionInterface {
    fn interface_id() -> InterfaceId {
        InterfaceId::parse(SESSION_SERVICE).expect("static session interface id is valid")
    }

    fn schema() -> phenix_core::InterfaceSchema {
        phenix_core::InterfaceSchema::of::<SessionCommand, SessionResponse>()
    }
}

pub struct SessionMutationInterface;

impl ComponentInterface for SessionMutationInterface {
    fn interface_id() -> InterfaceId {
        InterfaceId::parse(SESSION_MUTATION_SERVICE)
            .expect("static session mutation interface id is valid")
    }

    fn schema() -> phenix_core::InterfaceSchema {
        phenix_core::InterfaceSchema::of::<SessionMutationCommand, SessionMutationResponse>()
    }
}

#[must_use]
pub fn session_service() -> ServiceId {
    ServiceId::parse(SESSION_SERVICE).expect("static session service id is valid")
}

#[must_use]
pub fn session_mutation_service() -> ServiceId {
    ServiceId::parse(SESSION_MUTATION_SERVICE).expect("static session mutation service id is valid")
}
