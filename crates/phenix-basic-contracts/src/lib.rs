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

/// Verify a single-record storage acknowledgement against its exact CAS
/// precondition and the bytes supplied by the owning plugin. A later revision,
/// unrelated payload or additional record is not proof of this transaction.
fn checked_single_commit_ack(
    records: &[StoredRecord],
    expected: u64,
    data: &Blob,
) -> Result<u64, Failure> {
    if records.len() != 1 || !records[0].matches_commit(expected, data) {
        return Err(Failure::new(
            Code::PluginFailed,
            "storage returned an invalid single-record CAS acknowledgement",
        ));
    }
    Ok(records[0].revision)
}

/// Share the mechanical single-record Storage CAS boundary only. Domain plugins
/// retain authoritative live state, validation, resource checks and recovery
/// semantics. Storage never becomes an independently writable state mirror.
pub fn transact_one(
    host: &Host<'_>,
    slot: &str,
    session: &str,
    key: &str,
    expected: u64,
    data: Blob,
) -> CallResult<u64, BasicError> {
    expected
        .checked_add(1)
        .ok_or_else(|| CallError::Domain(error(ErrorCode::Conflict, "revision overflow")))?;
    let records = host.invoke::<Storage::transact>(
        slot,
        &StorageTransaction {
            session: session.into(),
            edits: vec![StorageEdit {
                key: key.into(),
                expected,
                data: data.clone(),
            }],
        },
    )?;
    checked_single_commit_ack(&records, expected, &data).map_err(CallError::Kernel)
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
/// Exactly one transcript role and its role-specific payload.
///
/// Invalid cross-role fields do not exist in the wire schema. Model history
/// remains untrusted until the consumer verifies sequencing and call IDs.
#[derive(Debug, Clone, PartialEq, Eq, Portable, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum Message {
    User {
        text: String,
    },
    Assistant {
        text: String,
        calls: Vec<ToolCall>,
    },
    Tool {
        call_id: String,
        outcome: ToolOutcome,
    },
}

impl Message {
    pub fn user(text: impl Into<String>) -> Self {
        Self::User { text: text.into() }
    }

    pub fn assistant(text: impl Into<String>, calls: Vec<ToolCall>) -> Self {
        Self::Assistant {
            text: text.into(),
            calls,
        }
    }

    pub fn tool(call_id: impl Into<String>, outcome: ToolOutcome) -> Self {
        Self::Tool {
            call_id: call_id.into(),
            outcome,
        }
    }

    pub fn role(&self) -> Role {
        match self {
            Self::User { .. } => Role::User,
            Self::Assistant { .. } => Role::Assistant,
            Self::Tool { .. } => Role::Tool,
        }
    }

    pub fn text(&self) -> &str {
        match self {
            Self::User { text } | Self::Assistant { text, .. } => text,
            Self::Tool { outcome, .. } => outcome.text(),
        }
    }

    pub fn text_mut(&mut self) -> &mut String {
        match self {
            Self::User { text } | Self::Assistant { text, .. } => text,
            Self::Tool {
                outcome: ToolOutcome::Succeeded { text },
                ..
            } => text,
            Self::Tool {
                outcome: ToolOutcome::Failed { output, .. },
                ..
            } => output,
        }
    }

    pub fn calls(&self) -> &[ToolCall] {
        match self {
            Self::Assistant { calls, .. } => calls,
            Self::User { .. } | Self::Tool { .. } => &[],
        }
    }

    pub fn call_id(&self) -> &str {
        match self {
            Self::Tool { call_id, .. } => call_id,
            Self::User { .. } | Self::Assistant { .. } => "",
        }
    }

    pub fn tool_call_id_mut(&mut self) -> Option<&mut String> {
        match self {
            Self::Tool { call_id, .. } => Some(call_id),
            _ => None,
        }
    }

    pub fn error(&self) -> Option<&str> {
        match self {
            Self::Tool { outcome, .. } => outcome.error(),
            _ => None,
        }
    }

    pub fn outcome(&self) -> Option<&ToolOutcome> {
        match self {
            Self::Tool { outcome, .. } => Some(outcome),
            _ => None,
        }
    }
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

/// Internal staged graph wave outcome, distinct from a completed Basic turn.
/// A re-preparation commits a watched request and allows the kernel's existing
/// graph scheduling to trigger another Model wave; it is not an Answer.
#[derive(Debug, Clone, PartialEq, Eq, Portable, Serialize, Deserialize)]
pub enum TurnAdvanceOutcome {
    /// A committed preparation wave, never an answer.
    Reprepare,
    /// A terminal, authoritative Basic turn result.
    Complete { result: RunResult },
}

#[derive(Debug, Clone, PartialEq, Eq, Portable, Serialize, Deserialize)]
pub struct CatalogCursor {
    pub session: String,
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

impl ToolPage {
    /// A page is untrusted until checked against the original session query.
    /// The catalog's native producer may be replaced by a dynamic plugin; a
    /// cursor cannot switch sessions, skip tools or invent a revision.
    pub fn validate_for(&self, request: &ToolQuery) -> Result<(), BasicError> {
        if request.limit == 0 {
            return Err(error(
                ErrorCode::InvalidInput,
                "catalog page size must be positive",
            ));
        }
        if let Some(previous) = &request.cursor {
            if previous.session != request.session || previous.query != request.query {
                return Err(error(ErrorCode::InvalidCursor, "foreign catalog cursor"));
            }
            if previous.revision != self.revision {
                return Err(error(ErrorCode::InvalidCursor, "stale catalog cursor"));
            }
        }
        let count = u64::try_from(self.tools.len())
            .map_err(|_| error(ErrorCode::InvalidReply, "catalog page is oversized"))?;
        if self.revision == 0 || count > request.limit {
            return Err(error(
                ErrorCode::InvalidReply,
                "invalid catalog page size or revision",
            ));
        }
        if let Some(next) = &self.next {
            let offset = request.cursor.as_ref().map_or(0, |cursor| cursor.offset);
            if count == 0
                || next.session != request.session
                || next.query != request.query
                || next.revision != self.revision
                || offset.checked_add(count) != Some(next.offset)
            {
                return Err(error(
                    ErrorCode::InvalidReply,
                    "invalid catalog continuation",
                ));
            }
        }
        Ok(())
    }
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

/// Every completed tool effect explicitly records success or failure.
/// Missing metadata is not silently interpreted as success.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Portable, Serialize, Deserialize)]
pub enum ToolCompletion {
    Succeeded,
    Failed,
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
/// A tool effect has exactly one reconciled outcome. A failed result
/// explicitly carries both any partial output and its error reason.
#[derive(Debug, Clone, PartialEq, Eq, Portable, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum ToolOutcome {
    Succeeded { text: String },
    Failed { output: String, reason: String },
}

impl ToolOutcome {
    pub fn text(&self) -> &str {
        match self {
            Self::Succeeded { text } => text,
            Self::Failed { output, .. } => output,
        }
    }

    pub fn error(&self) -> Option<&str> {
        match self {
            Self::Succeeded { .. } => None,
            Self::Failed { reason, .. } => Some(reason),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Portable, Serialize, Deserialize)]
pub struct ReconcileTool {
    pub session: String,
    pub call_id: String,
    pub outcome: ToolOutcome,
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
impl StoredRecord {
    /// An acknowledged single-record CAS must return the exact committed
    /// revision and payload, not just any later revision or unrelated bytes.
    pub fn matches_commit(&self, expected: u64, data: &Blob) -> bool {
        expected.checked_add(1) == Some(self.revision) && &self.data == data
    }
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
pub enum Submission {
    /// An admitted action with its payload; this cannot be a cancellation.
    Dispatch { action: String, payload: Blob },
    /// Control-plane cancellation has no action identifier or payload.
    Cancel,
}
impl Submission {
    pub fn as_dispatch(&self) -> Option<(&str, &Blob)> {
        match self {
            Self::Dispatch { action, payload } => Some((action, payload)),
            Self::Cancel => None,
        }
    }

    pub fn is_cancel(&self) -> bool {
        matches!(self, Self::Cancel)
    }
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
/// An immutable tool contribution accompanied by its own executable handler.
/// Registering tool metadata never grants invocation authority. The selected
/// M1 import slot itself pins both list and execute to the same implementation.
#[contract(id = "phenix.basic.tool-registrations@1")]
pub trait ToolRegistrations {
    fn list(&self) -> Result<Vec<ToolInfo>, BasicError>;
    fn execute(&self, request: ToolRequest) -> Result<ToolResult, BasicError>;
}

/// The tool manager owns these events, not the kernel. Contributing plugins
/// only subscribe and call the typed registry contract; registration is not
/// permission to execute a tool.
#[event(id = "phenix.tools.populate")]
#[derive(Debug, Clone, PartialEq, Eq, Portable, Serialize, Deserialize)]
pub struct ToolsPopulate {}

#[event(id = "phenix.tools.ready")]
#[derive(Debug, Clone, PartialEq, Eq, Portable, Serialize, Deserialize)]
pub struct ToolsReady {
    pub revision: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Portable, Serialize, Deserialize)]
pub struct ToolKernelBegin {}

#[derive(Debug, Clone, PartialEq, Eq, Portable, Serialize, Deserialize)]
pub struct ToolsPopulated {}

#[contract(id = "phenix.tools.startup@1")]
pub trait ToolStartup {
    fn begin(&self, event: ToolKernelBegin) -> Result<(), BasicError>;
}

#[contract(id = "phenix.tools.populate@1")]
pub trait ToolPopulate {
    fn register(&self, event: ToolsPopulate) -> Result<(), BasicError>;
}

#[contract(id = "phenix.tools.settlement@1")]
pub trait ToolSettlement {
    fn complete(&self, event: ToolsPopulated) -> Result<(), BasicError>;
}

/// Staged metadata. Execution routing is *never* supplied by the contributor;
/// the manager verifies metadata against the resolver-selected
/// ToolRegistrations slot before pinning its invocation route.
#[contract(id = "phenix.tools.registry@1")]
pub trait ToolRegistry {
    fn register(&self, tool: ToolInfo) -> Result<(), BasicError>;
}

/// Thin author-facing facade around a normal, authority-checked M1 import.
pub struct Tools<'a, 'b> {
    host: &'a Host<'b>,
    slot: &'a str,
}
impl<'a, 'b> Tools<'a, 'b> {
    pub fn new(host: &'a Host<'b>, slot: &'a str) -> Self {
        Self { host, slot }
    }

    pub fn register(&self, tool: ToolInfo) -> CallResult<(), BasicError> {
        self.host.invoke::<ToolRegistry::register>(self.slot, &tool)
    }

    pub fn register_all(&self, tools: impl IntoIterator<Item = ToolInfo>) -> CallResult<(), BasicError> {
        for tool in tools {
            self.register(tool)?;
        }
        Ok(())
    }
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

#[derive(Debug, Clone, PartialEq, Eq, Portable, Serialize, Deserialize)]
pub struct GraphStateChange {
    pub epoch: u64,
    pub properties: Vec<String>,
}

/// A separately authorized continuation, activated only by a watched
/// prepared-request property after the Basic preparation graph wave.
#[contract(id = "phenix.basic.turn-advance@1")]
pub trait BasicTurnAdvance {
    fn advance(&self, notification: GraphStateChange) -> Result<TurnAdvanceOutcome, BasicError>;
}

// The selected Advanced graph preparer owns the prepared request.
// Basic Agent imports only its read-only view for source-bound admission.
#[property(
    owner = "phenix.advanced.graph-preparer",
    id = "prepared_model_request",
    get,
    watch
)]
pub type AdvancedPreparedModelRequest = Option<ModelRequest>;

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
    define(ToolRegistrations),
    define(ToolStartup),
    define(ToolPopulate),
    define(ToolSettlement),
    define(ToolRegistry),
    define(Skills),
    define(Context),
    define(Session),
    define(EventJournal),
    define(Storage),
    define(BasicInput),
    define(BasicTurnPrepare),
    define(BasicTurnAdvance),
    // Optional imports must have a canonical definition even when the
    // advanced provider is not selected in the Basic-only composition.
    define(AdvancedPreparedModelRequest),
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
    fn single_record_cas_ack_rejects_spurious_or_ambiguous_results() {
        let payload = Blob(vec![1, 2, 3]);
        let acknowledged = StoredRecord {
            revision: 8,
            data: payload.clone(),
        };
        assert_eq!(
            checked_single_commit_ack(std::slice::from_ref(&acknowledged), 7, &payload).unwrap(),
            8
        );
        assert!(checked_single_commit_ack(&[], 7, &payload).is_err());
        assert!(checked_single_commit_ack(
            &[acknowledged.clone(), acknowledged.clone()],
            7,
            &payload,
        )
        .is_err());
        assert!(
            checked_single_commit_ack(std::slice::from_ref(&acknowledged), 6, &payload).is_err()
        );
        let wrong_payload = Blob(vec![4]);
        assert!(checked_single_commit_ack(&[acknowledged], 7, &wrong_payload).is_err());
    }

    #[test]
    fn storage_ack_requires_exact_revision_and_payload() {
        let data = Blob(vec![1, 2, 3]);
        let accepted = StoredRecord {
            revision: 5,
            data: data.clone(),
        };
        assert!(accepted.matches_commit(4, &data));
        assert!(!accepted.matches_commit(3, &data));
        assert!(!accepted.matches_commit(5, &data));
        assert!(!accepted.matches_commit(4, &Blob(vec![1, 2, 4])));
        let overflow = StoredRecord { revision: 0, data };
        assert!(!overflow.matches_commit(u64::MAX, &Blob(vec![1, 2, 3])));
    }
    #[test]
    fn tool_pages_reject_foreign_cursor_skips_and_unbounded_response() {
        let request = ToolQuery {
            session: "s1".into(),
            query: "read".into(),
            cursor: None,
            limit: 2,
        };
        let mut page = ToolPage {
            tools: vec![ToolInfo {
                id: "read-one".into(),
                description: "One".into(),
                arguments_schema: r#"{"type":"object"}"#.into(),
                idempotent: true,
            }],
            revision: 7,
            next: Some(CatalogCursor {
                session: "s1".into(),
                query: "read".into(),
                revision: 7,
                offset: 1,
            }),
        };
        page.validate_for(&request).unwrap();
        let mut wrong = page.clone();
        wrong.next.as_mut().unwrap().session = "s2".into();
        assert_eq!(
            wrong.validate_for(&request).unwrap_err().code,
            ErrorCode::InvalidReply
        );
        wrong = page.clone();
        wrong.next.as_mut().unwrap().query = "write".into();
        assert_eq!(
            wrong.validate_for(&request).unwrap_err().code,
            ErrorCode::InvalidReply
        );
        wrong = page.clone();
        wrong.next.as_mut().unwrap().revision = 6;
        assert_eq!(
            wrong.validate_for(&request).unwrap_err().code,
            ErrorCode::InvalidReply
        );
        for offset in [0, 2, 100] {
            wrong = page.clone();
            wrong.next.as_mut().unwrap().offset = offset;
            assert_eq!(
                wrong.validate_for(&request).unwrap_err().code,
                ErrorCode::InvalidReply
            );
        }
        wrong = page.clone();
        wrong.tools.clear();
        assert_eq!(
            wrong.validate_for(&request).unwrap_err().code,
            ErrorCode::InvalidReply
        );
        wrong = page.clone();
        wrong.tools.extend(page.tools.clone());
        wrong.tools.extend(page.tools.clone());
        assert_eq!(
            wrong.validate_for(&request).unwrap_err().code,
            ErrorCode::InvalidReply
        );

        let mut continued = request.clone();
        continued.cursor = page.next.clone();
        let next_page = ToolPage {
            tools: page.tools.clone(),
            revision: 7,
            next: Some(CatalogCursor {
                session: "s1".into(),
                query: "read".into(),
                revision: 7,
                offset: 2,
            }),
        };
        next_page.validate_for(&continued).unwrap();
        continued.session = "another".into();
        assert_eq!(
            next_page.validate_for(&continued).unwrap_err().code,
            ErrorCode::InvalidCursor
        );
        continued.session = "s1".into();
        continued.cursor.as_mut().unwrap().revision = 6;
        assert_eq!(
            next_page.validate_for(&continued).unwrap_err().code,
            ErrorCode::InvalidCursor
        );
        page.next = None;
        page.validate_for(&request).unwrap();
    }

    #[test]
    fn reconciliation_outcomes_reject_ambiguous_wire_payloads() {
        for outcome in [
            ToolOutcome::Succeeded {
                text: "hello".into(),
            },
            ToolOutcome::Failed {
                output: String::new(),
                reason: "failed".into(),
            },
        ] {
            let encoded = outcome.to_value();
            assert!(ToolOutcome::SCHEMA.owned().unwrap().accepts(&encoded));
            assert_eq!(ToolOutcome::from_value(encoded).unwrap(), outcome);
        }
        let ambiguous = Value::map([
            (
                "Succeeded",
                Value::map([("text", Value::String("hi".into()))]),
            ),
            (
                "Failed",
                Value::map([
                    ("output", Value::String("".into())),
                    ("reason", Value::String("no".into())),
                ]),
            ),
        ]);
        assert!(!ToolOutcome::SCHEMA.owned().unwrap().accepts(&ambiguous));
        assert!(ToolOutcome::from_value(ambiguous).is_err());
    }

    #[test]
    fn internal_reprepare_is_distinct_from_a_completed_turn() {
        let reprepare = TurnAdvanceOutcome::Reprepare;
        assert_eq!(
            decode::<TurnAdvanceOutcome>(&encode(&reprepare).unwrap()).unwrap(),
            reprepare
        );
        let complete = TurnAdvanceOutcome::Complete {
            result: RunResult {
                session: "s1".into(),
                run: "run-1".into(),
                answer: String::new(),
                model_turns: 1,
                tool_calls: 0,
                usage: Usage::default(),
            },
        };
        assert_ne!(reprepare, complete);
        assert_eq!(
            decode::<TurnAdvanceOutcome>(&encode(&complete).unwrap()).unwrap(),
            complete
        );
    }

    #[test]
    fn message_roles_reject_cross_role_wire_fields() {
        let cases = [
            Message::user("hello"),
            Message::assistant("answer", vec![]),
            Message::tool(
                "tool-1",
                ToolOutcome::Failed {
                    output: String::new(),
                    reason: "denied".into(),
                },
            ),
        ];
        for message in cases {
            let value = message.to_value();
            assert!(Message::SCHEMA.owned().unwrap().accepts(&value));
            assert_eq!(Message::from_value(value).unwrap(), message);
            assert_eq!(
                serde_json::from_str::<Message>(&serde_json::to_string(&message).unwrap()).unwrap(),
                message
            );
        }

        let invalid = Value::map([(
            "User",
            Value::map([
                ("text", Value::String("hello".into())),
                ("calls", Value::List(vec![])),
            ]),
        )]);
        assert!(!Message::SCHEMA.owned().unwrap().accepts(&invalid));
        assert!(Message::from_value(invalid).is_err());

        let ambiguous = Value::map([
            ("User", Value::map([("text", Value::String("hi".into()))])),
            (
                "Tool",
                Value::map([
                    ("call_id", Value::String("c1".into())),
                    (
                        "outcome",
                        ToolOutcome::Succeeded {
                            text: "done".into(),
                        }
                        .to_value(),
                    ),
                ]),
            ),
        ]);
        assert!(!Message::SCHEMA.owned().unwrap().accepts(&ambiguous));
        assert!(Message::from_value(ambiguous).is_err());

        assert!(
            serde_json::from_str::<Message>(r#"{"User":{"text":"hello","calls":[]}}"#).is_err()
        );
        assert!(serde_json::from_str::<Message>(
            r#"{"Assistant":{"text":"ok","calls":[],"call_id":"c1"}}"#
        )
        .is_err());
        assert!(serde_json::from_str::<Message>(
            r#"{"Tool":{"call_id":"c1","outcome":{"Succeeded":{"text":"ok"}},"error":"denied"}}"#
        )
        .is_err());
        assert!(serde_json::from_str::<ToolOutcome>(
            r#"{"Succeeded":{"text":"ok","reason":"denied"}}"#
        )
        .is_err());
        assert!(serde_json::from_str::<ToolOutcome>(
            r#"{"Failed":{"output":"","reason":"denied","text":"ok"}}"#
        )
        .is_err());
        assert!(serde_json::from_str::<Message>(
            r#"{"Tool":{"call_id":"c1","outcome":{"Succeeded":{"text":"ok","reason":"denied"}}}}"#
        )
        .is_err());
    }

    #[test]
    fn complete_transcript_and_definition_package_use_canonical_types() {
        let request = ModelRequest {
            session: "test".into(),
            run: "r1".into(),
            history: vec![
                Message::user("hello"),
                Message::assistant(
                    "",
                    vec![ToolCall {
                        id: "c1".into(),
                        tool: "uppercase".into(),
                        arguments: "{}".into(),
                    }],
                ),
                Message::tool(
                    "c1",
                    ToolOutcome::Succeeded {
                        text: "HELLO".into(),
                    },
                ),
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
        assert_eq!(metadata.definitions.len(), 19);
        assert!(metadata
            .definitions
            .iter()
            .any(|definition| definition.contract == "phenix.basic.turn-advance@1"));
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

    #[test]
    fn tool_events_and_settled_handlers_have_canonical_schemas() {
        assert_eq!(ToolsPopulate::EVENT.id, "phenix.tools.populate");
        assert_eq!(ToolsReady::EVENT.id, "phenix.tools.ready");
        assert_eq!(
            ToolKernelBegin::SCHEMA.owned().unwrap(),
            phenix_sdk::standard_kernel_events()[0].payload
        );
        assert_eq!(
            ToolsPopulated::SCHEMA.owned().unwrap(),
            phenix_sdk::settled_event_schema()
        );
    }

    #[test]
    fn submission_variants_exclude_cancel_with_action_and_payload() {
        let cancel = Submission::Cancel;
        assert!(cancel.is_cancel());
        assert!(cancel.as_dispatch().is_none());
        assert_eq!(
            decode::<Submission>(&encode(&cancel).unwrap()).unwrap(),
            cancel
        );

        let dispatch = Submission::Dispatch {
            action: "phenix.basic.submit".into(),
            payload: encode(&PromptSubmitted {
                session: "s1".into(),
                text: "hi".into(),
                limits: Limits::default(),
            })
            .unwrap(),
        };
        assert!(!dispatch.is_cancel());
        assert_eq!(dispatch.as_dispatch().unwrap().0, "phenix.basic.submit");
        assert_eq!(
            decode::<Submission>(&encode(&dispatch).unwrap()).unwrap(),
            dispatch
        );

        let invalid = Value::map([
            ("Cancel", Value::Null),
            ("Dispatch", Value::Map(Default::default())),
        ]);
        assert!(!Submission::SCHEMA.owned().unwrap().accepts(&invalid));
        assert!(Submission::from_value(invalid).is_err());
    }
}
