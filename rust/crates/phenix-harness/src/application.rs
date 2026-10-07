use crate::{
    PhenixRuntime, default_application_root_authority, default_suite_authority,
    runtime_config::publish_routing_profile_runtime_state, runtime_orchestration_authority,
    workspace_discovery,
};
use parking_lot::Mutex;
use phenix_acp_stdio::{
    ApplicationEvent, ApplicationInvocation, ChannelTransport, ClientCallableCallbacks,
    ClientReferenceIdentity, SdkApplicationService, WeakChannelTransport,
    execute_admitted_client_tool_call, model_tool_surface, serve_stdio_with_events_and_callbacks,
};
use phenix_application_interface::{
    AddClientTool, Authenticate, Cancel, CloseSession, CreateSession, DecideReview,
    DiscoverAuthentication, GetSdk, InvokeCallable, InvokeCallableReference, ListCallables,
    ListDefaultSelections, ListSelections, ListSessions, Operation, Prompt, QueryLogs,
    ReadLogReference, RemoveClientTool, RenameSession, ResumeSession, SelectDefaultSelection,
    SelectSelection, SetInteractionHandlers,
    types::{
        Acknowledged, ApplicationError, AuthenticateInput, AuthenticationMethod,
        AuthenticationMethods, AuthenticationResult, CallableInvocation, CallableInvocationResult,
        Content, ElicitationHandlerRef, Empty, ExecutionChange, ExecutionState,
        InteractionHandlers, LogPage, LogQueryInput, LogRecord, LogReferenceContent,
        LogReferenceInput, Message, MessageRole, PageInput, PermissionHandlerRef,
        PermissionRequest, PermissionResponse, PromptInput, PromptResult, ReviewDecisionInput,
        ReviewRecord, SelectionDefaultSelectInput, SelectionInfo, SelectionPresentation,
        SelectionSelectInput, Selections, SessionChange, SessionCreateInput, SessionInfo,
        SessionInput as ApplicationSessionInput, SessionList, SessionProjection,
        SessionProjectionState, SessionRenameInput, SessionResumeInput, SessionSnapshot,
        SessionUpdate, SetInteractionHandlersInput, StopReason,
    },
};
use phenix_core::{
    ArtifactRevision, Authority, Bytes, CallableId, ClientConnectionId, ComponentEntryTrigger,
    ComponentExport, ComponentId, ComponentImport, ComponentInterface, ComponentManifest,
    ContentReference, ContractId, EntryTriggerKind, GenerationId, GraphReconciler, HasPhenixSchema,
    InterfaceId, InterfaceSchema, Key, LocalPersistence, LogSink, ModelToolCall,
    ModelToolDescriptor, ModelToolResult, ObservableError, ObservableRegistration, ObservableStore,
    PermissionId, PhenixContract, PhenixSchema, PhenixValue, PluginArtifact, PluginArtifactInput,
    PluginArtifactStore, PluginArtifactStoreError, PluginBuildEvidence, PluginBuildExecution,
    PluginBuildExecutor, PluginBuildFailure, PluginBuildOutput, PluginBuildPlan, PluginBuildReport,
    PluginContext, PluginExecution, PluginHost, PluginId, PluginInstance, PluginLoadRequest,
    PluginManagementContext, PluginManagementPolicy, PluginManagementRequest, PluginManifest,
    PluginRuntimeId, Project, ReconciliationPreview, ReferenceGenerationId,
    RootExecutionConstraints, RootExecutionHandle, RoutingProfileId, SdkClient,
    ServiceContribution, ServiceId, ServiceRole, SessionId, SharedCallableRegistry,
    SharedPluginInvocation, SnapshotPolicy, StructuredLogReader, ValueCodec, ValueId, ValuePath,
};
use phenix_plugin_catalog::{
    AgentLoopCommand, AgentLoopControlInterface, AgentLoopControlRequest, AgentLoopControlResponse,
    AgentLoopFailure, AgentLoopProgress, AgentLoopProgressInterface, AgentLoopProgressRecord,
    AgentLoopProgressResponse, AgentLoopResponse, AgentToolExecutionInterface,
    AgentToolExecutionRequest, AgentToolExecutionResponse, ExecutionReviewCommand,
    ExecutionReviewResponse, OptionStartupPrecedence, SDK_PLUGIN, SessionCommand, SessionInterface,
    SessionJournalDraft, SessionJournalEntry, SessionLifecycle, SessionRecord, SessionResponse,
    SessionTransition, agent_loop_control_service, agent_loop_progress_authority,
    agent_loop_progress_service, agent_loop_service, agent_tool_execution_service,
    execution_review_service, sdk_contribution, session_service, workspace_service,
};
use phenix_provider_sdk::{
    Auth, AuthKind, ProviderAuthCommand, ProviderAuthResponse, ProviderAuthenticationResult,
    ProviderModelsCommand, ProviderModelsResponse, auth, provider_auth_service,
    provider_models_service,
};
use phenix_sdk::{
    CodeQuery, CodeQueryResult, ContextCommand, ContextInjectionLifetime,
    ContextInjectionRequester, ContextResourceKind, ContextResponse, ContextScope,
    ExecutionAuthority, ExecutionCommand, ExecutionInspectionCommand, ExecutionInspectionInterface,
    ExecutionInspectionResponse, ExecutionResourceCommand, ExecutionResourceResponse,
    ExecutionResponse, LanguageCommand, LanguageInterface, LanguageResponse, MemoryCommand,
    MemoryInterface, MemoryRecallQuery, MemoryRecord, MemoryResponse, ModelCommand, ModelResponse,
    ModelTarget, OptionCommand, OptionContext, OptionKey, OptionResponse, OptionScope,
    OptionSubjectId, OptionValue, OptionValueSource, RepositoryContextSource, RootBudgetLedger,
    RootBudgetLimits, RoutingProfile, WorkspaceCommand, WorkspaceEntryKind, WorkspaceFileVersion,
    WorkspaceInterface, WorkspaceResponse, context_service, execution_resource_service,
    execution_service, model_routing_service, options_service,
};
use serde::de::DeserializeOwned;
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    env, fs,
    path::{Path, PathBuf},
    sync::{
        Arc, Weak,
        atomic::{AtomicBool, Ordering},
    },
    time::{SystemTime, UNIX_EPOCH},
};
use tokio::sync::mpsc;

pub const APPLICATION_INVOCATION_CAPACITY: usize = 64;
pub const CLIENT_CALLABLE_CAPACITY: usize = 64;
pub const APPLICATION_EVENT_CAPACITY: usize = 256;
const APPLICATION_EXECUTION_CAPACITY: usize = 64;
pub const SESSION_PROJECTION_VALUE: &str = "phenix.application.sessions@1";
const DEFAULT_APPLICATION_AGENT: &str = "agent.coordinator";
const APPLICATION_AGENT_TOOL_PLUGIN: &str = "phenix.application-agent-tools";
const APPLICATION_AGENT_TOOL_COMPONENT: &str = "phenix.application-agent-tools";
const APPLICATION_SHELL_TOOL_SERVICE: &str = "phenix.application-agent-tools.shell@1";
const APPLICATION_WORKSPACE_READ_TOOL_SERVICE: &str =
    "phenix.application-agent-tools.workspace-read@1";
const APPLICATION_WORKSPACE_SEARCH_TOOL_SERVICE: &str =
    "phenix.application-agent-tools.workspace-search@1";
const APPLICATION_WORKSPACE_WRITE_TOOL_SERVICE: &str =
    "phenix.application-agent-tools.workspace-write@1";
const APPLICATION_WORKSPACE_GIT_TOOL_SERVICE: &str =
    "phenix.application-agent-tools.workspace-git@1";
const APPLICATION_WORKSPACE_DISCOVERY_TOOL_SERVICE: &str =
    "phenix.application-agent-tools.workspace-discovery@1";
const APPLICATION_CODE_QUERY_TOOL_SERVICE: &str = "phenix.application-agent-tools.code-query@1";
const APPLICATION_MEMORY_RECORD_TOOL_SERVICE: &str =
    "phenix.application-agent-tools.memory-record@1";
const APPLICATION_MEMORY_RECALL_TOOL_SERVICE: &str =
    "phenix.application-agent-tools.memory-recall@1";
const RUNTIME_INSPECTION_READ_PERMISSION: &str = "kernel.persistence.read";
const APPLICATION_SESSION_CONTROL_PERMISSION: &str = "application.session.control";
const RUNTIME_GENERATION_SELECT_PERMISSION: &str = "runtime.generation.select";
const RUNTIME_PLUGIN_INSPECT_PERMISSION: &str = "runtime.plugin.inspect";
const RUNTIME_PLUGIN_BUILD_PERMISSION: &str = "runtime.plugin.build";
const RUNTIME_PLUGIN_TRIAL_PERMISSION: &str = "runtime.plugin.trial";
const RUNTIME_PLUGIN_PROMOTE_PERMISSION: &str = "runtime.plugin.promote";
const RUNTIME_PLUGIN_RETIRE_PERMISSION: &str = "runtime.plugin.retire";
const RUNTIME_ORCHESTRATION_OPTION: &str = "agent.runtime_orchestration";

#[derive(Clone, Debug, Eq, PartialEq, phenix_sdk::PhenixValue)]
struct ApplicationShellToolRequest {
    command: String,
}

#[derive(Clone, Debug, Eq, PartialEq, phenix_sdk::PhenixValue)]
struct ApplicationWorkspaceReadToolRequest {
    path: String,
}

#[derive(Clone, Debug, Eq, PartialEq, phenix_sdk::PhenixValue)]
struct ApplicationWorkspaceSearchToolRequest {
    needle: String,
    path: Option<String>,
    case_sensitive: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, phenix_sdk::PhenixValue)]
struct ApplicationWorkspaceWriteToolRequest {
    path: String,
    content: String,
    expected_content_hash: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, phenix_sdk::PhenixValue)]
struct ApplicationWorkspaceGitToolRequest {
    arguments: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, phenix_sdk::PhenixValue)]
struct ApplicationWorkspaceDiscoveryRequest {
    workspace_ids: Vec<String>,
    repository_remotes: Vec<String>,
    recall_terms: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, phenix_sdk::PhenixValue)]
struct ApplicationWorkspaceDiscoveryCandidate {
    workspace_id: String,
    canonical_root: String,
    repository_remotes: Vec<String>,
    matched_by: String,
    matched_terms: u32,
    confirmed_recoveries: u32,
    observation_count: u32,
    last_observed_at: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, phenix_sdk::PhenixValue)]
struct ApplicationWorkspaceDiscoveryResponse {
    candidates: Vec<ApplicationWorkspaceDiscoveryCandidate>,
    scanned_descriptors: u32,
    invalid_descriptors: u32,
    complete: bool,
    reason: Option<String>,
}

struct ApplicationShellToolInterface;
struct ApplicationWorkspaceReadToolInterface;
struct ApplicationWorkspaceSearchToolInterface;
struct ApplicationWorkspaceWriteToolInterface;
struct ApplicationWorkspaceGitToolInterface;
struct ApplicationWorkspaceDiscoveryToolInterface;
struct ApplicationCodeQueryToolInterface;
struct ApplicationMemoryRecordToolInterface;
struct ApplicationMemoryRecallToolInterface;

#[derive(Clone, Debug, Eq, PartialEq, phenix_sdk::PhenixValue)]
struct ApplicationMemoryRecallResponse {
    records: Vec<MemoryRecord>,
}

impl ComponentInterface for ApplicationShellToolInterface {
    fn interface_id() -> InterfaceId {
        InterfaceId::parse(APPLICATION_SHELL_TOOL_SERVICE)
            .expect("static application shell tool interface id is valid")
    }

    fn schema() -> InterfaceSchema {
        InterfaceSchema::of::<ApplicationShellToolRequest, WorkspaceResponse>()
    }
}

macro_rules! workspace_tool_interface {
    ($interface:ty, $service:expr, $request:ty) => {
        impl ComponentInterface for $interface {
            fn interface_id() -> InterfaceId {
                InterfaceId::parse($service).expect("static workspace tool interface id is valid")
            }

            fn schema() -> InterfaceSchema {
                InterfaceSchema::of::<$request, WorkspaceResponse>()
            }
        }
    };
}

workspace_tool_interface!(
    ApplicationWorkspaceReadToolInterface,
    APPLICATION_WORKSPACE_READ_TOOL_SERVICE,
    ApplicationWorkspaceReadToolRequest
);
workspace_tool_interface!(
    ApplicationWorkspaceSearchToolInterface,
    APPLICATION_WORKSPACE_SEARCH_TOOL_SERVICE,
    ApplicationWorkspaceSearchToolRequest
);
workspace_tool_interface!(
    ApplicationWorkspaceWriteToolInterface,
    APPLICATION_WORKSPACE_WRITE_TOOL_SERVICE,
    ApplicationWorkspaceWriteToolRequest
);
workspace_tool_interface!(
    ApplicationWorkspaceGitToolInterface,
    APPLICATION_WORKSPACE_GIT_TOOL_SERVICE,
    ApplicationWorkspaceGitToolRequest
);

impl ComponentInterface for ApplicationWorkspaceDiscoveryToolInterface {
    fn interface_id() -> InterfaceId {
        InterfaceId::parse(APPLICATION_WORKSPACE_DISCOVERY_TOOL_SERVICE)
            .expect("static workspace discovery tool interface id is valid")
    }

    fn schema() -> InterfaceSchema {
        InterfaceSchema::of::<
            ApplicationWorkspaceDiscoveryRequest,
            ApplicationWorkspaceDiscoveryResponse,
        >()
    }
}

impl ComponentInterface for ApplicationCodeQueryToolInterface {
    fn interface_id() -> InterfaceId {
        InterfaceId::parse(APPLICATION_CODE_QUERY_TOOL_SERVICE)
            .expect("static code query tool interface id is valid")
    }

    fn schema() -> InterfaceSchema {
        InterfaceSchema::of::<CodeQuery, CodeQueryResult>()
    }
}

impl ComponentInterface for ApplicationMemoryRecordToolInterface {
    fn interface_id() -> InterfaceId {
        InterfaceId::parse(APPLICATION_MEMORY_RECORD_TOOL_SERVICE)
            .expect("static memory record tool interface id is valid")
    }

    fn schema() -> InterfaceSchema {
        InterfaceSchema::of::<MemoryRecord, MemoryRecord>()
    }
}

impl ComponentInterface for ApplicationMemoryRecallToolInterface {
    fn interface_id() -> InterfaceId {
        InterfaceId::parse(APPLICATION_MEMORY_RECALL_TOOL_SERVICE)
            .expect("static memory recall tool interface id is valid")
    }

    fn schema() -> InterfaceSchema {
        InterfaceSchema::of::<MemoryRecallQuery, ApplicationMemoryRecallResponse>()
    }
}

fn workspace_capability(value: &str) -> Authority {
    Authority::new([PermissionId::parse(value).expect("static workspace permission is valid")])
}

fn application_workspace_authority() -> Authority {
    Authority::new(
        [
            "workspace.read",
            "workspace.write",
            "workspace.shell",
            "workspace.git",
        ]
        .into_iter()
        .map(|value| PermissionId::parse(value).expect("static workspace permission is valid")),
    )
}

fn application_shell_authority() -> Authority {
    workspace_capability("workspace.shell")
}

fn application_workspace_read_authority() -> Authority {
    workspace_capability("workspace.read")
}

fn application_workspace_write_authority() -> Authority {
    workspace_capability("workspace.write")
}

fn application_workspace_git_authority() -> Authority {
    workspace_capability("workspace.git")
}

fn application_code_query_authority() -> Authority {
    Authority::new([PermissionId::parse("kernel.persistence.read")
        .expect("static persistence read permission is valid")])
}

fn application_memory_authority() -> Authority {
    Authority::new(
        [
            "kernel.persistence.schema",
            "kernel.persistence.read",
            "kernel.persistence.write",
        ]
        .into_iter()
        .map(|value| PermissionId::parse(value).expect("static persistence permission is valid")),
    )
}

fn application_agent_tool_trigger(
    interface: InterfaceId,
    callable_id: &str,
    description: &str,
    required_authority: Authority,
) -> ComponentEntryTrigger {
    ComponentEntryTrigger {
        component: application_agent_tool_component_id(),
        interface,
        trigger: EntryTriggerKind::ToolCall {
            callable_id: CallableId::parse(callable_id)
                .expect("static application agent tool id is valid"),
            description: description.to_owned(),
        },
        required_authority,
    }
}

#[must_use]
pub(crate) fn application_workspace_tool_triggers() -> Vec<ComponentEntryTrigger> {
    vec![
        application_agent_tool_trigger(
            ApplicationShellToolInterface::interface_id(),
            "bash",
            "Run a shell command in the configured Phenix workspace. The workspace provider owns execution, so the same tool can target local, SSH, container, or other workspace backends.",
            application_shell_authority(),
        ),
        application_agent_tool_trigger(
            ApplicationWorkspaceReadToolInterface::interface_id(),
            "workspace.read",
            "Read one UTF-8 text file by workspace-relative path and return its exact content version.",
            application_workspace_read_authority(),
        ),
        application_agent_tool_trigger(
            ApplicationWorkspaceSearchToolInterface::interface_id(),
            "workspace.search",
            "Search workspace text files for a string, optionally below a relative path.",
            application_workspace_read_authority(),
        ),
        application_agent_tool_trigger(
            ApplicationWorkspaceWriteToolInterface::interface_id(),
            "workspace.write",
            "Write one UTF-8 text file. Pass the exact observed content hash, or null only when the file was observed absent.",
            application_workspace_write_authority(),
        ),
        application_agent_tool_trigger(
            ApplicationWorkspaceGitToolInterface::interface_id(),
            "workspace.git",
            "Run Git with explicit arguments in the configured workspace.",
            application_workspace_git_authority(),
        ),
        application_agent_tool_trigger(
            ApplicationWorkspaceDiscoveryToolInterface::interface_id(),
            "workspace.discover",
            "Find up to three previously observed local Phenix workspaces by stable workspace id, normalized repository remote, or bounded recall terms.",
            Authority::default(),
        ),
    ]
}

#[must_use]
pub(crate) fn application_code_tool_triggers() -> Vec<ComponentEntryTrigger> {
    vec![application_agent_tool_trigger(
        ApplicationCodeQueryToolInterface::interface_id(),
        "code.query",
        "Run a bounded provider-neutral semantic code query over the canonical language index.",
        application_code_query_authority(),
    )]
}

#[must_use]
pub(crate) fn application_memory_tool_triggers() -> Vec<ComponentEntryTrigger> {
    vec![
        application_agent_tool_trigger(
            ApplicationMemoryRecordToolInterface::interface_id(),
            "memory.record",
            "Persist one typed memory record in the configured memory provider. Use durable source references for remembered claims.",
            application_memory_authority(),
        ),
        application_agent_tool_trigger(
            ApplicationMemoryRecallToolInterface::interface_id(),
            "memory.recall",
            "Recall typed durable memories using bounded scope, kind, text, time, and result limits. Durable memory is not injected into every fresh session; call this tool when the current task depends on previously stored memory.",
            application_memory_authority(),
        ),
    ]
}

#[must_use]
pub fn session_projection_value_id() -> ValueId {
    ValueId::parse(SESSION_PROJECTION_VALUE).expect("static session projection value id is valid")
}

#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum SessionProjectionError {
    #[error("session projection is missing for {session_id}")]
    Missing { session_id: phenix_core::SessionId },
    #[error("session projection sequence gap for {session_id}: expected {expected}, got {actual}")]
    SequenceGap {
        session_id: phenix_core::SessionId,
        expected: u64,
        actual: u64,
    },
    #[error("session projection repair for {expected} received an update for {actual}")]
    RepairSessionMismatch {
        expected: phenix_core::SessionId,
        actual: phenix_core::SessionId,
    },
}

#[derive(Debug, thiserror::Error)]
pub enum SessionProjectionStoreError {
    #[error(transparent)]
    Projection(#[from] SessionProjectionError),
    #[error(transparent)]
    Observable(#[from] ObservableError),
}

#[derive(Clone, Debug, PartialEq)]
pub struct SessionProjectionReducer {
    state: SessionProjectionState,
}

impl Default for SessionProjectionReducer {
    fn default() -> Self {
        Self::new()
    }
}

impl SessionProjectionReducer {
    #[must_use]
    pub fn new() -> Self {
        Self {
            state: SessionProjectionState {
                sessions: BTreeMap::new(),
            },
        }
    }

    #[must_use]
    pub fn state(&self) -> &SessionProjectionState {
        &self.state
    }

    pub fn insert_created(&mut self, session: SessionInfo) {
        self.state.sessions.insert(
            session.session_id.as_str().to_owned(),
            SessionProjection {
                session,
                through_sequence: 0,
                updates: Vec::new(),
            },
        );
    }

    pub fn replace_snapshot(&mut self, snapshot: SessionSnapshot) {
        let SessionSnapshot {
            session,
            through_sequence,
            updates,
        } = snapshot;
        self.state.sessions.insert(
            session.session_id.as_str().to_owned(),
            SessionProjection {
                session,
                through_sequence,
                updates,
            },
        );
    }

    pub fn apply_update(&mut self, update: SessionUpdate) -> Result<(), SessionProjectionError> {
        let key = update.session_id.as_str().to_owned();
        let projection =
            self.state
                .sessions
                .get_mut(&key)
                .ok_or_else(|| SessionProjectionError::Missing {
                    session_id: update.session_id.clone(),
                })?;
        let expected = projection.through_sequence + 1;
        if update.sequence != expected {
            return Err(SessionProjectionError::SequenceGap {
                session_id: update.session_id,
                expected,
                actual: update.sequence,
            });
        }
        if let SessionChange::Renamed { title } = &update.update {
            projection.session.title = Some(title.clone());
        }
        projection.through_sequence = update.sequence;
        projection.updates.push(update);
        Ok(())
    }
}

pub struct SessionProjectionStore {
    store: ObservableStore,
    value_id: ValueId,
    reducer: SessionProjectionReducer,
}

impl SessionProjectionStore {
    pub fn new() -> Result<Self, ObservableError> {
        let reducer = SessionProjectionReducer::new();
        let store = ObservableStore::default();
        let value_id = session_projection_value_id();
        store.register(ObservableRegistration {
            id: value_id.clone(),
            owner: PluginId::parse(SDK_PLUGIN).expect("static SDK plugin id is valid"),
            schema: SessionProjectionState::phenix_schema(),
            snapshot_policy: SnapshotPolicy::CopyOnChange,
            initial: reducer.state().to_value(),
        })?;
        Ok(Self {
            store,
            value_id,
            reducer,
        })
    }

    #[must_use]
    pub fn store(&self) -> &ObservableStore {
        &self.store
    }

    #[must_use]
    pub fn state(&self) -> &SessionProjectionState {
        self.reducer.state()
    }

    #[must_use]
    pub fn value_id(&self) -> &ValueId {
        &self.value_id
    }

    pub fn insert_created(
        &mut self,
        session: SessionInfo,
    ) -> Result<(), SessionProjectionStoreError> {
        self.commit(move |reducer| {
            reducer.insert_created(session);
            Ok(())
        })
    }

    pub fn replace_snapshot(
        &mut self,
        snapshot: SessionSnapshot,
    ) -> Result<(), SessionProjectionStoreError> {
        self.commit(move |reducer| {
            reducer.replace_snapshot(snapshot);
            Ok(())
        })
    }

    pub fn apply_update(
        &mut self,
        update: SessionUpdate,
    ) -> Result<(), SessionProjectionStoreError> {
        self.commit(move |reducer| reducer.apply_update(update))
    }

    /// Replaces one session with an authoritative full snapshot, then consumes
    /// only the still-relevant contiguous suffix buffered while repair ran.
    pub fn repair_with_snapshot(
        &mut self,
        snapshot: SessionSnapshot,
        buffered: impl IntoIterator<Item = SessionUpdate>,
    ) -> Result<(), SessionProjectionStoreError> {
        let session_id = snapshot.session.session_id.clone();
        self.replace_snapshot(snapshot)?;

        for update in buffered {
            if update.session_id != session_id {
                return Err(SessionProjectionError::RepairSessionMismatch {
                    expected: session_id,
                    actual: update.session_id,
                }
                .into());
            }
            let through_sequence =
                self.reducer.state.sessions[session_id.as_str()].through_sequence;
            if update.sequence <= through_sequence {
                continue;
            }
            self.apply_update(update)?;
        }
        Ok(())
    }

    fn commit(
        &mut self,
        mutate: impl FnOnce(&mut SessionProjectionReducer) -> Result<(), SessionProjectionError>,
    ) -> Result<(), SessionProjectionStoreError> {
        let mut next = self.reducer.clone();
        mutate(&mut next)?;
        let value = next.state().to_value();
        self.store.transaction([&self.value_id], |transaction| {
            transaction.replace(&self.value_id, ValuePath::root(), value)
        })?;
        self.reducer = next;
        Ok(())
    }
}

pub struct ApplicationWorker {
    harness: Arc<Mutex<PhenixRuntime>>,
    authority: Authority,
    projection: SessionProjectionStore,
    interaction_handlers: InteractionHandlers,
    event_sender: Option<mpsc::Sender<ApplicationEvent>>,
    log_reader: Result<StructuredLogReader, String>,
}

impl ApplicationWorker {
    pub fn new(harness: PhenixRuntime) -> Result<Self, ObservableError> {
        Ok(Self {
            harness: Arc::new(Mutex::new(harness)),
            authority: default_suite_authority(),
            projection: SessionProjectionStore::new()?,
            interaction_handlers: InteractionHandlers {
                permission: None,
                elicitation: None,
            },
            event_sender: None,
            log_reader: configured_log_reader(),
        })
    }

    #[must_use]
    pub fn projection(&self) -> &SessionProjectionStore {
        &self.projection
    }

    #[must_use]
    pub fn projection_mut(&mut self) -> &mut SessionProjectionStore {
        &mut self.projection
    }

    #[must_use]
    pub fn with_event_sender(mut self, sender: mpsc::Sender<ApplicationEvent>) -> Self {
        self.event_sender = Some(sender);
        self
    }

    #[must_use]
    pub fn interaction_handlers(&self) -> &InteractionHandlers {
        &self.interaction_handlers
    }

    pub fn clear_interaction_handlers(&mut self) {
        self.interaction_handlers = InteractionHandlers {
            permission: None,
            elicitation: None,
        };
    }

    pub fn invoke_with_client_callables(
        &mut self,
        operation: &ContractId,
        input: PhenixValue,
        mut admit: impl FnMut(
            &phenix_core::CallableRef,
            phenix_core::Type,
        ) -> Result<(), ApplicationError>,
    ) -> Result<PhenixValue, ApplicationError> {
        if operation.as_str() == SetInteractionHandlers::ID {
            return self
                .set_interaction_handlers(decode(input)?, &mut admit)
                .map(|value| value.to_value());
        }
        self.invoke(operation, input)
    }

    pub fn invoke(
        &mut self,
        operation: &ContractId,
        input: PhenixValue,
    ) -> Result<PhenixValue, ApplicationError> {
        match operation.as_str() {
            DiscoverAuthentication::ID => self
                .discover_authentication(decode(input)?)
                .map(|value| value.to_value()),
            Authenticate::ID => self
                .authenticate(decode(input)?)
                .map(|value| value.to_value()),
            ListDefaultSelections::ID => self
                .list_default_selections(decode(input)?)
                .map(|value| value.to_value()),
            SelectDefaultSelection::ID => self
                .select_default_selection(decode(input)?)
                .map(|value| value.to_value()),
            ListSelections::ID => self
                .list_selections(decode(input)?)
                .map(|value| value.to_value()),
            SelectSelection::ID => self
                .select_selection(decode(input)?)
                .map(|value| value.to_value()),
            CreateSession::ID => self
                .create_session(decode(input)?)
                .map(|value| value.to_value()),
            ListSessions::ID => self
                .list_sessions(decode(input)?)
                .map(|value| value.to_value()),
            ResumeSession::ID => self
                .resume_session(decode(input)?)
                .map(|value| value.to_value()),
            RenameSession::ID => self
                .rename_session(decode(input)?)
                .map(|value| value.to_value()),
            CloseSession::ID => self
                .close_session(decode(input)?)
                .map(|value| value.to_value()),
            Prompt::ID => self.prompt(decode(input)?).map(|value| value.to_value()),
            Cancel::ID => self.cancel(decode(input)?).map(|value| value.to_value()),
            DecideReview::ID => self
                .decide_review(decode(input)?)
                .map(|value| value.to_value()),
            QueryLogs::ID => self
                .query_logs(decode(input)?)
                .map(|value| value.to_value()),
            ReadLogReference::ID => self
                .read_log_reference(decode(input)?)
                .map(|value| value.to_value()),
            _ => Err(ApplicationError::UnsupportedCapability {
                capability: operation.clone(),
            }),
        }
    }

    fn log_reader(&self) -> Result<&StructuredLogReader, ApplicationError> {
        self.log_reader
            .as_ref()
            .map_err(|message| ApplicationError::Failed {
                message: message.clone(),
            })
    }

    fn query_logs(&self, request: LogQueryInput) -> Result<LogPage, ApplicationError> {
        const DEFAULT_LIMIT: usize = 200;
        const MAX_LIMIT: usize = 1000;
        const READ_BATCH: usize = 512;

        let mut cursor = request
            .cursor
            .as_deref()
            .unwrap_or("0")
            .parse::<u64>()
            .map_err(|error| ApplicationError::InvalidInput {
                message: format!("invalid log cursor: {error}"),
            })?;
        let limit = usize::try_from(request.limit.unwrap_or(DEFAULT_LIMIT as u64))
            .unwrap_or(MAX_LIMIT)
            .clamp(1, MAX_LIMIT);
        let reader = self.log_reader()?;
        let mut records = Vec::new();

        let next_cursor = 'scan: loop {
            let page = reader
                .read_page(cursor, READ_BATCH)
                .map_err(|message| ApplicationError::Failed { message })?;
            if page.records.is_empty() {
                break 'scan None;
            }

            let page_has_more = page.next_cursor.is_some();
            let page_record_count = page.records.len();
            for (index, raw) in page.records.into_iter().enumerate() {
                let record_cursor = cursor;
                cursor = cursor.saturating_add(1);
                if !log_record_matches(
                    &raw,
                    request.session_id.as_ref(),
                    request.execution_id.as_deref(),
                ) {
                    continue;
                }
                records.push(application_log_record(record_cursor, raw)?);
                if records.len() == limit {
                    break 'scan (index + 1 < page_record_count || page_has_more)
                        .then(|| cursor.to_string());
                }
            }

            match page.next_cursor {
                Some(next) => cursor = next,
                None => break 'scan None,
            }
        };

        Ok(LogPage {
            records,
            next_cursor,
        })
    }

    fn read_log_reference(
        &self,
        request: LogReferenceInput,
    ) -> Result<LogReferenceContent, ApplicationError> {
        let reader = self.log_reader()?;
        let content = reader
            .read_reference(&request.reference)
            .map_err(|message| ApplicationError::Failed { message })?
            .ok_or_else(|| ApplicationError::NotFound {
                resource: request.reference.digest.to_string(),
            })?;
        Ok(LogReferenceContent {
            reference: request.reference,
            content: Bytes::from(content),
        })
    }

    fn set_interaction_handlers(
        &mut self,
        request: SetInteractionHandlersInput,
        admit: &mut impl FnMut(
            &phenix_core::CallableRef,
            phenix_core::Type,
        ) -> Result<(), ApplicationError>,
    ) -> Result<Acknowledged, ApplicationError> {
        let handlers = request.handlers;
        if let Some(permission) = &handlers.permission {
            admit(
                permission.reference(),
                <PermissionHandlerRef as ValueCodec>::phenix_type(),
            )?;
        }
        if let Some(elicitation) = &handlers.elicitation {
            admit(
                elicitation.reference(),
                <ElicitationHandlerRef as ValueCodec>::phenix_type(),
            )?;
        }
        self.interaction_handlers = handlers;
        Ok(Acknowledged {})
    }

    fn list_default_selections(&mut self, _request: Empty) -> Result<Selections, ApplicationError> {
        let selected = self.default_routing_profile()?;
        self.selection_catalog(selected)
    }

    fn list_selections(
        &mut self,
        request: ApplicationSessionInput,
    ) -> Result<Selections, ApplicationError> {
        self.require_open_application_session(&request.session_id)?;
        let selected = self.selected_routing_profile(&request.session_id)?;
        self.selection_catalog(selected)
    }

    fn selection_catalog(
        &self,
        selected: RoutingProfileId,
    ) -> Result<Selections, ApplicationError> {
        let provider_authentication = self.provider_authentication_states()?;
        let provider_names = self.refresh_provider_model_catalogs(&provider_authentication);
        let descriptors = match self.invoke_model_command(ModelCommand::ListProfiles)? {
            ModelResponse::Profiles { profiles } => profiles,
            response => {
                return Err(ApplicationError::InvalidResponse {
                    message: format!(
                        "model routing returned an unexpected profile-list response: {response:?}"
                    ),
                });
            }
        };

        let mut available = Vec::with_capacity(descriptors.len());
        for descriptor in descriptors {
            let profile = match self.invoke_model_command(ModelCommand::GetProfile {
                id: descriptor.id.clone(),
            })? {
                ModelResponse::Profile {
                    profile: Some(profile),
                } => profile,
                response => {
                    return Err(ApplicationError::InvalidResponse {
                        message: format!(
                            "model routing returned an unexpected profile response: {response:?}"
                        ),
                    });
                }
            };
            let authenticated =
                self.routing_profile_authenticated(&profile, &provider_authentication);
            available.push(selection_info(&profile, authenticated, &provider_names)?);
        }
        if !available.iter().any(|item| item.id == selected)
            && let ModelResponse::Profile {
                profile: Some(profile),
            } = self.invoke_model_command(ModelCommand::GetProfile {
                id: selected.clone(),
            })?
        {
            let authenticated =
                self.routing_profile_authenticated(&profile, &provider_authentication);
            available.push(selection_info(&profile, authenticated, &provider_names)?);
        }
        available.sort_by(|left, right| {
            selection_presentation_rank(&left.presentation)
                .cmp(&selection_presentation_rank(&right.presentation))
                .then_with(|| left.provider.cmp(&right.provider))
                .then_with(|| left.name.cmp(&right.name))
                .then_with(|| left.thinking.cmp(&right.thinking))
                .then_with(|| left.id.cmp(&right.id))
        });
        Ok(Selections {
            available,
            selected: Some(selected),
        })
    }

    fn routing_profile_authenticated(
        &self,
        profile: &RoutingProfile,
        provider_authentication: &BTreeMap<PluginId, bool>,
    ) -> bool {
        std::iter::once(&profile.default_target)
            .chain(profile.fallback_targets.iter())
            .chain(profile.callable_targets.values())
            .all(|target| {
                provider_authentication
                    .get(&target.provider_plugin)
                    .copied()
                    .unwrap_or(true)
            })
    }

    fn provider_authentication_states(&self) -> Result<BTreeMap<PluginId, bool>, ApplicationError> {
        let mut states = BTreeMap::new();
        for provider in self.provider_auth_plugins() {
            states.insert(provider.clone(), self.provider_authenticated(&provider)?);
        }
        Ok(states)
    }

    fn provider_authenticated(&self, provider: &PluginId) -> Result<bool, ApplicationError> {
        match self.invoke_provider_auth(provider, ProviderAuthCommand::Status)? {
            ProviderAuthResponse::Status { authenticated } => Ok(authenticated),
            response => Err(ApplicationError::InvalidResponse {
                message: format!(
                    "provider {provider} returned an unexpected authentication-status response: {response:?}"
                ),
            }),
        }
    }

    fn select_default_selection(
        &mut self,
        request: SelectionDefaultSelectInput,
    ) -> Result<Selections, ApplicationError> {
        self.require_routing_profile(&request.selection_id)?;
        let response = self.invoke_option_command(OptionCommand::Set {
            key: model_default_option(),
            scope: OptionScope::Global,
            value: OptionValue::String(request.selection_id.to_string()),
        })?;
        if !matches!(response, OptionResponse::Updated { .. }) {
            return Err(ApplicationError::InvalidResponse {
                message: format!(
                    "options service rejected default routing selection: {response:?}"
                ),
            });
        }
        self.list_default_selections(Empty {})
    }

    fn require_routing_profile(
        &self,
        selection_id: &RoutingProfileId,
    ) -> Result<(), ApplicationError> {
        match self.invoke_model_command(ModelCommand::ListProfiles)? {
            ModelResponse::Profiles { profiles }
                if profiles.iter().any(|profile| &profile.id == selection_id) =>
            {
                Ok(())
            }
            ModelResponse::Profiles { .. } => Err(ApplicationError::InvalidInput {
                message: format!("routing selection {selection_id} is not available"),
            }),
            response => Err(ApplicationError::InvalidResponse {
                message: format!(
                    "model routing returned an unexpected profile-list response: {response:?}"
                ),
            }),
        }
    }

    fn select_selection(
        &mut self,
        request: SelectionSelectInput,
    ) -> Result<Selections, ApplicationError> {
        self.require_open_application_session(&request.session_id)?;
        self.require_routing_profile(&request.selection_id)?;

        let subject = OptionSubjectId::parse(request.session_id.as_str()).map_err(|error| {
            ApplicationError::InvalidInput {
                message: error.to_owned(),
            }
        })?;
        let response = self.invoke_option_command(OptionCommand::Set {
            key: model_default_option(),
            scope: OptionScope::Session(subject),
            value: OptionValue::String(request.selection_id.to_string()),
        })?;
        if !matches!(response, OptionResponse::Updated { .. }) {
            return Err(ApplicationError::InvalidResponse {
                message: format!("options service rejected routing selection: {response:?}"),
            });
        }
        self.list_selections(ApplicationSessionInput {
            session_id: request.session_id,
        })
    }

    fn default_routing_profile(&self) -> Result<RoutingProfileId, ApplicationError> {
        self.resolve_routing_profile(OptionContext {
            session: None,
            agent: None,
        })
    }

    fn selected_routing_profile(
        &self,
        session_id: &SessionId,
    ) -> Result<RoutingProfileId, ApplicationError> {
        let subject = OptionSubjectId::parse(session_id.as_str()).map_err(|error| {
            ApplicationError::InvalidInput {
                message: error.to_owned(),
            }
        })?;
        self.resolve_routing_profile(OptionContext {
            session: Some(subject),
            agent: None,
        })
    }

    fn resolve_routing_profile(
        &self,
        context: OptionContext,
    ) -> Result<RoutingProfileId, ApplicationError> {
        match self.invoke_option_command(OptionCommand::Resolve {
            key: model_default_option(),
            context,
        })? {
            OptionResponse::Value { option } => match option.value {
                OptionValue::String(value) => RoutingProfileId::parse(value).map_err(|error| {
                    ApplicationError::InvalidResponse {
                        message: format!("model.default is not a valid routing profile: {error}"),
                    }
                }),
                value => Err(ApplicationError::InvalidResponse {
                    message: format!("model.default must be a string, got {value:?}"),
                }),
            },
            response => Err(ApplicationError::InvalidResponse {
                message: format!("options service returned an unexpected response: {response:?}"),
            }),
        }
    }

    fn application_root_authority(
        &self,
        session_id: &SessionId,
    ) -> Result<Authority, ApplicationError> {
        let subject = OptionSubjectId::parse(session_id.as_str()).map_err(|error| {
            ApplicationError::InvalidInput {
                message: error.to_owned(),
            }
        })?;
        let response = self.invoke_option_command(OptionCommand::Resolve {
            key: OptionKey::parse(RUNTIME_ORCHESTRATION_OPTION).map_err(|error| {
                ApplicationError::InvalidInput {
                    message: error.to_owned(),
                }
            })?,
            context: OptionContext {
                session: Some(subject),
                agent: Some(OptionSubjectId::parse(DEFAULT_APPLICATION_AGENT).map_err(
                    |error| ApplicationError::InvalidInput {
                        message: error.to_owned(),
                    },
                )?),
            },
        })?;
        let OptionResponse::Value { option } = response else {
            return Err(ApplicationError::InvalidResponse {
                message: format!(
                    "option {RUNTIME_ORCHESTRATION_OPTION} returned a non-value response"
                ),
            });
        };
        let OptionValue::Bool(mut enabled) = option.value else {
            return Err(ApplicationError::InvalidResponse {
                message: format!("{RUNTIME_ORCHESTRATION_OPTION} must be boolean"),
            });
        };
        if !enabled
            && option.source == OptionValueSource::Default
            && self
                .harness
                .lock()
                .resolved_generation()
                .plugins()
                .iter()
                .any(|plugin| plugin.id.as_str() == "phenix.product.full")
        {
            enabled = true;
        }

        let application = default_application_root_authority();
        if !enabled {
            return Ok(application);
        }
        let orchestration = runtime_orchestration_authority();
        Ok(Authority::new(
            application
                .permissions()
                .cloned()
                .chain(orchestration.permissions().cloned()),
        ))
    }

    fn invoke_model_command(
        &self,
        command: ModelCommand,
    ) -> Result<ModelResponse, ApplicationError> {
        let input = serde_json::to_vec(&PhenixValue::from(&command)).map_err(|error| {
            ApplicationError::InvalidInput {
                message: error.to_string(),
            }
        })?;
        let output = self
            .harness
            .lock()
            .invoke(&model_routing_service(), &input, &self.authority, None)
            .map_err(|error| ApplicationError::Failed {
                message: error.to_string(),
            })?;
        let output: PhenixValue =
            serde_json::from_slice(&output).map_err(|error| ApplicationError::InvalidResponse {
                message: error.to_string(),
            })?;
        ModelResponse::try_from(Project(&output)).map_err(|error| {
            ApplicationError::InvalidResponse {
                message: error.to_string(),
            }
        })
    }

    fn invoke_option_command(
        &self,
        command: OptionCommand,
    ) -> Result<OptionResponse, ApplicationError> {
        let input = serde_json::to_vec(&PhenixValue::from(&command)).map_err(|error| {
            ApplicationError::InvalidInput {
                message: error.to_string(),
            }
        })?;
        let output = self
            .harness
            .lock()
            .invoke(&options_service(), &input, &self.authority, None)
            .map_err(|error| ApplicationError::Failed {
                message: error.to_string(),
            })?;
        let output: PhenixValue =
            serde_json::from_slice(&output).map_err(|error| ApplicationError::InvalidResponse {
                message: error.to_string(),
            })?;
        OptionResponse::try_from(Project(&output)).map_err(|error| {
            ApplicationError::InvalidResponse {
                message: error.to_string(),
            }
        })
    }

    fn resolve_bool_option_on(
        &self,
        root: &RootExecutionHandle,
        session_id: &SessionId,
        key: &str,
    ) -> Result<bool, ApplicationError> {
        let response = self.invoke_option_command_on(
            root,
            OptionCommand::Resolve {
                key: OptionKey::parse(key).map_err(|error| ApplicationError::InvalidInput {
                    message: error.to_owned(),
                })?,
                context: OptionContext {
                    session: Some(
                        OptionSubjectId::parse(session_id.as_str().to_owned()).map_err(
                            |error| ApplicationError::InvalidInput {
                                message: error.to_owned(),
                            },
                        )?,
                    ),
                    agent: Some(OptionSubjectId::parse(DEFAULT_APPLICATION_AGENT).map_err(
                        |error| ApplicationError::InvalidInput {
                            message: error.to_owned(),
                        },
                    )?),
                },
            },
        )?;
        let OptionResponse::Value { option } = response else {
            return Err(ApplicationError::InvalidResponse {
                message: format!("option {key} returned a non-value response"),
            });
        };
        match option.value {
            OptionValue::Bool(value) => Ok(value),
            other => Err(ApplicationError::InvalidResponse {
                message: format!("option {key} must be boolean, got {other:?}"),
            }),
        }
    }

    fn workspace_context_sources_on(
        &self,
        root: &RootExecutionHandle,
    ) -> Result<Vec<RepositoryContextSource>, ApplicationError> {
        let WorkspaceResponse::List { entries } = self.invoke_workspace_command_on(
            root,
            WorkspaceCommand::List {
                path: None,
                recursive: true,
            },
        )?
        else {
            return Err(ApplicationError::InvalidResponse {
                message: "workspace list returned a non-list response".into(),
            });
        };
        let mut paths = entries
            .into_iter()
            .filter(|entry| entry.kind == WorkspaceEntryKind::File)
            .map(|entry| entry.path)
            .filter(|path| {
                matches!(
                    path.rsplit('/').next().unwrap_or(path.as_str()),
                    "AGENTS.md" | "AGENTS.override.md" | "CONTRIBUTING.md" | "DEVELOPMENT.md"
                )
            })
            .collect::<Vec<_>>();
        paths.sort();
        paths.dedup();

        paths
            .into_iter()
            .map(|path| {
                let WorkspaceResponse::Read { content, .. } = self.invoke_workspace_command_on(
                    root,
                    WorkspaceCommand::Read { path: path.clone() },
                )?
                else {
                    return Err(ApplicationError::InvalidResponse {
                        message: format!("workspace read returned a non-read response for {path}"),
                    });
                };
                Ok(RepositoryContextSource {
                    path,
                    content: content.into_bytes().into(),
                })
            })
            .collect()
    }

    fn prepare_execution_context_on(
        &self,
        root: &RootExecutionHandle,
        session: &SessionInfo,
        execution_id: &str,
    ) -> Result<(), ApplicationError> {
        observe_local_workspace_discovery(session);
        let context_auto =
            self.resolve_bool_option_on(root, &session.session_id, "context.auto_load")?;
        if !context_auto {
            return Ok(());
        }

        let workspace_id = workspace_context_id(&session.working_directory);
        let sources = self.workspace_context_sources_on(root)?;
        let mut descriptors = if sources.is_empty() {
            Vec::new()
        } else {
            match self.invoke_context_command_on(
                root,
                ContextCommand::DiscoverRepository {
                    workspace_id,
                    sources,
                },
            )? {
                ContextResponse::Discovered { descriptors } => descriptors,
                _ => {
                    return Err(ApplicationError::InvalidResponse {
                        message: "repository context discovery returned an unexpected response"
                            .into(),
                    });
                }
            }
        };

        descriptors.sort_by(|left, right| left.resource_id.cmp(&right.resource_id));
        for descriptor in descriptors {
            let mandatory_project_instruction = descriptor.kind
                == ContextResourceKind::ProjectInstruction
                && descriptor.scope == ContextScope::Workspace;
            if !mandatory_project_instruction {
                continue;
            }
            let response = self.invoke_context_command_on(
                root,
                ContextCommand::Load {
                    execution_id: execution_id.to_owned(),
                    resource_id: descriptor.resource_id,
                    revision: descriptor.revision,
                    requester: ContextInjectionRequester::ContextPolicy,
                    lifetime: ContextInjectionLifetime::Execution,
                    reason: "auto-load workspace project instruction".into(),
                },
            )?;
            if !matches!(response, ContextResponse::Loaded { .. }) {
                return Err(ApplicationError::InvalidResponse {
                    message: "context load returned an unexpected response".into(),
                });
            }
        }
        Ok(())
    }
    fn create_session(
        &mut self,
        request: SessionCreateInput,
    ) -> Result<SessionInfo, ApplicationError> {
        let response = self.invoke_session(SessionCommand::Allocate {
            working_directory: Some(request.working_directory),
            title: request.title,
        })?;
        let SessionResponse::Created { session } = response else {
            return Err(unexpected_session_response("create", response));
        };
        let info = application_session_info(&session)?;
        self.projection
            .insert_created(info.clone())
            .map_err(application_projection_error)?;
        Ok(info)
    }

    fn list_sessions(&mut self, request: PageInput) -> Result<SessionList, ApplicationError> {
        if request.cursor.is_some() {
            return Err(ApplicationError::InvalidInput {
                message: "session list does not issue cursors".to_owned(),
            });
        }
        let response = self.invoke_session(SessionCommand::List)?;
        let SessionResponse::Sessions { sessions } = response else {
            return Err(unexpected_session_response("list", response));
        };
        Ok(SessionList {
            sessions: sessions
                .into_iter()
                .filter_map(|session| application_session_info(&session).ok())
                .collect(),
            next_cursor: None,
        })
    }

    fn rename_session(
        &mut self,
        request: SessionRenameInput,
    ) -> Result<SessionInfo, ApplicationError> {
        self.require_open_application_session(&request.session_id)?;
        self.reserve_session_event_slot()?;
        let change = SessionChange::Renamed {
            title: request.title.clone(),
        };
        let response = self.invoke_session(SessionCommand::Transition {
            id: request.session_id.clone(),
            transition: SessionTransition::Rename {
                title: request.title,
            },
            journal: session_change_journal(&change),
        })?;
        let SessionResponse::Transitioned { session, journal } = response else {
            return Err(unexpected_session_response("rename", response));
        };
        let info = application_session_info(&session)?;
        self.project_journal_entry(info.clone(), journal)?;
        Ok(info)
    }

    fn close_session(
        &mut self,
        request: ApplicationSessionInput,
    ) -> Result<Acknowledged, ApplicationError> {
        self.require_open_application_session(&request.session_id)?;
        self.reserve_session_event_slot()?;
        let change = SessionChange::Closed;
        let response = self.invoke_session(SessionCommand::Transition {
            id: request.session_id.clone(),
            transition: SessionTransition::Close,
            journal: session_change_journal(&change),
        })?;
        let SessionResponse::Transitioned { session, journal } = response else {
            return Err(unexpected_session_response("close", response));
        };
        let info = application_session_info(&session)?;
        self.project_journal_entry(info, journal)?;
        Ok(Acknowledged {})
    }

    fn prompt(&mut self, request: PromptInput) -> Result<PromptResult, ApplicationError> {
        let authority = self.application_root_authority(&request.session_id)?;
        let root = {
            let harness = self.harness.lock();
            harness.root_execution_handle(&authority)
        };
        self.prompt_on(&root, request)
    }

    fn prompt_on(
        &mut self,
        root: &RootExecutionHandle,
        request: PromptInput,
    ) -> Result<PromptResult, ApplicationError> {
        let session = self.require_open_application_session_on(root, &request.session_id)?;
        let execution_id = self.allocate_root_execution_on(root)?;
        if let Err(error) = self.prepare_execution_context_on(
            root,
            &application_session_info(&session)?,
            &execution_id,
        ) {
            let _ = self.finish_root_execution_on(root, &execution_id, false);
            return Err(error);
        }
        if let Err(error) = self.append_session_change_on(
            root,
            &session,
            SessionChange::Message {
                message: Message {
                    role: MessageRole::User,
                    content: request.content,
                },
            },
        ) {
            let _ = self.finish_root_execution_on(root, &execution_id, false);
            return Err(error);
        }
        Ok(PromptResult {
            execution_id,
            stop_reason: StopReason::EndTurn,
        })
    }

    fn cancel(
        &mut self,
        request: ApplicationSessionInput,
    ) -> Result<Acknowledged, ApplicationError> {
        self.require_open_application_session(&request.session_id)?;
        Ok(Acknowledged {})
    }

    fn decide_review(
        &mut self,
        request: ReviewDecisionInput,
    ) -> Result<ReviewRecord, ApplicationError> {
        let command = ExecutionReviewCommand::Decide { input: request };
        let input = serde_json::to_vec(&PhenixValue::from(&command)).map_err(|error| {
            ApplicationError::InvalidInput {
                message: error.to_string(),
            }
        })?;
        let output = self
            .harness
            .lock()
            .invoke(&execution_review_service(), &input, &self.authority, None)
            .map_err(|error| ApplicationError::Failed {
                message: error.to_string(),
            })?;
        let output: PhenixValue =
            serde_json::from_slice(&output).map_err(|error| ApplicationError::InvalidResponse {
                message: error.to_string(),
            })?;
        let response = ExecutionReviewResponse::try_from(Project(&output)).map_err(|error| {
            ApplicationError::InvalidResponse {
                message: error.to_string(),
            }
        })?;
        match response {
            ExecutionReviewResponse::Review { review } => {
                let session = self.require_open_application_session(&review.session_id)?;
                self.append_session_change(
                    &session,
                    SessionChange::Review {
                        review: review.clone(),
                    },
                )?;
                Ok(review)
            }
            ExecutionReviewResponse::Conflict { message, .. } => {
                Err(ApplicationError::Conflict { message })
            }
            other => Err(ApplicationError::InvalidResponse {
                message: format!("unexpected execution review response: {other:?}"),
            }),
        }
    }

    fn append_session_change(
        &mut self,
        session: &SessionRecord,
        change: SessionChange,
    ) -> Result<SessionUpdate, ApplicationError> {
        self.reserve_session_event_slot()?;
        let response = self.invoke_session(SessionCommand::AppendJournal {
            id: session.id.clone(),
            entry: session_change_journal(&change),
        })?;
        let SessionResponse::JournalAppended { entry } = response else {
            return Err(unexpected_session_response("append journal", response));
        };
        self.project_journal_entry(application_session_info(session)?, entry)
    }

    #[cfg(test)]
    fn allocate_root_execution(&mut self) -> Result<String, ApplicationError> {
        let response = self.invoke_execution(ExecutionCommand::AllocateExecution {
            prefix: "execution-".to_owned(),
            requested_authority: ExecutionAuthority::new(
                default_application_root_authority()
                    .permissions()
                    .map(|capability| capability.as_str().to_owned()),
            ),
        })?;
        let ExecutionResponse::Execution { execution } = response else {
            return Err(ApplicationError::InvalidResponse {
                message: format!("unexpected execution allocation response: {response:?}"),
            });
        };
        let execution_id = execution.id;

        let response =
            match self.invoke_execution_resource(ExecutionResourceCommand::RegisterRootBudget {
                ledger: RootBudgetLedger {
                    root_execution_id: execution_id.clone(),
                    // Default roots have no cumulative lifetime token or turn ceiling.
                    // Explicit policy and intrinsic model capacities may still bound a request.
                    limits: RootBudgetLimits {
                        fresh_input_tokens: u64::MAX,
                        output_tokens: u64::MAX,
                        cost_microunits: None,
                        attempts: u32::MAX,
                    },
                    reservations: BTreeMap::new(),
                },
            }) {
                Ok(response) => response,
                Err(error) => {
                    let _ = self.finish_root_execution(&execution_id, false);
                    return Err(error);
                }
            };
        if !matches!(response, ExecutionResourceResponse::RootBudget { .. }) {
            let error = ApplicationError::InvalidResponse {
                message: format!("unexpected root-budget registration response: {response:?}"),
            };
            let _ = self.finish_root_execution(&execution_id, false);
            return Err(error);
        }
        Ok(execution_id)
    }

    #[cfg(test)]
    fn finish_root_execution(
        &mut self,
        execution_id: &str,
        success: bool,
    ) -> Result<(), ApplicationError> {
        let response = self.invoke_execution(ExecutionCommand::FinishExecution {
            id: execution_id.to_owned(),
            success,
        })?;
        if matches!(response, ExecutionResponse::Execution { .. }) {
            Ok(())
        } else {
            Err(ApplicationError::InvalidResponse {
                message: format!("unexpected execution completion response: {response:?}"),
            })
        }
    }

    #[cfg(test)]
    fn invoke_execution(
        &mut self,
        command: ExecutionCommand,
    ) -> Result<ExecutionResponse, ApplicationError> {
        self.invoke_runtime(execution_service(), command)
    }

    #[cfg(test)]
    fn invoke_execution_resource(
        &mut self,
        command: ExecutionResourceCommand,
    ) -> Result<ExecutionResourceResponse, ApplicationError> {
        self.invoke_runtime(execution_resource_service(), command)
    }

    #[cfg(test)]
    fn invoke_runtime<C, R>(
        &mut self,
        service: phenix_core::ServiceId,
        command: C,
    ) -> Result<R, ApplicationError>
    where
        for<'a> PhenixValue: From<&'a C>,
        R: for<'a> TryFrom<Project<&'a PhenixValue>, Error = phenix_core::ValueError>,
    {
        let input = serde_json::to_vec(&PhenixValue::from(&command)).map_err(|error| {
            ApplicationError::InvalidInput {
                message: error.to_string(),
            }
        })?;
        let output = self
            .harness
            .lock()
            .invoke(&service, &input, &self.authority, None)
            .map_err(|error| ApplicationError::Failed {
                message: error.to_string(),
            })?;
        let output: PhenixValue =
            serde_json::from_slice(&output).map_err(|error| ApplicationError::InvalidResponse {
                message: error.to_string(),
            })?;
        R::try_from(Project(&output)).map_err(|error| ApplicationError::InvalidResponse {
            message: error.to_string(),
        })
    }

    fn invoke_runtime_on<C, R>(
        &self,
        root: &RootExecutionHandle,
        service: phenix_core::ServiceId,
        command: C,
    ) -> Result<R, ApplicationError>
    where
        for<'a> PhenixValue: From<&'a C>,
        R: for<'a> TryFrom<Project<&'a PhenixValue>, Error = phenix_core::ValueError>,
    {
        let input = serde_json::to_vec(&PhenixValue::from(&command)).map_err(|error| {
            ApplicationError::InvalidInput {
                message: error.to_string(),
            }
        })?;
        let output =
            root.invoke(&service, &input, None)
                .map_err(|error| ApplicationError::Failed {
                    message: error.to_string(),
                })?;
        let output: PhenixValue =
            serde_json::from_slice(&output).map_err(|error| ApplicationError::InvalidResponse {
                message: error.to_string(),
            })?;
        R::try_from(Project(&output)).map_err(|error| ApplicationError::InvalidResponse {
            message: error.to_string(),
        })
    }

    fn invoke_session_on(
        &self,
        root: &RootExecutionHandle,
        command: SessionCommand,
    ) -> Result<SessionResponse, ApplicationError> {
        self.invoke_runtime_on(root, session_service(), command)
    }

    fn create_session_on(
        &mut self,
        root: &RootExecutionHandle,
        request: SessionCreateInput,
    ) -> Result<SessionInfo, ApplicationError> {
        let response = self.invoke_session_on(
            root,
            SessionCommand::Allocate {
                working_directory: Some(request.working_directory),
                title: request.title,
            },
        )?;
        let SessionResponse::Created { session } = response else {
            return Err(unexpected_session_response("create", response));
        };
        let info = application_session_info(&session)?;
        self.projection
            .insert_created(info.clone())
            .map_err(application_projection_error)?;
        Ok(info)
    }

    fn list_sessions_on(
        &self,
        root: &RootExecutionHandle,
        request: PageInput,
    ) -> Result<SessionList, ApplicationError> {
        if request.cursor.is_some() {
            return Err(ApplicationError::InvalidInput {
                message: "session list does not issue cursors".to_owned(),
            });
        }
        let response = self.invoke_session_on(root, SessionCommand::List)?;
        let SessionResponse::Sessions { sessions } = response else {
            return Err(unexpected_session_response("list", response));
        };
        Ok(SessionList {
            sessions: sessions
                .into_iter()
                .filter_map(|session| application_session_info(&session).ok())
                .collect(),
            next_cursor: None,
        })
    }

    fn rename_session_on(
        &mut self,
        root: &RootExecutionHandle,
        request: SessionRenameInput,
    ) -> Result<SessionInfo, ApplicationError> {
        self.require_open_application_session_on(root, &request.session_id)?;
        self.reserve_session_event_slot()?;
        let change = SessionChange::Renamed {
            title: request.title.clone(),
        };
        let response = self.invoke_session_on(
            root,
            SessionCommand::Transition {
                id: request.session_id.clone(),
                transition: SessionTransition::Rename {
                    title: request.title,
                },
                journal: session_change_journal(&change),
            },
        )?;
        let SessionResponse::Transitioned { session, journal } = response else {
            return Err(unexpected_session_response("rename", response));
        };
        let info = application_session_info(&session)?;
        self.project_journal_entry_on(root, info.clone(), journal)?;
        Ok(info)
    }

    fn close_session_on(
        &mut self,
        root: &RootExecutionHandle,
        request: ApplicationSessionInput,
    ) -> Result<Acknowledged, ApplicationError> {
        self.require_open_application_session_on(root, &request.session_id)?;
        self.reserve_session_event_slot()?;
        let change = SessionChange::Closed;
        let response = self.invoke_session_on(
            root,
            SessionCommand::Transition {
                id: request.session_id.clone(),
                transition: SessionTransition::Close,
                journal: session_change_journal(&change),
            },
        )?;
        let SessionResponse::Transitioned { session, journal } = response else {
            return Err(unexpected_session_response("close", response));
        };
        let info = application_session_info(&session)?;
        self.project_journal_entry_on(root, info, journal)?;
        Ok(Acknowledged {})
    }

    fn invoke_session_application_operation_on(
        &mut self,
        root: &RootExecutionHandle,
        operation: &ContractId,
        input: PhenixValue,
    ) -> Result<PhenixValue, ApplicationError> {
        match operation.as_str() {
            CreateSession::ID => self
                .create_session_on(root, decode(input)?)
                .map(|value| value.to_value()),
            ListSessions::ID => self
                .list_sessions_on(root, decode(input)?)
                .map(|value| value.to_value()),
            ResumeSession::ID => self
                .resume_session_on(root, decode(input)?)
                .map(|value| value.to_value()),
            RenameSession::ID => self
                .rename_session_on(root, decode(input)?)
                .map(|value| value.to_value()),
            CloseSession::ID => self
                .close_session_on(root, decode(input)?)
                .map(|value| value.to_value()),
            _ => Err(ApplicationError::UnsupportedCapability {
                capability: operation.clone(),
            }),
        }
    }

    fn invoke_execution_on(
        &self,
        root: &RootExecutionHandle,
        command: ExecutionCommand,
    ) -> Result<ExecutionResponse, ApplicationError> {
        self.invoke_runtime_on(root, execution_service(), command)
    }

    fn invoke_execution_resource_on(
        &self,
        root: &RootExecutionHandle,
        command: ExecutionResourceCommand,
    ) -> Result<ExecutionResourceResponse, ApplicationError> {
        self.invoke_runtime_on(root, execution_resource_service(), command)
    }

    fn invoke_option_command_on(
        &self,
        root: &RootExecutionHandle,
        command: OptionCommand,
    ) -> Result<OptionResponse, ApplicationError> {
        self.invoke_runtime_on(root, options_service(), command)
    }

    fn invoke_context_command_on(
        &self,
        root: &RootExecutionHandle,
        command: ContextCommand,
    ) -> Result<ContextResponse, ApplicationError> {
        self.invoke_runtime_on(root, context_service(), command)
    }

    fn invoke_workspace_command_on(
        &self,
        root: &RootExecutionHandle,
        command: WorkspaceCommand,
    ) -> Result<WorkspaceResponse, ApplicationError> {
        self.invoke_runtime_on(root, workspace_service(), command)
    }

    fn session_record_on(
        &self,
        root: &RootExecutionHandle,
        id: &SessionId,
    ) -> Result<Option<SessionRecord>, ApplicationError> {
        let response = self.invoke_session_on(root, SessionCommand::Get { id: id.clone() })?;
        let SessionResponse::Session { session } = response else {
            return Err(unexpected_session_response("get", response));
        };
        Ok(session)
    }

    fn require_open_application_session_on(
        &self,
        root: &RootExecutionHandle,
        id: &SessionId,
    ) -> Result<SessionRecord, ApplicationError> {
        let session =
            self.session_record_on(root, id)?
                .ok_or_else(|| ApplicationError::NotFound {
                    resource: id.to_string(),
                })?;
        application_session_info(&session)?;
        if session.lifecycle != SessionLifecycle::Open {
            return Err(ApplicationError::Conflict {
                message: format!("session is closed: {id}"),
            });
        }
        Ok(session)
    }

    fn load_session_snapshot_on(
        &self,
        root: &RootExecutionHandle,
        request: SessionResumeInput,
    ) -> Result<SessionSnapshot, ApplicationError> {
        let session = self
            .session_record_on(root, &request.session_id)?
            .ok_or_else(|| ApplicationError::NotFound {
                resource: request.session_id.to_string(),
            })?;
        let session = application_session_info(&session)?;
        let response = self.invoke_session_on(
            root,
            SessionCommand::Journal {
                id: request.session_id.clone(),
                stream: session_change_stream(),
                after_sequence: request.after_sequence,
            },
        )?;
        let SessionResponse::Journal {
            through_sequence,
            entries,
        } = response
        else {
            return Err(unexpected_session_response("journal", response));
        };
        if request
            .after_sequence
            .is_some_and(|sequence| sequence > through_sequence)
        {
            return Err(ApplicationError::Conflict {
                message: format!(
                    "resume sequence for {} is ahead of runtime watermark {through_sequence}",
                    request.session_id
                ),
            });
        }
        let mut expected = request.after_sequence.unwrap_or(0).saturating_add(1);
        let mut updates = Vec::with_capacity(entries.len());
        for entry in entries {
            if entry.sequence != expected {
                return Err(ApplicationError::InvalidResponse {
                    message: format!(
                        "session journal gap for {}: expected {expected}, got {}",
                        request.session_id, entry.sequence
                    ),
                });
            }
            expected = expected
                .checked_add(1)
                .ok_or_else(|| ApplicationError::Failed {
                    message: "session journal sequence overflow".to_owned(),
                })?;
            updates.push(session_update_from_journal(&request.session_id, entry)?);
        }
        Ok(SessionSnapshot {
            session,
            through_sequence,
            updates,
        })
    }

    fn resume_session_on(
        &mut self,
        root: &RootExecutionHandle,
        request: SessionResumeInput,
    ) -> Result<SessionSnapshot, ApplicationError> {
        let replace_projection = request.after_sequence.is_none();
        let snapshot = self.load_session_snapshot_on(root, request)?;
        if replace_projection {
            self.projection
                .replace_snapshot(snapshot.clone())
                .map_err(application_projection_error)?;
        }
        Ok(snapshot)
    }

    fn project_journal_entry_on(
        &mut self,
        root: &RootExecutionHandle,
        session: SessionInfo,
        journal: SessionJournalEntry,
    ) -> Result<SessionUpdate, ApplicationError> {
        let update = session_update_from_journal(&session.session_id, journal)?;
        let contiguous = self
            .projection
            .state()
            .sessions
            .get(session.session_id.as_str())
            .and_then(|projection| projection.through_sequence.checked_add(1))
            == Some(update.sequence);
        if contiguous {
            self.projection
                .apply_update(update.clone())
                .map_err(application_projection_error)?;
        } else {
            let snapshot = self.load_session_snapshot_on(
                root,
                SessionResumeInput {
                    session_id: session.session_id,
                    after_sequence: None,
                },
            )?;
            self.projection
                .repair_with_snapshot(snapshot, [update.clone()])
                .map_err(application_projection_error)?;
        }
        self.emit_session_update(update.clone())?;
        Ok(update)
    }

    fn append_session_change_on(
        &mut self,
        root: &RootExecutionHandle,
        session: &SessionRecord,
        change: SessionChange,
    ) -> Result<SessionUpdate, ApplicationError> {
        self.reserve_session_event_slot()?;
        let response = self.invoke_session_on(
            root,
            SessionCommand::AppendJournal {
                id: session.id.clone(),
                entry: session_change_journal(&change),
            },
        )?;
        let SessionResponse::JournalAppended { entry } = response else {
            return Err(unexpected_session_response("append journal", response));
        };
        self.project_journal_entry_on(root, application_session_info(session)?, entry)
    }

    fn append_execution_change_on(
        &mut self,
        root: &RootExecutionHandle,
        session_id: &SessionId,
        execution_id: &str,
        update: ExecutionChange,
    ) -> Result<SessionUpdate, ApplicationError> {
        let session = self.require_open_application_session_on(root, session_id)?;
        self.append_session_change_on(
            root,
            &session,
            SessionChange::Execution {
                execution_id: execution_id.to_owned(),
                update,
            },
        )
    }

    fn allocate_root_execution_on(
        &mut self,
        root: &RootExecutionHandle,
    ) -> Result<String, ApplicationError> {
        let response = self.invoke_execution_on(
            root,
            ExecutionCommand::AllocateExecution {
                prefix: "execution-".to_owned(),
                requested_authority: ExecutionAuthority::new(
                    root.authority()
                        .permissions()
                        .map(|capability| capability.as_str().to_owned()),
                ),
            },
        )?;
        let ExecutionResponse::Execution { execution } = response else {
            return Err(ApplicationError::InvalidResponse {
                message: format!("unexpected execution allocation response: {response:?}"),
            });
        };
        let execution_id = execution.id;

        let response = match self.invoke_execution_resource_on(
            root,
            ExecutionResourceCommand::RegisterRootBudget {
                ledger: RootBudgetLedger {
                    root_execution_id: execution_id.clone(),
                    limits: RootBudgetLimits {
                        fresh_input_tokens: u64::MAX,
                        output_tokens: u64::MAX,
                        cost_microunits: None,
                        attempts: u32::MAX,
                    },
                    reservations: BTreeMap::new(),
                },
            },
        ) {
            Ok(response) => response,
            Err(error) => {
                let _ = self.finish_root_execution_on(root, &execution_id, false);
                return Err(error);
            }
        };
        if !matches!(response, ExecutionResourceResponse::RootBudget { .. }) {
            let error = ApplicationError::InvalidResponse {
                message: format!("unexpected root-budget registration response: {response:?}"),
            };
            let _ = self.finish_root_execution_on(root, &execution_id, false);
            return Err(error);
        }
        Ok(execution_id)
    }

    fn finish_root_execution_on(
        &self,
        root: &RootExecutionHandle,
        execution_id: &str,
        success: bool,
    ) -> Result<(), ApplicationError> {
        let response = self.invoke_execution_on(
            root,
            ExecutionCommand::FinishExecution {
                id: execution_id.to_owned(),
                success,
            },
        )?;
        if matches!(response, ExecutionResponse::Execution { .. }) {
            Ok(())
        } else {
            Err(ApplicationError::InvalidResponse {
                message: format!("unexpected execution completion response: {response:?}"),
            })
        }
    }

    fn require_open_application_session(
        &mut self,
        id: &SessionId,
    ) -> Result<SessionRecord, ApplicationError> {
        let session = self
            .session_record(id)?
            .ok_or_else(|| ApplicationError::NotFound {
                resource: id.to_string(),
            })?;
        application_session_info(&session)?;
        if session.lifecycle != SessionLifecycle::Open {
            return Err(ApplicationError::Conflict {
                message: format!("session is closed: {id}"),
            });
        }
        Ok(session)
    }

    fn session_record(
        &mut self,
        id: &SessionId,
    ) -> Result<Option<SessionRecord>, ApplicationError> {
        let response = self.invoke_session(SessionCommand::Get { id: id.clone() })?;
        let SessionResponse::Session { session } = response else {
            return Err(unexpected_session_response("get", response));
        };
        Ok(session)
    }

    fn resume_session(
        &mut self,
        request: SessionResumeInput,
    ) -> Result<SessionSnapshot, ApplicationError> {
        let replace_projection = request.after_sequence.is_none();
        let snapshot = self.load_session_snapshot(request)?;
        if replace_projection {
            self.projection
                .replace_snapshot(snapshot.clone())
                .map_err(application_projection_error)?;
        }
        Ok(snapshot)
    }

    fn load_session_snapshot(
        &mut self,
        request: SessionResumeInput,
    ) -> Result<SessionSnapshot, ApplicationError> {
        let session = self.session_record(&request.session_id)?.ok_or_else(|| {
            ApplicationError::NotFound {
                resource: request.session_id.to_string(),
            }
        })?;
        let session = application_session_info(&session)?;
        let response = self.invoke_session(SessionCommand::Journal {
            id: request.session_id.clone(),
            stream: session_change_stream(),
            after_sequence: request.after_sequence,
        })?;
        let SessionResponse::Journal {
            through_sequence,
            entries,
        } = response
        else {
            return Err(unexpected_session_response("journal", response));
        };
        if request
            .after_sequence
            .is_some_and(|sequence| sequence > through_sequence)
        {
            return Err(ApplicationError::Conflict {
                message: format!(
                    "resume sequence for {} is ahead of runtime watermark {through_sequence}",
                    request.session_id
                ),
            });
        }
        let mut expected = request.after_sequence.unwrap_or(0).saturating_add(1);
        let mut updates = Vec::with_capacity(entries.len());
        for entry in entries {
            if entry.sequence != expected {
                return Err(ApplicationError::InvalidResponse {
                    message: format!(
                        "session journal gap for {}: expected {expected}, got {}",
                        request.session_id, entry.sequence
                    ),
                });
            }
            expected = expected
                .checked_add(1)
                .ok_or_else(|| ApplicationError::Failed {
                    message: "session journal sequence overflow".to_owned(),
                })?;
            updates.push(session_update_from_journal(&request.session_id, entry)?);
        }
        Ok(SessionSnapshot {
            session,
            through_sequence,
            updates,
        })
    }

    fn project_journal_entry(
        &mut self,
        session: SessionInfo,
        journal: SessionJournalEntry,
    ) -> Result<SessionUpdate, ApplicationError> {
        let update = session_update_from_journal(&session.session_id, journal)?;
        let contiguous = self
            .projection
            .state()
            .sessions
            .get(session.session_id.as_str())
            .and_then(|projection| projection.through_sequence.checked_add(1))
            == Some(update.sequence);
        if contiguous {
            self.projection
                .apply_update(update.clone())
                .map_err(application_projection_error)?;
        } else {
            let snapshot = self.load_session_snapshot(SessionResumeInput {
                session_id: session.session_id,
                after_sequence: None,
            })?;
            self.projection
                .repair_with_snapshot(snapshot, [update.clone()])
                .map_err(application_projection_error)?;
        }
        self.emit_session_update(update.clone())?;
        Ok(update)
    }

    fn reserve_session_event_slot(&self) -> Result<(), ApplicationError> {
        let Some(sender) = &self.event_sender else {
            return Ok(());
        };
        if sender.is_closed() {
            return Err(ApplicationError::Disconnected);
        }
        if sender.capacity() == 0 {
            return Err(ApplicationError::Conflict {
                message: "application event queue is full".to_owned(),
            });
        }
        Ok(())
    }

    fn emit_session_update(&self, update: SessionUpdate) -> Result<(), ApplicationError> {
        let Some(sender) = &self.event_sender else {
            return Ok(());
        };
        sender
            .try_send(ApplicationEvent {
                event: ContractId::parse("phenix.application.session-update@1")
                    .expect("static session update event id is valid"),
                payload: update.to_value(),
            })
            .map_err(|error| match error {
                mpsc::error::TrySendError::Full(_) => ApplicationError::Conflict {
                    message: "application event queue is full".to_owned(),
                },
                mpsc::error::TrySendError::Closed(_) => ApplicationError::Disconnected,
            })
    }

    fn discover_authentication(
        &self,
        _request: Empty,
    ) -> Result<AuthenticationMethods, ApplicationError> {
        let providers = self.provider_auth_plugins();
        let mut methods = Vec::new();
        for provider in providers {
            let authenticated = self.provider_authenticated(&provider)?;
            let response =
                self.invoke_provider_auth(&provider, ProviderAuthCommand::InteractiveMethods)?;
            let ProviderAuthResponse::InteractiveMethods {
                methods: provider_methods,
            } = response
            else {
                return Err(ApplicationError::InvalidResponse {
                    message: format!(
                        "provider {provider} returned an unexpected interactive-auth response"
                    ),
                });
            };
            for method in provider_methods {
                let kind = match method.kind {
                    AuthKind::ApiToken => "api_token",
                    AuthKind::OAuth => "oauth",
                };
                methods.push(AuthenticationMethod {
                    id: authentication_method_id(&provider, &method.id)?,
                    provider: provider.clone(),
                    provider_name: method.provider_name,
                    authenticated,
                    kind: kind.to_owned(),
                    name: method.name,
                    description: method.description,
                });
            }
        }
        methods.sort_by(|left, right| left.id.cmp(&right.id));
        Ok(AuthenticationMethods { methods })
    }

    fn authenticate(
        &self,
        request: AuthenticateInput,
    ) -> Result<AuthenticationResult, ApplicationError> {
        let (provider, method) = parse_authentication_method_id(&request.method_id)?;
        let available = match self
            .invoke_provider_auth(&provider, ProviderAuthCommand::InteractiveMethods)?
        {
            ProviderAuthResponse::InteractiveMethods { methods } => methods,
            response => {
                return Err(ApplicationError::InvalidResponse {
                    message: format!(
                        "provider {provider} returned an unexpected interactive-auth response: {response:?}"
                    ),
                });
            }
        };
        let descriptor = available
            .into_iter()
            .find(|candidate| candidate.id == method)
            .ok_or_else(|| ApplicationError::InvalidInput {
                message: format!(
                    "provider {provider} does not expose authentication method {method:?}"
                ),
            })?;

        if descriptor.kind == AuthKind::ApiToken {
            let secret = request
                .secret
                .ok_or_else(|| ApplicationError::InvalidInput {
                    message: format!(
                    "authentication method {method:?} for provider {provider} requires an API key"
                ),
                })?;
            let source = auth::ApiToken::literal(secret).map_err(|error| {
                ApplicationError::InvalidInput {
                    message: format!("invalid API key for provider {provider}: {error}"),
                }
            })?;
            let response = self.invoke_provider_auth(
                &provider,
                ProviderAuthCommand::Add {
                    auth: Auth::ApiToken { source },
                },
            )?;
            if !matches!(response, ProviderAuthResponse::Added { .. }) {
                return Err(ApplicationError::InvalidResponse {
                    message: format!(
                        "provider {provider} returned an unexpected credential-add response: {response:?}"
                    ),
                });
            }
            let _ = self.refresh_provider_model_catalog(&provider);
            return Ok(AuthenticationResult::Authenticated);
        }

        if request.secret.is_some() {
            return Err(ApplicationError::InvalidInput {
                message: format!(
                    "authentication method {method:?} for provider {provider} does not accept a secret"
                ),
            });
        }

        let response =
            self.invoke_provider_auth(&provider, ProviderAuthCommand::Authenticate { method })?;
        let ProviderAuthResponse::Authentication { authentication } = response else {
            return Err(ApplicationError::InvalidResponse {
                message: format!(
                    "provider {provider} returned an unexpected authentication response"
                ),
            });
        };
        match authentication {
            ProviderAuthenticationResult::Authenticated => {
                let _ = self.refresh_provider_model_catalog(&provider);
                Ok(AuthenticationResult::Authenticated)
            }
            ProviderAuthenticationResult::External { uri, instructions } => {
                Ok(AuthenticationResult::External { uri, instructions })
            }
        }
    }

    fn provider_model_plugins(&self) -> Vec<PluginId> {
        let service = provider_models_service();
        let harness = self.harness.lock();
        let mut providers = harness
            .kernel()
            .config()
            .manifests()
            .filter(|manifest| {
                manifest
                    .services
                    .iter()
                    .any(|contribution| contribution.service == service)
            })
            .map(|manifest| manifest.id.clone())
            .collect::<Vec<_>>();
        providers.sort();
        providers.dedup();
        providers
    }

    fn invoke_provider_models(
        &self,
        provider: &PluginId,
        command: ProviderModelsCommand,
    ) -> Result<ProviderModelsResponse, ApplicationError> {
        let input =
            serde_json::to_vec(&command).map_err(|error| ApplicationError::InvalidInput {
                message: error.to_string(),
            })?;
        let output = self
            .harness
            .lock()
            .invoke(
                &provider_models_service(),
                &input,
                &self.authority,
                Some(provider),
            )
            .map_err(|error| ApplicationError::Failed {
                message: error.to_string(),
            })?;
        serde_json::from_slice(&output).map_err(|error| ApplicationError::InvalidResponse {
            message: error.to_string(),
        })
    }

    fn refresh_provider_model_catalogs(
        &self,
        provider_authentication: &BTreeMap<PluginId, bool>,
    ) -> BTreeMap<PluginId, String> {
        let mut provider_names = BTreeMap::new();
        for provider in self.provider_model_plugins() {
            if provider_authentication
                .get(&provider)
                .copied()
                .unwrap_or(true)
                && let Ok(provider_name) = self.refresh_provider_model_catalog(&provider)
            {
                provider_names.insert(provider, provider_name);
            }
        }
        provider_names
    }

    fn refresh_provider_model_catalog(
        &self,
        provider: &PluginId,
    ) -> Result<String, ApplicationError> {
        let response = self.invoke_provider_models(provider, ProviderModelsCommand::List)?;
        let ProviderModelsResponse::Models {
            provider_name,
            models,
        } = response;
        let mut profiles = Vec::new();
        for model in models {
            let base_target = ModelTarget {
                provider_plugin: provider.clone(),
                model: model.id,
                options: BTreeMap::new(),
            };
            profiles.push(direct_provider_model_profile(base_target.clone())?);
            for effort in model.thinking {
                let mut target = base_target.clone();
                target.options.insert(
                    "inference".to_owned(),
                    PhenixValue::Map(BTreeMap::from([(
                        "effort".to_owned(),
                        PhenixValue::String(effort),
                    )])),
                );
                profiles.push(direct_provider_model_profile(target)?);
            }
        }

        let published =
            self.invoke_model_command(ModelCommand::PublishProviderCatalogProfiles {
                provider_plugin: provider.clone(),
                profiles: profiles.clone(),
            })?;
        if !matches!(published, ModelResponse::Profiles { .. }) {
            return Err(ApplicationError::InvalidResponse {
                message: format!(
                    "model routing returned an unexpected provider catalog publication response: {published:?}"
                ),
            });
        }

        let mut harness = self.harness.lock();
        for profile in profiles {
            publish_routing_profile_runtime_state(&mut harness, &profile).map_err(|error| {
                ApplicationError::Failed {
                    message: format!(
                        "cannot publish runtime state for provider catalog route {}: {error}",
                        profile.id
                    ),
                }
            })?;
        }
        Ok(provider_name)
    }

    fn provider_auth_plugins(&self) -> Vec<PluginId> {
        let service = provider_auth_service();
        let harness = self.harness.lock();
        let mut providers = harness
            .kernel()
            .config()
            .manifests()
            .filter(|manifest| {
                manifest
                    .services
                    .iter()
                    .any(|contribution| contribution.service == service)
            })
            .map(|manifest| manifest.id.clone())
            .collect::<Vec<_>>();
        providers.sort();
        providers.dedup();
        providers
    }

    fn invoke_provider_auth(
        &self,
        provider: &PluginId,
        command: ProviderAuthCommand,
    ) -> Result<ProviderAuthResponse, ApplicationError> {
        let input =
            serde_json::to_vec(&command).map_err(|error| ApplicationError::InvalidInput {
                message: error.to_string(),
            })?;
        let output = self
            .harness
            .lock()
            .invoke(
                &provider_auth_service(),
                &input,
                &self.authority,
                Some(provider),
            )
            .map_err(|error| ApplicationError::Failed {
                message: error.to_string(),
            })?;
        serde_json::from_slice(&output).map_err(|error| ApplicationError::InvalidResponse {
            message: error.to_string(),
        })
    }

    fn invoke_session(
        &mut self,
        command: SessionCommand,
    ) -> Result<SessionResponse, ApplicationError> {
        let input = serde_json::to_vec(&PhenixValue::from(&command)).map_err(|error| {
            ApplicationError::InvalidInput {
                message: error.to_string(),
            }
        })?;
        let output = self
            .harness
            .lock()
            .invoke(&session_service(), &input, &self.authority, None)
            .map_err(|error| ApplicationError::Failed {
                message: error.to_string(),
            })?;
        let output: PhenixValue =
            serde_json::from_slice(&output).map_err(|error| ApplicationError::InvalidResponse {
                message: error.to_string(),
            })?;
        SessionResponse::try_from(Project(&output)).map_err(|error| {
            ApplicationError::InvalidResponse {
                message: error.to_string(),
            }
        })
    }
}

fn configured_log_reader() -> Result<StructuredLogReader, String> {
    let sink = if let Some(spec) =
        env::var_os("PHENIX_DEBUG_LOG").filter(|value| !value.as_os_str().is_empty())
    {
        LogSink::parse(&spec.to_string_lossy())
    } else {
        LogSink::from_env().map(|sink| sink.unwrap_or_else(LogSink::default_local))
    }?;
    StructuredLogReader::configured(sink)
}

fn log_record_matches(
    value: &serde_json::Value,
    session_id: Option<&SessionId>,
    execution_id: Option<&str>,
) -> bool {
    session_id
        .is_none_or(|session_id| log_json_contains_field(value, "session_id", session_id.as_str()))
        && execution_id
            .is_none_or(|execution_id| log_json_contains_field(value, "execution_id", execution_id))
}

fn log_json_contains_field(value: &serde_json::Value, key: &str, expected: &str) -> bool {
    match value {
        serde_json::Value::Array(values) => values
            .iter()
            .any(|value| log_json_contains_field(value, key, expected)),
        serde_json::Value::Object(values) => {
            values
                .get(key)
                .and_then(serde_json::Value::as_str)
                .is_some_and(|value| value == expected)
                || values
                    .values()
                    .any(|value| log_json_contains_field(value, key, expected))
        }
        _ => false,
    }
}

fn application_log_record(
    cursor: u64,
    value: serde_json::Value,
) -> Result<LogRecord, ApplicationError> {
    let object = value
        .as_object()
        .ok_or_else(|| ApplicationError::InvalidResponse {
            message: "structured log record must be an object".to_owned(),
        })?;
    let timestamp_ms = object
        .get("timestamp_ms")
        .and_then(serde_json::Value::as_u64)
        .ok_or_else(|| ApplicationError::InvalidResponse {
            message: "structured log record is missing timestamp_ms".to_owned(),
        })?;
    let pid = object
        .get("pid")
        .and_then(serde_json::Value::as_u64)
        .ok_or_else(|| ApplicationError::InvalidResponse {
            message: "structured log record is missing pid".to_owned(),
        })?;
    let kind = object
        .get("kind")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| ApplicationError::InvalidResponse {
            message: "structured log record is missing kind".to_owned(),
        })?
        .to_owned();
    let payload = json_value_to_phenix(
        object
            .get("payload")
            .cloned()
            .unwrap_or(serde_json::Value::Null),
    )?;
    Ok(LogRecord {
        cursor: cursor.to_string(),
        timestamp_ms,
        pid,
        kind,
        payload,
    })
}

fn json_value_to_phenix(value: serde_json::Value) -> Result<PhenixValue, ApplicationError> {
    Ok(match value {
        serde_json::Value::Null => PhenixValue::Option(None),
        serde_json::Value::Bool(value) => PhenixValue::Bool(value),
        serde_json::Value::Number(value) => {
            if let Some(value) = value.as_i64() {
                PhenixValue::I64(value)
            } else if let Some(value) = value.as_u64() {
                PhenixValue::U64(value)
            } else if let Some(value) = value.as_f64() {
                PhenixValue::F64(value)
            } else {
                return Err(ApplicationError::InvalidResponse {
                    message: "structured log number cannot be represented".to_owned(),
                });
            }
        }
        serde_json::Value::String(value) => PhenixValue::String(value),
        serde_json::Value::Array(values) => PhenixValue::List(
            values
                .into_iter()
                .map(json_value_to_phenix)
                .collect::<Result<Vec<_>, _>>()?,
        ),
        serde_json::Value::Object(values) => {
            let is_reference = values.len() == 4
                && ["digest", "media_type", "bytes", "locator"]
                    .iter()
                    .all(|key| values.contains_key(*key));
            if is_reference {
                let reference = serde_json::from_value::<ContentReference>(
                    serde_json::Value::Object(values.clone()),
                )
                .map_err(|error| ApplicationError::InvalidResponse {
                    message: format!("invalid log content reference: {error}"),
                })?;
                PhenixValue::from(&reference)
            } else {
                PhenixValue::Map(
                    values
                        .into_iter()
                        .map(|(key, value)| json_value_to_phenix(value).map(|value| (key, value)))
                        .collect::<Result<BTreeMap<_, _>, _>>()?,
                )
            }
        }
    })
}

fn authentication_method_id(provider: &PluginId, method: &str) -> Result<String, ApplicationError> {
    if method.is_empty() {
        return Err(ApplicationError::InvalidResponse {
            message: format!("provider {provider} exposed an empty authentication method id"),
        });
    }
    serde_json::to_string(&(provider.as_str(), method)).map_err(|error| {
        ApplicationError::InvalidResponse {
            message: error.to_string(),
        }
    })
}

fn parse_authentication_method_id(value: &str) -> Result<(PluginId, String), ApplicationError> {
    let (provider, method): (String, String) =
        serde_json::from_str(value).map_err(|error| ApplicationError::InvalidInput {
            message: format!("invalid authentication method id: {error}"),
        })?;
    if method.is_empty() {
        return Err(ApplicationError::InvalidInput {
            message: "authentication method id contains an empty provider method".to_owned(),
        });
    }
    let provider = PluginId::parse(provider).map_err(|error| ApplicationError::InvalidInput {
        message: format!("invalid authentication provider id: {error}"),
    })?;
    Ok((provider, method))
}

fn direct_provider_model_profile(target: ModelTarget) -> Result<RoutingProfile, ApplicationError> {
    let encoded =
        serde_json::to_vec(&target).map_err(|error| ApplicationError::InvalidResponse {
            message: format!("cannot encode provider model target: {error}"),
        })?;
    let digest = Sha256::digest(encoded);
    let suffix = digest[..8]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    let id = RoutingProfileId::parse(format!(
        "model.{}.{}.{}",
        target.provider_plugin, target.model, suffix
    ))
    .map_err(|error| ApplicationError::InvalidResponse {
        message: format!("provider model target cannot form a direct route: {error}"),
    })?;
    Ok(RoutingProfile {
        id,
        default_target: target,
        fallback_targets: Vec::new(),
        callable_targets: BTreeMap::new(),
    })
}

fn model_default_option() -> OptionKey {
    OptionKey::parse("model.default").expect("static model.default option key is valid")
}

fn selection_info(
    profile: &RoutingProfile,
    authenticated: bool,
    provider_names: &BTreeMap<PluginId, String>,
) -> Result<SelectionInfo, ApplicationError> {
    let mut targets = BTreeMap::new();
    for target in std::iter::once(&profile.default_target)
        .chain(profile.fallback_targets.iter())
        .chain(profile.callable_targets.values())
    {
        let key =
            serde_json::to_string(target).map_err(|error| ApplicationError::InvalidResponse {
                message: error.to_string(),
            })?;
        targets.entry(key).or_insert(target);
    }

    if targets.len() == 1 {
        let target = targets
            .into_values()
            .next()
            .expect("one routing target was counted");
        let thinking = model_selection_thinking(target);
        return Ok(SelectionInfo {
            id: profile.id.clone(),
            provider: target.provider_plugin.clone(),
            provider_name: provider_names
                .get(&target.provider_plugin)
                .cloned()
                .unwrap_or_else(|| target.provider_plugin.to_string()),
            model: Some(target.model.to_string()),
            thinking,
            authenticated,
            name: target.model.to_string(),
            description: Some(model_selection_description(target)),
            presentation: SelectionPresentation::Model,
        });
    }

    let providers = profile.default_target.provider_plugin.to_string();
    Ok(SelectionInfo {
        id: profile.id.clone(),
        provider: profile.default_target.provider_plugin.clone(),
        provider_name: provider_names
            .get(&profile.default_target.provider_plugin)
            .cloned()
            .unwrap_or_else(|| profile.default_target.provider_plugin.to_string()),
        model: None,
        thinking: None,
        authenticated,
        name: profile.id.to_string(),
        description: Some(providers),
        presentation: SelectionPresentation::Router,
    })
}

fn model_selection_thinking(target: &phenix_sdk::ModelTarget) -> Option<String> {
    let Some(PhenixValue::Map(inference)) = target.options.get("inference") else {
        return None;
    };
    let Some(PhenixValue::String(effort)) = inference.get("effort") else {
        return None;
    };
    (!effort.is_empty()).then(|| effort.clone())
}

fn model_selection_description(target: &phenix_sdk::ModelTarget) -> String {
    let mut details = vec![target.provider_plugin.to_string()];
    if let Some(effort) = model_selection_thinking(target) {
        details.push(format!("effort {effort}"));
    }
    details.join(" · ")
}

fn selection_presentation_rank(presentation: &SelectionPresentation) -> u8 {
    match presentation {
        SelectionPresentation::Router => 0,
        SelectionPresentation::Model => 1,
    }
}

fn session_change_stream() -> ContractId {
    SessionChange::contract_id()
}

fn session_change_journal(change: &SessionChange) -> SessionJournalDraft {
    SessionJournalDraft {
        stream: session_change_stream(),
        payload: change.to_value(),
    }
}

fn session_update_from_journal(
    session_id: &SessionId,
    entry: SessionJournalEntry,
) -> Result<SessionUpdate, ApplicationError> {
    let expected = session_change_stream();
    if entry.stream != expected {
        return Err(ApplicationError::InvalidResponse {
            message: format!(
                "unexpected session journal stream: expected {expected}, got {}",
                entry.stream
            ),
        });
    }
    let update = SessionChange::from_value(&entry.payload).map_err(|error| {
        ApplicationError::InvalidResponse {
            message: error.to_string(),
        }
    })?;
    Ok(SessionUpdate {
        session_id: session_id.clone(),
        sequence: entry.sequence,
        update,
    })
}

fn decode<T: ValueCodec>(value: PhenixValue) -> Result<T, ApplicationError> {
    T::from_value(&value).map_err(|error| ApplicationError::InvalidInput {
        message: error.to_string(),
    })
}

fn observe_local_workspace_discovery(session: &SessionInfo) {
    let Some(discovery_root) = workspace_discovery::workspace_discovery_root() else {
        return;
    };
    let Ok(canonical_root) = fs::canonicalize(&session.working_directory) else {
        return;
    };
    if !canonical_root.is_dir() {
        return;
    }
    let workspace_id = workspace_context_id(&canonical_root.to_string_lossy());
    let terms = workspace_recall_terms(&canonical_root);
    let observed_at = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or_default();
    let _ = workspace_discovery::observe_local_workspace(
        &discovery_root,
        &workspace_id,
        &canonical_root,
        Vec::<String>::new(),
        terms,
        observed_at,
    );
}

fn workspace_recall_terms(root: &Path) -> BTreeSet<String> {
    root.file_name()
        .and_then(|name| name.to_str())
        .into_iter()
        .flat_map(|name| {
            name.split(|character: char| !character.is_alphanumeric())
                .map(str::to_lowercase)
                .collect::<Vec<_>>()
        })
        .filter(|term| !term.is_empty())
        .filter(|term| term.len() <= workspace_discovery::MAX_WORKSPACE_DISCOVERY_TERM_BYTES)
        .collect()
}

fn discover_workspaces(
    request: ApplicationWorkspaceDiscoveryRequest,
) -> Result<ApplicationWorkspaceDiscoveryResponse, String> {
    let query = workspace_discovery::WorkspaceDiscoveryQuery {
        workspace_ids: request
            .workspace_ids
            .into_iter()
            .filter(|value| !value.trim().is_empty())
            .collect(),
        repository_remotes: request
            .repository_remotes
            .into_iter()
            .filter(|value| !value.trim().is_empty())
            .collect(),
        recall_terms: request
            .recall_terms
            .into_iter()
            .map(|value| value.trim().to_lowercase())
            .filter(|value| !value.is_empty())
            .filter(|value| value.len() <= workspace_discovery::MAX_WORKSPACE_DISCOVERY_TERM_BYTES)
            .collect(),
    };
    let Some(root) = workspace_discovery::workspace_discovery_root() else {
        return Ok(ApplicationWorkspaceDiscoveryResponse {
            candidates: Vec::new(),
            scanned_descriptors: 0,
            invalid_descriptors: 0,
            complete: true,
            reason: None,
        });
    };
    let read = workspace_discovery::read_workspace_descriptors(&root)
        .map_err(|error| format!("workspace discovery read failed: {error}"))?;
    let scan = workspace_discovery::scan_workspace_descriptors(read.descriptors, &query);
    let mut reason = match scan.completeness {
        workspace_discovery::WorkspaceDiscoveryCompleteness::Complete => None,
        workspace_discovery::WorkspaceDiscoveryCompleteness::Incomplete { reason } => Some(reason),
    };
    if read.truncated {
        reason = Some(format!(
            "workspace descriptor read exceeded {} entries",
            workspace_discovery::MAX_WORKSPACE_DISCOVERY_SCAN
        ));
    }
    let candidates = scan
        .candidates
        .into_iter()
        .take(workspace_discovery::MAX_WORKSPACE_DISCOVERY_PREPARED_CANDIDATES)
        .map(|candidate| {
            let (matched_by, matched_terms) = match candidate.evidence {
                workspace_discovery::WorkspaceDiscoveryMatch::ExactWorkspaceId => {
                    ("workspace_id".to_owned(), 0)
                }
                workspace_discovery::WorkspaceDiscoveryMatch::ExactRepositoryRemote => {
                    ("repository_remote".to_owned(), 0)
                }
                workspace_discovery::WorkspaceDiscoveryMatch::LexicalTerms { matched } => {
                    ("recall_terms".to_owned(), matched)
                }
            };
            ApplicationWorkspaceDiscoveryCandidate {
                workspace_id: candidate.descriptor.workspace_id,
                canonical_root: candidate.descriptor.canonical_root.display().to_string(),
                repository_remotes: candidate
                    .descriptor
                    .repository_remotes
                    .into_iter()
                    .collect(),
                matched_by,
                matched_terms,
                confirmed_recoveries: candidate.descriptor.confirmed_recoveries,
                observation_count: candidate.descriptor.observation_count,
                last_observed_at: candidate.descriptor.last_observed_at,
            }
        })
        .collect();

    Ok(ApplicationWorkspaceDiscoveryResponse {
        candidates,
        scanned_descriptors: read.scanned_entries,
        invalid_descriptors: read
            .invalid_descriptors
            .saturating_add(scan.invalid_descriptors),
        complete: reason.is_none(),
        reason,
    })
}

// Keep repository context identity stable across sessions that use the same workspace.
fn workspace_context_id(working_directory: &str) -> String {
    let digest = Sha256::digest(working_directory.as_bytes());
    let suffix = digest
        .iter()
        .take(12)
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    format!("workspace-{suffix}")
}

fn application_session_info(session: &SessionRecord) -> Result<SessionInfo, ApplicationError> {
    let Some(working_directory) = session.working_directory.clone() else {
        return Err(ApplicationError::NotFound {
            resource: session.id.to_string(),
        });
    };
    Ok(SessionInfo {
        session_id: session.id.clone(),
        title: session.title.clone(),
        working_directory,
    })
}

fn unexpected_session_response(operation: &str, response: SessionResponse) -> ApplicationError {
    ApplicationError::InvalidResponse {
        message: format!("unexpected session {operation} response: {response:?}"),
    }
}

fn application_projection_error(error: SessionProjectionStoreError) -> ApplicationError {
    match error {
        SessionProjectionStoreError::Observable(error) => error.into(),
        SessionProjectionStoreError::Projection(error) => ApplicationError::Conflict {
            message: error.to_string(),
        },
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ConfiguredApplicationError {
    #[error(transparent)]
    Harness(#[from] crate::PhenixRuntimeBuildError),
    #[error(transparent)]
    Kernel(#[from] phenix_core::KernelError),
    #[error(transparent)]
    Observable(#[from] ObservableError),
    #[error(transparent)]
    Sdk(#[from] phenix_core::SdkResolutionError),
    #[error("configured application startup failed: {message}")]
    Configuration { message: String },
    #[error("ACP stdio server failed: {message}")]
    Stdio { message: String },
}

pub async fn serve_default_application() -> Result<(), ConfiguredApplicationError> {
    let state = configured_state_path()?;
    if let Some(parent) = state.parent() {
        fs::create_dir_all(parent).map_err(|error| ConfiguredApplicationError::Configuration {
            message: error.to_string(),
        })?;
    }
    let persistence = LocalPersistence::open(state).map_err(|error| {
        ConfiguredApplicationError::Configuration {
            message: error.to_string(),
        }
    })?;
    let mut harness = PhenixRuntime::default_suite_with_persistence(persistence)?;
    harness.activate()?;
    apply_application_configuration(&mut harness)?;
    serve_configured_application(harness).await
}

fn configured_state_path() -> Result<PathBuf, ConfiguredApplicationError> {
    if let Some(path) = env::var_os("PHENIX_STATE_DB") {
        return Ok(PathBuf::from(path));
    }
    if let Some(directory) = env::var_os("PHENIX_STATE_DIR") {
        return Ok(PathBuf::from(directory).join("acp.sqlite"));
    }
    if let Some(state_home) = env::var_os("XDG_STATE_HOME") {
        return Ok(PathBuf::from(state_home).join("phenix/acp.sqlite"));
    }
    if let Some(home) = env::var_os("HOME") {
        return Ok(PathBuf::from(home).join(".local/state/phenix/acp.sqlite"));
    }
    Err(ConfiguredApplicationError::Configuration {
        message:
            "cannot determine durable state path; set PHENIX_STATE_DB, PHENIX_STATE_DIR, or XDG_STATE_HOME"
                .to_owned(),
    })
}

fn apply_application_configuration(
    harness: &mut PhenixRuntime,
) -> Result<(), ConfiguredApplicationError> {
    if let Some(path) = env::var_os("PHENIX_DEFAULT_CONFIG_DIR") {
        crate::runtime_config::apply_default_config_directory(harness, Path::new(&path)).map_err(
            |error| ConfiguredApplicationError::Configuration {
                message: error.to_string(),
            },
        )?;
    }

    let config_directory = env::var_os("PHENIX_CONFIG_DIR").map(PathBuf::from);
    let nix_settings = env::var_os("PHENIX_NIX_SETTINGS").map(PathBuf::from);
    if config_directory.is_some() || nix_settings.is_some() {
        let precedence = match env::var("PHENIX_SETTINGS_PRECEDENCE") {
            Ok(value) if value == "file" => OptionStartupPrecedence::File,
            Ok(value) if value == "nix" => OptionStartupPrecedence::Nix,
            Err(env::VarError::NotPresent) => OptionStartupPrecedence::Nix,
            Ok(value) => {
                return Err(ConfiguredApplicationError::Configuration {
                    message: format!("invalid PHENIX_SETTINGS_PRECEDENCE: {value}"),
                });
            }
            Err(error) => {
                return Err(ConfiguredApplicationError::Configuration {
                    message: error.to_string(),
                });
            }
        };
        crate::runtime_config::apply_startup_settings(
            harness,
            config_directory.as_deref(),
            nix_settings.as_deref(),
            precedence,
        )
        .map_err(|error| ConfiguredApplicationError::Configuration {
            message: error.to_string(),
        })?;
    }
    Ok(())
}

pub async fn serve_configured_application(
    harness: PhenixRuntime,
) -> Result<(), ConfiguredApplicationError> {
    let sdk = harness
        .resolved_generation()
        .resolve_sdk_contributions([sdk_contribution()])?;
    let (event_sender, event_receiver) =
        mpsc::channel::<ApplicationEvent>(APPLICATION_EVENT_CAPACITY);
    let worker = ApplicationWorker::new(harness)?.with_event_sender(event_sender);
    let runtime = PluginRuntimeId::parse("phenix.application-runtime")
        .expect("static application runtime id is valid");
    let generation = {
        let harness = worker.harness.lock();
        ReferenceGenerationId::from(harness.generation())
    };
    // The process owner supplies one connection identity pair. Standalone stdio
    // callers retain the conventional first-connection identity.
    let (client_id, client_generation) = match (
        env::var("PHENIX_ACP_CLIENT_ID"),
        env::var("PHENIX_ACP_CLIENT_GENERATION"),
    ) {
        (Ok(id), Ok(generation)) => (id, generation),
        (Err(env::VarError::NotPresent), Err(env::VarError::NotPresent)) =>
            ("lua-client-1".to_owned(), "connection-1".to_owned()),
        _ => return Err(ConfiguredApplicationError::Configuration {
            message: "PHENIX_ACP_CLIENT_ID and PHENIX_ACP_CLIENT_GENERATION must both be valid strings or both be absent".to_owned(),
        }),
    };
    let client = ClientReferenceIdentity::new(
        ClientConnectionId::parse(client_id).map_err(|error| {
            ConfiguredApplicationError::Configuration {
                message: error.to_string(),
            }
        })?,
        ReferenceGenerationId::parse(client_generation).map_err(|error| {
            ConfiguredApplicationError::Configuration {
                message: error.to_string(),
            }
        })?,
    );
    let (client_callbacks, callback_receiver) =
        ClientCallableCallbacks::bounded(CLIENT_CALLABLE_CAPACITY);
    let service = SdkApplicationService::new(
        &sdk,
        worker.projection().store(),
        SharedCallableRegistry::default(),
        runtime,
        generation,
        client_callbacks,
        client,
    )?;
    let (transport, receiver) = ChannelTransport::new(APPLICATION_INVOCATION_CAPACITY);
    let worker = serve_application_worker(worker, service.clone(), transport.clone(), receiver);
    let stdio = serve_stdio_with_events_and_callbacks(
        transport,
        configured_capabilities(),
        event_receiver,
        callback_receiver,
    );
    let (stdio, ()) = tokio::join!(stdio, worker);
    stdio.map_err(|error| ConfiguredApplicationError::Stdio {
        message: error.to_string(),
    })
}

struct ActiveExecution {
    execution_id: String,
    root: RootExecutionHandle,
    cancellation: Arc<AtomicBool>,
    progress_error: Option<ApplicationError>,
    prompt: ApplicationInvocation,
}

struct ExecutionProgress {
    session_id: SessionId,
    execution_id: String,
    update: SessionUpdate,
}

struct ExecutionCompletion {
    session_id: SessionId,
    execution_id: String,
    result: Result<String, ApplicationError>,
}

enum ExecutionWorkerEvent {
    Progress(ExecutionProgress),
    Complete(ExecutionCompletion),
}

#[derive(Clone)]
struct ApplicationAgentToolRun {
    service: SdkApplicationService,
    control_transport: WeakChannelTransport,
    harness: Weak<Mutex<PhenixRuntime>>,
    session_id: SessionId,
    execution_id: String,
    root_generation: GenerationId,
    root_constraints: RootExecutionConstraints,
    permission_handler: Option<PermissionHandlerRef>,
    tools: Vec<ModelToolDescriptor>,
    runtime_entry_triggers: BTreeMap<CallableId, ComponentEntryTrigger>,
    cancellation: Arc<AtomicBool>,
    progress_sender: mpsc::Sender<ExecutionWorkerEvent>,
}

#[derive(Clone, Default)]
pub(crate) struct ApplicationAgentToolRegistry(
    Arc<Mutex<BTreeMap<String, ApplicationAgentToolRun>>>,
);

impl ApplicationAgentToolRegistry {
    fn register(
        &self,
        execution_id: String,
        run: ApplicationAgentToolRun,
    ) -> Result<(), ApplicationError> {
        let mut runs = self.0.lock();
        if runs.contains_key(&execution_id) {
            return Err(ApplicationError::Conflict {
                message: format!("agent tool adapter already has execution {execution_id}"),
            });
        }
        runs.insert(execution_id, run);
        Ok(())
    }

    fn remove(&self, execution_id: &str) {
        self.0.lock().remove(execution_id);
    }

    fn get(&self, execution_id: &str) -> Result<ApplicationAgentToolRun, String> {
        self.0
            .lock()
            .get(execution_id)
            .cloned()
            .ok_or_else(|| format!("agent tool adapter has no execution {execution_id}"))
    }
}

#[must_use]
pub(crate) fn application_agent_tool_manifest(maximum_authority: Authority) -> PluginManifest {
    PluginManifest {
        id: PluginId::parse(APPLICATION_AGENT_TOOL_PLUGIN)
            .expect("static application agent tool plugin id is valid"),
        version: 1,
        execution: PluginExecution::Embedded,
        dependencies: vec![
            PluginId::parse("phenix.sessions").expect("static session plugin id is valid"),
        ],
        services: vec![
            ServiceContribution {
                role: ServiceRole::Terminal,
                service: agent_loop_control_service(),
                priority: 100,
                required_authority: Authority::default(),
            },
            ServiceContribution {
                role: ServiceRole::Terminal,
                service: agent_tool_execution_service(),
                priority: 100,
                required_authority: Authority::default(),
            },
            ServiceContribution {
                role: ServiceRole::Terminal,
                service: agent_loop_progress_service(),
                priority: 100,
                required_authority: agent_loop_progress_authority(),
            },
        ],
        resource_namespaces: Vec::new(),
        maximum_authority,
    }
}

fn application_agent_tool_component_id() -> ComponentId {
    ComponentId::parse(APPLICATION_AGENT_TOOL_COMPONENT)
        .expect("static application agent tool component id is valid")
}

#[must_use]
pub(crate) fn application_agent_tool_component_manifest(
    maximum_authority: Authority,
) -> ComponentManifest {
    ComponentManifest {
        listeners: Vec::new(),
        id: application_agent_tool_component_id(),
        owner: PluginId::parse(APPLICATION_AGENT_TOOL_PLUGIN)
            .expect("static application agent tool plugin id is valid"),
        imports: vec![
            ComponentImport {
                interface: WorkspaceInterface::interface_id(),
                schema: WorkspaceInterface::schema(),
                required: false,
                authority: application_workspace_authority(),
            },
            ComponentImport {
                interface: ExecutionInspectionInterface::interface_id(),
                schema: ExecutionInspectionInterface::schema(),
                required: false,
                authority: Authority::new([PermissionId::parse("kernel.persistence.read")
                    .expect("static persistence read permission is valid")]),
            },
            ComponentImport {
                interface: SessionInterface::interface_id(),
                schema: SessionInterface::schema(),
                required: true,
                authority: agent_loop_progress_authority(),
            },
            ComponentImport {
                interface: LanguageInterface::interface_id(),
                schema: LanguageInterface::schema(),
                required: false,
                authority: application_code_query_authority(),
            },
            ComponentImport {
                interface: MemoryInterface::interface_id(),
                schema: MemoryInterface::schema(),
                required: false,
                authority: application_memory_authority(),
            },
        ],
        exports: vec![
            ComponentExport {
                interface: ApplicationShellToolInterface::interface_id(),
                schema: ApplicationShellToolInterface::schema(),
                priority: 100,
                required_authority: application_shell_authority(),
            },
            ComponentExport {
                interface: ApplicationWorkspaceReadToolInterface::interface_id(),
                schema: ApplicationWorkspaceReadToolInterface::schema(),
                priority: 100,
                required_authority: application_workspace_read_authority(),
            },
            ComponentExport {
                interface: ApplicationWorkspaceSearchToolInterface::interface_id(),
                schema: ApplicationWorkspaceSearchToolInterface::schema(),
                priority: 100,
                required_authority: application_workspace_read_authority(),
            },
            ComponentExport {
                interface: ApplicationWorkspaceWriteToolInterface::interface_id(),
                schema: ApplicationWorkspaceWriteToolInterface::schema(),
                priority: 100,
                required_authority: application_workspace_write_authority(),
            },
            ComponentExport {
                interface: ApplicationWorkspaceGitToolInterface::interface_id(),
                schema: ApplicationWorkspaceGitToolInterface::schema(),
                priority: 100,
                required_authority: application_workspace_git_authority(),
            },
            ComponentExport {
                interface: ApplicationWorkspaceDiscoveryToolInterface::interface_id(),
                schema: ApplicationWorkspaceDiscoveryToolInterface::schema(),
                priority: 100,
                required_authority: Authority::default(),
            },
            ComponentExport {
                interface: ApplicationCodeQueryToolInterface::interface_id(),
                schema: ApplicationCodeQueryToolInterface::schema(),
                priority: 100,
                required_authority: Authority::default(),
            },
            ComponentExport {
                interface: ApplicationMemoryRecordToolInterface::interface_id(),
                schema: ApplicationMemoryRecordToolInterface::schema(),
                priority: 100,
                required_authority: Authority::default(),
            },
            ComponentExport {
                interface: ApplicationMemoryRecallToolInterface::interface_id(),
                schema: ApplicationMemoryRecallToolInterface::schema(),
                priority: 100,
                required_authority: Authority::default(),
            },
            ComponentExport {
                interface: AgentLoopControlInterface::interface_id(),
                schema: AgentLoopControlInterface::schema(),
                priority: 100,
                required_authority: Authority::default(),
            },
            ComponentExport {
                interface: AgentToolExecutionInterface::interface_id(),
                schema: AgentToolExecutionInterface::schema(),
                priority: 100,
                required_authority: Authority::default(),
            },
            ComponentExport {
                interface: AgentLoopProgressInterface::interface_id(),
                schema: AgentLoopProgressInterface::schema(),
                priority: 100,
                required_authority: agent_loop_progress_authority(),
            },
        ],
        maximum_authority,
    }
}

#[must_use]
pub(crate) fn application_agent_tool_factory(
    registry: ApplicationAgentToolRegistry,
) -> Box<dyn PluginInstance> {
    Box::new(ApplicationAgentToolPlugin { registry })
}

struct ApplicationAgentToolSdk<'host, 'runtime> {
    workspace: SdkClient<'host, 'runtime, WorkspaceInterface>,
    execution: SdkClient<'host, 'runtime, ExecutionInspectionInterface>,
    sessions: SdkClient<'host, 'runtime, SessionInterface>,
    language: SdkClient<'host, 'runtime, LanguageInterface>,
    memory: SdkClient<'host, 'runtime, MemoryInterface>,
}

type ApplicationAgentToolContext<'host, 'runtime> =
    PluginContext<'host, 'runtime, ApplicationAgentToolSdk<'host, 'runtime>>;

fn application_agent_tool_context<'host, 'runtime>(
    host: &'host PluginHost<'runtime>,
) -> ApplicationAgentToolContext<'host, 'runtime> {
    PluginContext::new(
        host,
        ApplicationAgentToolSdk {
            workspace: SdkClient::new(host, application_agent_tool_component_id()),
            execution: SdkClient::new(host, application_agent_tool_component_id()),
            sessions: SdkClient::new(host, application_agent_tool_component_id()),
            language: SdkClient::new(host, application_agent_tool_component_id()),
            memory: SdkClient::new(host, application_agent_tool_component_id()),
        },
        (),
        (),
    )
}

struct ApplicationAgentToolPlugin {
    registry: ApplicationAgentToolRegistry,
}

struct ApplicationAgentToolInvocation {
    registry: ApplicationAgentToolRegistry,
}

impl PluginInstance for ApplicationAgentToolPlugin {
    fn start(&mut self, _host: &PluginHost<'_>) -> Result<(), String> {
        Ok(())
    }

    fn shared_invocation(&self) -> Option<Arc<dyn SharedPluginInvocation>> {
        Some(Arc::new(ApplicationAgentToolInvocation {
            registry: self.registry.clone(),
        }))
    }
}

impl SharedPluginInvocation for ApplicationAgentToolInvocation {
    fn invoke(
        &self,
        service: &ServiceId,
        input: &[u8],
        host: &PluginHost<'_>,
    ) -> Result<Vec<u8>, String> {
        let context = application_agent_tool_context(host);
        if service.as_str() == APPLICATION_SHELL_TOOL_SERVICE {
            let request = context
                .kernel
                .decode_projected::<ApplicationShellToolRequest>(
                    &ApplicationShellToolInterface::interface_id(),
                    input,
                )
                .map_err(|error| error.to_string())?;
            if request.command.trim().is_empty() {
                return Err("bash tool command must be a non-empty string".into());
            }
            let response = context
                .sdk
                .workspace
                .invoke_projected::<WorkspaceCommand, WorkspaceResponse>(&WorkspaceCommand::Shell {
                    command: request.command,
                })
                .map_err(|error| error.to_string())?;
            return context
                .kernel
                .encode_value(&response)
                .map_err(|error| error.to_string());
        }
        if service.as_str() == APPLICATION_WORKSPACE_READ_TOOL_SERVICE {
            let request = context
                .kernel
                .decode_projected::<ApplicationWorkspaceReadToolRequest>(
                    &ApplicationWorkspaceReadToolInterface::interface_id(),
                    input,
                )
                .map_err(|error| error.to_string())?;
            let response = context
                .sdk
                .workspace
                .invoke_projected::<WorkspaceCommand, WorkspaceResponse>(&WorkspaceCommand::Read {
                    path: request.path,
                })
                .map_err(|error| error.to_string())?;
            return context
                .kernel
                .encode_value(&response)
                .map_err(|error| error.to_string());
        }
        if service.as_str() == APPLICATION_WORKSPACE_SEARCH_TOOL_SERVICE {
            let request = context
                .kernel
                .decode_projected::<ApplicationWorkspaceSearchToolRequest>(
                    &ApplicationWorkspaceSearchToolInterface::interface_id(),
                    input,
                )
                .map_err(|error| error.to_string())?;
            let response = context
                .sdk
                .workspace
                .invoke_projected::<WorkspaceCommand, WorkspaceResponse>(
                    &WorkspaceCommand::Search {
                        needle: request.needle,
                        path: request.path,
                        case_sensitive: request.case_sensitive,
                    },
                )
                .map_err(|error| error.to_string())?;
            return context
                .kernel
                .encode_value(&response)
                .map_err(|error| error.to_string());
        }
        if service.as_str() == APPLICATION_WORKSPACE_WRITE_TOOL_SERVICE {
            let request = context
                .kernel
                .decode_projected::<ApplicationWorkspaceWriteToolRequest>(
                    &ApplicationWorkspaceWriteToolInterface::interface_id(),
                    input,
                )
                .map_err(|error| error.to_string())?;
            let expected_version = match request.expected_content_hash {
                Some(content_hash) => WorkspaceFileVersion::Present { content_hash },
                None => WorkspaceFileVersion::Absent,
            };
            let response = context
                .sdk
                .workspace
                .invoke_projected::<WorkspaceCommand, WorkspaceResponse>(&WorkspaceCommand::Write {
                    path: request.path,
                    content: request.content,
                    expected_version,
                })
                .map_err(|error| error.to_string())?;
            return context
                .kernel
                .encode_value(&response)
                .map_err(|error| error.to_string());
        }
        if service.as_str() == APPLICATION_WORKSPACE_GIT_TOOL_SERVICE {
            let request = context
                .kernel
                .decode_projected::<ApplicationWorkspaceGitToolRequest>(
                    &ApplicationWorkspaceGitToolInterface::interface_id(),
                    input,
                )
                .map_err(|error| error.to_string())?;
            let response = context
                .sdk
                .workspace
                .invoke_projected::<WorkspaceCommand, WorkspaceResponse>(&WorkspaceCommand::Git {
                    arguments: request.arguments,
                })
                .map_err(|error| error.to_string())?;
            return context
                .kernel
                .encode_value(&response)
                .map_err(|error| error.to_string());
        }
        if service.as_str() == APPLICATION_WORKSPACE_DISCOVERY_TOOL_SERVICE {
            let request = context
                .kernel
                .decode_projected::<ApplicationWorkspaceDiscoveryRequest>(
                    &ApplicationWorkspaceDiscoveryToolInterface::interface_id(),
                    input,
                )
                .map_err(|error| error.to_string())?;
            let response = discover_workspaces(request)?;
            return context
                .kernel
                .encode_value(&response)
                .map_err(|error| error.to_string());
        }
        if service.as_str() == APPLICATION_CODE_QUERY_TOOL_SERVICE {
            let query = context
                .kernel
                .decode_projected::<CodeQuery>(
                    &ApplicationCodeQueryToolInterface::interface_id(),
                    input,
                )
                .map_err(|error| error.to_string())?;
            let response = context
                .sdk
                .language
                .invoke_projected::<LanguageCommand, LanguageResponse>(&LanguageCommand::Query {
                    query,
                })
                .map_err(|error| error.to_string())?;
            let LanguageResponse::Query { result } = response else {
                return Err("language service returned a non-query response".into());
            };
            return context
                .kernel
                .encode_value(&result)
                .map_err(|error| error.to_string());
        }
        if service.as_str() == APPLICATION_MEMORY_RECORD_TOOL_SERVICE {
            let record = context
                .kernel
                .decode_projected::<MemoryRecord>(
                    &ApplicationMemoryRecordToolInterface::interface_id(),
                    input,
                )
                .map_err(|error| error.to_string())?;
            let response = context
                .sdk
                .memory
                .invoke_projected::<MemoryCommand, MemoryResponse>(&MemoryCommand::Record {
                    record,
                })
                .map_err(|error| error.to_string())?;
            let MemoryResponse::Record { record } = response else {
                return Err("memory service returned a non-record response".into());
            };
            return context
                .kernel
                .encode_value(&record)
                .map_err(|error| error.to_string());
        }
        if service.as_str() == APPLICATION_MEMORY_RECALL_TOOL_SERVICE {
            let query = context
                .kernel
                .decode_projected::<MemoryRecallQuery>(
                    &ApplicationMemoryRecallToolInterface::interface_id(),
                    input,
                )
                .map_err(|error| error.to_string())?;
            let response = context
                .sdk
                .memory
                .invoke_projected::<MemoryCommand, MemoryResponse>(&MemoryCommand::Recall { query })
                .map_err(|error| error.to_string())?;
            let MemoryResponse::Recall { records } = response else {
                return Err("memory service returned a non-recall response".into());
            };
            return context
                .kernel
                .encode_value(&ApplicationMemoryRecallResponse { records })
                .map_err(|error| error.to_string());
        }
        if service == &agent_loop_control_service() {
            let request = context
                .kernel
                .decode_projected::<AgentLoopControlRequest>(
                    &AgentLoopControlInterface::interface_id(),
                    input,
                )
                .map_err(|error| error.to_string())?;
            let response = check_application_agent_control(&self.registry, request)?;
            return context
                .kernel
                .encode_value(&response)
                .map_err(|error| error.to_string());
        }
        if service == &agent_tool_execution_service() {
            let request = context
                .kernel
                .decode_projected::<AgentToolExecutionRequest>(
                    &AgentToolExecutionInterface::interface_id(),
                    input,
                )
                .map_err(|error| error.to_string())?;
            let response = execute_application_agent_tool(&context, &self.registry, request)?;
            return context
                .kernel
                .encode_value(&response)
                .map_err(|error| error.to_string());
        }
        if service == &agent_loop_progress_service() {
            let record = context
                .kernel
                .decode_projected::<AgentLoopProgressRecord>(
                    &AgentLoopProgressInterface::interface_id(),
                    input,
                )
                .map_err(|error| error.to_string())?;
            let response = record_application_agent_progress(&context, &self.registry, record)?;
            return context
                .kernel
                .encode_value(&response)
                .map_err(|error| error.to_string());
        }
        Err(format!(
            "unsupported application agent tool service: {service}"
        ))
    }
}

fn check_application_agent_control(
    registry: &ApplicationAgentToolRegistry,
    request: AgentLoopControlRequest,
) -> Result<AgentLoopControlResponse, String> {
    let run = registry.get(&request.execution_id)?;
    if request.session_id.as_ref() != Some(&run.session_id) {
        return Err("agent loop control session identity changed".into());
    }
    if run.cancellation.load(Ordering::Acquire) {
        Ok(AgentLoopControlResponse::Cancelled)
    } else {
        Ok(AgentLoopControlResponse::Continue)
    }
}

fn execute_application_agent_tool(
    context: &ApplicationAgentToolContext<'_, '_>,
    registry: &ApplicationAgentToolRegistry,
    request: AgentToolExecutionRequest,
) -> Result<AgentToolExecutionResponse, String> {
    let run = registry.get(&request.execution_id)?;
    if request.session_id.as_ref() != Some(&run.session_id) {
        return Err("agent tool execution session identity changed".into());
    }
    if run.cancellation.load(Ordering::Acquire) {
        return Ok(AgentToolExecutionResponse::Cancelled);
    }

    let call = request.call;
    let dispatch_call = match normalize_model_tool_call(&run.tools, &call) {
        Ok(call) => call,
        Err(error) => {
            return Ok(AgentToolExecutionResponse::Completed {
                result: ModelToolResult {
                    call_id: call.call_id,
                    callable_id: call.callable_id,
                    output: error.to_value(),
                    is_error: true,
                },
                activated_tools: Vec::new(),
                observation: None,
            });
        }
    };
    let change = if let Some(trigger) = run.runtime_entry_triggers.get(&dispatch_call.callable_id) {
        execute_runtime_entry_trigger(context, trigger, &dispatch_call)
    } else if dispatch_call.callable_id.as_str() == "phenix.inspect" {
        execute_runtime_inspect_tool_call(context, &run, &dispatch_call)
    } else if dispatch_call.callable_id.as_str() == "phenix.session" {
        execute_runtime_session_tool_call(context, &run, &dispatch_call)
    } else if dispatch_call.callable_id.as_str() == "phenix.plugin" {
        execute_runtime_plugin_tool_call(context, &run, &dispatch_call)
    } else {
        execute_admitted_client_tool_call(
            &run.service,
            &run.session_id,
            &request.execution_id,
            dispatch_call,
            |request| {
                invoke_permission_handler(&run.service, run.permission_handler.as_ref(), request)
            },
        )
    };

    if run.cancellation.load(Ordering::Acquire) {
        return Ok(AgentToolExecutionResponse::Cancelled);
    }

    let result = match change {
        ExecutionChange::ToolResult { call_id, output } => ModelToolResult {
            call_id,
            callable_id: call.callable_id,
            output,
            is_error: false,
        },
        ExecutionChange::ToolFailed { call_id, error } => {
            if matches!(error, ApplicationError::Cancelled) {
                return Ok(AgentToolExecutionResponse::Cancelled);
            }
            ModelToolResult {
                call_id,
                callable_id: call.callable_id,
                output: error.to_value(),
                is_error: true,
            }
        }
        _ => return Err("tool executor returned a non-terminal tool change".into()),
    };

    Ok(AgentToolExecutionResponse::Completed {
        result,
        activated_tools: Vec::new(),
        observation: None,
    })
}

fn record_application_agent_progress(
    context: &ApplicationAgentToolContext<'_, '_>,
    registry: &ApplicationAgentToolRegistry,
    record: AgentLoopProgressRecord,
) -> Result<AgentLoopProgressResponse, String> {
    let run = registry.get(&record.execution_id)?;
    if record.session_id.as_ref() != Some(&run.session_id) {
        return Err("agent progress session identity changed".into());
    }
    if run.cancellation.load(Ordering::Acquire) {
        return Ok(AgentLoopProgressResponse::Recorded);
    }

    let change = match record.progress {
        AgentLoopProgress::ToolCall { call } => ExecutionChange::ToolCall {
            call_id: call.call_id,
            callable_id: call.callable_id,
            input: call.input,
        },
        AgentLoopProgress::ToolResult { result } if !result.is_error => {
            ExecutionChange::ToolResult {
                call_id: result.call_id,
                output: result.output,
            }
        }
        AgentLoopProgress::ToolResult { result } => {
            let error = ApplicationError::from_value(&result.output).unwrap_or_else(|decode| {
                ApplicationError::Failed {
                    message: format!(
                        "tool {} failed with a non-application error payload: {decode}",
                        result.callable_id
                    ),
                }
            });
            ExecutionChange::ToolFailed {
                call_id: result.call_id,
                error,
            }
        }
    };

    let response: SessionResponse = context
        .sdk
        .sessions
        .invoke_projected(&SessionCommand::AppendJournal {
            id: run.session_id.clone(),
            entry: session_change_journal(&SessionChange::Execution {
                execution_id: record.execution_id.clone(),
                update: change,
            }),
        })
        .map_err(|error| format!("failed to persist agent progress: {error}"))?;
    let SessionResponse::JournalAppended { entry } = response else {
        return Err("session service returned a non-journal progress response".into());
    };
    let update = session_update_from_journal(&run.session_id, entry)
        .map_err(|error| format!("failed to project persisted agent progress: {error:?}"))?;

    run.progress_sender
        .blocking_send(ExecutionWorkerEvent::Progress(ExecutionProgress {
            session_id: run.session_id,
            execution_id: record.execution_id,
            update,
        }))
        .map_err(|_| "application execution progress channel disconnected".to_owned())?;

    Ok(AgentLoopProgressResponse::Recorded)
}

async fn serve_application_worker(
    worker: ApplicationWorker,
    service: SdkApplicationService,
    control_transport: ChannelTransport,
    receiver: mpsc::Receiver<ApplicationInvocation>,
) {
    serve_application_worker_with_execution_capacity(
        worker,
        service,
        control_transport,
        receiver,
        APPLICATION_EXECUTION_CAPACITY,
    )
    .await;
}

async fn serve_application_worker_with_execution_capacity(
    mut worker: ApplicationWorker,
    service: SdkApplicationService,
    control_transport: ChannelTransport,
    mut receiver: mpsc::Receiver<ApplicationInvocation>,
    execution_capacity: usize,
) {
    let weak_control_transport = control_transport.downgrade();
    drop(control_transport);

    let (execution_sender, mut execution_events) =
        mpsc::channel::<ExecutionWorkerEvent>(execution_capacity);
    let mut active = BTreeMap::<String, ActiveExecution>::new();
    let mut deferred = VecDeque::<ApplicationInvocation>::new();
    let mut input_closed = false;

    loop {
        if input_closed && active.is_empty() && deferred.is_empty() {
            break;
        }
        if active.is_empty()
            && let Some(invocation) = deferred.pop_front()
        {
            dispatch_application_invocation(
                &mut worker,
                &service,
                &execution_sender,
                &weak_control_transport,
                &mut active,
                invocation,
            );
            continue;
        }
        tokio::select! {
            invocation = receiver.recv(), if !input_closed => {
                let Some(invocation) = invocation else {
                    input_closed = true;
                    continue;
                };
                if should_defer_application_invocation(&active, &invocation) {
                    deferred.push_back(invocation);
                    continue;
                }
                dispatch_application_invocation(
                    &mut worker,
                    &service,
                    &execution_sender,
                    &weak_control_transport,
                    &mut active,
                    invocation,
                );
            }
            event = execution_events.recv(), if !active.is_empty() => {
                if let Some(event) = event {
                    match event {
                        ExecutionWorkerEvent::Progress(progress) => {
                            handle_execution_progress(&mut worker, &mut active, progress);
                        }
                        ExecutionWorkerEvent::Complete(completion) => {
                            finish_prompt(&mut worker, &mut active, completion);
                        }
                    }
                }
            }
        }
    }

    for (_, execution) in active {
        execution.cancellation.store(true, Ordering::Release);
        execution
            .prompt
            .respond(Err(ApplicationError::Disconnected));
    }
    worker.clear_interaction_handlers();
    service.retire_client();
}

fn should_defer_application_invocation(
    active: &BTreeMap<String, ActiveExecution>,
    invocation: &ApplicationInvocation,
) -> bool {
    if active.is_empty() || is_sdk_operation(&invocation.operation) {
        return false;
    }

    match invocation.operation.as_str() {
        Cancel::ID => decode::<ApplicationSessionInput>(invocation.input.clone())
            .is_ok_and(|request| !active.contains_key(request.session_id.as_str())),
        Prompt::ID => false,
        CreateSession::ID | ListSessions::ID | ResumeSession::ID | RenameSession::ID => false,
        CloseSession::ID => decode::<ApplicationSessionInput>(invocation.input.clone())
            .is_ok_and(|request| active.contains_key(request.session_id.as_str())),
        _ => true,
    }
}

fn dispatch_application_invocation(
    worker: &mut ApplicationWorker,
    service: &SdkApplicationService,
    execution_sender: &mpsc::Sender<ExecutionWorkerEvent>,
    control_transport: &WeakChannelTransport,
    active: &mut BTreeMap<String, ActiveExecution>,
    invocation: ApplicationInvocation,
) {
    if invocation.operation.as_str() == Prompt::ID {
        start_prompt(
            worker,
            service,
            execution_sender,
            control_transport,
            active,
            invocation,
        );
        return;
    }
    if invocation.operation.as_str() == Cancel::ID {
        cancel_prompt(worker, active, invocation);
        return;
    }

    let operation = invocation.operation.clone();
    let input = invocation.input.clone();
    let selected_root = invocation.root.as_ref().map(|selection| {
        let harness = worker.harness.lock();
        harness
            .root_execution_handle_in_generation(&selection.generation, &selection.constraints)
            .map_err(|error| ApplicationError::Failed {
                message: error.to_string(),
            })
    });
    let result = if let Some(root) = selected_root {
        root.and_then(|root| {
            worker.invoke_session_application_operation_on(&root, &operation, input)
        })
    } else if is_sdk_operation(&operation) {
        service.invoke(&operation, input)
    } else {
        worker.invoke_with_client_callables(&operation, input, |callable, schema| {
            service.admit_current_client_callable(callable, schema)
        })
    };
    invocation.respond(result);
}

fn start_prompt(
    worker: &mut ApplicationWorker,
    service: &SdkApplicationService,
    execution_sender: &mpsc::Sender<ExecutionWorkerEvent>,
    control_transport: &WeakChannelTransport,
    active: &mut BTreeMap<String, ActiveExecution>,
    invocation: ApplicationInvocation,
) {
    let request = match decode::<PromptInput>(invocation.input.clone()) {
        Ok(request) => request,
        Err(error) => {
            invocation.respond(Err(error));
            return;
        }
    };
    let key = request.session_id.as_str().to_owned();
    if active.contains_key(&key) {
        invocation.respond(Err(ApplicationError::Conflict {
            message: format!(
                "session {} already has a running execution",
                request.session_id
            ),
        }));
        return;
    }

    let default_authority = if invocation.root.is_none() {
        match worker.application_root_authority(&request.session_id) {
            Ok(authority) => Some(authority),
            Err(error) => {
                invocation.respond(Err(error));
                return;
            }
        }
    } else {
        None
    };
    let root = {
        let harness = worker.harness.lock();
        match invocation.root.as_ref() {
            Some(selection) => harness
                .root_execution_handle_in_generation(&selection.generation, &selection.constraints)
                .map_err(|error| ApplicationError::Failed {
                    message: error.to_string(),
                }),
            None => Ok(harness.root_execution_handle(
                default_authority
                    .as_ref()
                    .expect("unqualified application prompt resolved root authority"),
            )),
        }
    };
    let root = match root {
        Ok(root) => root,
        Err(error) => {
            invocation.respond(Err(error));
            return;
        }
    };

    if let Err(error) = ensure_session_projection_on(worker, &root, &request.session_id) {
        invocation.respond(Err(error));
        return;
    }
    let model_input = match model_input_from_session(
        worker.projection().state(),
        &request.session_id,
        &request.content,
    ) {
        Ok(input) => input,
        Err(error) => {
            invocation.respond(Err(error));
            return;
        }
    };

    let tool_surface = {
        let harness = worker.harness.lock();
        let resolved = root
            .generation()
            .ok_or_else(|| ApplicationError::Failed {
                message: "application prompt root has no resolved graph generation".to_owned(),
            })
            .and_then(|generation| {
                harness
                    .resolved_generation_by_id(generation)
                    .map_err(|error| ApplicationError::Failed {
                        message: error.to_string(),
                    })
            });
        match resolved.and_then(|resolved| {
            application_model_tool_surface(service, &request.session_id, resolved, root.authority())
        }) {
            Ok(surface) => Ok((surface, harness.application_agent_tools().clone())),
            Err(error) => Err(error),
        }
    };
    let (tool_surface, adapter) = match tool_surface {
        Ok(value) => value,
        Err(error) => {
            invocation.respond(Err(error));
            return;
        }
    };
    let tools = tool_surface.tools;
    let runtime_entry_triggers = tool_surface.runtime_entry_triggers;

    let prompt = match worker.prompt_on(&root, request.clone()) {
        Ok(prompt) => prompt,
        Err(error) => {
            invocation.respond(Err(error));
            return;
        }
    };
    if let Err(error) = worker.append_execution_change_on(
        &root,
        &request.session_id,
        &prompt.execution_id,
        ExecutionChange::State {
            state: ExecutionState::Running,
        },
    ) {
        invocation.respond(Err(error));
        return;
    }

    let cancellation = Arc::new(AtomicBool::new(false));
    active.insert(
        key,
        ActiveExecution {
            execution_id: prompt.execution_id.clone(),
            root: root.clone(),
            cancellation: Arc::clone(&cancellation),
            progress_error: None,
            prompt: invocation,
        },
    );

    let permission_handler = worker.interaction_handlers().permission.clone();
    let harness = Arc::downgrade(&worker.harness);
    let application_service = service.clone();
    let application_control = control_transport.clone();
    let session_id = request.session_id;
    let execution_id = prompt.execution_id;
    let runtime_execution_id = execution_id.clone();
    let sender = execution_sender.clone();
    let progress_sender = execution_sender.clone();
    tokio::spawn(async move {
        let blocking_cancellation = Arc::clone(&cancellation);
        let execution_session = session_id.clone();
        let result = tokio::task::spawn_blocking(move || {
            run_agent_execution(
                adapter,
                root,
                harness,
                application_service,
                application_control,
                AgentExecutionContext {
                    session_id: execution_session,
                    execution_id: runtime_execution_id,
                    input: model_input,
                    tools,
                    runtime_entry_triggers,
                    permission_handler,
                    progress_sender,
                },
                blocking_cancellation,
            )
        })
        .await
        .map_err(|error| ApplicationError::Failed {
            message: format!("application execution worker failed: {error}"),
        })
        .and_then(|result| result);
        let _ = sender
            .send(ExecutionWorkerEvent::Complete(ExecutionCompletion {
                session_id,
                execution_id,
                result,
            }))
            .await;
    });
}

fn cancel_prompt(
    worker: &mut ApplicationWorker,
    active: &mut BTreeMap<String, ActiveExecution>,
    invocation: ApplicationInvocation,
) {
    let request = match decode::<ApplicationSessionInput>(invocation.input.clone()) {
        Ok(request) => request,
        Err(error) => {
            invocation.respond(Err(error));
            return;
        }
    };
    if let Some(execution) = active.get(request.session_id.as_str()) {
        execution.cancellation.store(true, Ordering::Release);
        invocation.respond(Ok(Acknowledged {}.to_value()));
        return;
    }

    invocation.respond(worker.cancel(request).map(|value| value.to_value()));
}

fn handle_execution_progress(
    worker: &mut ApplicationWorker,
    active: &mut BTreeMap<String, ActiveExecution>,
    progress: ExecutionProgress,
) {
    let key = progress.session_id.as_str().to_owned();
    let Some(execution) = active.get(&key) else {
        return;
    };
    if execution.execution_id != progress.execution_id {
        return;
    }

    let result = project_persisted_agent_progress(worker, progress.update);

    if let Err(error) = result {
        let Some(execution) = active.get_mut(&key) else {
            return;
        };
        execution.cancellation.store(true, Ordering::Release);
        execution.progress_error.get_or_insert(error);
    }
}

fn project_persisted_agent_progress(
    worker: &mut ApplicationWorker,
    update: SessionUpdate,
) -> Result<(), ApplicationError> {
    let projection = worker
        .projection
        .state()
        .sessions
        .get(update.session_id.as_str())
        .ok_or_else(|| ApplicationError::NotFound {
            resource: format!("session {}", update.session_id),
        })?;

    if update.sequence <= projection.through_sequence {
        let existing = projection
            .updates
            .iter()
            .find(|existing| existing.sequence == update.sequence);
        return match existing {
            Some(existing) if existing == &update => Ok(()),
            Some(_) => Err(ApplicationError::Conflict {
                message: format!(
                    "persisted agent progress at sequence {} conflicts with the projected session update",
                    update.sequence
                ),
            }),
            None => Err(ApplicationError::Conflict {
                message: format!(
                    "persisted agent progress at sequence {} is behind the projection watermark {} but is missing from projected history",
                    update.sequence, projection.through_sequence
                ),
            }),
        };
    }

    let expected =
        projection
            .through_sequence
            .checked_add(1)
            .ok_or_else(|| ApplicationError::Conflict {
                message: "session progress sequence overflowed".to_owned(),
            })?;
    if update.sequence != expected {
        return Err(ApplicationError::Conflict {
            message: format!(
                "persisted agent progress is not contiguous: expected {expected}, got {}",
                update.sequence
            ),
        });
    }

    worker
        .projection
        .apply_update(update.clone())
        .map_err(application_projection_error)?;
    worker.emit_session_update(update)
}

fn finish_prompt(
    worker: &mut ApplicationWorker,
    active: &mut BTreeMap<String, ActiveExecution>,
    completion: ExecutionCompletion,
) {
    let key = completion.session_id.as_str().to_owned();
    let Some(execution) = active.remove(&key) else {
        return;
    };
    if execution.execution_id != completion.execution_id {
        execution.prompt.respond(Err(ApplicationError::Conflict {
            message: "execution completion identity changed while the prompt was active".to_owned(),
        }));
        return;
    }
    let root = &execution.root;
    if let Some(error) = execution.progress_error {
        let _ = worker.finish_root_execution_on(root, &completion.execution_id, false);
        let _ = worker.append_execution_change_on(
            root,
            &completion.session_id,
            &completion.execution_id,
            ExecutionChange::State {
                state: ExecutionState::Failed {
                    error: error.clone(),
                },
            },
        );
        execution.prompt.respond(Err(error));
        return;
    }
    if execution.cancellation.load(Ordering::Acquire) {
        let _ = worker.finish_root_execution_on(root, &completion.execution_id, false);
        let _ = worker.append_execution_change_on(
            root,
            &completion.session_id,
            &completion.execution_id,
            ExecutionChange::State {
                state: ExecutionState::Cancelled,
            },
        );
        execution.prompt.respond(Ok(PromptResult {
            execution_id: completion.execution_id,
            stop_reason: StopReason::Cancelled,
        }
        .to_value()));
        return;
    }

    let result = match completion.result {
        Ok(text) => worker
            .finish_root_execution_on(root, &completion.execution_id, true)
            .and_then(|()| {
                complete_prompt_output_on(
                    worker,
                    root,
                    &completion.session_id,
                    &completion.execution_id,
                    text,
                )
            }),
        Err(error) => {
            let _ = worker.finish_root_execution_on(root, &completion.execution_id, false);
            let _ = worker.append_execution_change_on(
                root,
                &completion.session_id,
                &completion.execution_id,
                ExecutionChange::State {
                    state: ExecutionState::Failed {
                        error: error.clone(),
                    },
                },
            );
            Err(error)
        }
    };
    execution
        .prompt
        .respond(result.map(|value| value.to_value()));
}

fn complete_prompt_output_on(
    worker: &mut ApplicationWorker,
    root: &RootExecutionHandle,
    session_id: &SessionId,
    execution_id: &str,
    text: String,
) -> Result<PromptResult, ApplicationError> {
    let session = worker.require_open_application_session_on(root, session_id)?;
    worker.append_session_change_on(
        root,
        &session,
        SessionChange::TextDelta {
            execution_id: execution_id.to_owned(),
            text: text.clone(),
        },
    )?;
    worker.append_session_change_on(
        root,
        &session,
        SessionChange::Message {
            message: Message {
                role: MessageRole::Assistant,
                content: vec![Content::Text { text }],
            },
        },
    )?;
    worker.append_execution_change_on(
        root,
        session_id,
        execution_id,
        ExecutionChange::State {
            state: ExecutionState::Completed,
        },
    )?;
    Ok(PromptResult {
        execution_id: execution_id.to_owned(),
        stop_reason: StopReason::EndTurn,
    })
}

fn ensure_session_projection_on(
    worker: &mut ApplicationWorker,
    root: &RootExecutionHandle,
    session_id: &SessionId,
) -> Result<(), ApplicationError> {
    if worker
        .projection()
        .state()
        .sessions
        .contains_key(session_id.as_str())
    {
        return Ok(());
    }
    worker
        .resume_session_on(
            root,
            SessionResumeInput {
                session_id: session_id.clone(),
                after_sequence: None,
            },
        )
        .map(|_| ())
}

#[cfg(test)]
fn ensure_session_projection(
    worker: &mut ApplicationWorker,
    session_id: &SessionId,
) -> Result<(), ApplicationError> {
    if worker
        .projection()
        .state()
        .sessions
        .contains_key(session_id.as_str())
    {
        return Ok(());
    }
    worker
        .resume_session(SessionResumeInput {
            session_id: session_id.clone(),
            after_sequence: None,
        })
        .map(|_| ())
}

fn model_input_from_session(
    state: &SessionProjectionState,
    session_id: &SessionId,
    current: &[Content],
) -> Result<Bytes, ApplicationError> {
    let current = validated_model_text(current)?;
    let projection =
        state
            .sessions
            .get(session_id.as_str())
            .ok_or_else(|| ApplicationError::NotFound {
                resource: format!("session {session_id}"),
            })?;
    let messages = projection.updates.iter().filter_map(|update| {
        if let SessionChange::Message { message } = &update.update {
            Some(message)
        } else {
            None
        }
    });

    let mut messages = messages.peekable();
    if messages.peek().is_none() {
        return Ok(Bytes::new(current.into_bytes()));
    }

    let mut transcript = String::from("--- phenix session-history ---\n");
    for message in messages {
        transcript.push_str(match &message.role {
            MessageRole::User => "--- user ---\n",
            MessageRole::Assistant => "--- assistant ---\n",
        });
        transcript.push_str(&model_text_from_content(&message.content)?);
        transcript.push('\n');
    }
    transcript.push_str("--- phenix current-user ---\n");
    transcript.push_str(&current);
    Ok(Bytes::new(transcript.into_bytes()))
}

fn model_text_from_content(content: &[Content]) -> Result<String, ApplicationError> {
    let mut text = String::new();
    for part in content {
        let Content::Text { text: part } = part else {
            return Err(ApplicationError::InvalidInput {
                message: "the current model-turn ABI accepts text prompt content only; resource and image turns require the typed model-turn contract".to_owned(),
            });
        };
        text.push_str(part);
    }
    Ok(text)
}

fn validated_model_text(content: &[Content]) -> Result<String, ApplicationError> {
    let text = model_text_from_content(content)?;
    if text.trim().is_empty() {
        return Err(ApplicationError::InvalidInput {
            message: "prompt text must not be empty".to_owned(),
        });
    }
    Ok(text)
}

#[cfg(test)]
fn model_input_from_content(content: &[Content]) -> Result<Bytes, ApplicationError> {
    validated_model_text(content).map(|text| Bytes::new(text.into_bytes()))
}

struct AgentExecutionContext {
    session_id: SessionId,
    execution_id: String,
    input: Bytes,
    tools: Vec<ModelToolDescriptor>,
    runtime_entry_triggers: BTreeMap<CallableId, ComponentEntryTrigger>,
    permission_handler: Option<PermissionHandlerRef>,
    progress_sender: mpsc::Sender<ExecutionWorkerEvent>,
}

fn run_agent_execution(
    adapter: ApplicationAgentToolRegistry,
    root: RootExecutionHandle,
    harness: Weak<Mutex<PhenixRuntime>>,
    service: SdkApplicationService,
    control_transport: WeakChannelTransport,
    context: AgentExecutionContext,
    cancellation: Arc<AtomicBool>,
) -> Result<String, ApplicationError> {
    let AgentExecutionContext {
        session_id,
        execution_id,
        input,
        tools,
        runtime_entry_triggers,
        permission_handler,
        progress_sender,
    } = context;
    let callable_id =
        CallableId::parse(DEFAULT_APPLICATION_AGENT).map_err(|error| ApplicationError::Failed {
            message: format!("invalid application agent id: {error}"),
        })?;

    if cancellation.load(Ordering::Acquire) {
        return Err(ApplicationError::Cancelled);
    }

    let root_generation = root
        .generation()
        .cloned()
        .ok_or_else(|| ApplicationError::Failed {
            message: "application agent root has no graph generation".to_owned(),
        })?;
    let root_constraints = root.constraints().clone();

    adapter.register(
        execution_id.clone(),
        ApplicationAgentToolRun {
            service,
            control_transport,
            harness,
            session_id: session_id.clone(),
            execution_id: execution_id.clone(),
            root_generation,
            root_constraints,
            permission_handler,
            tools: tools.clone(),
            runtime_entry_triggers,
            cancellation: Arc::clone(&cancellation),
            progress_sender,
        },
    )?;

    let result = (|| {
        let command = AgentLoopCommand::Run {
            execution_id: execution_id.clone(),
            session_id: Some(session_id),
            parent_attempt_id: None,
            callable_id: Some(callable_id),
            input,
            tools,
        };
        let encoded = serde_json::to_vec(&PhenixValue::from(&command)).map_err(|error| {
            ApplicationError::InvalidInput {
                message: error.to_string(),
            }
        })?;
        let output = root
            .invoke(&agent_loop_service(), &encoded, None)
            .map_err(|error| ApplicationError::Failed {
                message: error.to_string(),
            })?;
        if cancellation.load(Ordering::Acquire) {
            return Err(ApplicationError::Cancelled);
        }
        let value: PhenixValue =
            serde_json::from_slice(&output).map_err(|error| ApplicationError::InvalidResponse {
                message: error.to_string(),
            })?;
        let response = AgentLoopResponse::try_from(Project(&value)).map_err(|error| {
            ApplicationError::InvalidResponse {
                message: error.to_string(),
            }
        })?;
        match response {
            AgentLoopResponse::Completed { output, .. } => {
                String::from_utf8(output.as_ref().to_vec()).map_err(|error| {
                    ApplicationError::InvalidResponse {
                        message: format!("agent output is not UTF-8: {error}"),
                    }
                })
            }
            AgentLoopResponse::Cancelled { .. } => Err(ApplicationError::Cancelled),
            AgentLoopResponse::Failed { failure, .. } => match failure {
                AgentLoopFailure::ModelTurnLimitExceeded { limit } => {
                    Err(ApplicationError::Conflict {
                        message: format!("model tool turn limit exceeded ({limit})"),
                    })
                }
                AgentLoopFailure::ToolCallLimitExceeded { limit, actual } => {
                    Err(ApplicationError::Conflict {
                        message: format!(
                            "agent returned {actual} tool calls; the per-turn limit is {limit}"
                        ),
                    })
                }
            },
        }
    })();

    adapter.remove(&execution_id);
    result
}

fn normalize_model_tool_call(
    tools: &[ModelToolDescriptor],
    call: &ModelToolCall,
) -> Result<ModelToolCall, ApplicationError> {
    let descriptor = tools
        .iter()
        .find(|tool| tool.id == call.callable_id)
        .ok_or_else(|| ApplicationError::InvalidInput {
            message: format!("model requested unavailable tool {}", call.callable_id),
        })?;
    let input = normalize_model_tool_input(&descriptor.input_schema, call.input.clone()).map_err(
        |message| ApplicationError::InvalidInput {
            message: format!("invalid input for tool {}: {message}", call.callable_id),
        },
    )?;
    Ok(ModelToolCall {
        call_id: call.call_id.clone(),
        callable_id: call.callable_id.clone(),
        input,
    })
}

fn normalize_model_tool_input(
    schema: &PhenixSchema,
    value: PhenixValue,
) -> Result<PhenixValue, String> {
    if schema.parse(&value).is_ok() {
        return Ok(value);
    }

    let normalized = match (schema, value) {
        (PhenixSchema::Variant(variants), PhenixValue::Map(values)) => {
            normalize_model_tool_variant(variants, values)?
        }
        (PhenixSchema::Variant(variants), PhenixValue::Table(values)) => {
            let values = values
                .into_iter()
                .map(|(key, value)| (key.as_str().to_owned(), value))
                .collect();
            normalize_model_tool_variant(variants, values)?
        }
        (PhenixSchema::Table(fields), PhenixValue::Map(values)) => {
            normalize_model_tool_table(fields, values)?
        }
        (PhenixSchema::Table(fields), PhenixValue::Table(values)) => {
            let values = values
                .into_iter()
                .map(|(key, value)| (key.as_str().to_owned(), value))
                .collect();
            normalize_model_tool_table(fields, values)?
        }
        (PhenixSchema::List(item), PhenixValue::List(values)) => PhenixValue::List(
            values
                .into_iter()
                .map(|value| normalize_model_tool_input(item, value))
                .collect::<Result<Vec<_>, _>>()?,
        ),
        (PhenixSchema::Array { item, len }, PhenixValue::List(values)) => {
            if values.len() != *len {
                return Err(format!("expected {len} values, got {}", values.len()));
            }
            PhenixValue::List(
                values
                    .into_iter()
                    .map(|value| normalize_model_tool_input(item, value))
                    .collect::<Result<Vec<_>, _>>()?,
            )
        }
        (PhenixSchema::Map(item), PhenixValue::Map(values)) => PhenixValue::Map(
            values
                .into_iter()
                .map(|(key, value)| {
                    normalize_model_tool_input(item, value).map(|value| (key, value))
                })
                .collect::<Result<BTreeMap<_, _>, _>>()?,
        ),
        (PhenixSchema::Option(_), PhenixValue::Unit) => PhenixValue::Option(None),
        (PhenixSchema::Option(item), PhenixValue::Option(Some(value))) => {
            PhenixValue::Option(Some(Box::new(normalize_model_tool_input(item, *value)?)))
        }
        (PhenixSchema::Option(_), PhenixValue::Option(None)) => PhenixValue::Option(None),
        (PhenixSchema::Option(item), value) => {
            PhenixValue::Option(Some(Box::new(normalize_model_tool_input(item, value)?)))
        }
        (PhenixSchema::I64, PhenixValue::U64(value)) if value <= i64::MAX as u64 => {
            PhenixValue::I64(value as i64)
        }
        (PhenixSchema::U64, PhenixValue::I64(value)) if value >= 0 => {
            PhenixValue::U64(value as u64)
        }
        (PhenixSchema::F64, PhenixValue::I64(value)) => PhenixValue::F64(value as f64),
        (PhenixSchema::F64, PhenixValue::U64(value)) => PhenixValue::F64(value as f64),
        (_, value) => return Err(format!("expected {}, got {}", schema.kind(), value.kind())),
    };

    schema
        .parse(&normalized)
        .map_err(|error| format!("{error:?}"))?;
    Ok(normalized)
}

fn normalize_model_tool_variant(
    variants: &BTreeMap<Key, PhenixSchema>,
    mut values: BTreeMap<String, PhenixValue>,
) -> Result<PhenixValue, String> {
    if let Some(key) = values
        .keys()
        .find(|key| key.as_str() != "tag" && key.as_str() != "value")
    {
        return Err(format!("unexpected variant field {key}"));
    }

    let tag = values
        .remove("tag")
        .ok_or_else(|| "missing variant field tag".to_owned())?;
    let tag = match tag {
        PhenixValue::String(tag) => tag,
        value => {
            return Err(format!(
                "variant tag must be a string, got {}",
                value.kind()
            ));
        }
    };
    let tag = Key::parse(tag).map_err(str::to_owned)?;
    let schema = variants
        .get(&tag)
        .ok_or_else(|| format!("unknown variant {tag}"))?;
    let value = values
        .remove("value")
        .ok_or_else(|| "missing variant field value".to_owned())?;
    let value = normalize_model_tool_input(schema, value)?;

    Ok(PhenixValue::Variant {
        tag,
        value: Box::new(value),
    })
}

fn normalize_model_tool_table(
    fields: &BTreeMap<Key, PhenixSchema>,
    values: BTreeMap<String, PhenixValue>,
) -> Result<PhenixValue, String> {
    if let Some(key) = values.keys().find(|key| !fields.contains_key(key.as_str())) {
        return Err(format!("unexpected field {key}"));
    }

    let mut normalized = BTreeMap::new();
    for (key, field_schema) in fields {
        let value = values
            .get(key.as_str())
            .cloned()
            .ok_or_else(|| format!("missing field {key}"))?;
        normalized.insert(
            key.clone(),
            normalize_model_tool_input(field_schema, value)?,
        );
    }
    Ok(PhenixValue::Table(normalized))
}

struct ApplicationModelToolSurface {
    tools: Vec<ModelToolDescriptor>,
    runtime_entry_triggers: BTreeMap<CallableId, ComponentEntryTrigger>,
}

fn application_model_tool_surface(
    service: &SdkApplicationService,
    session_id: &SessionId,
    resolved: &phenix_core::ResolvedGeneration,
    authority: &Authority,
) -> Result<ApplicationModelToolSurface, ApplicationError> {
    let mut runtime_entry_triggers = BTreeMap::new();
    let mut graph_tools = Vec::new();

    for trigger in resolved.entry_triggers() {
        if !authority.permits_all(&trigger.required_authority) {
            continue;
        }
        let EntryTriggerKind::ToolCall {
            callable_id,
            description,
        } = &trigger.trigger;
        let component = resolved
            .components()
            .iter()
            .find(|component| component.id == trigger.component)
            .ok_or_else(|| ApplicationError::Failed {
                message: format!(
                    "resolved entry trigger targets missing component {}",
                    trigger.component
                ),
            })?;
        let export = component
            .exports
            .iter()
            .find(|export| export.interface == trigger.interface)
            .ok_or_else(|| ApplicationError::Failed {
                message: format!(
                    "resolved entry trigger targets missing export {}:{}",
                    trigger.component, trigger.interface
                ),
            })?;
        if !authority.permits_all(&export.required_authority) {
            continue;
        }

        graph_tools.push(ModelToolDescriptor {
            id: callable_id.clone(),
            description: description.clone(),
            input_schema: export.schema.request().clone(),
            output_schema: export.schema.response().clone(),
        });
        if runtime_entry_triggers
            .insert(callable_id.clone(), trigger.clone())
            .is_some()
        {
            return Err(ApplicationError::Conflict {
                message: format!("duplicate runtime entry trigger {callable_id}"),
            });
        }
    }

    graph_tools.extend(host_model_tools(authority));
    let tools = model_tool_surface(service, session_id, graph_tools)?;
    Ok(ApplicationModelToolSurface {
        tools,
        runtime_entry_triggers,
    })
}

fn host_model_tools(authority: &Authority) -> Vec<ModelToolDescriptor> {
    let mut tools = vec![ModelToolDescriptor {
        id: CallableId::parse("phenix.inspect")
            .expect("static inspection callable id is valid"),
        description: "Read canonical Phenix runtime state and retained metadata-only diagnostics for debugging. Queries: graph, execution, execution <execution-id>, dag, dag <execution-id>, trace, trace <execution-id>, values, value <value-id>. A bare application execution id such as execution-42 is accepted as execution lookup shorthand.".to_owned(),
        input_schema: PhenixSchema::Table(BTreeMap::from([(
            Key::parse("query").expect("static inspection field is valid"),
            PhenixSchema::String,
        )])),
        output_schema: PhenixSchema::Any,
    }];

    let session_control = PermissionId::parse(APPLICATION_SESSION_CONTROL_PERMISSION)
        .expect("static application session control permission is valid");
    if authority.permits(&session_control) {
        tools.push(ModelToolDescriptor {
            id: CallableId::parse("phenix.session")
                .expect("static session control callable id is valid"),
            description: "Create, list, resume, prompt, or close another Phenix session through canonical application operations. Arguments: create {working_directory?,title?}; list {}; resume {session_id,after_sequence?}; prompt {session_id,content,generation?}, where content parts use kind=text, kind=image, or kind=resource with their corresponding fields; close {session_id}. Agent-created children inherit the controller working directory and pinned Environment; an explicit working_directory must match it. Prompt waits for terminal completion. Omitted generation keeps the controller root generation.".to_owned(),
            input_schema: PhenixSchema::Table(BTreeMap::from([
                (
                    Key::parse("operation").expect("static session operation field is valid"),
                    PhenixSchema::String,
                ),
                (
                    Key::parse("arguments").expect("static session arguments field is valid"),
                    PhenixSchema::Any,
                ),
            ])),
            output_schema: PhenixSchema::Any,
        });
    }

    let plugin_visible = [
        RUNTIME_PLUGIN_INSPECT_PERMISSION,
        RUNTIME_PLUGIN_BUILD_PERMISSION,
        RUNTIME_PLUGIN_TRIAL_PERMISSION,
        RUNTIME_PLUGIN_PROMOTE_PERMISSION,
        RUNTIME_PLUGIN_RETIRE_PERMISSION,
    ]
    .into_iter()
    .any(|capability| {
        authority.permits(
            &PermissionId::parse(capability).expect("static runtime plugin permission is valid"),
        )
    });
    if plugin_visible {
        tools.push(ModelToolDescriptor {
            id: CallableId::parse("phenix.plugin")
                .expect("static plugin management callable id is valid"),
            description: "Manage resident Plugin generations through Core. Arguments: inspect {}; build {plan}; trial {request}; promote, rollback, or retire {generation}. Build and trial use the configured workspace backend. For model-facing build plans, omitted or empty requested_authority uses the narrow workspace build authority, still bounded by the parent root; a non-empty requested_authority attenuates it. Build returns a complete artifact object that can be used as a ready runtime artifact in a trial request. Trial keeps the current default; promote and rollback change the default for future roots.".to_owned(),
            input_schema: PhenixSchema::Table(BTreeMap::from([
                (
                    Key::parse("operation").expect("static plugin operation field is valid"),
                    PhenixSchema::String,
                ),
                (
                    Key::parse("arguments").expect("static plugin arguments field is valid"),
                    PhenixSchema::Any,
                ),
            ])),
            output_schema: PhenixSchema::Any,
        });
    }

    tools
}
fn execute_runtime_entry_trigger(
    context: &ApplicationAgentToolContext<'_, '_>,
    trigger: &ComponentEntryTrigger,
    call: &ModelToolCall,
) -> ExecutionChange {
    let result = (|| -> Result<PhenixValue, ApplicationError> {
        let component = context
            .kernel
            .component_graph()
            .component(&trigger.component)
            .ok_or_else(|| ApplicationError::NotFound {
                resource: trigger.component.to_string(),
            })?;
        let service = ServiceId::parse(trigger.interface.as_str()).map_err(|error| {
            ApplicationError::Failed {
                message: format!("entry trigger interface is not a valid service id: {error}"),
            }
        })?;
        let input =
            serde_json::to_vec(&call.input).map_err(|error| ApplicationError::InvalidInput {
                message: error.to_string(),
            })?;
        let output = context
            .kernel
            .invoke_component_abi(
                &trigger.component,
                &service,
                &input,
                context.call.authority,
                &component.owning_plugin,
            )
            .map_err(|error| ApplicationError::Failed {
                message: error.to_string(),
            })?;
        serde_json::from_slice(&output).map_err(|error| ApplicationError::InvalidResponse {
            message: error.to_string(),
        })
    })();

    match result {
        Ok(output) => ExecutionChange::ToolResult {
            call_id: call.call_id.clone(),
            output,
        },
        Err(error) => ExecutionChange::ToolFailed {
            call_id: call.call_id.clone(),
            error,
        },
    }
}
fn execute_runtime_plugin_tool_call(
    context: &ApplicationAgentToolContext<'_, '_>,
    run: &ApplicationAgentToolRun,
    call: &ModelToolCall,
) -> ExecutionChange {
    let result = (|| -> Result<PhenixValue, ApplicationError> {
        let PhenixValue::Table(fields) = &call.input else {
            return Err(ApplicationError::InvalidInput {
                message: "phenix.plugin input must contain operation and arguments".to_owned(),
            });
        };
        let operation = match fields.get("operation") {
            Some(PhenixValue::String(operation)) if !operation.trim().is_empty() => {
                operation.trim()
            }
            _ => {
                return Err(ApplicationError::InvalidInput {
                    message: "phenix.plugin operation must be a non-empty string".to_owned(),
                });
            }
        };
        let arguments = fields
            .get("arguments")
            .ok_or_else(|| ApplicationError::InvalidInput {
                message: "phenix.plugin input is missing arguments".to_owned(),
            })?;
        execute_runtime_plugin_control(context, run, operation, arguments)
    })();

    match result {
        Ok(output) => ExecutionChange::ToolResult {
            call_id: call.call_id.clone(),
            output,
        },
        Err(error) => ExecutionChange::ToolFailed {
            call_id: call.call_id.clone(),
            error,
        },
    }
}

fn require_runtime_plugin_capability(
    authority: &Authority,
    capability: &str,
) -> Result<(), ApplicationError> {
    let capability_id =
        PermissionId::parse(capability).map_err(|error| ApplicationError::Failed {
            message: error.to_string(),
        })?;
    if authority.permits(&capability_id) {
        Ok(())
    } else {
        Err(ApplicationError::PermissionDenied {
            message: format!("phenix.plugin requires {capability}"),
        })
    }
}

fn runtime_plugin_build_authority() -> Authority {
    Authority::new(
        ["workspace.read", "workspace.shell"]
            .into_iter()
            .map(|value| PermissionId::parse(value).expect("static build permission is valid")),
    )
}

fn runtime_plugin_policy(required_permission: &str) -> PluginManagementPolicy {
    PluginManagementPolicy::new(
        Authority::new([PermissionId::parse(required_permission)
            .expect("static plugin management permission is valid")]),
        runtime_plugin_build_authority(),
    )
}

fn apply_runtime_plugin_default_build_authority(
    plan: &mut PluginBuildPlan,
) -> Result<(), ApplicationError> {
    if plan.requested_authority().permissions().next().is_some() {
        return Ok(());
    }

    *plan = PluginBuildPlan::new(
        plan.source().clone(),
        plan.steps().to_vec(),
        plan.artifact_output().clone(),
        plan.configuration().clone(),
        runtime_plugin_build_authority(),
    )
    .map_err(|error| ApplicationError::InvalidInput {
        message: format!("invalid phenix.plugin build plan: {error}"),
    })?;
    Ok(())
}

fn runtime_plugin_typed_argument<T>(
    arguments: &PhenixValue,
    key: &str,
) -> Result<T, ApplicationError>
where
    T: DeserializeOwned,
{
    let value =
        session_control_field(arguments, key).ok_or_else(|| ApplicationError::InvalidInput {
            message: format!("phenix.plugin argument {key} is required"),
        })?;
    let json =
        serde_json::Value::from_value(value).map_err(|error| ApplicationError::InvalidInput {
            message: format!("phenix.plugin argument {key} is not JSON-compatible: {error}"),
        })?;
    serde_json::from_value(json).map_err(|error| ApplicationError::InvalidInput {
        message: format!("invalid phenix.plugin argument {key}: {error}"),
    })
}

fn runtime_reconciliation_preview_value(preview: &ReconciliationPreview) -> PhenixValue {
    let components = preview
        .diff
        .components
        .iter()
        .map(|change| {
            PhenixValue::Map(BTreeMap::from([
                (
                    "component".to_owned(),
                    PhenixValue::String(change.component.to_string()),
                ),
                (
                    "kind".to_owned(),
                    PhenixValue::String(format!("{:?}", change.kind).to_ascii_lowercase()),
                ),
            ]))
        })
        .collect();
    let bindings =
        preview
            .diff
            .bindings
            .iter()
            .map(|change| {
                PhenixValue::Map(BTreeMap::from([
                    (
                        "importer".to_owned(),
                        PhenixValue::String(change.importer.to_string()),
                    ),
                    (
                        "interface".to_owned(),
                        PhenixValue::String(change.interface.to_string()),
                    ),
                    (
                        "previous_provider".to_owned(),
                        PhenixValue::Option(
                            change.previous_provider.as_ref().map(|provider| {
                                Box::new(PhenixValue::String(provider.to_string()))
                            }),
                        ),
                    ),
                    (
                        "next_provider".to_owned(),
                        PhenixValue::Option(
                            change.next_provider.as_ref().map(|provider| {
                                Box::new(PhenixValue::String(provider.to_string()))
                            }),
                        ),
                    ),
                    (
                        "authority_changed".to_owned(),
                        PhenixValue::Bool(change.authority_changed),
                    ),
                ]))
            })
            .collect();
    let interposition = preview
        .diff
        .interposition
        .iter()
        .map(|change| {
            PhenixValue::Map(BTreeMap::from([
                (
                    "service".to_owned(),
                    PhenixValue::String(change.service.to_string()),
                ),
                (
                    "previous_layers".to_owned(),
                    PhenixValue::U64(change.previous.len() as u64),
                ),
                (
                    "next_layers".to_owned(),
                    PhenixValue::U64(change.next.len() as u64),
                ),
            ]))
        })
        .collect();
    let resources = preview
        .diff
        .resources
        .iter()
        .map(|change| {
            PhenixValue::Map(BTreeMap::from([
                (
                    "resource".to_owned(),
                    PhenixValue::String(change.resource.clone()),
                ),
                (
                    "kind".to_owned(),
                    PhenixValue::String(format!("{:?}", change.kind).to_ascii_lowercase()),
                ),
                (
                    "invalidation_targets".to_owned(),
                    PhenixValue::List(
                        change
                            .invalidation_targets
                            .iter()
                            .cloned()
                            .map(PhenixValue::String)
                            .collect(),
                    ),
                ),
            ]))
        })
        .collect();

    PhenixValue::Map(BTreeMap::from([
        (
            "active_generation".to_owned(),
            PhenixValue::String(preview.active_generation.as_str().to_owned()),
        ),
        (
            "candidate_generation".to_owned(),
            PhenixValue::String(preview.candidate_generation.as_str().to_owned()),
        ),
        ("components".to_owned(), PhenixValue::List(components)),
        ("bindings".to_owned(), PhenixValue::List(bindings)),
        ("interposition".to_owned(), PhenixValue::List(interposition)),
        ("resources".to_owned(), PhenixValue::List(resources)),
    ]))
}

fn runtime_plugin_build_report_value(report: &PluginBuildReport) -> PhenixValue {
    PhenixValue::Map(BTreeMap::from([
        (
            "artifact".to_owned(),
            PhenixValue::from(
                serde_json::to_value(&report.artifact).expect("plugin artifact is JSON-compatible"),
            ),
        ),
        (
            "artifact_locator".to_owned(),
            PhenixValue::String(report.artifact.locator.clone()),
        ),
        (
            "artifact_revision".to_owned(),
            PhenixValue::String(report.artifact.revision.as_ref().to_owned()),
        ),
        (
            "provenance".to_owned(),
            PhenixValue::List(
                report
                    .evidence
                    .provenance()
                    .iter()
                    .cloned()
                    .map(PhenixValue::String)
                    .collect(),
            ),
        ),
        (
            "diagnostics".to_owned(),
            PhenixValue::List(
                report
                    .evidence
                    .diagnostics()
                    .iter()
                    .cloned()
                    .map(PhenixValue::String)
                    .collect(),
            ),
        ),
    ]))
}

struct WorkspacePluginBuildExecutor<'context, 'host, 'runtime> {
    context: &'context ApplicationAgentToolContext<'host, 'runtime>,
}

impl PluginBuildExecutor for WorkspacePluginBuildExecutor<'_, '_, '_> {
    fn execute(
        &mut self,
        plan: &PluginBuildPlan,
        effective_authority: &Authority,
    ) -> Result<PluginBuildExecution, PluginBuildFailure> {
        for capability in ["workspace.shell", "workspace.read"] {
            let capability_id =
                PermissionId::parse(capability).expect("static workspace permission is valid");
            if !effective_authority.permits(&capability_id) {
                return Err(PluginBuildFailure {
                    message: format!("plugin build authority denied: {capability}"),
                    evidence: PluginBuildEvidence::bounded(
                        Vec::new(),
                        vec![format!(
                            "build plan must explicitly request and receive {capability}"
                        )],
                    ),
                });
            }
        }

        let mut provenance = Vec::new();
        let mut diagnostics = Vec::new();
        for (index, step) in plan.steps().iter().enumerate() {
            let command = WorkspaceCommand::Exec {
                program: step.executable.as_ref().to_owned(),
                arguments: step
                    .argv
                    .iter()
                    .map(|argument| argument.as_ref().to_owned())
                    .collect(),
                working_directory: Some(step.working_directory.as_ref().to_owned()),
                environment: step
                    .environment
                    .iter()
                    .map(|(name, value)| (name.as_ref().to_owned(), value.to_owned()))
                    .collect(),
            };
            let response = self
                .context
                .sdk
                .workspace
                .invoke_projected::<WorkspaceCommand, WorkspaceResponse>(&command)
                .map_err(|error| PluginBuildFailure {
                    message: format!("plugin build step {index} could not execute: {error}"),
                    evidence: PluginBuildEvidence::bounded(provenance.clone(), diagnostics.clone()),
                })?;
            let WorkspaceResponse::Process {
                exit_code,
                stderr,
                stderr_complete,
                ..
            } = response
            else {
                return Err(PluginBuildFailure {
                    message: format!("plugin build step {index} returned a non-process response"),
                    evidence: PluginBuildEvidence::bounded(provenance, diagnostics),
                });
            };
            provenance.push(format!(
                "workspace-exec:{index}:{}:{}",
                step.executable.as_ref(),
                exit_code
            ));
            if !stderr.trim().is_empty() {
                diagnostics.push(stderr);
            }
            if !stderr_complete {
                diagnostics.push(format!("plugin build step {index} stderr was truncated"));
            }
            if exit_code != 0 {
                return Err(PluginBuildFailure {
                    message: format!("plugin build step {index} exited with status {exit_code}"),
                    evidence: PluginBuildEvidence::bounded(provenance, diagnostics),
                });
            }
        }

        let artifact_path = plan.artifact_output().as_ref().to_owned();
        let response = self
            .context
            .sdk
            .workspace
            .invoke_projected::<WorkspaceCommand, WorkspaceResponse>(&WorkspaceCommand::ReadBytes {
                path: artifact_path.clone(),
            })
            .map_err(|error| PluginBuildFailure {
                message: format!("plugin build output {artifact_path} is unavailable: {error}"),
                evidence: PluginBuildEvidence::bounded(provenance.clone(), diagnostics.clone()),
            })?;
        let WorkspaceResponse::ReadBytes { content, .. } = response else {
            return Err(PluginBuildFailure {
                message: format!(
                    "plugin build output {artifact_path} returned a non-read response"
                ),
                evidence: PluginBuildEvidence::bounded(provenance, diagnostics),
            });
        };
        provenance.push(format!(
            "workspace-artifact:{}:{}",
            artifact_path,
            ArtifactRevision::from_content(&content)
        ));

        Ok(PluginBuildExecution {
            output: Some(PluginBuildOutput::new(artifact_path, content)),
            evidence: PluginBuildEvidence::bounded(provenance, diagnostics),
        })
    }
}

struct WorkspacePluginArtifactStore<'context, 'host, 'runtime> {
    context: &'context ApplicationAgentToolContext<'host, 'runtime>,
    authority: &'context Authority,
}

impl WorkspacePluginArtifactStore<'_, '_, '_> {
    fn require(&self, capability: &str) -> Result<(), PluginArtifactStoreError> {
        let capability_id =
            PermissionId::parse(capability).expect("static workspace permission is valid");
        if self.authority.permits(&capability_id) {
            Ok(())
        } else {
            Err(PluginArtifactStoreError {
                message: format!("plugin artifact store requires {capability}"),
            })
        }
    }

    fn read(&self, path: &str) -> Result<Vec<u8>, PluginArtifactStoreError> {
        self.require("workspace.read")?;
        let response = self
            .context
            .sdk
            .workspace
            .invoke_projected::<WorkspaceCommand, WorkspaceResponse>(&WorkspaceCommand::ReadBytes {
                path: path.to_owned(),
            })
            .map_err(|error| PluginArtifactStoreError {
                message: error.to_string(),
            })?;
        match response {
            WorkspaceResponse::ReadBytes { content, .. } => Ok(content),
            other => Err(PluginArtifactStoreError {
                message: format!("plugin artifact read returned unexpected response: {other:?}"),
            }),
        }
    }
}

impl PluginArtifactStore for WorkspacePluginArtifactStore<'_, '_, '_> {
    fn preflight(&mut self) -> Result<(), PluginArtifactStoreError> {
        let response = self
            .context
            .sdk
            .workspace
            .invoke_projected::<WorkspaceCommand, WorkspaceResponse>(
                &WorkspaceCommand::Capabilities,
            )
            .map_err(|error| PluginArtifactStoreError {
                message: error.to_string(),
            })?;
        match response {
            WorkspaceResponse::Capabilities { .. } => Ok(()),
            other => Err(PluginArtifactStoreError {
                message: format!(
                    "plugin artifact store workspace preflight returned unexpected response: {other:?}"
                ),
            }),
        }
    }

    fn verify_ready(&mut self, artifact: &PluginArtifact) -> Result<(), PluginArtifactStoreError> {
        let content = self.read(&artifact.locator)?;
        let observed = ArtifactRevision::from_content(&content);
        if observed == artifact.revision {
            Ok(())
        } else {
            Err(PluginArtifactStoreError {
                message: format!(
                    "plugin artifact revision mismatch at {}: expected {}, observed {}",
                    artifact.locator, artifact.revision, observed
                ),
            })
        }
    }

    fn store_built(
        &mut self,
        artifact: &PluginArtifact,
        content: &[u8],
    ) -> Result<String, PluginArtifactStoreError> {
        self.require("workspace.write")?;
        let observed = ArtifactRevision::from_content(content);
        if observed != artifact.revision {
            return Err(PluginArtifactStoreError {
                message: format!(
                    "plugin build content revision mismatch: expected {}, observed {}",
                    artifact.revision, observed
                ),
            });
        }
        let digest = artifact
            .revision
            .as_ref()
            .strip_prefix("sha256:")
            .expect("ArtifactRevision is canonical sha256");
        let path = format!(".phenix/artifacts/{digest}");
        let response = self
            .context
            .sdk
            .workspace
            .invoke_projected::<WorkspaceCommand, WorkspaceResponse>(
                &WorkspaceCommand::WriteBytes {
                    path: path.clone(),
                    content: content.to_vec(),
                    expected_version: WorkspaceFileVersion::Absent,
                },
            )
            .map_err(|error| PluginArtifactStoreError {
                message: error.to_string(),
            })?;
        match response {
            WorkspaceResponse::Written { .. } => {}
            WorkspaceResponse::VersionConflict { conflicts } => {
                return Err(PluginArtifactStoreError {
                    message: format!(
                        "content-addressed plugin artifact path conflicted: {conflicts:?}"
                    ),
                });
            }
            other => {
                return Err(PluginArtifactStoreError {
                    message: format!(
                        "plugin artifact store returned unexpected write response: {other:?}"
                    ),
                });
            }
        }
        let stored = self.read(&path)?;
        if ArtifactRevision::from_content(&stored) != artifact.revision {
            return Err(PluginArtifactStoreError {
                message: format!("plugin artifact verification failed after storing {path}"),
            });
        }
        Ok(path)
    }
}

struct RuntimeOrchestrationTrace<'a> {
    kind: &'a str,
    operation: &'a str,
    target_session: Option<&'a SessionId>,
    child_execution: Option<&'a str>,
    selected_generation: &'a GenerationId,
    target_generation: Option<&'a GenerationId>,
}

fn record_runtime_orchestration(
    context: &ApplicationAgentToolContext<'_, '_>,
    run: &ApplicationAgentToolRun,
    trace: RuntimeOrchestrationTrace<'_>,
    result: &Result<PhenixValue, ApplicationError>,
) {
    context
        .kernel
        .record_runtime_trace(phenix_core::RuntimeTraceEvent::Orchestration {
            controller_session: run.session_id.to_string(),
            controller_execution: run.execution_id.clone(),
            kind: trace.kind.to_owned(),
            operation: trace.operation.to_owned(),
            target_session: trace.target_session.map(ToString::to_string),
            child_execution: trace.child_execution.map(str::to_owned),
            selected_generation: trace.selected_generation.as_str().to_owned(),
            target_generation: trace
                .target_generation
                .map(|generation| generation.as_str().to_owned()),
            success: result.is_ok(),
            error: result.as_ref().err().map(|error| format!("{error:?}")),
        });
}

fn orchestration_output_string(output: &PhenixValue, key: &str) -> Option<String> {
    match output {
        PhenixValue::Map(fields) => fields.get(key),
        PhenixValue::Table(fields) => fields.get(key),
        _ => None,
    }
    .and_then(|value| match value {
        PhenixValue::String(value) => Some(value.clone()),
        _ => None,
    })
}

fn execute_runtime_plugin_control(
    context: &ApplicationAgentToolContext<'_, '_>,
    run: &ApplicationAgentToolRun,
    operation: &str,
    arguments: &PhenixValue,
) -> Result<PhenixValue, ApplicationError> {
    let mut target_generation = None;
    let result = (|| -> Result<PhenixValue, ApplicationError> {
        let harness = run
            .harness
            .upgrade()
            .ok_or(ApplicationError::Disconnected)?;
        match operation {
            "inspect" => {
                require_runtime_plugin_capability(
                    context.call.authority,
                    RUNTIME_PLUGIN_INSPECT_PERMISSION,
                )?;
                let harness = harness.lock();
                runtime_plugin_inspection_value(&harness)
            }
            "build" => {
                require_runtime_plugin_capability(
                    context.call.authority,
                    RUNTIME_PLUGIN_BUILD_PERMISSION,
                )?;
                let mut plan: PluginBuildPlan = runtime_plugin_typed_argument(arguments, "plan")?;
                apply_runtime_plugin_default_build_authority(&mut plan)?;
                let policy = runtime_plugin_policy(RUNTIME_PLUGIN_BUILD_PERMISSION);
                let mut store = WorkspacePluginArtifactStore {
                    context,
                    authority: context.call.authority,
                };
                let mut executor = WorkspacePluginBuildExecutor { context };
                let mut management = PluginManagementContext {
                    caller_authority: context.call.authority,
                    policy: &policy,
                    artifact_store: &mut store,
                    build_executor: &mut executor,
                };
                let report =
                    GraphReconciler::build_artifact(plan, &mut management).map_err(|error| {
                        ApplicationError::Failed {
                            message: error.to_string(),
                        }
                    })?;
                Ok(runtime_plugin_build_report_value(&report))
            }
            "trial" => {
                require_runtime_plugin_capability(
                    context.call.authority,
                    RUNTIME_PLUGIN_TRIAL_PERMISSION,
                )?;
                let mut request: PluginLoadRequest =
                    runtime_plugin_typed_argument(arguments, "request")?;
                if let PluginExecution::Runtime {
                    artifact: PluginArtifactInput::Build(plan),
                    ..
                } = &mut request.manifest.execution
                {
                    apply_runtime_plugin_default_build_authority(plan)?;
                }
                let plugin = request.manifest.id.clone();
                let ready_artifact_revision = match &request.manifest.execution {
                    PluginExecution::Runtime {
                        artifact: PluginArtifactInput::Ready(artifact),
                        ..
                    } => Some(artifact.revision.as_ref().to_owned()),
                    PluginExecution::Embedded
                    | PluginExecution::ResourceOnly
                    | PluginExecution::Runtime {
                        artifact: PluginArtifactInput::Build(_),
                        ..
                    } => None,
                };
                let policy = runtime_plugin_policy(RUNTIME_PLUGIN_TRIAL_PERMISSION);
                let mut store = WorkspacePluginArtifactStore {
                    context,
                    authority: context.call.authority,
                };
                let mut executor = WorkspacePluginBuildExecutor { context };
                let mut management = PluginManagementContext {
                    caller_authority: context.call.authority,
                    policy: &policy,
                    artifact_store: &mut store,
                    build_executor: &mut executor,
                };
                let result = harness
                    .lock()
                    .trial_plugin_management(
                        PluginManagementRequest::load(request),
                        run.root_constraints.authority(),
                        &run.root_constraints,
                        &mut management,
                    )
                    .map_err(|error| ApplicationError::Failed {
                        message: error.to_string(),
                    })?;
                target_generation = Some(result.generation.clone());
                let artifact_revision = result
                    .build
                    .as_ref()
                    .map(|report| report.artifact.revision.as_ref().to_owned())
                    .or(ready_artifact_revision);
                Ok(PhenixValue::Map(BTreeMap::from([
                    (
                        "generation".to_owned(),
                        PhenixValue::String(result.generation.as_str().to_owned()),
                    ),
                    ("plugin".to_owned(), PhenixValue::String(plugin.to_string())),
                    (
                        "artifact_revision".to_owned(),
                        PhenixValue::Option(
                            artifact_revision
                                .map(|revision| Box::new(PhenixValue::String(revision))),
                        ),
                    ),
                    (
                        "diff".to_owned(),
                        runtime_reconciliation_preview_value(&result.preview),
                    ),
                    (
                        "build".to_owned(),
                        PhenixValue::Option(
                            result
                                .build
                                .as_ref()
                                .map(|report| Box::new(runtime_plugin_build_report_value(report))),
                        ),
                    ),
                ])))
            }
            "promote" | "rollback" => {
                require_runtime_plugin_capability(
                    context.call.authority,
                    RUNTIME_PLUGIN_PROMOTE_PERMISSION,
                )?;
                let generation = runtime_plugin_generation_argument(arguments)?;
                target_generation = Some(generation.clone());
                let constraints = run.root_constraints.clone();
                let result = harness
                    .lock()
                    .promote_resident(&generation, &constraints)
                    .map_err(|error| ApplicationError::Failed {
                        message: error.to_string(),
                    })?;
                Ok(PhenixValue::Map(BTreeMap::from([
                    (
                        "previous_generation".to_owned(),
                        PhenixValue::String(result.previous_generation.as_str().to_owned()),
                    ),
                    (
                        "active_generation".to_owned(),
                        PhenixValue::String(result.active_generation.as_str().to_owned()),
                    ),
                ])))
            }
            "retire" => {
                require_runtime_plugin_capability(
                    context.call.authority,
                    RUNTIME_PLUGIN_RETIRE_PERMISSION,
                )?;
                let generation = runtime_plugin_generation_argument(arguments)?;
                target_generation = Some(generation.clone());
                let constraints = run.root_constraints.clone();
                harness
                    .lock()
                    .retire_resident(&generation, &constraints)
                    .map_err(|error| ApplicationError::Failed {
                        message: error.to_string(),
                    })?;
                Ok(PhenixValue::Map(BTreeMap::from([(
                    "retired_generation".to_owned(),
                    PhenixValue::String(generation.as_str().to_owned()),
                )])))
            }
            other => Err(ApplicationError::InvalidInput {
                message: format!(
                    "unknown phenix.plugin operation {other}; expected inspect, build, trial, promote, rollback, or retire"
                ),
            }),
        }
    })();

    record_runtime_orchestration(
        context,
        run,
        RuntimeOrchestrationTrace {
            kind: "plugin",
            operation,
            target_session: None,
            child_execution: None,
            selected_generation: &run.root_generation,
            target_generation: target_generation.as_ref(),
        },
        &result,
    );
    result
}

fn runtime_plugin_generation_argument(
    arguments: &PhenixValue,
) -> Result<GenerationId, ApplicationError> {
    let value = session_control_required_string(arguments, "generation")?;
    Ok(GenerationId::from(value))
}

fn runtime_plugin_inspection_value(
    harness: &PhenixRuntime,
) -> Result<PhenixValue, ApplicationError> {
    let active = harness.generation().clone();
    let generations = harness
        .selectable_generations()
        .into_iter()
        .map(|generation| {
            let resolved = harness
                .resolved_generation_by_id(&generation)
                .map_err(|error| ApplicationError::Failed {
                    message: error.to_string(),
                })?;
            let plugins = resolved
                .plugins()
                .iter()
                .map(|manifest| {
                    let (execution_kind, runtime, artifact_revision) = match &manifest.execution {
                        PluginExecution::Embedded => ("embedded", None, None),
                        PluginExecution::ResourceOnly => ("resource_only", None, None),
                        PluginExecution::Runtime { runtime, artifact } => (
                            "runtime",
                            Some(runtime.as_str().to_owned()),
                            Some(artifact.revision.as_ref().to_owned()),
                        ),
                    };
                    Ok(PhenixValue::Map(BTreeMap::from([
                        (
                            "plugin".to_owned(),
                            PhenixValue::String(manifest.id.to_string()),
                        ),
                        (
                            "version".to_owned(),
                            PhenixValue::U64(u64::from(manifest.version)),
                        ),
                        (
                            "execution_kind".to_owned(),
                            PhenixValue::String(execution_kind.to_owned()),
                        ),
                        (
                            "runtime".to_owned(),
                            PhenixValue::Option(
                                runtime.map(|value| Box::new(PhenixValue::String(value))),
                            ),
                        ),
                        (
                            "artifact_revision".to_owned(),
                            PhenixValue::Option(
                                artifact_revision.map(|value| Box::new(PhenixValue::String(value))),
                            ),
                        ),
                    ])))
                })
                .collect::<Result<Vec<_>, ApplicationError>>()?;
            Ok(PhenixValue::Map(BTreeMap::from([
                (
                    "generation".to_owned(),
                    PhenixValue::String(generation.as_str().to_owned()),
                ),
                (
                    "default".to_owned(),
                    PhenixValue::Bool(generation == active),
                ),
                ("plugins".to_owned(), PhenixValue::List(plugins)),
            ])))
        })
        .collect::<Result<Vec<_>, ApplicationError>>()?;
    Ok(PhenixValue::Map(BTreeMap::from([
        (
            "active_generation".to_owned(),
            PhenixValue::String(active.as_str().to_owned()),
        ),
        ("generations".to_owned(), PhenixValue::List(generations)),
    ])))
}

fn execute_runtime_session_tool_call(
    context: &ApplicationAgentToolContext<'_, '_>,
    run: &ApplicationAgentToolRun,
    call: &ModelToolCall,
) -> ExecutionChange {
    let result = (|| -> Result<PhenixValue, ApplicationError> {
        require_application_session_control(context.call.authority)?;
        let PhenixValue::Table(fields) = &call.input else {
            return Err(ApplicationError::InvalidInput {
                message: "phenix.session input must contain operation and arguments".to_owned(),
            });
        };
        let operation = match fields.get("operation") {
            Some(PhenixValue::String(operation)) if !operation.trim().is_empty() => {
                operation.trim()
            }
            _ => {
                return Err(ApplicationError::InvalidInput {
                    message: "phenix.session operation must be a non-empty string".to_owned(),
                });
            }
        };
        let arguments = fields
            .get("arguments")
            .ok_or_else(|| ApplicationError::InvalidInput {
                message: "phenix.session input is missing arguments".to_owned(),
            })?;
        execute_application_session_control(context, run, operation, arguments)
    })();

    match result {
        Ok(output) => ExecutionChange::ToolResult {
            call_id: call.call_id.clone(),
            output,
        },
        Err(error) => ExecutionChange::ToolFailed {
            call_id: call.call_id.clone(),
            error,
        },
    }
}

fn require_application_session_control(authority: &Authority) -> Result<(), ApplicationError> {
    let capability = PermissionId::parse(APPLICATION_SESSION_CONTROL_PERMISSION)
        .expect("static application session control permission is valid");
    if authority.permits(&capability) {
        Ok(())
    } else {
        Err(ApplicationError::PermissionDenied {
            message: format!("phenix.session requires {APPLICATION_SESSION_CONTROL_PERMISSION}"),
        })
    }
}

fn child_session_working_directory(
    controller: &SessionInfo,
    requested: Option<String>,
) -> Result<String, ApplicationError> {
    match requested {
        None => Ok(controller.working_directory.clone()),
        Some(requested) if requested == controller.working_directory => Ok(requested),
        Some(requested) => Err(ApplicationError::Conflict {
            message: format!(
                "phenix.session create cannot change the pinned Environment working directory from {} to {requested}; use the controller working directory or create a separately authorized root",
                controller.working_directory
            ),
        }),
    }
}

fn execute_application_session_control(
    context: &ApplicationAgentToolContext<'_, '_>,
    run: &ApplicationAgentToolRun,
    operation: &str,
    arguments: &PhenixValue,
) -> Result<PhenixValue, ApplicationError> {
    let mut target_session = None;
    let mut child_execution = None;
    let mut selected_generation = run.root_generation.clone();
    let result = (|| -> Result<PhenixValue, ApplicationError> {
        match operation {
            "create" => {
                let sessions: SessionList =
                    invoke_application_control(run, ListSessions::ID, PageInput { cursor: None })?;
                let controller = sessions
                    .sessions
                    .into_iter()
                    .find(|session| session.session_id == run.session_id)
                    .ok_or_else(|| ApplicationError::NotFound {
                        resource: run.session_id.to_string(),
                    })?;
                let working_directory = child_session_working_directory(
                    &controller,
                    session_control_optional_string(arguments, "working_directory")?,
                )?;
                let title = session_control_optional_string(arguments, "title")?;
                let response: SessionInfo = invoke_application_control(
                    run,
                    CreateSession::ID,
                    SessionCreateInput {
                        working_directory,
                        title,
                    },
                )?;
                target_session = Some(response.session_id.clone());
                Ok(response.to_value())
            }
            "list" => {
                let response: SessionList =
                    invoke_application_control(run, ListSessions::ID, PageInput { cursor: None })?;
                Ok(response.to_value())
            }
            "resume" => {
                let session_id = session_control_session_id(arguments)?;
                target_session = Some(session_id.clone());
                let after_sequence = session_control_optional_u64(arguments, "after_sequence")?;
                let response: SessionSnapshot = invoke_application_control(
                    run,
                    ResumeSession::ID,
                    SessionResumeInput {
                        session_id,
                        after_sequence,
                    },
                )?;
                Ok(response.to_value())
            }
            "prompt" => {
                let session_id = session_control_session_id(arguments)?;
                target_session = Some(session_id.clone());
                if session_id == run.session_id {
                    return Err(ApplicationError::Conflict {
                        message:
                            "phenix.session cannot synchronously prompt its own active session"
                                .to_owned(),
                    });
                }
                let content = session_control_content(arguments)?;
                let generation = match session_control_optional_string(arguments, "generation")? {
                    Some(generation) => {
                        require_runtime_generation_select(context.call.authority)?;
                        GenerationId::from(generation)
                    }
                    None => run.root_generation.clone(),
                };
                selected_generation = generation.clone();
                let output = prompt_child_session(run, session_id, content, generation)?;
                child_execution = orchestration_output_string(&output, "execution_id");
                Ok(output)
            }
            "close" => {
                let session_id = session_control_session_id(arguments)?;
                target_session = Some(session_id.clone());
                if session_id == run.session_id {
                    return Err(ApplicationError::Conflict {
                        message: "phenix.session cannot close its own active session".to_owned(),
                    });
                }
                let response: Acknowledged = invoke_application_control(
                    run,
                    CloseSession::ID,
                    ApplicationSessionInput { session_id },
                )?;
                Ok(response.to_value())
            }
            other => Err(ApplicationError::InvalidInput {
                message: format!(
                    "unknown phenix.session operation {other}; expected create, list, resume, prompt, or close"
                ),
            }),
        }
    })();

    record_runtime_orchestration(
        context,
        run,
        RuntimeOrchestrationTrace {
            kind: "session",
            operation,
            target_session: target_session.as_ref(),
            child_execution: child_execution.as_deref(),
            selected_generation: &selected_generation,
            target_generation: None,
        },
        &result,
    );
    result
}

fn require_runtime_generation_select(authority: &Authority) -> Result<(), ApplicationError> {
    let capability = PermissionId::parse(RUNTIME_GENERATION_SELECT_PERMISSION)
        .expect("static runtime generation selection permission is valid");
    if authority.permits(&capability) {
        Ok(())
    } else {
        Err(ApplicationError::PermissionDenied {
            message: format!(
                "explicit phenix.session generation selection requires {RUNTIME_GENERATION_SELECT_PERMISSION}"
            ),
        })
    }
}

fn prompt_child_session(
    run: &ApplicationAgentToolRun,
    session_id: SessionId,
    content: Vec<Content>,
    generation: GenerationId,
) -> Result<PhenixValue, ApplicationError> {
    if run.cancellation.load(Ordering::Acquire) {
        return Err(ApplicationError::Cancelled);
    }

    let operation = ContractId::parse(Prompt::ID).map_err(|error| ApplicationError::Failed {
        message: error.to_string(),
    })?;
    let constraints = run.root_constraints.clone();
    let mut pending = run.control_transport.begin_blocking_in_generation(
        &operation,
        PromptInput {
            session_id: session_id.clone(),
            content,
        }
        .to_value(),
        generation.clone(),
        constraints.clone(),
    )?;

    let mut cancellation_requested = false;
    let response = loop {
        if let Some(response) = pending.try_recv()? {
            break response;
        }
        if run.cancellation.load(Ordering::Acquire) && !cancellation_requested {
            cancellation_requested = true;
            let _: Acknowledged = invoke_application_control_in_generation(
                run,
                Cancel::ID,
                ApplicationSessionInput {
                    session_id: session_id.clone(),
                },
                generation.clone(),
                constraints.clone(),
            )?;
        }
        std::thread::sleep(std::time::Duration::from_millis(1));
    };

    let prompt = PromptResult::try_from(Project(&response)).map_err(|error| {
        ApplicationError::InvalidResponse {
            message: error.to_string(),
        }
    })?;
    let snapshot: SessionSnapshot = invoke_application_control_in_generation(
        run,
        ResumeSession::ID,
        SessionResumeInput {
            session_id: session_id.clone(),
            after_sequence: None,
        },
        generation.clone(),
        constraints,
    )?;
    let assistant_message = snapshot
        .updates
        .iter()
        .rev()
        .find_map(|update| match &update.update {
            SessionChange::TextDelta { execution_id, text }
                if execution_id == &prompt.execution_id =>
            {
                Some(text.clone())
            }
            _ => None,
        });

    Ok(PhenixValue::Map(BTreeMap::from([
        (
            "session_id".to_owned(),
            PhenixValue::String(session_id.to_string()),
        ),
        (
            "execution_id".to_owned(),
            PhenixValue::String(prompt.execution_id),
        ),
        (
            "generation".to_owned(),
            PhenixValue::String(generation.as_str().to_owned()),
        ),
        (
            "stop_reason".to_owned(),
            PhenixValue::String(
                match prompt.stop_reason {
                    StopReason::EndTurn => "end_turn",
                    StopReason::Cancelled => "cancelled",
                    StopReason::MaxTokens => "max_tokens",
                    StopReason::Refused => "refused",
                }
                .to_owned(),
            ),
        ),
        (
            "through_sequence".to_owned(),
            PhenixValue::U64(snapshot.through_sequence),
        ),
        (
            "assistant_message".to_owned(),
            PhenixValue::Option(assistant_message.map(|text| Box::new(PhenixValue::String(text)))),
        ),
    ])))
}

fn invoke_application_control<C, R>(
    run: &ApplicationAgentToolRun,
    operation: &str,
    command: C,
) -> Result<R, ApplicationError>
where
    for<'a> PhenixValue: From<&'a C>,
    R: for<'a> TryFrom<Project<&'a PhenixValue>, Error = phenix_core::ValueError>,
{
    let operation = ContractId::parse(operation).map_err(|error| ApplicationError::Failed {
        message: error.to_string(),
    })?;
    let output = run.control_transport.invoke_blocking_in_generation(
        &operation,
        PhenixValue::from(&command),
        run.root_generation.clone(),
        run.root_constraints.clone(),
    )?;
    R::try_from(Project(&output)).map_err(|error| ApplicationError::InvalidResponse {
        message: error.to_string(),
    })
}

fn invoke_application_control_in_generation<C, R>(
    run: &ApplicationAgentToolRun,
    operation: &str,
    command: C,
    generation: GenerationId,
    constraints: phenix_core::RootExecutionConstraints,
) -> Result<R, ApplicationError>
where
    for<'a> PhenixValue: From<&'a C>,
    R: for<'a> TryFrom<Project<&'a PhenixValue>, Error = phenix_core::ValueError>,
{
    let operation = ContractId::parse(operation).map_err(|error| ApplicationError::Failed {
        message: error.to_string(),
    })?;
    let output = run.control_transport.invoke_blocking_in_generation(
        &operation,
        PhenixValue::from(&command),
        generation,
        constraints,
    )?;
    R::try_from(Project(&output)).map_err(|error| ApplicationError::InvalidResponse {
        message: error.to_string(),
    })
}

fn session_control_session_id(arguments: &PhenixValue) -> Result<SessionId, ApplicationError> {
    let value = session_control_required_string(arguments, "session_id")?;
    SessionId::parse(value).map_err(|error| ApplicationError::InvalidInput {
        message: error.to_string(),
    })
}

fn session_control_content(arguments: &PhenixValue) -> Result<Vec<Content>, ApplicationError> {
    let value = session_control_field(arguments, "content").ok_or_else(|| {
        ApplicationError::InvalidInput {
            message: "phenix.session argument content is required".to_owned(),
        }
    })?;
    let PhenixValue::List(parts) = value else {
        return Err(ApplicationError::InvalidInput {
            message: "phenix.session argument content must be a list of application content parts"
                .to_owned(),
        });
    };
    parts.iter().map(session_control_content_part).collect()
}

fn session_control_content_part(part: &PhenixValue) -> Result<Content, ApplicationError> {
    if let Ok(content) = Content::from_value(part) {
        return Ok(content);
    }

    let kind = session_control_required_string(part, "kind")?;
    match kind.as_str() {
        "text" => Ok(Content::Text {
            text: session_control_required_string(part, "text")?,
        }),
        "resource" => Ok(Content::Resource {
            uri: session_control_required_string(part, "uri")?,
            mime_type: session_control_optional_string(part, "mime_type")?,
            text: session_control_optional_string(part, "text")?,
        }),
        "image" => {
            let data = match session_control_field(part, "data") {
                Some(PhenixValue::Bytes(data)) => Bytes::new(data.clone()),
                Some(PhenixValue::String(data)) => Bytes::new(data.as_bytes().to_vec()),
                Some(_) => {
                    return Err(ApplicationError::InvalidInput {
                        message: "phenix.session image data must be bytes or a string".to_owned(),
                    });
                }
                None => {
                    return Err(ApplicationError::InvalidInput {
                        message: "phenix.session image data is required".to_owned(),
                    });
                }
            };
            Ok(Content::Image {
                mime_type: session_control_required_string(part, "mime_type")?,
                data,
            })
        }
        other => Err(ApplicationError::InvalidInput {
            message: format!(
                "unsupported phenix.session content kind {other}; expected text, image, or resource"
            ),
        }),
    }
}

fn session_control_required_string(
    arguments: &PhenixValue,
    key: &str,
) -> Result<String, ApplicationError> {
    match session_control_field(arguments, key) {
        Some(PhenixValue::String(value)) if !value.trim().is_empty() => Ok(value.clone()),
        Some(_) => Err(ApplicationError::InvalidInput {
            message: format!("phenix.session argument {key} must be a non-empty string"),
        }),
        None => Err(ApplicationError::InvalidInput {
            message: format!("phenix.session argument {key} is required"),
        }),
    }
}

fn session_control_optional_string(
    arguments: &PhenixValue,
    key: &str,
) -> Result<Option<String>, ApplicationError> {
    match session_control_field(arguments, key) {
        None | Some(PhenixValue::Unit) | Some(PhenixValue::Option(None)) => Ok(None),
        Some(PhenixValue::String(value)) => Ok(Some(value.clone())),
        Some(PhenixValue::Option(Some(value))) => match value.as_ref() {
            PhenixValue::String(value) => Ok(Some(value.clone())),
            _ => Err(ApplicationError::InvalidInput {
                message: format!("phenix.session argument {key} must be a string or null"),
            }),
        },
        Some(_) => Err(ApplicationError::InvalidInput {
            message: format!("phenix.session argument {key} must be a string or null"),
        }),
    }
}

fn session_control_optional_u64(
    arguments: &PhenixValue,
    key: &str,
) -> Result<Option<u64>, ApplicationError> {
    match session_control_field(arguments, key) {
        None | Some(PhenixValue::Unit) | Some(PhenixValue::Option(None)) => Ok(None),
        Some(PhenixValue::U64(value)) => Ok(Some(*value)),
        Some(PhenixValue::I64(value)) if *value >= 0 => Ok(Some(*value as u64)),
        Some(PhenixValue::Option(Some(value))) => match value.as_ref() {
            PhenixValue::U64(value) => Ok(Some(*value)),
            PhenixValue::I64(value) if *value >= 0 => Ok(Some(*value as u64)),
            _ => Err(ApplicationError::InvalidInput {
                message: format!(
                    "phenix.session argument {key} must be a non-negative integer or null"
                ),
            }),
        },
        Some(_) => Err(ApplicationError::InvalidInput {
            message: format!(
                "phenix.session argument {key} must be a non-negative integer or null"
            ),
        }),
    }
}

fn session_control_field<'a>(arguments: &'a PhenixValue, key: &str) -> Option<&'a PhenixValue> {
    match arguments {
        PhenixValue::Map(fields) => fields.get(key),
        PhenixValue::Table(fields) => fields.get(key),
        _ => None,
    }
}

fn execute_runtime_inspect_tool_call(
    context: &ApplicationAgentToolContext<'_, '_>,
    run: &ApplicationAgentToolRun,
    call: &ModelToolCall,
) -> ExecutionChange {
    let result = (|| -> Result<PhenixValue, ApplicationError> {
        let PhenixValue::Table(fields) = &call.input else {
            return Err(ApplicationError::InvalidInput {
                message: "phenix.inspect input must be an object with a query field".to_owned(),
            });
        };
        let query = match fields.get("query") {
            Some(PhenixValue::String(query)) if !query.trim().is_empty() => query.trim(),
            Some(_) => {
                return Err(ApplicationError::InvalidInput {
                    message: "phenix.inspect query must be a non-empty string".to_owned(),
                });
            }
            None => {
                return Err(ApplicationError::InvalidInput {
                    message: "phenix.inspect input is missing query".to_owned(),
                });
            }
        };
        inspect_runtime(context, run, query)
    })();

    match result {
        Ok(output) => ExecutionChange::ToolResult {
            call_id: call.call_id.clone(),
            output,
        },
        Err(error) => ExecutionChange::ToolFailed {
            call_id: call.call_id.clone(),
            error,
        },
    }
}

fn require_runtime_inspection_read(authority: &Authority) -> Result<(), ApplicationError> {
    let capability = PermissionId::parse(RUNTIME_INSPECTION_READ_PERMISSION)
        .expect("static runtime inspection read permission is valid");
    if authority.permits(&capability) {
        Ok(())
    } else {
        Err(ApplicationError::PermissionDenied {
            message: format!("phenix.inspect query requires {RUNTIME_INSPECTION_READ_PERMISSION}"),
        })
    }
}

fn inspect_runtime(
    context: &ApplicationAgentToolContext<'_, '_>,
    run: &ApplicationAgentToolRun,
    query: &str,
) -> Result<PhenixValue, ApplicationError> {
    match query {
        "values" => {
            require_runtime_inspection_read(context.call.authority)?;
            run.service.inspect_values()
        }
        "execution" => {
            require_runtime_inspection_read(context.call.authority)?;
            inspect_execution(context, &run.execution_id)
        }
        "dag" => {
            require_runtime_inspection_read(context.call.authority)?;
            inspect_execution_dag(context, &run.execution_id)
        }
        "graph" => Ok(inspect_component_graph(context)),
        "trace" => {
            require_runtime_inspection_read(context.call.authority)?;
            inspect_runtime_trace(context)
        }
        "help" => Ok(PhenixValue::List(
            [
                "graph",
                "execution",
                "execution <execution-id>",
                "dag",
                "dag <execution-id>",
                "trace",
                "trace <execution-id>",
                "trace-chain <execution-id>",
                "values",
                "value <value-id>",
            ]
            .into_iter()
            .map(|query| PhenixValue::String(query.to_owned()))
            .collect(),
        )),
        _ => {
            if let Some(execution_id) = query.strip_prefix("execution ").map(str::trim) {
                if execution_id.is_empty() {
                    return Err(ApplicationError::InvalidInput {
                        message: "execution query requires an execution id".to_owned(),
                    });
                }
                require_runtime_inspection_read(context.call.authority)?;
                return inspect_execution(context, execution_id);
            }
            if let Some(execution_id) = query.strip_prefix("dag ").map(str::trim) {
                if execution_id.is_empty() {
                    return Err(ApplicationError::InvalidInput {
                        message: "dag query requires an execution id".to_owned(),
                    });
                }
                require_runtime_inspection_read(context.call.authority)?;
                return inspect_execution_dag(context, execution_id);
            }
            if let Some(execution_id) = query.strip_prefix("trace ").map(str::trim) {
                if execution_id.is_empty() {
                    return Err(ApplicationError::InvalidInput {
                        message: "trace query requires an execution id".to_owned(),
                    });
                }
                require_runtime_inspection_read(context.call.authority)?;
                return inspect_runtime_trace_execution(context, execution_id);
            }
            if let Some(execution_id) = query.strip_prefix("trace-chain ").map(str::trim) {
                if execution_id.is_empty() {
                    return Err(ApplicationError::InvalidInput {
                        message: "trace-chain query requires an execution id".to_owned(),
                    });
                }
                require_runtime_inspection_read(context.call.authority)?;
                return inspect_runtime_trace_chain(context, execution_id);
            }
            if let Some(id) = query.strip_prefix("value ").map(str::trim) {
                if id.is_empty() {
                    return Err(ApplicationError::InvalidInput {
                        message: "value query requires a ValueId".to_owned(),
                    });
                }
                require_runtime_inspection_read(context.call.authority)?;
                return run.service.inspect_value(id);
            }
            if query.starts_with("execution-") {
                require_runtime_inspection_read(context.call.authority)?;
                return inspect_execution(context, query);
            }
            Err(ApplicationError::InvalidInput {
                message: format!("unknown phenix.inspect query: {query}"),
            })
        }
    }
}

fn inspect_runtime_trace(
    context: &ApplicationAgentToolContext<'_, '_>,
) -> Result<PhenixValue, ApplicationError> {
    serde_json::to_value(context.kernel.runtime_trace())
        .map(PhenixValue::from)
        .map_err(|error| ApplicationError::Failed {
            message: format!("failed to encode runtime trace: {error}"),
        })
}

fn inspect_runtime_trace_execution(
    context: &ApplicationAgentToolContext<'_, '_>,
    execution_id: &str,
) -> Result<PhenixValue, ApplicationError> {
    let events = context
        .kernel
        .runtime_trace()
        .into_iter()
        .filter(|event| event.is_associated_with_execution(execution_id))
        .collect::<Vec<_>>();
    serde_json::to_value(events)
        .map(PhenixValue::from)
        .map_err(|error| ApplicationError::Failed {
            message: format!("failed to encode execution runtime trace: {error}"),
        })
}

fn inspect_runtime_trace_chain(
    context: &ApplicationAgentToolContext<'_, '_>,
    execution_id: &str,
) -> Result<PhenixValue, ApplicationError> {
    let trace = context.kernel.runtime_trace();
    let mut executions = BTreeSet::from([execution_id.to_owned()]);
    loop {
        let before = executions.len();
        for event in &trace {
            if let phenix_core::RuntimeTraceEvent::Orchestration {
                controller_execution,
                child_execution,
                ..
            } = event
            {
                let child_included = child_execution
                    .as_ref()
                    .is_some_and(|child| executions.contains(child));
                if executions.contains(controller_execution) || child_included {
                    executions.insert(controller_execution.clone());
                    if let Some(child) = child_execution {
                        executions.insert(child.clone());
                    }
                }
            }
        }
        if executions.len() == before {
            break;
        }
    }
    let events = trace
        .into_iter()
        .filter(|event| {
            executions
                .iter()
                .any(|execution| event.is_associated_with_execution(execution))
        })
        .collect::<Vec<_>>();
    serde_json::to_value(events)
        .map(PhenixValue::from)
        .map_err(|error| ApplicationError::Failed {
            message: format!("failed to encode runtime trace chain: {error}"),
        })
}

fn inspect_execution(
    context: &ApplicationAgentToolContext<'_, '_>,
    execution_id: &str,
) -> Result<PhenixValue, ApplicationError> {
    let response = context
        .sdk
        .execution
        .invoke_projected::<ExecutionInspectionCommand, ExecutionInspectionResponse>(
            &ExecutionInspectionCommand::GetExecution {
                id: execution_id.to_owned(),
            },
        )
        .map_err(|error| ApplicationError::Failed {
            message: error.to_string(),
        })?;
    match response {
        ExecutionInspectionResponse::ExecutionLookup {
            execution: Some(execution),
        } => Ok(execution.to_value()),
        ExecutionInspectionResponse::ExecutionLookup { execution: None } => {
            Err(ApplicationError::NotFound {
                resource: execution_id.to_owned(),
            })
        }
        other => Err(ApplicationError::InvalidResponse {
            message: format!("unexpected execution inspection response: {other:?}"),
        }),
    }
}

fn inspect_execution_dag(
    context: &ApplicationAgentToolContext<'_, '_>,
    execution_id: &str,
) -> Result<PhenixValue, ApplicationError> {
    let executions = context
        .sdk
        .execution
        .invoke_projected::<ExecutionInspectionCommand, ExecutionInspectionResponse>(
            &ExecutionInspectionCommand::ListExecutions,
        )
        .map_err(|error| ApplicationError::Failed {
            message: error.to_string(),
        })?;
    let ExecutionInspectionResponse::Executions { executions } = executions else {
        return Err(ApplicationError::InvalidResponse {
            message: format!("unexpected execution-list response: {executions:?}"),
        });
    };
    let Some(root) = executions
        .iter()
        .find(|execution| execution.id == execution_id)
    else {
        return Err(ApplicationError::NotFound {
            resource: execution_id.to_owned(),
        });
    };
    let root_generation = root.graph_generation.clone();

    let mut included = BTreeSet::from([execution_id.to_owned()]);
    loop {
        let before = included.len();
        for execution in &executions {
            if execution.graph_generation == root_generation
                && execution
                    .parent_execution
                    .as_ref()
                    .is_some_and(|parent| included.contains(parent))
            {
                included.insert(execution.id.clone());
            }
        }
        if included.len() == before {
            break;
        }
    }

    let tasks = context
        .sdk
        .execution
        .invoke_projected::<ExecutionInspectionCommand, ExecutionInspectionResponse>(
            &ExecutionInspectionCommand::ListTasks,
        )
        .map_err(|error| ApplicationError::Failed {
            message: error.to_string(),
        })?;
    let ExecutionInspectionResponse::Tasks { tasks } = tasks else {
        return Err(ApplicationError::InvalidResponse {
            message: format!("unexpected task-list response: {tasks:?}"),
        });
    };

    let executions = executions
        .into_iter()
        .filter(|execution| {
            execution.graph_generation == root_generation && included.contains(&execution.id)
        })
        .map(|execution| execution.to_value())
        .collect();
    let tasks = tasks
        .into_iter()
        .filter(|task| {
            task.graph_generation == root_generation && included.contains(&task.parent_execution)
        })
        .map(|task| task.to_value())
        .collect();

    Ok(PhenixValue::Map(BTreeMap::from([
        (
            "root_execution".to_owned(),
            PhenixValue::String(execution_id.to_owned()),
        ),
        (
            "generation".to_owned(),
            PhenixValue::String(root_generation),
        ),
        ("executions".to_owned(), PhenixValue::List(executions)),
        ("tasks".to_owned(), PhenixValue::List(tasks)),
    ])))
}

fn inspect_component_graph(context: &ApplicationAgentToolContext<'_, '_>) -> PhenixValue {
    let graph = context.kernel.component_graph();
    let components = graph
        .components()
        .map(|component| {
            let imports = component
                .imports
                .iter()
                .map(|import| {
                    let mut value = BTreeMap::from([
                        (
                            "interface".to_owned(),
                            PhenixValue::String(import.interface.to_string()),
                        ),
                        ("required".to_owned(), PhenixValue::Bool(import.required)),
                    ]);
                    if let Some(binding) = &import.binding {
                        value.insert(
                            "provider_component".to_owned(),
                            PhenixValue::String(binding.exporter().to_string()),
                        );
                        value.insert(
                            "provider_plugin".to_owned(),
                            PhenixValue::String(binding.owning_plugin().to_string()),
                        );
                        value.insert(
                            "provider_execution".to_owned(),
                            PhenixValue::String(format!("{:?}", binding.execution())),
                        );
                        value.insert(
                            "effective_authority".to_owned(),
                            PhenixValue::List(
                                binding
                                    .effective_authority()
                                    .permissions()
                                    .map(|capability| PhenixValue::String(capability.to_string()))
                                    .collect(),
                            ),
                        );
                    }
                    PhenixValue::Map(value)
                })
                .collect();
            PhenixValue::Map(BTreeMap::from([
                (
                    "component".to_owned(),
                    PhenixValue::String(component.id.to_string()),
                ),
                (
                    "plugin".to_owned(),
                    PhenixValue::String(component.owning_plugin.to_string()),
                ),
                (
                    "execution".to_owned(),
                    PhenixValue::String(format!("{:?}", component.execution)),
                ),
                ("imports".to_owned(), PhenixValue::List(imports)),
            ]))
        })
        .collect();

    let triggers = context
        .kernel
        .entry_triggers()
        .iter()
        .map(|trigger| {
            let (kind, callable) = match &trigger.trigger {
                EntryTriggerKind::ToolCall { callable_id, .. } => (
                    "tool_call".to_owned(),
                    Some(PhenixValue::String(callable_id.to_string())),
                ),
            };
            let mut value = BTreeMap::from([
                (
                    "component".to_owned(),
                    PhenixValue::String(trigger.component.to_string()),
                ),
                (
                    "interface".to_owned(),
                    PhenixValue::String(trigger.interface.to_string()),
                ),
                ("kind".to_owned(), PhenixValue::String(kind)),
                (
                    "required_authority".to_owned(),
                    PhenixValue::List(
                        trigger
                            .required_authority
                            .permissions()
                            .map(|capability| PhenixValue::String(capability.to_string()))
                            .collect(),
                    ),
                ),
            ]);
            if let Some(callable) = callable {
                value.insert("callable".to_owned(), callable);
            }
            PhenixValue::Map(value)
        })
        .collect();

    let generation = context
        .call
        .graph_generation
        .map(|generation| generation.as_str().to_owned())
        .unwrap_or_else(|| "unresolved".to_owned());
    PhenixValue::Map(BTreeMap::from([
        ("generation".to_owned(), PhenixValue::String(generation)),
        ("components".to_owned(), PhenixValue::List(components)),
        ("entry_triggers".to_owned(), PhenixValue::List(triggers)),
    ]))
}

fn invoke_permission_handler(
    service: &SdkApplicationService,
    handler: Option<&PermissionHandlerRef>,
    request: PermissionRequest,
) -> Result<PermissionResponse, ApplicationError> {
    let handler = handler.ok_or_else(|| ApplicationError::PermissionDenied {
        message: "client tool requires permission but no permission handler is registered"
            .to_owned(),
    })?;
    let operation = ContractId::parse(InvokeCallableReference::ID)
        .expect("static application capability invocation id is valid");
    let output = service.invoke(
        &operation,
        CallableInvocation {
            callable: handler.to_value(),
            input: request.to_value(),
        }
        .to_value(),
    )?;
    let result = CallableInvocationResult::from_value(&output).map_err(|error| {
        ApplicationError::InvalidResponse {
            message: error.to_string(),
        }
    })?;
    PermissionResponse::from_value(&result.output).map_err(|error| {
        ApplicationError::InvalidResponse {
            message: error.to_string(),
        }
    })
}

fn is_sdk_operation(operation: &ContractId) -> bool {
    matches!(
        operation.as_str(),
        GetSdk::ID
            | AddClientTool::ID
            | RemoveClientTool::ID
            | ListCallables::ID
            | InvokeCallable::ID
            | InvokeCallableReference::ID
    )
}

fn configured_capabilities() -> Vec<ContractId> {
    [
        "discovery",
        "authentication",
        "sessions",
        "session-list",
        "session-resume",
        "session-rename",
        "prompt",
        "routing",
        "sdk",
        "capabilities",
        "callables",
        "client-tools",
        "interaction",
        "review",
        "logs",
    ]
    .into_iter()
    .map(|name| {
        ContractId::parse(format!("phenix.application.capability.{name}@1"))
            .expect("static configured capability id is valid")
    })
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use phenix_application_interface::{
        AddClientTool, ApplicationTransport, Cancel, CloseSession, CreateSession,
        DiscoverAuthentication, ListSessions, Prompt, RenameSession, ResumeSession,
        types::{ClientToolAddInput, ClientToolDefinition, Content, Empty},
    };
    use phenix_core::{
        BuildEnvironment, BuildWorkingDirectory, Bytes, CallableId, CallableRef, DurableSchema,
        DurableSchemaRegistration, InvocationOutcome, LocalPersistence, ModelFeatureGenerationId,
        ModelId, ModelInferenceFailure, ModelToolTurn, PluginArtifactInput, PluginBuildSource,
        PluginBuildStep, PluginRuntimeAdapter, PluginRuntimeCandidate, ReferenceId,
        ReferenceOwnerId, ResourceNamespace, SessionId, SkillCommand, SkillDefinition, SkillId,
        SkillResponse, TransactionOp, Type, ValueAddress, plugin_runtime_adapter_service,
        skill_service,
    };
    use phenix_plugin_catalog::{
        ModelInferenceRequest, ModelInferenceResponse, model_inference_service,
        workspace_factory_for, workspace_manifest,
    };
    use phenix_sdk::{
        CapacityKnowledge, CodeEntityFacetRevisions, CodeEntityRevision, CodeQueryAnchor,
        CodeQueryBudget, CodeQueryProjection, CodeQuerySelection, ContextControl,
        DocumentProvenance, EffectiveModelFeatures, ExecutionRecord, LanguageDocumentIdentity,
        LogicalCodeEntity, MemoryKind, MemoryScope, MemorySourceReference, ModelLimits,
        ProviderEpoch,
    };
    use std::{
        fs,
        path::PathBuf,
        sync::{
            Condvar, Mutex as StdMutex,
            atomic::{AtomicU32, Ordering as AtomicOrdering},
        },
        time::{Duration, SystemTime, UNIX_EPOCH},
    };

    fn session(id: &str, title: Option<&str>) -> SessionInfo {
        SessionInfo {
            session_id: SessionId::parse(id).unwrap(),
            title: title.map(str::to_owned),
            working_directory: "/workspace".into(),
        }
    }

    fn invoke_operation<O: Operation>(
        worker: &mut ApplicationWorker,
        input: O::Input,
    ) -> Result<O::Output, ApplicationError> {
        let value = worker.invoke(&ContractId::parse(O::ID).unwrap(), input.to_value())?;
        O::Output::from_value(&value).map_err(|error| ApplicationError::InvalidResponse {
            message: error.to_string(),
        })
    }

    fn invoke_agent_tool(
        worker: &ApplicationWorker,
        execution_id: &str,
        session_id: &SessionId,
        call: ModelToolCall,
    ) -> ModelToolResult {
        let request = AgentToolExecutionRequest {
            execution_id: execution_id.to_owned(),
            session_id: Some(session_id.clone()),
            call,
        };
        let output = worker
            .harness
            .lock()
            .invoke(
                &agent_tool_execution_service(),
                &serde_json::to_vec(&PhenixValue::from(&request)).unwrap(),
                &worker.authority,
                None,
            )
            .unwrap();
        let value: PhenixValue = serde_json::from_slice(&output).unwrap();
        let response = AgentToolExecutionResponse::try_from(Project(&value)).unwrap();
        let AgentToolExecutionResponse::Completed { result, .. } = response else {
            panic!("application agent tool must complete");
        };
        result
    }

    fn application_worker() -> ApplicationWorker {
        let mut harness = PhenixRuntime::default_suite().unwrap();
        harness.activate().unwrap();
        ApplicationWorker::new(harness).unwrap()
    }

    fn enable_runtime_orchestration(worker: &ApplicationWorker) {
        let response = worker
            .invoke_option_command(OptionCommand::Set {
                key: OptionKey::parse(RUNTIME_ORCHESTRATION_OPTION).unwrap(),
                scope: OptionScope::Global,
                value: OptionValue::Bool(true),
            })
            .unwrap();
        assert!(matches!(response, OptionResponse::Updated { .. }));
    }

    #[test]
    fn application_agent_tool_imports_bind_required_providers_and_authority() {
        let harness = crate::PhenixRuntimeBuilder::with_default_suite()
            .unwrap()
            .build()
            .unwrap();
        let graph = harness.component_graph();
        let read = PermissionId::parse("kernel.persistence.read").unwrap();
        let write = PermissionId::parse("kernel.persistence.write").unwrap();
        let schema = PermissionId::parse("kernel.persistence.schema").unwrap();
        let shell = PermissionId::parse("workspace.shell").unwrap();

        let language = graph
            .import_handle(
                &application_agent_tool_component_id(),
                &LanguageInterface::interface_id(),
            )
            .unwrap()
            .expect("application code query binds the language provider");
        assert_eq!(
            language.owning_plugin(),
            &PluginId::parse("phenix.language").unwrap()
        );
        assert!(language.effective_authority().permits(&read));
        assert!(!language.effective_authority().permits(&write));
        assert!(!language.effective_authority().permits(&schema));
        assert!(!language.effective_authority().permits(&shell));

        let memory = graph
            .import_handle(
                &application_agent_tool_component_id(),
                &MemoryInterface::interface_id(),
            )
            .unwrap()
            .expect("application memory tools bind the memory provider");
        assert_eq!(
            memory.owning_plugin(),
            &PluginId::parse("phenix.memory").unwrap()
        );
        for capability in [&schema, &read, &write] {
            assert!(memory.effective_authority().permits(capability));
        }
        assert!(!memory.effective_authority().permits(&shell));
    }

    #[test]
    fn full_product_enables_runtime_orchestration_until_explicitly_disabled() {
        let builder = crate::PhenixRuntimeBuilder::with_selected_suite(&BTreeSet::from([
            "phenix.product.full".to_owned(),
        ]))
        .unwrap();
        let mut harness = builder.build().unwrap();
        harness.activate().unwrap();
        let worker = ApplicationWorker::new(harness).unwrap();
        let session_id = SessionId::parse("session-full-product-orchestration").unwrap();

        let authority = worker.application_root_authority(&session_id).unwrap();
        for capability in runtime_orchestration_authority().permissions() {
            assert!(
                authority.permits(capability),
                "full product missed {capability}"
            );
        }

        let response = worker
            .invoke_option_command(OptionCommand::Set {
                key: OptionKey::parse(RUNTIME_ORCHESTRATION_OPTION).unwrap(),
                scope: OptionScope::Global,
                value: OptionValue::Bool(false),
            })
            .unwrap();
        assert!(matches!(response, OptionResponse::Updated { .. }));

        let authority = worker.application_root_authority(&session_id).unwrap();
        for capability in runtime_orchestration_authority().permissions() {
            assert!(
                !authority.permits(capability),
                "explicit orchestration disable still granted {capability}"
            );
        }
    }

    #[tokio::test]
    async fn root_qualified_session_operation_never_falls_back_to_default_generation() {
        let mut worker = application_worker();
        let created = invoke_operation::<CreateSession>(
            &mut worker,
            SessionCreateInput {
                working_directory: "/workspace".into(),
                title: Some("qualified-root-regression".into()),
            },
        )
        .unwrap();

        let sdk = phenix_core::ResolvedSdkContributions::resolve(
            &[],
            &[],
            Vec::<phenix_core::SdkContribution>::new(),
        )
        .unwrap();
        let (callbacks, _callback_receiver) = ClientCallableCallbacks::bounded(1);
        let service = SdkApplicationService::new(
            &sdk,
            worker.projection().store(),
            SharedCallableRegistry::default(),
            PluginRuntimeId::parse("fixture-qualified-root-runtime").unwrap(),
            ReferenceGenerationId::parse("fixture-qualified-root-generation").unwrap(),
            callbacks,
            ClientReferenceIdentity::new(
                ClientConnectionId::parse("fixture-qualified-root-client").unwrap(),
                ReferenceGenerationId::parse("fixture-qualified-root-client-generation").unwrap(),
            ),
        )
        .unwrap();

        let constraints = {
            let harness = worker.harness.lock();
            harness
                .root_execution_handle(&worker.authority)
                .constraints()
                .clone()
        };
        let operation = ContractId::parse(ResumeSession::ID).unwrap();
        let input = SessionResumeInput {
            session_id: created.session_id,
            after_sequence: None,
        }
        .to_value();
        let missing_generation = GenerationId::from("fixture-missing-generation".to_owned());
        let (control_transport, mut control_receiver) = ChannelTransport::new(1);
        let weak_control_transport = control_transport.downgrade();
        let caller_transport = control_transport.clone();
        let call = tokio::task::spawn_blocking(move || {
            caller_transport.invoke_blocking_in_generation(
                &operation,
                input,
                missing_generation,
                constraints,
            )
        });
        let invocation = control_receiver
            .recv()
            .await
            .expect("qualified application invocation");
        let (execution_sender, _execution_receiver) =
            mpsc::channel::<ExecutionWorkerEvent>(APPLICATION_EXECUTION_CAPACITY);
        let mut active = BTreeMap::new();

        dispatch_application_invocation(
            &mut worker,
            &service,
            &execution_sender,
            &weak_control_transport,
            &mut active,
            invocation,
        );

        assert!(matches!(
            call.await.unwrap(),
            Err(ApplicationError::Failed { message })
                if message.contains("fixture-missing-generation")
        ));
    }

    #[test]
    fn application_log_operations_filter_and_expand_references() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "phenix-application-log-test-{}-{nonce}",
            std::process::id()
        ));
        let logger = phenix_core::StructuredLogger::new(LogSink::directory(&root))
            .unwrap()
            .with_detail_mode(phenix_core::LogDetailMode::Reference);
        let detail = serde_json::json!({"body": "referenced"});
        let reference = logger.store_json(&detail).unwrap();
        logger
            .record(
                "runtime_trace",
                serde_json::json!({
                    "session_id": "session.keep",
                    "execution_id": "execution.keep",
                    "reference": reference.clone(),
                }),
            )
            .unwrap();
        logger
            .record(
                "runtime_trace",
                serde_json::json!({
                    "session_id": "session.other",
                    "execution_id": "execution.other",
                }),
            )
            .unwrap();

        let mut worker = application_worker();
        worker.log_reader = Ok(StructuredLogReader::configured(LogSink::directory(&root)).unwrap());

        let page = invoke_operation::<QueryLogs>(
            &mut worker,
            LogQueryInput {
                cursor: None,
                limit: Some(1),
                session_id: Some(SessionId::parse("session.keep").unwrap()),
                execution_id: Some("execution.keep".into()),
            },
        )
        .unwrap();
        assert_eq!(page.records.len(), 1);
        assert_eq!(page.records[0].kind, "runtime_trace");
        assert_eq!(page.next_cursor.as_deref(), Some("1"));
        let payload_reference = match &page.records[0].payload {
            PhenixValue::Map(payload) => payload.get("reference").unwrap(),
            other => panic!("unexpected log payload: {other:?}"),
        };
        assert_eq!(payload_reference, &PhenixValue::from(&reference));

        let expanded = invoke_operation::<ReadLogReference>(
            &mut worker,
            LogReferenceInput {
                reference: reference.clone(),
            },
        )
        .unwrap();
        assert_eq!(expanded.reference, reference);
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(expanded.content.as_ref()).unwrap(),
            detail
        );

        let _ = fs::remove_dir_all(root);
    }

    async fn invoke_transport_operation<O: Operation>(
        transport: &ChannelTransport,
        input: O::Input,
    ) -> Result<O::Output, ApplicationError> {
        let value = transport
            .invoke(&ContractId::parse(O::ID).unwrap(), input.to_value())
            .await?;
        O::Output::from_value(&value).map_err(|error| ApplicationError::InvalidResponse {
            message: error.to_string(),
        })
    }

    fn continuation_model_manifest() -> PluginManifest {
        PluginManifest {
            id: PluginId::parse("fixture.tool-continuation-model").unwrap(),
            version: 1,
            execution: PluginExecution::Embedded,
            dependencies: Vec::new(),
            services: vec![ServiceContribution {
                role: ServiceRole::Terminal,
                service: model_inference_service(),
                priority: 100,
                required_authority: Authority::default(),
            }],
            resource_namespaces: Vec::new(),
            maximum_authority: Authority::default(),
        }
    }

    fn fail_once_model_manifest() -> PluginManifest {
        PluginManifest {
            id: PluginId::parse("fixture.fail-once-model").unwrap(),
            version: 1,
            execution: PluginExecution::Embedded,
            dependencies: Vec::new(),
            services: vec![ServiceContribution {
                role: ServiceRole::Terminal,
                service: model_inference_service(),
                priority: 100,
                required_authority: Authority::default(),
            }],
            resource_namespaces: Vec::new(),
            maximum_authority: Authority::default(),
        }
    }

    struct FailOnceModel {
        calls: Arc<AtomicU32>,
    }

    impl PluginInstance for FailOnceModel {
        fn start(&mut self, _host: &PluginHost<'_>) -> Result<(), String> {
            Ok(())
        }

        fn invoke(
            &mut self,
            service: &ServiceId,
            input: &[u8],
            _host: &PluginHost<'_>,
        ) -> Result<Vec<u8>, String> {
            if service != &model_inference_service() {
                return Err(format!("unsupported fail-once fixture service: {service}"));
            }
            let value: PhenixValue =
                serde_json::from_slice(input).map_err(|error| error.to_string())?;
            let _request = ModelInferenceRequest::try_from(Project(&value))
                .map_err(|error| error.to_string())?;

            if self.calls.fetch_add(1, AtomicOrdering::SeqCst) == 0 {
                // This regression exercises failed-execution recovery, not retry policy.
                let failure = ModelInferenceFailure::InvalidRequest {
                    message: "fixture provider failed this execution".into(),
                };
                return serde_json::to_vec(
                    &InvocationOutcome::domain_error(PhenixValue::from(&failure))
                        .into_transport_value(),
                )
                .map_err(|error| error.to_string());
            }

            let response = ModelInferenceResponse {
                output: Bytes::new(b"second prompt completed".to_vec()),
                provider_metadata: BTreeMap::new(),
                usage: Default::default(),
                tool_calls: Vec::new(),
            };
            serde_json::to_vec(&PhenixValue::from(&response)).map_err(|error| error.to_string())
        }
    }

    struct CancellationGate {
        state: StdMutex<(bool, bool)>,
        changed: Condvar,
    }

    impl CancellationGate {
        fn new() -> Self {
            Self {
                state: StdMutex::new((false, false)),
                changed: Condvar::new(),
            }
        }

        fn start_and_wait_for_release(&self) {
            let mut state = self.state.lock().unwrap();
            state.0 = true;
            self.changed.notify_all();
            while !state.1 {
                state = self.changed.wait(state).unwrap();
            }
        }

        fn wait_until_started(&self) {
            let mut state = self.state.lock().unwrap();
            while !state.0 {
                state = self.changed.wait(state).unwrap();
            }
        }

        fn release(&self) {
            let mut state = self.state.lock().unwrap();
            state.1 = true;
            self.changed.notify_all();
        }
    }

    fn session_control_model_manifest() -> PluginManifest {
        PluginManifest {
            id: PluginId::parse("fixture.session-control-model").unwrap(),
            version: 1,
            execution: PluginExecution::Embedded,
            dependencies: Vec::new(),
            services: vec![ServiceContribution {
                role: ServiceRole::Terminal,
                service: model_inference_service(),
                priority: 100,
                required_authority: Authority::default(),
            }],
            resource_namespaces: Vec::new(),
            maximum_authority: Authority::default(),
        }
    }

    struct SessionControlModel {
        child: Arc<StdMutex<Option<SessionId>>>,
    }

    impl PluginInstance for SessionControlModel {
        fn start(&mut self, _host: &PluginHost<'_>) -> Result<(), String> {
            Ok(())
        }

        fn invoke(
            &mut self,
            service: &ServiceId,
            input: &[u8],
            _host: &PluginHost<'_>,
        ) -> Result<Vec<u8>, String> {
            if service != &model_inference_service() {
                return Err(format!(
                    "unsupported session-control fixture service: {service}"
                ));
            }
            let value: PhenixValue =
                serde_json::from_slice(input).map_err(|error| error.to_string())?;
            let request = ModelInferenceRequest::try_from(Project(&value))
                .map_err(|error| error.to_string())?;
            let child = self
                .child
                .lock()
                .map_err(|_| "session-control fixture target lock poisoned".to_owned())?
                .clone()
                .ok_or_else(|| "session-control fixture child is not configured".to_owned())?;

            let response = if request.session_id.as_ref() == Some(&child) {
                ModelInferenceResponse {
                    output: Bytes::new(b"child session completed".to_vec()),
                    provider_metadata: BTreeMap::new(),
                    usage: Default::default(),
                    tool_calls: Vec::new(),
                }
            } else {
                match request.continuation.as_slice() {
                    [] => {
                        if !request
                            .tools
                            .iter()
                            .any(|tool| tool.id.as_str() == "phenix.session")
                        {
                            return Err(
                                "controller model did not receive the phenix.session tool".into()
                            );
                        }
                        ModelInferenceResponse {
                            output: Bytes::new(b"prompt child session".to_vec()),
                            provider_metadata: BTreeMap::new(),
                            usage: Default::default(),
                            tool_calls: vec![ModelToolCall {
                                call_id: "session-control-child-prompt".into(),
                                callable_id: CallableId::parse("phenix.session").unwrap(),
                                input: PhenixValue::Table(BTreeMap::from([
                                    (
                                        Key::parse("operation").unwrap(),
                                        PhenixValue::String("prompt".into()),
                                    ),
                                    (
                                        Key::parse("arguments").unwrap(),
                                        PhenixValue::Map(BTreeMap::from([
                                            (
                                                "session_id".into(),
                                                PhenixValue::String(child.to_string()),
                                            ),
                                            (
                                                "content".into(),
                                                PhenixValue::List(vec![PhenixValue::Map(
                                                    BTreeMap::from([
                                                        (
                                                            "kind".into(),
                                                            PhenixValue::String("text".into()),
                                                        ),
                                                        (
                                                            "text".into(),
                                                            PhenixValue::String(
                                                                "complete the child session".into(),
                                                            ),
                                                        ),
                                                    ]),
                                                )]),
                                            ),
                                        ])),
                                    ),
                                ])),
                            }],
                        }
                    }
                    [turn] => {
                        if turn.tool_results.len() != 1 {
                            return Err(
                                "controller model did not receive one child-session result".into(),
                            );
                        }
                        let result = &turn.tool_results[0];
                        if result.is_error {
                            return Err("child-session orchestration returned an error".into());
                        }
                        let PhenixValue::Map(fields) = &result.output else {
                            return Err("child-session result is not structured".into());
                        };
                        if !matches!(
                            fields.get("stop_reason"),
                            Some(PhenixValue::String(reason)) if reason == "end_turn"
                        ) {
                            return Err(format!(
                                "child-session result has unexpected stop reason: {:?}",
                                fields.get("stop_reason")
                            ));
                        }
                        ModelInferenceResponse {
                            output: Bytes::new(b"controller observed child completion".to_vec()),
                            provider_metadata: BTreeMap::new(),
                            usage: Default::default(),
                            tool_calls: Vec::new(),
                        }
                    }
                    turns => {
                        return Err(format!(
                            "session-control fixture received {} continuation turns",
                            turns.len()
                        ));
                    }
                }
            };
            serde_json::to_vec(&PhenixValue::from(&response)).map_err(|error| error.to_string())
        }
    }

    #[derive(Clone, Debug, Eq, PartialEq, phenix_sdk::PhenixValue)]
    struct MemoryDebugRequest {
        operation: String,
        value: Option<String>,
    }

    #[derive(Clone, Debug, Eq, PartialEq, phenix_sdk::PhenixValue)]
    struct MemoryDebugResponse {
        value: Option<String>,
    }

    const TRIAL_GENERATION_SKILL_MARKER: &str = "TRIAL_GENERATION_SKILL_MARKER";

    fn trial_generation_skill() -> SkillDefinition {
        SkillDefinition {
            id: SkillId::parse("trial-generation").unwrap(),
            content: Bytes::new(TRIAL_GENERATION_SKILL_MARKER.as_bytes().to_vec()),
        }
    }

    struct MemoryDebugPlugin {
        namespace: ResourceNamespace,
    }

    impl PluginInstance for MemoryDebugPlugin {
        fn start(&mut self, _host: &PluginHost<'_>) -> Result<(), String> {
            Ok(())
        }

        fn invoke(
            &mut self,
            service: &ServiceId,
            input: &[u8],
            host: &PluginHost<'_>,
        ) -> Result<Vec<u8>, String> {
            if service == &skill_service() {
                let value: PhenixValue =
                    serde_json::from_slice(input).map_err(|error| error.to_string())?;
                let command =
                    SkillCommand::from_value(&value).map_err(|error| error.to_string())?;
                let skill = trial_generation_skill();
                let response = match command {
                    SkillCommand::Required | SkillCommand::List => SkillResponse::Skills {
                        skills: vec![skill],
                    },
                    SkillCommand::Get { id } => SkillResponse::Skill {
                        skill: (id == skill.id).then_some(skill),
                    },
                    SkillCommand::Register { .. } => {
                        return Err("trial generation skill fixture is read-only".into());
                    }
                };
                return serde_json::to_vec(&response.to_value()).map_err(|error| error.to_string());
            }
            if service.as_str() != "fixture.memory-debug@1" {
                return Err(format!(
                    "unsupported memory-debug fixture service: {service}"
                ));
            }
            let value: PhenixValue =
                serde_json::from_slice(input).map_err(|error| error.to_string())?;
            let request =
                MemoryDebugRequest::from_value(&value).map_err(|error| error.to_string())?;
            let response = match request.operation.as_str() {
                "write" => {
                    let value = request
                        .value
                        .ok_or_else(|| "memory-debug write requires a value".to_owned())?;
                    host.transact_durable(
                        &self.namespace,
                        &[TransactionOp::Put {
                            key: "fact".into(),
                            value: value.as_bytes().to_vec(),
                        }],
                    )
                    .map_err(|error| error.to_string())?;
                    MemoryDebugResponse { value: Some(value) }
                }
                "read" => {
                    let value = host
                        .read_durable(&self.namespace, "fact")
                        .map_err(|error| error.to_string())?
                        .map(String::from_utf8)
                        .transpose()
                        .map_err(|error| error.to_string())?;
                    MemoryDebugResponse { value }
                }
                operation => {
                    return Err(format!(
                        "unsupported memory-debug fixture operation: {operation}"
                    ));
                }
            };
            serde_json::to_vec(&response.to_value()).map_err(|error| error.to_string())
        }
    }

    fn memory_debug_authority() -> Authority {
        Authority::new([
            PermissionId::parse("kernel.persistence.read").unwrap(),
            PermissionId::parse("kernel.persistence.write").unwrap(),
        ])
    }

    fn memory_debug_manifest(
        version: u32,
        namespace: ResourceNamespace,
        provides_skill: bool,
    ) -> PluginManifest {
        let persistence = memory_debug_authority();
        let mut services = vec![ServiceContribution {
            role: ServiceRole::Terminal,
            service: ServiceId::parse("fixture.memory-debug@1").unwrap(),
            priority: 100,
            required_authority: Authority::default(),
        }];
        if provides_skill {
            services.push(ServiceContribution {
                role: ServiceRole::Terminal,
                service: skill_service(),
                priority: 100,
                required_authority: Authority::default(),
            });
        }
        PluginManifest {
            id: PluginId::parse("fixture.memory-debug").unwrap(),
            version,
            execution: PluginExecution::Embedded,
            dependencies: Vec::new(),
            services,
            resource_namespaces: vec![namespace],
            maximum_authority: persistence,
        }
    }

    fn memory_debug_component(provides_skill: bool) -> ComponentManifest {
        let persistence = memory_debug_authority();
        let mut exports = vec![ComponentExport {
            interface: InterfaceId::parse("fixture.memory-debug@1").unwrap(),
            schema: InterfaceSchema::of::<MemoryDebugRequest, MemoryDebugResponse>(),
            priority: 100,
            required_authority: persistence.clone(),
        }];
        if provides_skill {
            exports.push(ComponentExport {
                interface: InterfaceId::parse("phenix.skills@1").unwrap(),
                schema: InterfaceSchema::of::<SkillCommand, SkillResponse>(),
                priority: 100,
                required_authority: Authority::default(),
            });
        }
        ComponentManifest {
            listeners: Vec::new(),
            id: ComponentId::parse("fixture.memory-debug").unwrap(),
            owner: PluginId::parse("fixture.memory-debug").unwrap(),
            imports: Vec::new(),
            exports,
            maximum_authority: persistence,
        }
    }

    fn memory_debug_trigger() -> ComponentEntryTrigger {
        ComponentEntryTrigger {
            component: ComponentId::parse("fixture.memory-debug").unwrap(),
            interface: InterfaceId::parse("fixture.memory-debug@1").unwrap(),
            trigger: EntryTriggerKind::ToolCall {
                callable_id: CallableId::parse("memory.debug").unwrap(),
                description: "Write or recall one durable memory fixture value".into(),
            },
            required_authority: memory_debug_authority(),
        }
    }

    #[derive(Default)]
    struct RuntimeOrchestrationModelState {
        controller: Option<SessionId>,
        initial_generation: Option<String>,
        trial_request: Option<PhenixValue>,
        controller_turn_one_entries: usize,
        controller_turn_two_entries: usize,
        model_entries: usize,
    }

    struct RuntimeOrchestrationModel {
        state: Arc<StdMutex<RuntimeOrchestrationModelState>>,
    }

    fn runtime_orchestration_model_manifest() -> PluginManifest {
        PluginManifest {
            id: PluginId::parse("fixture.runtime-orchestration-model").unwrap(),
            version: 1,
            execution: PluginExecution::Embedded,
            dependencies: Vec::new(),
            services: vec![ServiceContribution {
                role: ServiceRole::Terminal,
                service: model_inference_service(),
                priority: 100,
                required_authority: Authority::default(),
            }],
            resource_namespaces: Vec::new(),
            maximum_authority: Authority::default(),
        }
    }

    fn orchestration_result(
        request: &ModelInferenceRequest,
        turn_index: usize,
    ) -> Result<&ModelToolResult, String> {
        let turn = request
            .continuation
            .get(turn_index)
            .ok_or_else(|| format!("missing orchestration tool turn {turn_index}"))?;
        if turn.tool_results.len() != 1 {
            return Err(format!(
                "orchestration turn {turn_index} produced {} tool results",
                turn.tool_results.len()
            ));
        }
        let result = &turn.tool_results[0];
        if result.is_error {
            return Err(format!(
                "orchestration tool {} failed: {:?}",
                result.callable_id, result.output
            ));
        }
        Ok(result)
    }

    fn orchestration_string_field(
        request: &ModelInferenceRequest,
        turn_index: usize,
        key: &str,
    ) -> Result<String, String> {
        let result = orchestration_result(request, turn_index)?;
        let PhenixValue::Map(fields) = &result.output else {
            return Err(format!(
                "orchestration tool {} returned a non-map result",
                result.callable_id
            ));
        };
        match fields.get(key) {
            Some(PhenixValue::String(value)) => Ok(value.clone()),
            value => Err(format!(
                "orchestration result field {key} is not a string: {value:?}"
            )),
        }
    }

    fn orchestration_operation_call(
        call_id: &str,
        callable_id: &str,
        operation: &str,
        arguments: BTreeMap<String, PhenixValue>,
    ) -> ModelToolCall {
        ModelToolCall {
            call_id: call_id.into(),
            callable_id: CallableId::parse(callable_id).unwrap(),
            input: PhenixValue::Table(BTreeMap::from([
                (
                    Key::parse("operation").unwrap(),
                    PhenixValue::String(operation.into()),
                ),
                (
                    Key::parse("arguments").unwrap(),
                    PhenixValue::Map(arguments),
                ),
            ])),
        }
    }

    fn orchestration_response(
        output: &str,
        tool_calls: Vec<ModelToolCall>,
    ) -> ModelInferenceResponse {
        ModelInferenceResponse {
            output: Bytes::new(output.as_bytes().to_vec()),
            provider_metadata: BTreeMap::new(),
            usage: Default::default(),
            tool_calls,
        }
    }

    impl RuntimeOrchestrationModel {
        fn child_response(
            &self,
            request: &ModelInferenceRequest,
        ) -> Result<ModelInferenceResponse, String> {
            if !request
                .tools
                .iter()
                .any(|tool| tool.id.as_str() == "memory.debug")
            {
                return Err("selected child generation did not expose memory.debug".into());
            }
            let input = String::from_utf8_lossy(request.input.as_ref());
            if !input.contains(TRIAL_GENERATION_SKILL_MARKER) {
                return Err("selected child generation did not activate its required skill".into());
            }
            let write = input.contains("store Helios");
            let read = input.contains("recall Helios");
            if !write && !read {
                return Err(format!(
                    "unexpected child orchestration prompt: {}",
                    input.trim()
                ));
            }
            match request.continuation.as_slice() {
                [] => {
                    let tool_request = MemoryDebugRequest {
                        operation: if write { "write" } else { "read" }.into(),
                        value: write.then(|| "Helios".to_owned()),
                    };
                    Ok(orchestration_response(
                        if write {
                            "store durable memory"
                        } else {
                            "recall durable memory"
                        },
                        vec![ModelToolCall {
                            call_id: if write {
                                "memory-debug-write"
                            } else {
                                "memory-debug-read"
                            }
                            .into(),
                            callable_id: CallableId::parse("memory.debug").unwrap(),
                            input: tool_request.to_value(),
                        }],
                    ))
                }
                [turn] => {
                    if turn.tool_results.len() != 1 || turn.tool_results[0].is_error {
                        return Err("memory.debug child tool call failed".into());
                    }
                    let response = MemoryDebugResponse::from_value(&turn.tool_results[0].output)
                        .map_err(|error| error.to_string())?;
                    if response.value.as_deref() != Some("Helios") {
                        return Err(format!(
                            "memory.debug returned unexpected value: {:?}",
                            response.value
                        ));
                    }
                    Ok(orchestration_response(
                        if write { "stored Helios" } else { "Helios" },
                        Vec::new(),
                    ))
                }
                turns => Err(format!(
                    "child orchestration received {} continuation turns",
                    turns.len()
                )),
            }
        }

        fn controller_response(
            &self,
            request: &ModelInferenceRequest,
            initial_generation: &str,
            trial_request: PhenixValue,
        ) -> Result<ModelInferenceResponse, String> {
            let g2 = || orchestration_string_field(request, 0, "generation");
            let reader_prompt = || -> Result<(SessionId, String), String> {
                let reader = SessionInfo::from_value(&orchestration_result(request, 4)?.output)
                    .map_err(|error| error.to_string())?;
                let prompt = orchestration_result(request, 5)?;
                let PhenixValue::Map(fields) = &prompt.output else {
                    return Err("reader prompt returned a non-map result".into());
                };
                let Some(PhenixValue::String(execution_id)) = fields.get("execution_id") else {
                    return Err("reader prompt did not return an execution id".into());
                };
                Ok((reader.session_id, execution_id.clone()))
            };
            match request.continuation.len() {
                0 => {
                    if String::from_utf8_lossy(request.input.as_ref())
                        .contains(TRIAL_GENERATION_SKILL_MARKER)
                    {
                        return Err("controller G1 unexpectedly activated the G2-only skill".into());
                    }
                    if request
                        .tools
                        .iter()
                        .any(|tool| tool.id.as_str() == "memory.debug")
                    {
                        return Err("controller G1 unexpectedly exposed memory.debug".into());
                    }
                    for tool in ["phenix.plugin", "phenix.session", "phenix.inspect"] {
                        if !request
                            .tools
                            .iter()
                            .any(|candidate| candidate.id.as_str() == tool)
                        {
                            return Err(format!("controller did not receive {tool}"));
                        }
                    }
                    Ok(orchestration_response(
                        "stage the changed memory plugin",
                        vec![orchestration_operation_call(
                            "orchestration-trial",
                            "phenix.plugin",
                            "trial",
                            BTreeMap::from([("request".into(), trial_request)]),
                        )],
                    ))
                }
                1 => {
                    let trial = orchestration_result(request, 0)?;
                    let PhenixValue::Map(fields) = &trial.output else {
                        return Err("plugin trial returned a non-map result".into());
                    };
                    if fields.get("plugin")
                        != Some(&PhenixValue::String("fixture.memory-debug".into()))
                    {
                        return Err(format!(
                            "plugin trial returned unexpected plugin: {:?}",
                            fields.get("plugin")
                        ));
                    }
                    let Some(PhenixValue::Map(diff)) = fields.get("diff") else {
                        return Err("plugin trial did not expose its reconciliation diff".into());
                    };
                    if diff.get("candidate_generation") != Some(&PhenixValue::String(g2()?)) {
                        return Err(format!(
                            "plugin trial diff returned unexpected candidate generation: {:?}",
                            diff.get("candidate_generation")
                        ));
                    }
                    Ok(orchestration_response(
                        "create writer session",
                        vec![orchestration_operation_call(
                            "orchestration-create-writer",
                            "phenix.session",
                            "create",
                            BTreeMap::from([
                                (
                                    "working_directory".into(),
                                    PhenixValue::String("/workspace".into()),
                                ),
                                ("title".into(), PhenixValue::String("writer".into())),
                            ]),
                        )],
                    ))
                }
                2 => {
                    let writer = SessionInfo::from_value(&orchestration_result(request, 1)?.output)
                        .map_err(|error| error.to_string())?;
                    Ok(orchestration_response(
                        "write memory through G2",
                        vec![orchestration_operation_call(
                            "orchestration-prompt-writer",
                            "phenix.session",
                            "prompt",
                            BTreeMap::from([
                                (
                                    "session_id".into(),
                                    PhenixValue::String(writer.session_id.to_string()),
                                ),
                                (
                                    "content".into(),
                                    PhenixValue::List(vec![PhenixValue::Map(BTreeMap::from([
                                        ("kind".into(), PhenixValue::String("text".into())),
                                        ("text".into(), PhenixValue::String("store Helios".into())),
                                    ]))]),
                                ),
                                ("generation".into(), PhenixValue::String(g2()?)),
                            ]),
                        )],
                    ))
                }
                3 => {
                    orchestration_result(request, 2)?;
                    let writer = SessionInfo::from_value(&orchestration_result(request, 1)?.output)
                        .map_err(|error| error.to_string())?;
                    Ok(orchestration_response(
                        "close writer session",
                        vec![orchestration_operation_call(
                            "orchestration-close-writer",
                            "phenix.session",
                            "close",
                            BTreeMap::from([(
                                "session_id".into(),
                                PhenixValue::String(writer.session_id.to_string()),
                            )]),
                        )],
                    ))
                }
                4 => {
                    orchestration_result(request, 3)?;
                    Ok(orchestration_response(
                        "create reader session",
                        vec![orchestration_operation_call(
                            "orchestration-create-reader",
                            "phenix.session",
                            "create",
                            BTreeMap::from([
                                (
                                    "working_directory".into(),
                                    PhenixValue::String("/workspace".into()),
                                ),
                                ("title".into(), PhenixValue::String("reader".into())),
                            ]),
                        )],
                    ))
                }
                5 => {
                    let reader = SessionInfo::from_value(&orchestration_result(request, 4)?.output)
                        .map_err(|error| error.to_string())?;
                    Ok(orchestration_response(
                        "recall memory through independent G2 session",
                        vec![orchestration_operation_call(
                            "orchestration-prompt-reader",
                            "phenix.session",
                            "prompt",
                            BTreeMap::from([
                                (
                                    "session_id".into(),
                                    PhenixValue::String(reader.session_id.to_string()),
                                ),
                                (
                                    "content".into(),
                                    PhenixValue::List(vec![PhenixValue::Map(BTreeMap::from([
                                        ("kind".into(), PhenixValue::String("text".into())),
                                        (
                                            "text".into(),
                                            PhenixValue::String("recall Helios".into()),
                                        ),
                                    ]))]),
                                ),
                                ("generation".into(), PhenixValue::String(g2()?)),
                            ]),
                        )],
                    ))
                }
                6 => {
                    let result = orchestration_result(request, 5)?;
                    let PhenixValue::Map(fields) = &result.output else {
                        return Err("reader prompt returned a non-map result".into());
                    };
                    if fields.get("assistant_message")
                        != Some(&PhenixValue::Option(Some(Box::new(PhenixValue::String(
                            "Helios".into(),
                        )))))
                    {
                        return Err(format!(
                            "reader did not recall Helios: {:?}",
                            fields.get("assistant_message")
                        ));
                    }
                    Ok(orchestration_response(
                        "inspect orchestration diagnostics",
                        vec![ModelToolCall {
                            call_id: "orchestration-inspect-trace".into(),
                            callable_id: CallableId::parse("phenix.inspect").unwrap(),
                            input: PhenixValue::Table(BTreeMap::from([(
                                Key::parse("query").unwrap(),
                                PhenixValue::String("trace".into()),
                            )])),
                        }],
                    ))
                }
                7 => {
                    let (reader, execution_id) = reader_prompt()?;
                    let trace = &orchestration_result(request, 6)?.output;
                    let PhenixValue::List(events) = trace else {
                        return Err("runtime trace returned a non-list result".into());
                    };
                    let expected_g2 = g2()?;
                    let controller_session =
                        request
                            .session_id
                            .as_ref()
                            .map(ToString::to_string)
                            .ok_or_else(|| "controller request has no session id".to_owned())?;
                    let observed = events.iter().any(|event| {
                        session_control_field(event, "event")
                            == Some(&PhenixValue::String("orchestration".into()))
                            && session_control_field(event, "kind")
                                == Some(&PhenixValue::String("session".into()))
                            && session_control_field(event, "operation")
                                == Some(&PhenixValue::String("prompt".into()))
                            && session_control_field(event, "controller_session")
                                == Some(&PhenixValue::String(controller_session.clone()))
                            && session_control_field(event, "target_session")
                                == Some(&PhenixValue::String(reader.to_string()))
                            && session_control_field(event, "child_execution")
                                == Some(&PhenixValue::String(execution_id.clone()))
                            && session_control_field(event, "selected_generation")
                                == Some(&PhenixValue::String(expected_g2.clone()))
                            && session_control_field(event, "success")
                                == Some(&PhenixValue::Bool(true))
                    });
                    if !observed {
                        return Err(format!(
                            "runtime trace did not correlate controller {controller_session}, reader {reader}, execution {execution_id}, and G2"
                        ));
                    }
                    Ok(orchestration_response(
                        "inspect the child session journal",
                        vec![orchestration_operation_call(
                            "orchestration-resume-reader",
                            "phenix.session",
                            "resume",
                            BTreeMap::from([(
                                "session_id".into(),
                                PhenixValue::String(reader.to_string()),
                            )]),
                        )],
                    ))
                }
                8 => {
                    let (reader, execution_id) = reader_prompt()?;
                    let snapshot =
                        SessionSnapshot::from_value(&orchestration_result(request, 7)?.output)
                            .map_err(|error| error.to_string())?;
                    let observed_memory_call = snapshot.updates.iter().any(|update| {
                        matches!(
                            &update.update,
                            SessionChange::Execution {
                                execution_id: observed_execution,
                                update: ExecutionChange::ToolCall { callable_id, .. },
                            } if observed_execution == &execution_id
                                && callable_id.as_str() == "memory.debug"
                        )
                    });
                    if !observed_memory_call {
                        return Err(format!(
                            "reader session {reader} did not record memory.debug for {execution_id}"
                        ));
                    }
                    Ok(orchestration_response(
                        "inspect the child execution",
                        vec![ModelToolCall {
                            call_id: "orchestration-inspect-child".into(),
                            callable_id: CallableId::parse("phenix.inspect").unwrap(),
                            input: PhenixValue::Table(BTreeMap::from([(
                                Key::parse("query").unwrap(),
                                PhenixValue::String(format!("execution {execution_id}")),
                            )])),
                        }],
                    ))
                }
                9 => {
                    let inspected =
                        ExecutionRecord::from_value(&orchestration_result(request, 8)?.output)
                            .map_err(|error| error.to_string())?;
                    if inspected.graph_generation != g2()? {
                        return Err(format!(
                            "child execution ran in {}, expected {}",
                            inspected.graph_generation,
                            g2()?
                        ));
                    }
                    Ok(orchestration_response(
                        "promote G2",
                        vec![orchestration_operation_call(
                            "orchestration-promote",
                            "phenix.plugin",
                            "promote",
                            BTreeMap::from([("generation".into(), PhenixValue::String(g2()?))]),
                        )],
                    ))
                }
                10 => {
                    let active = orchestration_string_field(request, 9, "active_generation")?;
                    if active != g2()? {
                        return Err(format!("promotion selected unexpected generation {active}"));
                    }
                    Ok(orchestration_response(
                        "roll back to G1",
                        vec![orchestration_operation_call(
                            "orchestration-rollback",
                            "phenix.plugin",
                            "rollback",
                            BTreeMap::from([(
                                "generation".into(),
                                PhenixValue::String(initial_generation.to_owned()),
                            )]),
                        )],
                    ))
                }
                11 => {
                    let active = orchestration_string_field(request, 10, "active_generation")?;
                    if active != initial_generation {
                        return Err(format!("rollback selected unexpected generation {active}"));
                    }
                    Ok(orchestration_response(
                        "retire G2",
                        vec![orchestration_operation_call(
                            "orchestration-retire",
                            "phenix.plugin",
                            "retire",
                            BTreeMap::from([("generation".into(), PhenixValue::String(g2()?))]),
                        )],
                    ))
                }
                12 => {
                    let retired = orchestration_string_field(request, 11, "retired_generation")?;
                    if retired != g2()? {
                        return Err(format!("retired unexpected generation {retired}"));
                    }
                    Ok(orchestration_response(
                        "inspect final generation state",
                        vec![orchestration_operation_call(
                            "orchestration-inspect-generations",
                            "phenix.plugin",
                            "inspect",
                            BTreeMap::new(),
                        )],
                    ))
                }
                13 => {
                    let result = orchestration_result(request, 12)?;
                    let PhenixValue::Map(fields) = &result.output else {
                        return Err("final plugin inspection returned a non-map result".into());
                    };
                    let active = orchestration_string_field(request, 12, "active_generation")?;
                    if active != initial_generation {
                        return Err(format!(
                            "final active generation is {active}, expected {initial_generation}"
                        ));
                    }
                    let Some(PhenixValue::List(generations)) = fields.get("generations") else {
                        return Err("final plugin inspection returned no generation list".into());
                    };
                    if generations.len() != 1 {
                        return Err(format!(
                            "final plugin inspection returned {} generations, expected one",
                            generations.len()
                        ));
                    }
                    let Some(PhenixValue::Map(generation)) = generations.first() else {
                        return Err("final plugin inspection generation was not a map".into());
                    };
                    if generation.get("generation")
                        != Some(&PhenixValue::String(initial_generation.to_owned()))
                        || generation.get("default") != Some(&PhenixValue::Bool(true))
                    {
                        return Err(format!(
                            "final plugin inspection returned unexpected generation: {generation:?}"
                        ));
                    }
                    Ok(orchestration_response("orchestration complete", Vec::new()))
                }
                turns => Err(format!(
                    "controller orchestration received {turns} continuation turns"
                )),
            }
        }
    }

    impl PluginInstance for RuntimeOrchestrationModel {
        fn start(&mut self, _host: &PluginHost<'_>) -> Result<(), String> {
            Ok(())
        }

        fn invoke(
            &mut self,
            service: &ServiceId,
            input: &[u8],
            _host: &PluginHost<'_>,
        ) -> Result<Vec<u8>, String> {
            if service != &model_inference_service() {
                return Err(format!(
                    "unsupported runtime-orchestration fixture service: {service}"
                ));
            }
            let value: PhenixValue =
                serde_json::from_slice(input).map_err(|error| error.to_string())?;
            let request = ModelInferenceRequest::try_from(Project(&value))
                .map_err(|error| error.to_string())?;
            let (controller, initial_generation, trial_request) = {
                let state = self
                    .state
                    .lock()
                    .map_err(|_| "runtime orchestration model state lock poisoned".to_owned())?;
                (
                    state.controller.clone(),
                    state.initial_generation.clone(),
                    state.trial_request.clone(),
                )
            };
            let controller = controller
                .ok_or_else(|| "runtime orchestration controller is not configured".to_owned())?;
            {
                let mut state = self
                    .state
                    .lock()
                    .map_err(|_| "runtime orchestration model state lock poisoned".to_owned())?;
                state.model_entries += 1;
                if state.model_entries > 32 {
                    return Err(format!(
                        "runtime orchestration exceeded 32 model entries at session {:?}, turn {}",
                        request.session_id,
                        request.continuation.len()
                    ));
                }
            }
            if request.session_id.as_ref() == Some(&controller) && request.continuation.len() == 1 {
                let mut state = self
                    .state
                    .lock()
                    .map_err(|_| "runtime orchestration model state lock poisoned".to_owned())?;
                state.controller_turn_one_entries += 1;
                if state.controller_turn_one_entries > 2 {
                    return Err(format!(
                        "controller turn 1 re-entered {} times for session {:?}",
                        state.controller_turn_one_entries, request.session_id
                    ));
                }
            }
            if request.session_id.as_ref() == Some(&controller) && request.continuation.len() == 2 {
                let mut state = self
                    .state
                    .lock()
                    .map_err(|_| "runtime orchestration model state lock poisoned".to_owned())?;
                state.controller_turn_two_entries += 1;
                if state.controller_turn_two_entries > 1 {
                    return Err(format!(
                        "controller turn 2 re-entered as session {:?} with input {:?}",
                        request.session_id,
                        String::from_utf8_lossy(request.input.as_ref())
                    ));
                }
            }
            let response = if request.session_id.as_ref() == Some(&controller) {
                self.controller_response(
                    &request,
                    initial_generation.as_deref().ok_or_else(|| {
                        "runtime orchestration initial generation is not configured".to_owned()
                    })?,
                    trial_request.ok_or_else(|| {
                        "runtime orchestration trial request is not configured".to_owned()
                    })?,
                )?
            } else {
                self.child_response(&request)?
            };
            serde_json::to_vec(&PhenixValue::from(&response)).map_err(|error| error.to_string())
        }
    }

    fn cancellation_gate_model_manifest() -> PluginManifest {
        PluginManifest {
            id: PluginId::parse("fixture.cancellation-gate-model").unwrap(),
            version: 1,
            execution: PluginExecution::Embedded,
            dependencies: Vec::new(),
            services: vec![ServiceContribution {
                role: ServiceRole::Terminal,
                service: model_inference_service(),
                priority: 100,
                required_authority: Authority::default(),
            }],
            resource_namespaces: Vec::new(),
            maximum_authority: Authority::default(),
        }
    }

    struct CancellationGateModel {
        gate: Arc<CancellationGate>,
    }

    impl PluginInstance for CancellationGateModel {
        fn start(&mut self, _host: &PluginHost<'_>) -> Result<(), String> {
            Ok(())
        }

        fn invoke(
            &mut self,
            service: &ServiceId,
            input: &[u8],
            _host: &PluginHost<'_>,
        ) -> Result<Vec<u8>, String> {
            if service != &model_inference_service() {
                return Err(format!(
                    "unsupported cancellation fixture service: {service}"
                ));
            }
            let value: PhenixValue =
                serde_json::from_slice(input).map_err(|error| error.to_string())?;
            let _request = ModelInferenceRequest::try_from(Project(&value))
                .map_err(|error| error.to_string())?;
            self.gate.start_and_wait_for_release();
            let response = ModelInferenceResponse {
                output: Bytes::new(b"completed after cancellation gate".to_vec()),
                provider_metadata: BTreeMap::new(),
                usage: Default::default(),
                tool_calls: Vec::new(),
            };
            serde_json::to_vec(&PhenixValue::from(&response)).map_err(|error| error.to_string())
        }
    }

    struct ToolContinuationModel;

    impl PluginInstance for ToolContinuationModel {
        fn start(&mut self, _host: &PluginHost<'_>) -> Result<(), String> {
            Ok(())
        }

        fn invoke(
            &mut self,
            service: &ServiceId,
            input: &[u8],
            _host: &PluginHost<'_>,
        ) -> Result<Vec<u8>, String> {
            if service != &model_inference_service() {
                return Err(format!(
                    "unsupported continuation fixture service: {service}"
                ));
            }
            let value: PhenixValue =
                serde_json::from_slice(input).map_err(|error| error.to_string())?;
            let request = ModelInferenceRequest::try_from(Project(&value))
                .map_err(|error| error.to_string())?;

            let response = match request.continuation.as_slice() {
                [] => {
                    if !request.tools.iter().any(|tool| tool.id.as_str() == "bash") {
                        return Err("continuation fixture did not receive the Bash tool".into());
                    }
                    ModelInferenceResponse {
                        output: Bytes::new(b"run two tools".to_vec()),
                        provider_metadata: BTreeMap::new(),
                        usage: Default::default(),
                        tool_calls: (0..2)
                            .map(|index| ModelToolCall {
                                call_id: format!("continuation-call-{index}"),
                                callable_id: CallableId::parse("bash").unwrap(),
                                input: PhenixValue::Map(BTreeMap::from([(
                                    "command".to_owned(),
                                    PhenixValue::String("printf continuation".into()),
                                )])),
                            })
                            .collect(),
                    }
                }
                [turn] => {
                    if turn.tool_calls.len() != 2 || turn.tool_results.len() != 2 {
                        return Err("continuation fixture did not receive both tool results".into());
                    }
                    if turn.tool_results.iter().any(|result| result.is_error) {
                        return Err("continuation fixture received a failed tool result".into());
                    }
                    ModelInferenceResponse {
                        output: Bytes::new(b"continued after tool progress".to_vec()),
                        provider_metadata: BTreeMap::new(),
                        usage: Default::default(),
                        tool_calls: Vec::new(),
                    }
                }
                turns => {
                    return Err(format!(
                        "continuation fixture received {} continuation turns",
                        turns.len()
                    ));
                }
            };
            serde_json::to_vec(&PhenixValue::from(&response)).map_err(|error| error.to_string())
        }
    }

    fn configure_fixture_routing(
        worker: &mut ApplicationWorker,
        provider: &str,
        model: &str,
        profile: &str,
    ) {
        let target = ModelTarget {
            provider_plugin: PluginId::parse(provider).unwrap(),
            model: ModelId::parse(model).unwrap(),
            options: BTreeMap::new(),
        };
        let profile = RoutingProfile {
            id: RoutingProfileId::parse(profile).unwrap(),
            default_target: target.clone(),
            fallback_targets: Vec::new(),
            callable_targets: BTreeMap::new(),
        };
        worker
            .invoke_model_command(ModelCommand::RegisterProfile {
                profile: profile.clone(),
            })
            .unwrap();
        worker
            .invoke_model_command(ModelCommand::PublishModelFeatures {
                features: EffectiveModelFeatures {
                    target,
                    generation: ModelFeatureGenerationId::parse(format!(
                        "fixture-generation-{model}"
                    ))
                    .unwrap(),
                    context: ContextControl::ReplaceableTurns,
                    capacity: CapacityKnowledge::Known {
                        limits: ModelLimits {
                            context_window_tokens: 128_000,
                            max_output_tokens: Some(16_000),
                        },
                    },
                    cache: Default::default(),
                    optional: BTreeSet::new(),
                },
            })
            .unwrap();
        worker
            .invoke_option_command(OptionCommand::Set {
                key: model_default_option(),
                scope: OptionScope::Global,
                value: OptionValue::String(profile.id.to_string()),
            })
            .unwrap();
    }

    #[test]
    fn model_tool_surface_tracks_the_selected_graph_generation() {
        fn resolved(callable: Option<&str>) -> phenix_core::ResolvedGeneration {
            let plugin = PluginId::parse("fixture.generation-tools").unwrap();
            let component = ComponentId::parse("fixture.generation-tools.component").unwrap();
            let interface = InterfaceId::parse("fixture.generation-tools.echo@1").unwrap();
            let schema = InterfaceSchema::of::<ApplicationShellToolRequest, WorkspaceResponse>();
            let manifest = PluginManifest {
                id: plugin.clone(),
                version: 1,
                execution: PluginExecution::Embedded,
                dependencies: Vec::new(),
                services: vec![ServiceContribution {
                    role: ServiceRole::Terminal,
                    service: ServiceId::parse(interface.as_str()).unwrap(),
                    priority: 100,
                    required_authority: Authority::default(),
                }],
                resource_namespaces: Vec::new(),
                maximum_authority: Authority::default(),
            };
            let component_manifest = ComponentManifest {
                listeners: Vec::new(),
                id: component.clone(),
                owner: plugin,
                imports: Vec::new(),
                exports: vec![ComponentExport {
                    interface: interface.clone(),
                    schema,
                    priority: 100,
                    required_authority: Authority::default(),
                }],
                maximum_authority: Authority::default(),
            };
            let triggers = callable
                .into_iter()
                .map(|callable_id| ComponentEntryTrigger {
                    component: component.clone(),
                    interface: interface.clone(),
                    trigger: EntryTriggerKind::ToolCall {
                        callable_id: CallableId::parse(callable_id).unwrap(),
                        description: "Generation-local fixture tool".into(),
                    },
                    required_authority: Authority::default(),
                })
                .collect::<Vec<_>>();

            phenix_core::ResolvedGeneration::resolve_with_durable_schemas_layer_policies_and_entry_triggers(
                [manifest],
                [component_manifest],
                [],
                triggers,
                [],
                BTreeMap::new(),
                &Authority::default(),
            )
            .unwrap()
        }

        let g1 = resolved(None);
        let g2 = resolved(Some("fixture.g2-only"));
        assert_ne!(g1.generation(), g2.generation());

        let sdk = phenix_core::ResolvedSdkContributions::resolve(
            &[],
            &[],
            Vec::<phenix_core::SdkContribution>::new(),
        )
        .unwrap();
        let (callbacks, _receiver) = ClientCallableCallbacks::bounded(1);
        let service = SdkApplicationService::new(
            &sdk,
            &ObservableStore::default(),
            SharedCallableRegistry::default(),
            PluginRuntimeId::parse("fixture-generation-tools-runtime").unwrap(),
            ReferenceGenerationId::parse("fixture-generation-tools-capability-generation").unwrap(),
            callbacks,
            ClientReferenceIdentity::new(
                ClientConnectionId::parse("fixture-generation-tools-client").unwrap(),
                ReferenceGenerationId::parse("fixture-generation-tools-client-generation").unwrap(),
            ),
        )
        .unwrap();
        let session_id = SessionId::parse("session-generation-tools").unwrap();

        let g1_surface =
            application_model_tool_surface(&service, &session_id, &g1, &Authority::default())
                .unwrap();
        let g2_surface =
            application_model_tool_surface(&service, &session_id, &g2, &Authority::default())
                .unwrap();

        assert!(
            !g1_surface
                .tools
                .iter()
                .any(|tool| tool.id.as_str() == "fixture.g2-only")
        );
        assert!(
            g2_surface
                .tools
                .iter()
                .any(|tool| tool.id.as_str() == "fixture.g2-only")
        );
        assert!(
            !g1_surface
                .runtime_entry_triggers
                .contains_key(&CallableId::parse("fixture.g2-only").unwrap())
        );
        assert!(
            g2_surface
                .runtime_entry_triggers
                .contains_key(&CallableId::parse("fixture.g2-only").unwrap())
        );
    }

    #[test]
    fn session_control_prompt_decodes_canonical_content_parts() {
        let content = vec![
            Content::Text {
                text: "hello".into(),
            },
            Content::Image {
                mime_type: "image/png".into(),
                data: Bytes::new(vec![1, 2, 3]),
            },
            Content::Resource {
                uri: "file:///workspace/context.txt".into(),
                mime_type: Some("text/plain".into()),
                text: Some("context".into()),
            },
        ];
        let arguments = PhenixValue::Map(BTreeMap::from([(
            "content".into(),
            PhenixValue::List(content.iter().map(ValueCodec::to_value).collect()),
        )]));

        assert_eq!(session_control_content(&arguments).unwrap(), content);
    }

    #[test]
    fn session_control_prompt_decodes_public_content_maps() {
        let arguments = PhenixValue::Map(BTreeMap::from([(
            "content".into(),
            PhenixValue::List(vec![
                PhenixValue::Map(BTreeMap::from([
                    ("kind".into(), PhenixValue::String("text".into())),
                    ("text".into(), PhenixValue::String("hello".into())),
                ])),
                PhenixValue::Map(BTreeMap::from([
                    ("kind".into(), PhenixValue::String("image".into())),
                    ("mime_type".into(), PhenixValue::String("image/png".into())),
                    ("data".into(), PhenixValue::Bytes(vec![1, 2, 3])),
                ])),
                PhenixValue::Map(BTreeMap::from([
                    ("kind".into(), PhenixValue::String("resource".into())),
                    (
                        "uri".into(),
                        PhenixValue::String("file:///workspace/context.txt".into()),
                    ),
                    ("mime_type".into(), PhenixValue::String("text/plain".into())),
                    ("text".into(), PhenixValue::String("context".into())),
                ])),
            ]),
        )]));

        assert_eq!(
            session_control_content(&arguments).unwrap(),
            vec![
                Content::Text {
                    text: "hello".into(),
                },
                Content::Image {
                    mime_type: "image/png".into(),
                    data: Bytes::new(vec![1, 2, 3]),
                },
                Content::Resource {
                    uri: "file:///workspace/context.txt".into(),
                    mime_type: Some("text/plain".into()),
                    text: Some("context".into()),
                },
            ]
        );
    }

    struct LifecycleRuntimeGuest;

    impl PluginInstance for LifecycleRuntimeGuest {
        fn start(&mut self, _host: &PluginHost<'_>) -> Result<(), String> {
            Ok(())
        }
    }

    struct LifecycleRuntimeAdapter;

    impl PluginRuntimeAdapter for LifecycleRuntimeAdapter {
        fn prepare(
            &mut self,
            _candidate: PluginRuntimeCandidate<'_>,
        ) -> Result<Box<dyn PluginInstance>, String> {
            Ok(Box::new(LifecycleRuntimeGuest))
        }
    }

    impl PluginInstance for LifecycleRuntimeAdapter {
        fn start(&mut self, _host: &PluginHost<'_>) -> Result<(), String> {
            Ok(())
        }

        fn plugin_runtime_adapter(&mut self) -> Option<&mut dyn PluginRuntimeAdapter> {
            Some(self)
        }
    }

    fn lifecycle_runtime_adapter_manifest(runtime: &PluginRuntimeId) -> PluginManifest {
        PluginManifest {
            id: PluginId::parse("fixture.lifecycle-runtime-adapter").unwrap(),
            version: 1,
            execution: PluginExecution::Embedded,
            dependencies: Vec::new(),
            services: vec![ServiceContribution {
                service: plugin_runtime_adapter_service(runtime),
                role: ServiceRole::Terminal,
                priority: 100,
                required_authority: Authority::default(),
            }],
            resource_namespaces: Vec::new(),
            maximum_authority: Authority::default(),
        }
    }

    #[test]
    fn runtime_plugin_build_executes_workspace_plan_without_changing_generation() {
        let lifecycle_runtime = PluginRuntimeId::parse("fixture.lifecycle-runtime").unwrap();
        let adapter_manifest = lifecycle_runtime_adapter_manifest(&lifecycle_runtime);
        let mut builder = crate::PhenixRuntimeBuilder::with_default_suite().unwrap();
        builder
            .add_embedded(adapter_manifest, || Box::new(LifecycleRuntimeAdapter))
            .unwrap();
        let mut harness = builder.build().unwrap();
        harness.activate().unwrap();
        let mut worker = ApplicationWorker::new(harness).unwrap();
        enable_runtime_orchestration(&worker);
        let session_id = invoke_operation::<CreateSession>(
            &mut worker,
            SessionCreateInput {
                working_directory: "/workspace".into(),
                title: Some("plugin build".into()),
            },
        )
        .unwrap()
        .session_id;
        let authority = worker.application_root_authority(&session_id).unwrap();
        for capability in [
            RUNTIME_PLUGIN_BUILD_PERMISSION,
            "workspace.read",
            "workspace.write",
            "workspace.shell",
        ] {
            assert!(
                authority.permits(&PermissionId::parse(capability).unwrap()),
                "runtime plugin build root is missing {capability}"
            );
        }

        let sdk = {
            let harness = worker.harness.lock();
            harness
                .resolved_generation()
                .resolve_sdk_contributions([sdk_contribution()])
                .unwrap()
        };
        let generation = {
            let harness = worker.harness.lock();
            ReferenceGenerationId::from(harness.generation())
        };
        let (callbacks, _receiver) = ClientCallableCallbacks::bounded(1);
        let service = SdkApplicationService::new(
            &sdk,
            worker.projection().store(),
            SharedCallableRegistry::default(),
            PluginRuntimeId::parse("fixture.plugin-build-runtime").unwrap(),
            generation,
            callbacks,
            ClientReferenceIdentity::new(
                ClientConnectionId::parse("fixture-plugin-build-client").unwrap(),
                ReferenceGenerationId::parse("fixture-plugin-build-client-generation").unwrap(),
            ),
        )
        .unwrap();
        let surface = {
            let harness = worker.harness.lock();
            application_model_tool_surface(
                &service,
                &session_id,
                harness.resolved_generation(),
                &authority,
            )
            .unwrap()
        };
        assert!(
            surface
                .tools
                .iter()
                .any(|tool| tool.id.as_str() == "phenix.plugin")
        );

        let execution_id = "execution-plugin-build".to_owned();
        let cancellation = Arc::new(AtomicBool::new(false));
        let (progress_sender, _progress_receiver) =
            mpsc::channel::<ExecutionWorkerEvent>(APPLICATION_EXECUTION_CAPACITY);
        let (adapter, root, generations_before) = {
            let harness = worker.harness.lock();
            let root = harness.root_execution_handle(&authority);
            (
                harness.application_agent_tools().clone(),
                root,
                harness.selectable_generations(),
            )
        };
        let root_generation = root
            .generation()
            .cloned()
            .expect("runtime plugin build root has a generation");
        let root_constraints = root.constraints().clone();
        let active_before = root_generation.clone();
        let (control_transport, _control_receiver) = ChannelTransport::new(1);
        adapter
            .register(
                execution_id.clone(),
                ApplicationAgentToolRun {
                    service,
                    control_transport: control_transport.downgrade(),
                    harness: Arc::downgrade(&worker.harness),
                    session_id: session_id.clone(),
                    execution_id: execution_id.clone(),
                    root_generation,
                    root_constraints,
                    permission_handler: None,
                    tools: surface.tools,
                    runtime_entry_triggers: surface.runtime_entry_triggers,
                    cancellation,
                    progress_sender,
                },
            )
            .unwrap();

        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let artifact_content = format!("phenix-plugin-build-{nonce}");
        let artifact_output = format!(
            "target/phenix-plugin-build-{}-{nonce}.bin",
            std::process::id()
        );
        let plan = PluginBuildPlan::new(
            PluginBuildSource {
                identity: "fixture:application-plugin-build".parse().unwrap(),
                revision: format!("fixture:{nonce}").parse().unwrap(),
            },
            vec![PluginBuildStep {
                executable: "sh".parse().unwrap(),
                argv: [
                    "-c".parse().unwrap(),
                    format!(
                        "mkdir -p target && printf %s '{}' > '{}'",
                        artifact_content, artifact_output
                    )
                    .parse()
                    .unwrap(),
                ]
                .into_iter()
                .collect(),
                working_directory: BuildWorkingDirectory::root(),
                environment: BuildEnvironment::default(),
            }],
            artifact_output.parse().unwrap(),
            BTreeMap::new(),
            Authority::default(),
        )
        .unwrap();
        assert!(
            plan.requested_authority().permissions().next().is_none(),
            "fixture must exercise the model-facing default build authority path"
        );

        let request = AgentToolExecutionRequest {
            execution_id: execution_id.clone(),
            session_id: Some(session_id.clone()),
            call: ModelToolCall {
                call_id: "plugin-build".into(),
                callable_id: CallableId::parse("phenix.plugin").unwrap(),
                input: PhenixValue::Table(BTreeMap::from([
                    (
                        Key::parse("operation").unwrap(),
                        PhenixValue::String("build".into()),
                    ),
                    (
                        Key::parse("arguments").unwrap(),
                        PhenixValue::Map(BTreeMap::from([(
                            "plan".into(),
                            PhenixValue::from(serde_json::to_value(&plan).unwrap()),
                        )])),
                    ),
                ])),
            },
        };
        let output = root
            .invoke(
                &agent_tool_execution_service(),
                &serde_json::to_vec(&PhenixValue::from(&request)).unwrap(),
                None,
            )
            .unwrap();
        let value: PhenixValue = serde_json::from_slice(&output).unwrap();
        let response = AgentToolExecutionResponse::try_from(Project(&value)).unwrap();
        let AgentToolExecutionResponse::Completed { result, .. } = response else {
            panic!("phenix.plugin build must complete through the application adapter");
        };
        assert!(
            !result.is_error,
            "phenix.plugin build failed: {:?}",
            result.output
        );

        let PhenixValue::Map(fields) = result.output else {
            panic!("phenix.plugin build returned a non-map result");
        };
        let locator = match fields.get("artifact_locator") {
            Some(PhenixValue::String(locator)) => locator.clone(),
            value => panic!("phenix.plugin build returned invalid artifact locator: {value:?}"),
        };
        let revision = match fields.get("artifact_revision") {
            Some(PhenixValue::String(revision)) => revision.clone(),
            value => panic!("phenix.plugin build returned invalid revision: {value:?}"),
        };
        let artifact_value = fields
            .get("artifact")
            .expect("phenix.plugin build must expose its structured artifact");
        let artifact_json = serde_json::Value::from_value(artifact_value)
            .expect("structured plugin artifact must be JSON-compatible");
        let artifact: PluginArtifact =
            serde_json::from_value(artifact_json).expect("structured plugin artifact must decode");
        assert_eq!(artifact.locator, locator);
        assert_eq!(artifact.revision.as_ref(), revision);
        assert_eq!(
            revision,
            ArtifactRevision::from_content(artifact_content.as_bytes()).to_string()
        );
        assert!(
            matches!(
                fields.get("provenance"),
                Some(PhenixValue::List(values))
                    if values.iter().any(|value| matches!(
                        value,
                        PhenixValue::String(entry) if entry == "workspace-exec:0:sh:0"
                    ))
            ),
            "phenix.plugin build did not report workspace execution provenance"
        );
        assert_eq!(fs::read(&locator).unwrap(), artifact_content.as_bytes());

        {
            let harness = worker.harness.lock();
            assert_eq!(harness.generation(), &active_before);
            assert_eq!(harness.selectable_generations(), generations_before);
        }

        let trial_request = PluginLoadRequest {
            manifest: PluginManifest {
                id: PluginId::parse("fixture.lifecycle-runtime-guest").unwrap(),
                version: 1,
                execution: PluginExecution::Runtime {
                    runtime: lifecycle_runtime,
                    artifact: PluginArtifactInput::Ready(artifact),
                },
                dependencies: Vec::new(),
                services: Vec::new(),
                resource_namespaces: Vec::new(),
                maximum_authority: Authority::default(),
            },
            components: Vec::new(),
            entry_triggers: Vec::new(),
            process_arguments: Vec::new(),
            expected_active_revision: None,
        };
        let trial_request = AgentToolExecutionRequest {
            execution_id: execution_id.clone(),
            session_id: Some(session_id.clone()),
            call: ModelToolCall {
                call_id: "plugin-trial-built-artifact".into(),
                callable_id: CallableId::parse("phenix.plugin").unwrap(),
                input: PhenixValue::Table(BTreeMap::from([
                    (
                        Key::parse("operation").unwrap(),
                        PhenixValue::String("trial".into()),
                    ),
                    (
                        Key::parse("arguments").unwrap(),
                        PhenixValue::Map(BTreeMap::from([(
                            "request".into(),
                            PhenixValue::from(serde_json::to_value(&trial_request).unwrap()),
                        )])),
                    ),
                ])),
            },
        };
        let trial_output = root
            .invoke(
                &agent_tool_execution_service(),
                &serde_json::to_vec(&PhenixValue::from(&trial_request)).unwrap(),
                None,
            )
            .unwrap();
        let trial_value: PhenixValue = serde_json::from_slice(&trial_output).unwrap();
        let trial_response = AgentToolExecutionResponse::try_from(Project(&trial_value)).unwrap();
        let AgentToolExecutionResponse::Completed { result: trial, .. } = trial_response else {
            panic!("phenix.plugin trial must complete through the application adapter");
        };
        assert!(
            !trial.is_error,
            "phenix.plugin trial failed: {:?}",
            trial.output
        );
        let PhenixValue::Map(trial_fields) = trial.output else {
            panic!("phenix.plugin trial returned a non-map result");
        };
        let trial_generation = match trial_fields.get("generation") {
            Some(PhenixValue::String(generation)) => GenerationId::from(generation.clone()),
            value => panic!("phenix.plugin trial returned invalid generation: {value:?}"),
        };
        assert_ne!(trial_generation, active_before);
        {
            let harness = worker.harness.lock();
            assert_eq!(harness.generation(), &active_before);
            assert!(harness.selectable_generations().contains(&trial_generation));
        }

        for (call_id, operation, generation, expected_active) in [
            (
                "plugin-promote-built-artifact",
                "promote",
                trial_generation.clone(),
                trial_generation.clone(),
            ),
            (
                "plugin-rollback-built-artifact",
                "rollback",
                active_before.clone(),
                active_before.clone(),
            ),
        ] {
            let request = AgentToolExecutionRequest {
                execution_id: execution_id.clone(),
                session_id: Some(session_id.clone()),
                call: ModelToolCall {
                    call_id: call_id.into(),
                    callable_id: CallableId::parse("phenix.plugin").unwrap(),
                    input: PhenixValue::Table(BTreeMap::from([
                        (
                            Key::parse("operation").unwrap(),
                            PhenixValue::String(operation.into()),
                        ),
                        (
                            Key::parse("arguments").unwrap(),
                            PhenixValue::Map(BTreeMap::from([(
                                "generation".into(),
                                PhenixValue::String(generation.as_str().to_owned()),
                            )])),
                        ),
                    ])),
                },
            };
            let output = root
                .invoke(
                    &agent_tool_execution_service(),
                    &serde_json::to_vec(&PhenixValue::from(&request)).unwrap(),
                    None,
                )
                .unwrap();
            let value: PhenixValue = serde_json::from_slice(&output).unwrap();
            let response = AgentToolExecutionResponse::try_from(Project(&value)).unwrap();
            let AgentToolExecutionResponse::Completed { result, .. } = response else {
                panic!("phenix.plugin {operation} must complete through the application adapter");
            };
            assert!(
                !result.is_error,
                "phenix.plugin {operation} failed: {:?}",
                result.output
            );
            let PhenixValue::Map(fields) = result.output else {
                panic!("phenix.plugin {operation} returned a non-map result");
            };
            assert_eq!(
                fields.get("active_generation"),
                Some(&PhenixValue::String(expected_active.as_str().to_owned()))
            );
        }

        let retire_request = AgentToolExecutionRequest {
            execution_id: execution_id.clone(),
            session_id: Some(session_id.clone()),
            call: ModelToolCall {
                call_id: "plugin-retire-built-artifact".into(),
                callable_id: CallableId::parse("phenix.plugin").unwrap(),
                input: PhenixValue::Table(BTreeMap::from([
                    (
                        Key::parse("operation").unwrap(),
                        PhenixValue::String("retire".into()),
                    ),
                    (
                        Key::parse("arguments").unwrap(),
                        PhenixValue::Map(BTreeMap::from([(
                            "generation".into(),
                            PhenixValue::String(trial_generation.as_str().to_owned()),
                        )])),
                    ),
                ])),
            },
        };
        let retire_output = root
            .invoke(
                &agent_tool_execution_service(),
                &serde_json::to_vec(&PhenixValue::from(&retire_request)).unwrap(),
                None,
            )
            .unwrap();
        let retire_value: PhenixValue = serde_json::from_slice(&retire_output).unwrap();
        let retire_response = AgentToolExecutionResponse::try_from(Project(&retire_value)).unwrap();
        let AgentToolExecutionResponse::Completed {
            result: retired, ..
        } = retire_response
        else {
            panic!("phenix.plugin retire must complete through the application adapter");
        };
        assert!(
            !retired.is_error,
            "phenix.plugin retire failed: {:?}",
            retired.output
        );
        {
            let harness = worker.harness.lock();
            assert_eq!(harness.generation(), &active_before);
            assert_eq!(harness.selectable_generations(), generations_before);
        }

        let failed_output = format!(
            "target/phenix-plugin-build-failed-{}-{nonce}.bin",
            std::process::id()
        );
        let failed_plan = PluginBuildPlan::new(
            PluginBuildSource {
                identity: "fixture:application-plugin-build-failure".parse().unwrap(),
                revision: format!("fixture:failed:{nonce}").parse().unwrap(),
            },
            vec![PluginBuildStep {
                executable: "false".parse().unwrap(),
                argv: Vec::new(),
                working_directory: BuildWorkingDirectory::root(),
                environment: BuildEnvironment::default(),
            }],
            failed_output.parse().unwrap(),
            BTreeMap::new(),
            Authority::default(),
        )
        .unwrap();
        let failed_request = AgentToolExecutionRequest {
            execution_id: execution_id.clone(),
            session_id: Some(session_id.clone()),
            call: ModelToolCall {
                call_id: "plugin-build-failed".into(),
                callable_id: CallableId::parse("phenix.plugin").unwrap(),
                input: PhenixValue::Table(BTreeMap::from([
                    (
                        Key::parse("operation").unwrap(),
                        PhenixValue::String("build".into()),
                    ),
                    (
                        Key::parse("arguments").unwrap(),
                        PhenixValue::Map(BTreeMap::from([(
                            "plan".into(),
                            PhenixValue::from(serde_json::to_value(&failed_plan).unwrap()),
                        )])),
                    ),
                ])),
            },
        };
        let failed_output_value = root
            .invoke(
                &agent_tool_execution_service(),
                &serde_json::to_vec(&PhenixValue::from(&failed_request)).unwrap(),
                None,
            )
            .unwrap();
        let failed_value: PhenixValue = serde_json::from_slice(&failed_output_value).unwrap();
        let failed_response = AgentToolExecutionResponse::try_from(Project(&failed_value)).unwrap();
        let AgentToolExecutionResponse::Completed { result: failed, .. } = failed_response else {
            panic!("failed phenix.plugin build must complete as a tool error");
        };
        assert!(
            failed.is_error,
            "failed plugin build unexpectedly succeeded"
        );
        let failure = ApplicationError::from_value(&failed.output).unwrap();
        assert!(
            matches!(
                failure,
                ApplicationError::Failed { ref message }
                    if message.contains("plugin build step 0 exited with status 1")
            ),
            "unexpected plugin build failure: {failure:?}"
        );
        {
            let harness = worker.harness.lock();
            assert_eq!(harness.generation(), &active_before);
            assert_eq!(harness.selectable_generations(), generations_before);
        }
        assert!(
            !std::path::Path::new(&failed_output).exists(),
            "failed plugin build left an artifact output behind"
        );

        let failed_trial_output = format!(
            "target/phenix-plugin-trial-build-failed-{}-{nonce}.bin",
            std::process::id()
        );
        let failed_trial_plan = PluginBuildPlan::new(
            PluginBuildSource {
                identity: "fixture:application-plugin-trial-build-failure"
                    .parse()
                    .unwrap(),
                revision: format!("fixture:trial-failed:{nonce}").parse().unwrap(),
            },
            vec![PluginBuildStep {
                executable: "false".parse().unwrap(),
                argv: Vec::new(),
                working_directory: BuildWorkingDirectory::root(),
                environment: BuildEnvironment::default(),
            }],
            failed_trial_output.parse().unwrap(),
            BTreeMap::new(),
            Authority::default(),
        )
        .unwrap();
        let failed_trial = PluginLoadRequest {
            manifest: PluginManifest {
                id: PluginId::parse("fixture.application-plugin-trial-build-failure").unwrap(),
                version: 1,
                execution: PluginExecution::Runtime {
                    runtime: PluginRuntimeId::parse("fixture.unreached-runtime").unwrap(),
                    artifact: PluginArtifactInput::Build(failed_trial_plan),
                },
                dependencies: Vec::new(),
                services: Vec::new(),
                resource_namespaces: Vec::new(),
                maximum_authority: Authority::default(),
            },
            components: Vec::new(),
            entry_triggers: Vec::new(),
            process_arguments: Vec::new(),
            expected_active_revision: None,
        };
        let failed_trial_request = AgentToolExecutionRequest {
            execution_id,
            session_id: Some(session_id),
            call: ModelToolCall {
                call_id: "plugin-trial-build-failed".into(),
                callable_id: CallableId::parse("phenix.plugin").unwrap(),
                input: PhenixValue::Table(BTreeMap::from([
                    (
                        Key::parse("operation").unwrap(),
                        PhenixValue::String("trial".into()),
                    ),
                    (
                        Key::parse("arguments").unwrap(),
                        PhenixValue::Map(BTreeMap::from([(
                            "request".into(),
                            PhenixValue::from(serde_json::to_value(&failed_trial).unwrap()),
                        )])),
                    ),
                ])),
            },
        };
        let failed_trial_output_value = root
            .invoke(
                &agent_tool_execution_service(),
                &serde_json::to_vec(&PhenixValue::from(&failed_trial_request)).unwrap(),
                None,
            )
            .unwrap();
        let failed_trial_value: PhenixValue =
            serde_json::from_slice(&failed_trial_output_value).unwrap();
        let failed_trial_response =
            AgentToolExecutionResponse::try_from(Project(&failed_trial_value)).unwrap();
        let AgentToolExecutionResponse::Completed {
            result: failed_trial,
            ..
        } = failed_trial_response
        else {
            panic!("failed build-backed phenix.plugin trial must complete as a tool error");
        };
        assert!(
            failed_trial.is_error,
            "failed build-backed plugin trial unexpectedly succeeded"
        );
        let failure = ApplicationError::from_value(&failed_trial.output).unwrap();
        assert!(
            matches!(
                failure,
                ApplicationError::Failed { ref message }
                    if message.contains("plugin build step 0 exited with status 1")
            ),
            "build-backed trial did not reach its build step: {failure:?}"
        );
        {
            let harness = worker.harness.lock();
            assert_eq!(harness.generation(), &active_before);
            assert_eq!(harness.selectable_generations(), generations_before);
        }
        assert!(
            !std::path::Path::new(&failed_trial_output).exists(),
            "failed build-backed plugin trial left an artifact output behind"
        );

        let _ = fs::remove_file(&artifact_output);
        let _ = fs::remove_file(&locator);
    }

    #[test]
    fn model_plugin_build_authority_defaults_only_when_empty() {
        let plan_value = |requested_authority: serde_json::Value| {
            serde_json::json!({
                "source": {
                    "identity": "fixture:model-build-authority",
                    "revision": "fixture:1"
                },
                "steps": [{
                    "executable": "false",
                    "argv": [],
                    "working_directory": ".",
                    "environment": {}
                }],
                "artifact_output": "target/model-build-authority.bin",
                "configuration": {},
                "requested_authority": requested_authority
            })
        };

        let mut defaulted: PluginBuildPlan =
            serde_json::from_value(plan_value(serde_json::json!([]))).unwrap();
        apply_runtime_plugin_default_build_authority(&mut defaulted).unwrap();
        assert!(
            defaulted
                .requested_authority()
                .permits(&PermissionId::parse("workspace.read").unwrap())
        );
        assert!(
            defaulted
                .requested_authority()
                .permits(&PermissionId::parse("workspace.shell").unwrap())
        );

        let mut attenuated: PluginBuildPlan =
            serde_json::from_value(plan_value(serde_json::json!(["workspace.read"]))).unwrap();
        apply_runtime_plugin_default_build_authority(&mut attenuated).unwrap();
        assert!(
            attenuated
                .requested_authority()
                .permits(&PermissionId::parse("workspace.read").unwrap())
        );
        assert!(
            !attenuated
                .requested_authority()
                .permits(&PermissionId::parse("workspace.shell").unwrap())
        );
    }

    #[test]
    fn plugin_build_policy_requires_explicit_runtime_and_workspace_authority() {
        let policy = runtime_plugin_policy(RUNTIME_PLUGIN_BUILD_PERMISSION);
        let build = PermissionId::parse(RUNTIME_PLUGIN_BUILD_PERMISSION).unwrap();
        let read = PermissionId::parse("workspace.read").unwrap();
        let shell = PermissionId::parse("workspace.shell").unwrap();
        let write = PermissionId::parse("workspace.write").unwrap();

        assert!(policy.required_authority().permits(&build));
        assert!(policy.build_authority().permits(&read));
        assert!(policy.build_authority().permits(&shell));
        assert!(!policy.build_authority().permits(&write));
    }

    #[test]
    fn workspace_project_instruction_is_loaded_into_execution_context() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "phenix-context-instruction-{}-{nonce}",
            std::process::id()
        ));
        let instruction = format!("PHENIX_CONTEXT_INSTRUCTION_{nonce}");
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("AGENTS.md"), &instruction).unwrap();

        let mut builder = crate::PhenixRuntimeBuilder::with_default_suite().unwrap();
        let workspace_root = root.clone();
        assert!(
            builder.replace_embedded_factory(workspace_manifest().id, move || {
                workspace_factory_for(workspace_root.clone())
            },)
        );
        let mut harness = builder.build().unwrap();
        harness.activate().unwrap();
        let mut worker = ApplicationWorker::new(harness).unwrap();

        let session = invoke_operation::<CreateSession>(
            &mut worker,
            SessionCreateInput {
                working_directory: root.to_string_lossy().into_owned(),
                title: Some("context instruction".into()),
            },
        )
        .unwrap();
        let prompt = worker
            .prompt(PromptInput {
                session_id: session.session_id.clone(),
                content: vec![Content::Text {
                    text: "show repository instructions".into(),
                }],
            })
            .unwrap();
        let authority = worker
            .application_root_authority(&session.session_id)
            .unwrap();
        let root_handle = {
            let harness = worker.harness.lock();
            harness.root_execution_handle(&authority)
        };
        let projected = worker
            .invoke_context_command_on(
                &root_handle,
                ContextCommand::Project {
                    execution_id: prompt.execution_id,
                },
            )
            .unwrap();
        let ContextResponse::Projection { projection } = projected else {
            panic!("context project returned an unexpected response");
        };
        assert!(
            projection.entries.iter().any(|entry| {
                entry.resource.descriptor.kind == ContextResourceKind::ProjectInstruction
                    && entry.resource.descriptor.scope == ContextScope::Workspace
                    && String::from_utf8_lossy(entry.resource.content.as_ref())
                        .contains(&instruction)
            }),
            "root AGENTS.md was not projected into the execution context"
        );

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn child_session_inherits_controller_working_directory_and_rejects_environment_switch() {
        let controller = SessionInfo {
            session_id: SessionId::parse("session-controller").unwrap(),
            title: None,
            working_directory: "/workspace".into(),
        };

        assert_eq!(
            child_session_working_directory(&controller, None).unwrap(),
            "/workspace"
        );
        assert_eq!(
            child_session_working_directory(&controller, Some("/workspace".into())).unwrap(),
            "/workspace"
        );
        assert!(matches!(
            child_session_working_directory(&controller, Some("/tmp/other".into())),
            Err(ApplicationError::Conflict { ref message })
                if message.contains("cannot change the pinned Environment working directory")
        ));
    }

    #[test]
    fn runtime_inspection_state_requires_persistence_read_authority() {
        let denied = Authority::default();
        assert!(matches!(
            require_runtime_inspection_read(&denied),
            Err(ApplicationError::PermissionDenied { .. })
        ));

        let allowed = Authority::new([PermissionId::parse(RUNTIME_INSPECTION_READ_PERMISSION)
            .expect("static runtime inspection permission is valid")]);
        assert_eq!(require_runtime_inspection_read(&allowed), Ok(()));
    }

    fn persistent_application_worker(path: &PathBuf) -> ApplicationWorker {
        let persistence = LocalPersistence::open(path).unwrap();
        let mut harness = PhenixRuntime::default_suite_with_persistence(persistence).unwrap();
        harness.activate().unwrap();
        ApplicationWorker::new(harness).unwrap()
    }

    fn temp_db(name: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "phenix-{name}-{}-{nonce}.sqlite",
            std::process::id()
        ))
    }

    fn rename(id: &str, sequence: u64, title: &str) -> SessionUpdate {
        SessionUpdate {
            session_id: SessionId::parse(id).unwrap(),
            sequence,
            update: SessionChange::Renamed {
                title: title.to_owned(),
            },
        }
    }

    fn client_callable(contract: &str, reference: &str) -> phenix_core::CallableRef {
        phenix_core::CallableRef::new(
            ContractId::parse(contract).unwrap(),
            phenix_core::ReferenceOwnerId::Client(
                phenix_core::ClientConnectionId::parse("client-1").unwrap(),
            ),
            phenix_core::ReferenceGenerationId::parse("generation-1").unwrap(),
            phenix_core::ReferenceId::parse(reference).unwrap(),
        )
    }

    fn selection_target(provider: &str, model: &str) -> phenix_sdk::ModelTarget {
        phenix_sdk::ModelTarget {
            provider_plugin: PluginId::parse(provider).unwrap(),
            model: phenix_core::ModelId::parse(model).unwrap(),
            options: BTreeMap::new(),
        }
    }

    #[test]
    fn already_projected_persisted_progress_is_idempotent_after_projection_repair() {
        let (sender, mut events) = mpsc::channel(4);
        let mut worker = application_worker().with_event_sender(sender);
        let created = invoke_operation::<CreateSession>(
            &mut worker,
            SessionCreateInput {
                working_directory: "/workspace".into(),
                title: None,
            },
        )
        .unwrap();
        let execution_id = "execution-race".to_owned();
        let progress_change = SessionChange::Execution {
            execution_id: execution_id.clone(),
            update: ExecutionChange::Progress {
                message: "persisted before worker projection".into(),
                fraction: Some(0.5),
            },
        };

        let persisted = worker
            .invoke_session(SessionCommand::AppendJournal {
                id: created.session_id.clone(),
                entry: session_change_journal(&progress_change),
            })
            .unwrap();
        let SessionResponse::JournalAppended { entry } = persisted else {
            panic!("fixture progress append must return its journal entry");
        };
        let progress_update = session_update_from_journal(&created.session_id, entry).unwrap();

        let session = worker.session_record(&created.session_id).unwrap().unwrap();
        worker
            .append_session_change(
                &session,
                SessionChange::Renamed {
                    title: "after-progress".into(),
                },
            )
            .unwrap();

        assert_eq!(
            worker.projection().state().sessions[created.session_id.as_str()].through_sequence,
            2
        );
        let rename_event = events
            .try_recv()
            .expect("rename event after projection repair");
        let rename_update =
            SessionUpdate::from_value(&rename_event.payload).expect("session update payload");
        assert_eq!(rename_update.sequence, 2);

        project_persisted_agent_progress(&mut worker, progress_update).unwrap();
        assert!(matches!(
            events.try_recv(),
            Err(tokio::sync::mpsc::error::TryRecvError::Empty)
        ));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn failed_prompt_does_not_poison_the_session_or_next_execution() {
        let calls = Arc::new(AtomicU32::new(0));
        let model_calls = Arc::clone(&calls);
        let mut builder = crate::PhenixRuntimeBuilder::with_default_suite().unwrap();
        builder
            .add_embedded(fail_once_model_manifest(), move || {
                Box::new(FailOnceModel {
                    calls: Arc::clone(&model_calls),
                })
            })
            .unwrap();
        let mut harness = builder.build().unwrap();
        harness.activate().unwrap();
        let mut worker = ApplicationWorker::new(harness).unwrap();
        configure_fixture_routing(
            &mut worker,
            "fixture.fail-once-model",
            "fixture-fail-once",
            "failed-prompt-resubmit-regression",
        );

        let sdk = {
            let harness = worker.harness.lock();
            harness
                .resolved_generation()
                .resolve_sdk_contributions([sdk_contribution()])
                .unwrap()
        };
        let generation = {
            let harness = worker.harness.lock();
            ReferenceGenerationId::from(harness.generation())
        };
        let (callbacks, _callback_receiver) = ClientCallableCallbacks::bounded(1);
        let service = SdkApplicationService::new(
            &sdk,
            worker.projection().store(),
            SharedCallableRegistry::default(),
            PluginRuntimeId::parse("fixture.failed-prompt-runtime").unwrap(),
            generation,
            callbacks,
            ClientReferenceIdentity::new(
                ClientConnectionId::parse("fixture-failed-prompt-client").unwrap(),
                ReferenceGenerationId::parse("fixture-failed-prompt-generation").unwrap(),
            ),
        )
        .unwrap();

        let (transport, receiver) = ChannelTransport::new(APPLICATION_INVOCATION_CAPACITY);
        let worker_task = tokio::spawn(serve_application_worker_with_execution_capacity(
            worker,
            service,
            transport.clone(),
            receiver,
            2,
        ));
        let created = invoke_transport_operation::<CreateSession>(
            &transport,
            SessionCreateInput {
                working_directory: "/workspace".into(),
                title: None,
            },
        )
        .await
        .unwrap();

        let first = invoke_transport_operation::<Prompt>(
            &transport,
            PromptInput {
                session_id: created.session_id.clone(),
                content: vec![Content::Text {
                    text: "fail this execution".into(),
                }],
            },
        )
        .await
        .expect_err("first provider invocation must fail");
        assert!(
            matches!(first, ApplicationError::Failed { .. }),
            "unexpected first prompt error: {first:?}"
        );

        let second = invoke_transport_operation::<Prompt>(
            &transport,
            PromptInput {
                session_id: created.session_id.clone(),
                content: vec![Content::Text {
                    text: "run a fresh execution".into(),
                }],
            },
        )
        .await
        .expect("a failed execution must not poison the session");
        assert_eq!(second.stop_reason, StopReason::EndTurn);
        assert_ne!(second.execution_id, "execution-1");
        assert_eq!(calls.load(AtomicOrdering::SeqCst), 2);

        let resumed = invoke_transport_operation::<ResumeSession>(
            &transport,
            SessionResumeInput {
                session_id: created.session_id,
                after_sequence: None,
            },
        )
        .await
        .unwrap();
        assert!(resumed.updates.iter().any(|entry| {
            matches!(
                &entry.update,
                SessionChange::Execution {
                    execution_id,
                    update: ExecutionChange::State {
                        state: ExecutionState::Failed { .. },
                    },
                } if execution_id == "execution-1"
            )
        }));
        assert!(resumed.updates.iter().any(|entry| {
            matches!(
                &entry.update,
                SessionChange::Execution {
                    execution_id,
                    update: ExecutionChange::State {
                        state: ExecutionState::Completed,
                    },
                } if execution_id == &second.execution_id
            )
        }));

        drop(transport);
        worker_task.await.unwrap();
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn model_session_control_prompts_an_independent_session_and_continues() {
        let child_target = Arc::new(StdMutex::new(None));
        let model_target = Arc::clone(&child_target);
        let mut builder = crate::PhenixRuntimeBuilder::with_default_suite().unwrap();
        builder
            .add_embedded(session_control_model_manifest(), move || {
                Box::new(SessionControlModel {
                    child: Arc::clone(&model_target),
                })
            })
            .unwrap();
        let mut harness = builder.build().unwrap();
        harness.activate().unwrap();
        let mut worker = ApplicationWorker::new(harness).unwrap();
        enable_runtime_orchestration(&worker);
        configure_fixture_routing(
            &mut worker,
            "fixture.session-control-model",
            "fixture-session-control",
            "session-control-orchestration-regression",
        );

        let sdk = {
            let harness = worker.harness.lock();
            harness
                .resolved_generation()
                .resolve_sdk_contributions([sdk_contribution()])
                .unwrap()
        };
        let generation = {
            let harness = worker.harness.lock();
            ReferenceGenerationId::from(harness.generation())
        };
        let (callbacks, _callback_receiver) = ClientCallableCallbacks::bounded(1);
        let service = SdkApplicationService::new(
            &sdk,
            worker.projection().store(),
            SharedCallableRegistry::default(),
            PluginRuntimeId::parse("fixture.session-control-runtime").unwrap(),
            generation,
            callbacks,
            ClientReferenceIdentity::new(
                ClientConnectionId::parse("fixture-session-control-client").unwrap(),
                ReferenceGenerationId::parse("fixture-session-control-client-generation").unwrap(),
            ),
        )
        .unwrap();

        let (transport, receiver) = ChannelTransport::new(APPLICATION_INVOCATION_CAPACITY);
        let worker_task = tokio::spawn(serve_application_worker_with_execution_capacity(
            worker,
            service,
            transport.clone(),
            receiver,
            4,
        ));

        let controller = invoke_transport_operation::<CreateSession>(
            &transport,
            SessionCreateInput {
                working_directory: "/workspace".into(),
                title: Some("controller".into()),
            },
        )
        .await
        .unwrap();
        let child = invoke_transport_operation::<CreateSession>(
            &transport,
            SessionCreateInput {
                working_directory: "/workspace".into(),
                title: Some("child".into()),
            },
        )
        .await
        .unwrap();
        *child_target.lock().unwrap() = Some(child.session_id.clone());

        let result = tokio::time::timeout(
            Duration::from_secs(5),
            invoke_transport_operation::<Prompt>(
                &transport,
                PromptInput {
                    session_id: controller.session_id.clone(),
                    content: vec![Content::Text {
                        text: "drive the child session".into(),
                    }],
                },
            ),
        )
        .await
        .expect("controller orchestration must not deadlock")
        .unwrap();
        assert_eq!(result.stop_reason, StopReason::EndTurn);

        let controller_snapshot = invoke_transport_operation::<ResumeSession>(
            &transport,
            SessionResumeInput {
                session_id: controller.session_id,
                after_sequence: None,
            },
        )
        .await
        .unwrap();
        assert!(controller_snapshot.updates.iter().any(|entry| {
            matches!(
                &entry.update,
                SessionChange::TextDelta { text, .. }
                    if text == "controller observed child completion"
            )
        }));

        let child_snapshot = invoke_transport_operation::<ResumeSession>(
            &transport,
            SessionResumeInput {
                session_id: child.session_id,
                after_sequence: None,
            },
        )
        .await
        .unwrap();
        assert!(child_snapshot.updates.iter().any(|entry| {
            matches!(
                &entry.update,
                SessionChange::TextDelta { text, .. }
                    if text == "child session completed"
            )
        }));
        assert!(child_snapshot.updates.iter().any(|entry| {
            matches!(
                &entry.update,
                SessionChange::Execution {
                    update: ExecutionChange::State {
                        state: ExecutionState::Completed,
                    },
                    ..
                }
            )
        }));

        drop(transport);
        worker_task.await.unwrap();
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn model_can_trial_plugin_test_memory_across_sessions_and_roll_back() {
        let namespace = ResourceNamespace::parse("fixture.runtime-orchestration.memory").unwrap();
        let first_manifest = memory_debug_manifest(1, namespace.clone(), false);
        let second_manifest = memory_debug_manifest(2, namespace.clone(), true);
        assert!(
            !first_manifest
                .services
                .iter()
                .any(|service| service.service == skill_service())
        );
        assert!(
            second_manifest
                .services
                .iter()
                .any(|service| service.service == skill_service())
        );
        let first_component = memory_debug_component(false);
        let second_component = memory_debug_component(true);
        let trial_request = PluginLoadRequest {
            manifest: second_manifest.map_artifact(PluginArtifactInput::Ready),
            components: vec![second_component],
            entry_triggers: vec![memory_debug_trigger()],
            process_arguments: Vec::new(),
            expected_active_revision: None,
        };
        let trial_request = PhenixValue::from(serde_json::to_value(&trial_request).unwrap());

        let state = Arc::new(StdMutex::new(RuntimeOrchestrationModelState::default()));
        let mut builder = crate::PhenixRuntimeBuilder::with_default_suite().unwrap();
        let namespace_for_factory = namespace.clone();
        builder
            .add_embedded(first_manifest.clone(), move || {
                Box::new(MemoryDebugPlugin {
                    namespace: namespace_for_factory.clone(),
                })
            })
            .unwrap();
        builder.add_durable_schema(DurableSchemaRegistration::new(
            first_manifest.id.clone(),
            DurableSchema::new(namespace, 1),
        ));
        builder.add_component(first_component);
        let state_for_factory = Arc::clone(&state);
        builder
            .add_embedded(runtime_orchestration_model_manifest(), move || {
                Box::new(RuntimeOrchestrationModel {
                    state: Arc::clone(&state_for_factory),
                })
            })
            .unwrap();

        let mut harness = builder.build().unwrap();
        harness.activate().unwrap();
        {
            let mut state = state.lock().unwrap();
            state.initial_generation = Some(harness.generation().as_str().to_owned());
            state.trial_request = Some(trial_request);
        }

        let mut worker = ApplicationWorker::new(harness).unwrap();
        enable_runtime_orchestration(&worker);
        configure_fixture_routing(
            &mut worker,
            "fixture.runtime-orchestration-model",
            "fixture-runtime-orchestration",
            "runtime-orchestration-regression",
        );

        let sdk = {
            let harness = worker.harness.lock();
            harness
                .resolved_generation()
                .resolve_sdk_contributions([sdk_contribution()])
                .unwrap()
        };
        let generation = {
            let harness = worker.harness.lock();
            ReferenceGenerationId::from(harness.generation())
        };
        let (callbacks, _callback_receiver) = ClientCallableCallbacks::bounded(1);
        let service = SdkApplicationService::new(
            &sdk,
            worker.projection().store(),
            SharedCallableRegistry::default(),
            PluginRuntimeId::parse("fixture.runtime-orchestration-runtime").unwrap(),
            generation,
            callbacks,
            ClientReferenceIdentity::new(
                ClientConnectionId::parse("fixture-runtime-orchestration-client").unwrap(),
                ReferenceGenerationId::parse("fixture-runtime-orchestration-client-generation")
                    .unwrap(),
            ),
        )
        .unwrap();

        let (transport, receiver) = ChannelTransport::new(APPLICATION_INVOCATION_CAPACITY);
        let worker_task = tokio::spawn(serve_application_worker_with_execution_capacity(
            worker,
            service,
            transport.clone(),
            receiver,
            4,
        ));

        let controller = invoke_transport_operation::<CreateSession>(
            &transport,
            SessionCreateInput {
                working_directory: "/workspace".into(),
                title: Some("controller".into()),
            },
        )
        .await
        .unwrap();
        state.lock().unwrap().controller = Some(controller.session_id.clone());

        let result = {
            let prompt = invoke_transport_operation::<Prompt>(
                &transport,
                PromptInput {
                    session_id: controller.session_id.clone(),
                    content: vec![Content::Text {
                        text: "trial the changed memory plugin and verify it".into(),
                    }],
                },
            );
            tokio::pin!(prompt);
            let mut observed_model_entries = state.lock().unwrap().model_entries;
            loop {
                match tokio::time::timeout(Duration::from_secs(10), &mut prompt).await {
                    Ok(result) => break result,
                    Err(_) => {
                        let model_entries = state.lock().unwrap().model_entries;
                        assert!(
                            model_entries > observed_model_entries,
                            "runtime orchestration made no model progress for 10s after {model_entries} entries"
                        );
                        observed_model_entries = model_entries;
                    }
                }
            }
        }
        .unwrap();
        assert_eq!(result.stop_reason, StopReason::EndTurn);

        let snapshot = invoke_transport_operation::<ResumeSession>(
            &transport,
            SessionResumeInput {
                session_id: controller.session_id,
                after_sequence: None,
            },
        )
        .await
        .unwrap();
        assert!(snapshot.updates.iter().any(|entry| {
            matches!(
                &entry.update,
                SessionChange::TextDelta { text, .. }
                    if text == "orchestration complete"
            )
        }));

        drop(transport);
        worker_task.await.unwrap();
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn parent_cancellation_propagates_to_a_pending_child_session_prompt() {
        let worker = application_worker();
        let sdk = {
            let harness = worker.harness.lock();
            harness
                .resolved_generation()
                .resolve_sdk_contributions([sdk_contribution()])
                .unwrap()
        };
        let capability_generation = {
            let harness = worker.harness.lock();
            ReferenceGenerationId::from(harness.generation())
        };
        let (callbacks, _callback_receiver) = ClientCallableCallbacks::bounded(1);
        let service = SdkApplicationService::new(
            &sdk,
            worker.projection().store(),
            SharedCallableRegistry::default(),
            PluginRuntimeId::parse("fixture.orchestration-cancel-runtime").unwrap(),
            capability_generation,
            callbacks,
            ClientReferenceIdentity::new(
                ClientConnectionId::parse("fixture-orchestration-cancel-client").unwrap(),
                ReferenceGenerationId::parse("fixture-orchestration-cancel-client-generation")
                    .unwrap(),
            ),
        )
        .unwrap();

        let controller = SessionId::parse("session-controller").unwrap();
        let child = SessionId::parse("session-child").unwrap();
        let (root_generation, root_constraints) = {
            let harness = worker.harness.lock();
            let root = harness.root_execution_handle(&worker.authority);
            (
                root.generation()
                    .cloned()
                    .expect("application root has a generation"),
                root.constraints().clone(),
            )
        };
        let cancellation = Arc::new(AtomicBool::new(false));
        let (progress_sender, _progress_receiver) =
            mpsc::channel::<ExecutionWorkerEvent>(APPLICATION_EXECUTION_CAPACITY);
        let (control_transport, mut control_receiver) = ChannelTransport::new(4);
        let run = ApplicationAgentToolRun {
            service,
            control_transport: control_transport.downgrade(),
            harness: Arc::downgrade(&worker.harness),
            session_id: controller,
            execution_id: "execution-controller".into(),
            root_generation: root_generation.clone(),
            root_constraints: root_constraints.clone(),
            permission_handler: None,
            tools: Vec::new(),
            runtime_entry_triggers: BTreeMap::new(),
            cancellation: Arc::clone(&cancellation),
            progress_sender,
        };

        let child_for_call = child.clone();
        let generation_for_call = root_generation.clone();
        let call = tokio::task::spawn_blocking(move || {
            prompt_child_session(
                &run,
                child_for_call,
                vec![Content::Text {
                    text: "wait for cancellation".into(),
                }],
                generation_for_call,
            )
        });

        let child_prompt = control_receiver
            .recv()
            .await
            .expect("child prompt application invocation");
        assert_eq!(child_prompt.operation.as_str(), Prompt::ID);
        let prompt_root = child_prompt
            .root
            .as_ref()
            .expect("child prompt selects the inherited root explicitly");
        assert_eq!(prompt_root.generation, root_generation);
        assert_eq!(prompt_root.constraints, root_constraints);
        let prompt_input = PromptInput::from_value(&child_prompt.input).unwrap();
        assert_eq!(prompt_input.session_id, child);

        cancellation.store(true, Ordering::Release);

        let child_cancel = tokio::time::timeout(Duration::from_secs(2), control_receiver.recv())
            .await
            .expect("parent cancellation must request child cancellation")
            .expect("child cancel application invocation");
        assert_eq!(child_cancel.operation.as_str(), Cancel::ID);
        let cancel_root = child_cancel
            .root
            .as_ref()
            .expect("child cancellation stays in the selected generation");
        assert_eq!(cancel_root.generation, root_generation);
        assert_eq!(cancel_root.constraints, root_constraints);
        let cancel_input = ApplicationSessionInput::from_value(&child_cancel.input).unwrap();
        assert_eq!(cancel_input.session_id, child);
        child_cancel.respond(Ok(Acknowledged {}.to_value()));

        child_prompt.respond(Ok(PromptResult {
            execution_id: "execution-child".into(),
            stop_reason: StopReason::Cancelled,
        }
        .to_value()));

        let child_resume = control_receiver
            .recv()
            .await
            .expect("child terminal state is resumed after prompt completion");
        assert_eq!(child_resume.operation.as_str(), ResumeSession::ID);
        let resume_root = child_resume
            .root
            .as_ref()
            .expect("child resume stays in the selected generation");
        assert_eq!(resume_root.generation, root_generation);
        assert_eq!(resume_root.constraints, root_constraints);
        let resume_input = SessionResumeInput::from_value(&child_resume.input).unwrap();
        assert_eq!(resume_input.session_id, child);
        child_resume.respond(Ok(SessionSnapshot {
            session: session("session-child", None),
            through_sequence: 4,
            updates: Vec::new(),
        }
        .to_value()));

        let result = call.await.unwrap().unwrap();
        let PhenixValue::Map(result) = result else {
            panic!("child orchestration result must be structured");
        };
        assert_eq!(
            result.get("execution_id"),
            Some(&PhenixValue::String("execution-child".into()))
        );
        assert_eq!(
            result.get("stop_reason"),
            Some(&PhenixValue::String("cancelled".into()))
        );
        assert_eq!(result.get("through_sequence"), Some(&PhenixValue::U64(4)));
        assert_eq!(
            result.get("assistant_message"),
            Some(&PhenixValue::Option(None))
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn cancel_remains_live_while_agent_execution_is_running() {
        let gate = Arc::new(CancellationGate::new());
        let model_gate = Arc::clone(&gate);
        let mut builder = crate::PhenixRuntimeBuilder::with_default_suite().unwrap();
        builder
            .add_embedded(cancellation_gate_model_manifest(), move || {
                Box::new(CancellationGateModel {
                    gate: Arc::clone(&model_gate),
                })
            })
            .unwrap();
        let mut harness = builder.build().unwrap();
        harness.activate().unwrap();
        let mut worker = ApplicationWorker::new(harness).unwrap();
        configure_fixture_routing(
            &mut worker,
            "fixture.cancellation-gate-model",
            "fixture-cancellation",
            "cancellation-regression",
        );

        let sdk = {
            let harness = worker.harness.lock();
            harness
                .resolved_generation()
                .resolve_sdk_contributions([sdk_contribution()])
                .unwrap()
        };
        let generation = {
            let harness = worker.harness.lock();
            ReferenceGenerationId::from(harness.generation())
        };
        let (callbacks, _callback_receiver) = ClientCallableCallbacks::bounded(1);
        let service = SdkApplicationService::new(
            &sdk,
            worker.projection().store(),
            SharedCallableRegistry::default(),
            PluginRuntimeId::parse("fixture.cancellation-runtime").unwrap(),
            generation,
            callbacks,
            ClientReferenceIdentity::new(
                ClientConnectionId::parse("fixture-cancellation-client").unwrap(),
                ReferenceGenerationId::parse("fixture-cancellation-client-generation").unwrap(),
            ),
        )
        .unwrap();

        let (transport, receiver) = ChannelTransport::new(APPLICATION_INVOCATION_CAPACITY);
        let worker_task = tokio::spawn(serve_application_worker_with_execution_capacity(
            worker,
            service,
            transport.clone(),
            receiver,
            2,
        ));
        let created = invoke_transport_operation::<CreateSession>(
            &transport,
            SessionCreateInput {
                working_directory: "/workspace".into(),
                title: None,
            },
        )
        .await
        .unwrap();

        let prompt_transport = transport.clone();
        let prompt_session = created.session_id.clone();
        let prompt_task = tokio::spawn(async move {
            invoke_transport_operation::<Prompt>(
                &prompt_transport,
                PromptInput {
                    session_id: prompt_session,
                    content: vec![Content::Text {
                        text: "wait until I cancel".into(),
                    }],
                },
            )
            .await
        });

        let wait_gate = Arc::clone(&gate);
        tokio::task::spawn_blocking(move || wait_gate.wait_until_started())
            .await
            .unwrap();

        let duplicate = tokio::time::timeout(
            Duration::from_secs(2),
            invoke_transport_operation::<Prompt>(
                &transport,
                PromptInput {
                    session_id: created.session_id.clone(),
                    content: vec![Content::Text {
                        text: "this must conflict with the active prompt".into(),
                    }],
                },
            ),
        )
        .await
        .expect("duplicate prompt must be rejected without waiting for the active prompt")
        .unwrap_err();
        assert!(matches!(duplicate, ApplicationError::Conflict { .. }));

        let cancellation = tokio::time::timeout(
            Duration::from_secs(2),
            invoke_transport_operation::<Cancel>(
                &transport,
                ApplicationSessionInput {
                    session_id: created.session_id.clone(),
                },
            ),
        )
        .await;
        gate.release();
        cancellation
            .expect("cancel must not wait for the harness mutex held by the agent")
            .unwrap();

        let prompt = tokio::time::timeout(Duration::from_secs(5), prompt_task)
            .await
            .expect("cancelled prompt must complete")
            .unwrap()
            .unwrap();
        assert_eq!(prompt.stop_reason, StopReason::Cancelled);

        let resumed = invoke_transport_operation::<ResumeSession>(
            &transport,
            SessionResumeInput {
                session_id: created.session_id,
                after_sequence: None,
            },
        )
        .await
        .unwrap();
        assert!(resumed.updates.iter().any(|entry| {
            matches!(
                &entry.update,
                SessionChange::Execution {
                    update: ExecutionChange::State {
                        state: ExecutionState::Cancelled,
                    },
                    ..
                }
            )
        }));

        drop(transport);
        worker_task.await.unwrap();
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn post_tool_progress_does_not_block_the_follow_up_model_turn() {
        let mut builder = crate::PhenixRuntimeBuilder::with_default_suite().unwrap();
        builder
            .add_embedded(continuation_model_manifest(), || {
                Box::new(ToolContinuationModel)
            })
            .unwrap();
        let mut harness = builder.build().unwrap();
        harness.activate().unwrap();
        let mut worker = ApplicationWorker::new(harness).unwrap();
        configure_fixture_routing(
            &mut worker,
            "fixture.tool-continuation-model",
            "fixture-continuation",
            "continuation-regression",
        );

        let sdk = {
            let harness = worker.harness.lock();
            harness
                .resolved_generation()
                .resolve_sdk_contributions([sdk_contribution()])
                .unwrap()
        };
        let generation = {
            let harness = worker.harness.lock();
            ReferenceGenerationId::from(harness.generation())
        };
        let (callbacks, _callback_receiver) = ClientCallableCallbacks::bounded(1);
        let service = SdkApplicationService::new(
            &sdk,
            worker.projection().store(),
            SharedCallableRegistry::default(),
            PluginRuntimeId::parse("fixture.continuation-runtime").unwrap(),
            generation,
            callbacks,
            ClientReferenceIdentity::new(
                ClientConnectionId::parse("fixture-continuation-client").unwrap(),
                ReferenceGenerationId::parse("fixture-continuation-client-generation").unwrap(),
            ),
        )
        .unwrap();

        let (transport, receiver) = ChannelTransport::new(APPLICATION_INVOCATION_CAPACITY);
        let worker_task = tokio::spawn(serve_application_worker_with_execution_capacity(
            worker,
            service,
            transport.clone(),
            receiver,
            2,
        ));

        let created = invoke_transport_operation::<CreateSession>(
            &transport,
            SessionCreateInput {
                working_directory: "/workspace".into(),
                title: None,
            },
        )
        .await
        .unwrap();

        let prompt = tokio::time::timeout(
            Duration::from_secs(5),
            invoke_transport_operation::<Prompt>(
                &transport,
                PromptInput {
                    session_id: created.session_id.clone(),
                    content: vec![Content::Text {
                        text: "exercise tool continuation".into(),
                    }],
                },
            ),
        )
        .await
        .expect("tool progress must not deadlock the application worker")
        .unwrap();
        assert_eq!(prompt.stop_reason, StopReason::EndTurn);

        let resumed = invoke_transport_operation::<ResumeSession>(
            &transport,
            SessionResumeInput {
                session_id: created.session_id,
                after_sequence: None,
            },
        )
        .await
        .unwrap();
        let tool_calls = resumed
            .updates
            .iter()
            .filter(|entry| {
                matches!(
                    &entry.update,
                    SessionChange::Execution {
                        update: ExecutionChange::ToolCall { .. },
                        ..
                    }
                )
            })
            .count();
        let tool_results = resumed
            .updates
            .iter()
            .filter(|entry| {
                matches!(
                    &entry.update,
                    SessionChange::Execution {
                        update: ExecutionChange::ToolResult { .. },
                        ..
                    }
                )
            })
            .count();
        assert_eq!(tool_calls, 2);
        assert_eq!(tool_results, 2);

        drop(transport);
        worker_task.await.unwrap();
    }

    #[test]
    fn model_tool_input_normalizes_provider_variant_encoding() {
        let schema = PhenixSchema::Variant(BTreeMap::from([
            (
                Key::parse("named").unwrap(),
                PhenixSchema::Table(BTreeMap::from([(
                    Key::parse("name").unwrap(),
                    PhenixSchema::String,
                )])),
            ),
            (Key::parse("all").unwrap(), PhenixSchema::Unit),
        ]));
        let input = PhenixValue::Map(BTreeMap::from([
            ("tag".to_owned(), PhenixValue::String("named".to_owned())),
            (
                "value".to_owned(),
                PhenixValue::Map(BTreeMap::from([(
                    "name".to_owned(),
                    PhenixValue::String("symbol".to_owned()),
                )])),
            ),
        ]));

        assert_eq!(
            normalize_model_tool_input(&schema, input).unwrap(),
            PhenixValue::Variant {
                tag: Key::parse("named").unwrap(),
                value: Box::new(PhenixValue::Table(BTreeMap::from([(
                    Key::parse("name").unwrap(),
                    PhenixValue::String("symbol".to_owned()),
                )]))),
            }
        );
    }

    #[test]
    fn default_runtime_exposes_and_executes_builtin_agent_tools() {
        let mut worker = application_worker();
        let session_id = invoke_operation::<CreateSession>(
            &mut worker,
            SessionCreateInput {
                working_directory: "/workspace".into(),
                title: None,
            },
        )
        .unwrap()
        .session_id;
        let sdk = {
            let harness = worker.harness.lock();
            harness
                .resolved_generation()
                .resolve_sdk_contributions([sdk_contribution()])
                .unwrap()
        };
        let generation = {
            let harness = worker.harness.lock();
            ReferenceGenerationId::from(harness.generation())
        };
        let (callbacks, _receiver) = ClientCallableCallbacks::bounded(1);
        let service = SdkApplicationService::new(
            &sdk,
            worker.projection().store(),
            SharedCallableRegistry::default(),
            PluginRuntimeId::parse("fixture.application-runtime").unwrap(),
            generation,
            callbacks,
            ClientReferenceIdentity::new(
                ClientConnectionId::parse("fixture-client").unwrap(),
                ReferenceGenerationId::parse("fixture-client-generation").unwrap(),
            ),
        )
        .unwrap();
        let authority = worker.application_root_authority(&session_id).unwrap();
        let surface = {
            let harness = worker.harness.lock();
            application_model_tool_surface(
                &service,
                &session_id,
                harness.resolved_generation(),
                &authority,
            )
            .unwrap()
        };
        let tools = surface.tools.clone();
        assert_eq!(tools.len(), 10);
        assert_eq!(
            tools
                .iter()
                .map(|tool| tool.id.as_str())
                .collect::<Vec<_>>(),
            vec![
                "bash",
                "code.query",
                "memory.recall",
                "memory.record",
                "phenix.inspect",
                "workspace.discover",
                "workspace.git",
                "workspace.read",
                "workspace.search",
                "workspace.write",
            ]
        );
        assert_eq!(
            tools[0].input_schema,
            PhenixSchema::Table(BTreeMap::from([(
                Key::parse("command").unwrap(),
                PhenixSchema::String,
            )]))
        );
        let report = crate::model_surface_fixture::model_surface_report(
            &phenix_core::ModelInferenceRequest {
                session_id: None,
                model: phenix_core::ModelId::parse("fixture-introspection").unwrap(),
                input: Bytes::new(b"show available capabilities".to_vec()),
                options: BTreeMap::new(),
                cache: Default::default(),
                tools: tools.clone(),
                continuation: Vec::new(),
            },
        );
        assert_eq!(report.tools.len(), 10);
        assert_eq!(
            report
                .tools
                .iter()
                .map(|tool| tool.id.as_str())
                .collect::<Vec<_>>(),
            vec![
                "bash",
                "code.query",
                "memory.recall",
                "memory.record",
                "phenix.inspect",
                "workspace.discover",
                "workspace.git",
                "workspace.read",
                "workspace.search",
                "workspace.write",
            ]
        );
        let recall_tool = tools
            .iter()
            .find(|tool| tool.id.as_str() == "memory.recall")
            .expect("memory.recall must be visible");
        assert!(
            recall_tool
                .description
                .contains("not injected into every fresh session"),
            "memory.recall must explain the explicit recall contract"
        );

        assert_eq!(report.request, "show available capabilities");

        let execution_id = worker.allocate_root_execution().unwrap();
        let cancellation = Arc::new(AtomicBool::new(false));
        let (progress_sender, mut progress_receiver) =
            mpsc::channel::<ExecutionWorkerEvent>(APPLICATION_EXECUTION_CAPACITY);
        let (adapter, root_generation, root_constraints) = {
            let harness = worker.harness.lock();
            let root = harness.root_execution_handle(&worker.authority);
            (
                harness.application_agent_tools().clone(),
                root.generation()
                    .cloned()
                    .expect("default application root has a generation"),
                root.constraints().clone(),
            )
        };
        let (control_transport, _control_receiver) = ChannelTransport::new(1);
        adapter
            .register(
                execution_id.clone(),
                ApplicationAgentToolRun {
                    service: service.clone(),
                    control_transport: control_transport.downgrade(),
                    harness: Arc::downgrade(&worker.harness),
                    session_id: session_id.clone(),
                    execution_id: execution_id.clone(),
                    root_generation: root_generation.clone(),
                    root_constraints: root_constraints.clone(),
                    permission_handler: None,
                    tools: tools.clone(),
                    runtime_entry_triggers: surface.runtime_entry_triggers.clone(),
                    cancellation: Arc::clone(&cancellation),
                    progress_sender,
                },
            )
            .unwrap();

        let call = ModelToolCall {
            call_id: "call-1".into(),
            callable_id: CallableId::parse("bash").unwrap(),
            input: PhenixValue::Table(BTreeMap::from([(
                Key::parse("command").unwrap(),
                PhenixValue::String("printf phenix-runtime-bash".into()),
            )])),
        };

        let progress_record = AgentLoopProgressRecord {
            execution_id: execution_id.clone(),
            session_id: Some(session_id.clone()),
            progress: AgentLoopProgress::ToolCall { call: call.clone() },
        };
        let progress_output = worker
            .harness
            .lock()
            .invoke(
                &agent_loop_progress_service(),
                &serde_json::to_vec(&PhenixValue::from(&progress_record)).unwrap(),
                &worker.authority,
                None,
            )
            .unwrap();
        let progress_value: PhenixValue = serde_json::from_slice(&progress_output).unwrap();
        assert_eq!(
            AgentLoopProgressResponse::try_from(Project(&progress_value)).unwrap(),
            AgentLoopProgressResponse::Recorded
        );
        let progress = progress_receiver.try_recv().expect("tool progress event");
        let ExecutionWorkerEvent::Progress(progress) = progress else {
            panic!("agent loop progress must stay on the ordered application worker channel");
        };
        assert_eq!(progress.session_id, session_id);
        assert_eq!(progress.execution_id, execution_id);
        let SessionChange::Execution {
            execution_id: update_execution_id,
            update:
                ExecutionChange::ToolCall {
                    call_id,
                    callable_id,
                    input,
                },
        } = progress.update.update
        else {
            panic!("tool call progress must retain its application execution shape");
        };
        assert_eq!(update_execution_id, execution_id);
        assert_eq!(call_id, call.call_id);
        assert_eq!(callable_id, call.callable_id);
        assert_eq!(input, call.input);

        cancellation.store(true, Ordering::Release);
        let cancelled_progress = AgentLoopProgressRecord {
            execution_id: execution_id.clone(),
            session_id: Some(session_id.clone()),
            progress: AgentLoopProgress::ToolCall { call: call.clone() },
        };
        worker
            .harness
            .lock()
            .invoke(
                &agent_loop_progress_service(),
                &serde_json::to_vec(&PhenixValue::from(&cancelled_progress)).unwrap(),
                &worker.authority,
                None,
            )
            .unwrap();
        assert!(matches!(
            progress_receiver.try_recv(),
            Err(tokio::sync::mpsc::error::TryRecvError::Empty)
        ));
        cancellation.store(false, Ordering::Release);
        let request = AgentToolExecutionRequest {
            execution_id: execution_id.clone(),
            session_id: Some(session_id.clone()),
            call: call.clone(),
        };
        let encoded = serde_json::to_vec(&PhenixValue::from(&request)).unwrap();
        let output = worker
            .harness
            .lock()
            .invoke(
                &agent_tool_execution_service(),
                &encoded,
                &worker.authority,
                None,
            )
            .unwrap();
        let value: PhenixValue = serde_json::from_slice(&output).unwrap();
        let response = AgentToolExecutionResponse::try_from(Project(&value)).unwrap();
        let AgentToolExecutionResponse::Completed { result, .. } = response else {
            panic!("default bash tool must complete through the application adapter");
        };
        assert_eq!(result.call_id, "call-1");
        assert_eq!(result.callable_id.as_str(), "bash");
        assert!(!result.is_error);

        let workspace_nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let workspace_path = format!(
            ".phenix-workspace-tool-{}-{workspace_nonce}.txt",
            std::process::id()
        );
        let workspace_content = format!("workspace-model-tool-{workspace_nonce}");

        let written = invoke_agent_tool(
            &worker,
            &execution_id,
            &session_id,
            ModelToolCall {
                call_id: "workspace-write".into(),
                callable_id: CallableId::parse("workspace.write").unwrap(),
                input: ApplicationWorkspaceWriteToolRequest {
                    path: workspace_path.clone(),
                    content: workspace_content.clone(),
                    expected_content_hash: None,
                }
                .to_value(),
            },
        );
        assert!(
            !written.is_error,
            "workspace.write failed: {:?}",
            written.output
        );
        assert!(matches!(
            WorkspaceResponse::from_value(&written.output).unwrap(),
            WorkspaceResponse::Written {
                ref path,
                version: WorkspaceFileVersion::Present { ref content_hash },
            } if path == &workspace_path && !content_hash.is_empty()
        ));

        let read = invoke_agent_tool(
            &worker,
            &execution_id,
            &session_id,
            ModelToolCall {
                call_id: "workspace-read".into(),
                callable_id: CallableId::parse("workspace.read").unwrap(),
                input: ApplicationWorkspaceReadToolRequest {
                    path: workspace_path.clone(),
                }
                .to_value(),
            },
        );
        assert!(!read.is_error, "workspace.read failed: {:?}", read.output);
        assert!(matches!(
            WorkspaceResponse::from_value(&read.output).unwrap(),
            WorkspaceResponse::Read {
                ref path,
                ref content,
                ..
            } if path == &workspace_path && content == &workspace_content
        ));

        let searched = invoke_agent_tool(
            &worker,
            &execution_id,
            &session_id,
            ModelToolCall {
                call_id: "workspace-search".into(),
                callable_id: CallableId::parse("workspace.search").unwrap(),
                input: ApplicationWorkspaceSearchToolRequest {
                    needle: workspace_content.clone(),
                    path: Some(workspace_path.clone()),
                    case_sensitive: true,
                }
                .to_value(),
            },
        );
        assert!(
            !searched.is_error,
            "workspace.search failed: {:?}",
            searched.output
        );
        assert!(matches!(
            WorkspaceResponse::from_value(&searched.output).unwrap(),
            WorkspaceResponse::Search { matches }
                if matches.iter().any(|found|
                    found.path == workspace_path
                        && found.line == 1
                        && found.text.contains(&workspace_content)
                )
        ));

        let git = invoke_agent_tool(
            &worker,
            &execution_id,
            &session_id,
            ModelToolCall {
                call_id: "workspace-git".into(),
                callable_id: CallableId::parse("workspace.git").unwrap(),
                input: ApplicationWorkspaceGitToolRequest {
                    arguments: vec!["rev-parse".into(), "--is-inside-work-tree".into()],
                }
                .to_value(),
            },
        );
        assert!(!git.is_error, "workspace.git failed: {:?}", git.output);
        assert!(matches!(
            WorkspaceResponse::from_value(&git.output).unwrap(),
            WorkspaceResponse::Process {
                exit_code: 0,
                ref stdout,
                ..
            } if stdout.trim() == "true"
        ));

        let discovered = invoke_agent_tool(
            &worker,
            &execution_id,
            &session_id,
            ModelToolCall {
                call_id: "workspace-discover".into(),
                callable_id: CallableId::parse("workspace.discover").unwrap(),
                input: ApplicationWorkspaceDiscoveryRequest {
                    workspace_ids: Vec::new(),
                    repository_remotes: Vec::new(),
                    recall_terms: Vec::new(),
                }
                .to_value(),
            },
        );
        assert!(
            !discovered.is_error,
            "workspace.discover failed: {:?}",
            discovered.output
        );
        let discovered =
            ApplicationWorkspaceDiscoveryResponse::from_value(&discovered.output).unwrap();
        assert!(discovered.complete || discovered.reason.is_some());

        let _ = fs::remove_file(&workspace_path);

        let semantic_repository = "fixture-semantic-repository";
        let semantic_entity = "fixture-semantic-entity";
        let semantic_revision = CodeEntityRevision {
            entity: LogicalCodeEntity {
                id: semantic_entity.into(),
                repository_id: semantic_repository.into(),
            },
            revision: "fixture-semantic-revision-1".into(),
            sequence: 1,
            document: LanguageDocumentIdentity {
                path: "src/semantic_fixture.rs".into(),
                file_version: Some("sha256:fixture-semantic-file".into()),
                provenance: DocumentProvenance::WorkspaceBacked,
            },
            symbol: Some("crate::semantic_fixture".into()),
            name: "semantic_fixture".into(),
            signature_identity: Some("fixture-signature".into()),
            body_identity: Some("fixture-body".into()),
            provider_id: "fixture-language-provider".into(),
            provider_epoch: ProviderEpoch::new(1).unwrap(),
            facets: CodeEntityFacetRevisions {
                existence: "fixture-existence".into(),
                name_location: "fixture-name-location".into(),
                signature: Some("fixture-signature".into()),
                body: Some("fixture-body".into()),
                relations: BTreeMap::new(),
            },
        };
        let language_input =
            serde_json::to_vec(&PhenixValue::from(&LanguageCommand::RecordEntityRevision {
                revision: semantic_revision,
            }))
            .unwrap();
        worker
            .harness
            .lock()
            .invoke(
                &phenix_plugin_catalog::language_service(),
                &language_input,
                &worker.authority,
                None,
            )
            .expect("semantic fixture repository must be indexed");

        let queried = invoke_agent_tool(
            &worker,
            &execution_id,
            &session_id,
            ModelToolCall {
                call_id: "code-query".into(),
                callable_id: CallableId::parse("code.query").unwrap(),
                input: CodeQuery {
                    anchor: CodeQueryAnchor::Repository {
                        repository_id: semantic_repository.into(),
                    },
                    selection: CodeQuerySelection::Entities,
                    traversal: None,
                    projection: CodeQueryProjection::Identity,
                    budget: CodeQueryBudget {
                        max_entities: 8,
                        max_relations: 8,
                        max_bytes: 16 * 1024,
                    },
                }
                .to_value(),
            },
        );
        assert!(!queried.is_error, "code.query failed: {:?}", queried.output);
        let queried = CodeQueryResult::from_value(&queried.output).unwrap();
        assert_eq!(queried.repository_id, semantic_repository);
        assert_eq!(queried.coverage.repository_sequence, 1);
        assert!(queried.coverage.complete);
        assert!(
            queried
                .entities
                .iter()
                .any(|entity| entity.entity.id == semantic_entity),
            "code.query did not return the seeded semantic entity: {queried:?}"
        );

        let memory_record = MemoryRecord {
            id: "application-agent-memory".into(),
            kind: MemoryKind::Fact,
            scope: MemoryScope::Session {
                session_id: session_id.clone(),
            },
            content: "Helios is durable".into(),
            source_refs: vec![MemorySourceReference {
                service: session_service(),
                resource: format!("session/{}", session_id.as_str()),
                start: Some(0),
                end: Some(0),
            }],
            supporting_dependencies: Vec::new(),
            supersedes: Vec::new(),
            valid_from: None,
            valid_until: None,
            created_at: 1,
        };
        let recorded = invoke_agent_tool(
            &worker,
            &execution_id,
            &session_id,
            ModelToolCall {
                call_id: "memory-record".into(),
                callable_id: CallableId::parse("memory.record").unwrap(),
                input: memory_record.to_value(),
            },
        );
        assert!(
            !recorded.is_error,
            "memory.record failed: {:?}",
            recorded.output
        );
        assert_eq!(
            MemoryRecord::from_value(&recorded.output).unwrap(),
            memory_record
        );

        let recalled = invoke_agent_tool(
            &worker,
            &execution_id,
            &session_id,
            ModelToolCall {
                call_id: "memory-recall".into(),
                callable_id: CallableId::parse("memory.recall").unwrap(),
                input: MemoryRecallQuery {
                    scopes: vec![MemoryScope::Session {
                        session_id: session_id.clone(),
                    }],
                    kinds: vec![MemoryKind::Fact],
                    query: "Helios".into(),
                    at: 2,
                    limit: 4,
                }
                .to_value(),
            },
        );
        assert!(
            !recalled.is_error,
            "memory.recall failed: {:?}",
            recalled.output
        );
        let recalled = ApplicationMemoryRecallResponse::from_value(&recalled.output).unwrap();
        assert_eq!(recalled.records, vec![memory_record]);

        let global_memory = MemoryRecord {
            id: "application-agent-global-memory".into(),
            kind: MemoryKind::Fact,
            scope: MemoryScope::Global,
            content: "Selene persists across fresh sessions".into(),
            source_refs: vec![MemorySourceReference {
                service: session_service(),
                resource: format!("session/{}", session_id.as_str()),
                start: Some(0),
                end: Some(0),
            }],
            supporting_dependencies: Vec::new(),
            supersedes: Vec::new(),
            valid_from: None,
            valid_until: None,
            created_at: 3,
        };
        let recorded_global = invoke_agent_tool(
            &worker,
            &execution_id,
            &session_id,
            ModelToolCall {
                call_id: "memory-record-global".into(),
                callable_id: CallableId::parse("memory.record").unwrap(),
                input: global_memory.to_value(),
            },
        );
        assert!(
            !recorded_global.is_error,
            "global memory.record failed: {:?}",
            recorded_global.output
        );

        let fresh_session = invoke_operation::<CreateSession>(
            &mut worker,
            SessionCreateInput {
                working_directory: "/workspace".into(),
                title: Some("fresh memory recall".into()),
            },
        )
        .unwrap();
        let fresh_execution_id = worker.allocate_root_execution().unwrap();
        let (fresh_progress_sender, _fresh_progress_receiver) =
            mpsc::channel::<ExecutionWorkerEvent>(APPLICATION_EXECUTION_CAPACITY);
        adapter
            .register(
                fresh_execution_id.clone(),
                ApplicationAgentToolRun {
                    service: service.clone(),
                    control_transport: control_transport.downgrade(),
                    harness: Arc::downgrade(&worker.harness),
                    session_id: fresh_session.session_id.clone(),
                    execution_id: fresh_execution_id.clone(),
                    root_generation: root_generation.clone(),
                    root_constraints: root_constraints.clone(),
                    permission_handler: None,
                    tools: tools.clone(),
                    runtime_entry_triggers: surface.runtime_entry_triggers.clone(),
                    cancellation: Arc::new(AtomicBool::new(false)),
                    progress_sender: fresh_progress_sender,
                },
            )
            .unwrap();

        let recalled_global = invoke_agent_tool(
            &worker,
            &fresh_execution_id,
            &fresh_session.session_id,
            ModelToolCall {
                call_id: "memory-recall-global-fresh-session".into(),
                callable_id: CallableId::parse("memory.recall").unwrap(),
                input: MemoryRecallQuery {
                    scopes: vec![MemoryScope::Global],
                    kinds: vec![MemoryKind::Fact],
                    query: "Selene".into(),
                    at: 4,
                    limit: 4,
                }
                .to_value(),
            },
        );
        assert!(
            !recalled_global.is_error,
            "fresh-session memory.recall failed: {:?}",
            recalled_global.output
        );
        let recalled_global =
            ApplicationMemoryRecallResponse::from_value(&recalled_global.output).unwrap();
        assert_eq!(recalled_global.records, vec![global_memory]);
        adapter.remove(&fresh_execution_id);

        {
            let mut harness = worker.harness.lock();
            let selected_generation = harness.generation().as_str().to_owned();
            let traces = Arc::new(phenix_core::RuntimeTraceBuffer::default());
            harness.kernel_mut().set_runtime_trace_sink(traces.clone());
            phenix_core::RuntimeTraceSink::record(
                traces.as_ref(),
                phenix_core::RuntimeTraceEvent::Orchestration {
                    controller_session: session_id.to_string(),
                    controller_execution: execution_id.clone(),
                    kind: "fixture".into(),
                    operation: "matching".into(),
                    target_session: None,
                    child_execution: Some("execution-child".into()),
                    selected_generation: selected_generation.clone(),
                    target_generation: None,
                    success: true,
                    error: None,
                },
            );
            phenix_core::RuntimeTraceSink::record(
                traces.as_ref(),
                phenix_core::RuntimeTraceEvent::Orchestration {
                    controller_session: session_id.to_string(),
                    controller_execution: "execution-unrelated".into(),
                    kind: "fixture".into(),
                    operation: "unrelated".into(),
                    target_session: None,
                    child_execution: None,
                    selected_generation,
                    target_generation: None,
                    success: true,
                    error: None,
                },
            );
            phenix_core::RuntimeTraceSink::record(
                traces.as_ref(),
                phenix_core::RuntimeTraceEvent::ExecutionStage {
                    execution_id: execution_id.clone(),
                    session_id: Some(session_id.to_string()),
                    source: "fixture".into(),
                    stage: "model_dispatch".into(),
                    outcome: "completed".into(),
                    reason: None,
                },
            );
            phenix_core::RuntimeTraceSink::record(
                traces.as_ref(),
                phenix_core::RuntimeTraceEvent::ExecutionStage {
                    execution_id: "execution-child".into(),
                    session_id: Some(session_id.to_string()),
                    source: "fixture".into(),
                    stage: "model_dispatch".into(),
                    outcome: "completed".into(),
                    reason: None,
                },
            );
        }

        let inspect_queries = [
            ("inspect-help", "help".to_owned()),
            ("inspect-graph", "graph".to_owned()),
            ("inspect-execution-current", "execution".to_owned()),
            (
                "inspect-execution-explicit",
                format!("execution {execution_id}"),
            ),
            ("inspect-execution-shorthand", execution_id.clone()),
            ("inspect-dag-current", "dag".to_owned()),
            ("inspect-dag-explicit", format!("dag {execution_id}")),
            ("inspect-trace", "trace".to_owned()),
            ("inspect-trace-execution", format!("trace {execution_id}")),
            ("inspect-trace-chain", format!("trace-chain {execution_id}")),
            ("inspect-values", "values".to_owned()),
            ("inspect-value", format!("value {SESSION_PROJECTION_VALUE}")),
        ];
        for (call_id, query) in inspect_queries {
            let inspected = invoke_agent_tool(
                &worker,
                &execution_id,
                &session_id,
                ModelToolCall {
                    call_id: call_id.into(),
                    callable_id: CallableId::parse("phenix.inspect").unwrap(),
                    input: PhenixValue::Map(BTreeMap::from([(
                        "query".to_owned(),
                        PhenixValue::String(query.clone()),
                    )])),
                },
            );
            assert!(
                !inspected.is_error,
                "{query} inspection failed: {:?}",
                inspected.output
            );

            if query == "execution"
                || query == execution_id
                || query == format!("execution {execution_id}")
            {
                let execution = ExecutionRecord::from_value(&inspected.output).unwrap();
                assert_eq!(execution.id, execution_id);
                continue;
            }

            if query == "dag" || query == format!("dag {execution_id}") {
                let PhenixValue::Map(dag) = inspected.output else {
                    panic!("{query} inspection returned a non-map DAG");
                };
                assert_eq!(
                    dag.get("root_execution"),
                    Some(&PhenixValue::String(execution_id.clone()))
                );
                assert!(
                    matches!(dag.get("executions"), Some(PhenixValue::List(values)) if !values.is_empty())
                );
                continue;
            }

            match (query.as_str(), inspected.output) {
                ("help", PhenixValue::List(queries)) => {
                    assert!(
                        queries.contains(&PhenixValue::String("execution <execution-id>".into()))
                    );
                    assert!(queries.contains(&PhenixValue::String("dag <execution-id>".into())));
                    assert!(
                        queries.contains(&PhenixValue::String("trace-chain <execution-id>".into()))
                    );
                }
                ("graph", PhenixValue::Map(graph)) => {
                    assert!(matches!(
                        graph.get("generation"),
                        Some(PhenixValue::String(_))
                    ));
                    assert!(
                        matches!(graph.get("components"), Some(PhenixValue::List(values)) if !values.is_empty())
                    );
                }
                ("trace", PhenixValue::List(events)) => {
                    assert!(!events.is_empty());
                    assert!(events.iter().all(|event| {
                        matches!(
                            event,
                            PhenixValue::Map(fields)
                                if matches!(fields.get("event"), Some(PhenixValue::String(_)))
                        )
                    }));
                    assert!(events.iter().any(|event| {
                        matches!(
                            event,
                            PhenixValue::Map(fields)
                                if fields.get("event")
                                    == Some(&PhenixValue::String("service_invocation".into()))
                        )
                    }));
                }
                (query, PhenixValue::List(events)) if query.starts_with("trace ") => {
                    assert_eq!(events.len(), 2);
                    assert!(events.iter().any(|event| matches!(
                        event,
                        PhenixValue::Map(fields)
                            if fields.get("event")
                                == Some(&PhenixValue::String("orchestration".into()))
                                && fields.get("controller_execution")
                                    == Some(&PhenixValue::String(execution_id.clone()))
                                && fields.get("operation")
                                    == Some(&PhenixValue::String("matching".into()))
                    )));
                    assert!(events.iter().any(|event| matches!(
                        event,
                        PhenixValue::Map(fields)
                            if fields.get("event")
                                == Some(&PhenixValue::String("execution_stage".into()))
                                && fields.get("execution_id")
                                    == Some(&PhenixValue::String(execution_id.clone()))
                    )));
                }
                (query, PhenixValue::List(events)) if query.starts_with("trace-chain ") => {
                    assert_eq!(events.len(), 3);
                    assert!(events.iter().any(|event| matches!(
                        event,
                        PhenixValue::Map(fields)
                            if fields.get("event")
                                == Some(&PhenixValue::String("execution_stage".into()))
                                && fields.get("execution_id")
                                    == Some(&PhenixValue::String("execution-child".into()))
                    )));
                }
                ("values", PhenixValue::List(values)) => {
                    assert!(values.iter().any(|value| {
                        matches!(
                            value,
                            PhenixValue::Map(fields)
                                if matches!(
                                    fields.get("id"),
                                    Some(PhenixValue::String(id)) if id == SESSION_PROJECTION_VALUE
                                )
                        )
                    }));
                }
                (query, PhenixValue::Map(value)) if query.starts_with("value ") => {
                    assert_eq!(
                        value.get("id"),
                        Some(&PhenixValue::String(SESSION_PROJECTION_VALUE.into()))
                    );
                }
                (query, value) => panic!("unexpected {query} inspection value: {value:?}"),
            }
        }

        let rejected_inspect = invoke_agent_tool(
            &worker,
            &execution_id,
            &session_id,
            ModelToolCall {
                call_id: "inspect-invalid".into(),
                callable_id: CallableId::parse("phenix.inspect").unwrap(),
                input: PhenixValue::Map(BTreeMap::from([(
                    "query".to_owned(),
                    PhenixValue::String("definitely-not-an-inspect-query".into()),
                )])),
            },
        );
        assert!(rejected_inspect.is_error);
        assert!(matches!(
            ApplicationError::from_value(&rejected_inspect.output).unwrap(),
            ApplicationError::InvalidInput { message }
                if message.contains("unknown phenix.inspect query")
        ));
        let recovered_inspect = invoke_agent_tool(
            &worker,
            &execution_id,
            &session_id,
            ModelToolCall {
                call_id: "inspect-after-error".into(),
                callable_id: CallableId::parse("phenix.inspect").unwrap(),
                input: PhenixValue::Map(BTreeMap::from([(
                    "query".to_owned(),
                    PhenixValue::String("graph".into()),
                )])),
            },
        );
        assert!(
            !recovered_inspect.is_error,
            "inspection did not recover after a rejected query: {:?}",
            recovered_inspect.output
        );

        let rejected = AgentToolExecutionRequest {
            execution_id: execution_id.clone(),
            session_id: Some(session_id),
            call: ModelToolCall {
                call_id: "call-2".into(),
                callable_id: CallableId::parse("fixture.unadvertised").unwrap(),
                input: PhenixValue::Unit,
            },
        };
        let rejected = worker
            .harness
            .lock()
            .invoke(
                &agent_tool_execution_service(),
                &serde_json::to_vec(&PhenixValue::from(&rejected)).unwrap(),
                &worker.authority,
                None,
            )
            .unwrap();
        let rejected: PhenixValue = serde_json::from_slice(&rejected).unwrap();
        let rejected = AgentToolExecutionResponse::try_from(Project(&rejected)).unwrap();
        let AgentToolExecutionResponse::Completed {
            result: rejected, ..
        } = rejected
        else {
            panic!("unadvertised tool must be reported as a tool failure");
        };
        assert!(rejected.is_error);
        assert!(matches!(
            ApplicationError::from_value(&rejected.output).unwrap(),
            ApplicationError::InvalidInput { message }
                if message.contains("unavailable tool fixture.unadvertised")
        ));

        adapter.remove(&execution_id);

        let workspace = WorkspaceResponse::from_value(&result.output).unwrap();
        assert!(matches!(
            workspace,
            WorkspaceResponse::Process {
                exit_code: 0,
                ref stdout,
                ref stderr,
                ..
            } if stdout == "phenix-runtime-bash" && stderr.is_empty()
        ));

        let request = phenix_core::ModelInferenceRequest {
            session_id: None,
            model: phenix_core::ModelId::parse("gpt-fixture").unwrap(),
            input: Bytes::new(b"run the command".to_vec()),
            options: BTreeMap::new(),
            cache: Default::default(),
            tools,
            continuation: vec![ModelToolTurn {
                assistant_output: Bytes::new(Vec::new()),
                tool_calls: vec![call],
                tool_results: vec![result],
            }],
        };
        let encoded = phenix_provider_sdk::ProtocolAdapter::encode(
            &phenix_provider_sdk::Protocol::OpenAiResponses,
            &phenix_provider_sdk::Endpoint::parse("https://example.com/v1").unwrap(),
            &request,
        )
        .expect("typed Bash result must serialize into the next model request");
        let body: serde_json::Value = serde_json::from_slice(&encoded.body).unwrap();
        let outputs = body["input"].as_array().unwrap();
        let tool_output = outputs
            .iter()
            .find(|item| item["type"] == "function_call_output")
            .expect("continuation contains the Bash result");
        let output: serde_json::Value =
            serde_json::from_str(tool_output["output"].as_str().unwrap()).unwrap();
        assert_eq!(output["tag"], "Process");
        assert_eq!(output["value"]["exit_code"], 0);
        assert_eq!(output["value"]["stdout"], "phenix-runtime-bash");
    }

    #[test]
    fn application_options_use_the_resolved_component_endpoint() {
        let worker = application_worker();
        let response = worker
            .invoke_option_command(OptionCommand::Resolve {
                key: model_default_option(),
                context: OptionContext::default(),
            })
            .unwrap();
        let OptionResponse::Value { option } = response else {
            panic!("options resolve returned an unexpected response");
        };
        assert_eq!(option.value, OptionValue::String("default".into()));
    }

    #[test]
    fn selection_presentation_is_derived_from_route_cardinality() {
        let fixed = selection_target("provider-a", "model-a");
        let fixed_profile = RoutingProfile {
            id: RoutingProfileId::parse("fixed").unwrap(),
            default_target: fixed.clone(),
            fallback_targets: vec![fixed],
            callable_targets: BTreeMap::new(),
        };
        let provider_names = BTreeMap::from([(
            PluginId::parse("provider-a").unwrap(),
            "Provider A".to_owned(),
        )]);
        let fixed_info = selection_info(&fixed_profile, true, &provider_names).unwrap();
        assert_eq!(fixed_info.presentation, SelectionPresentation::Model);
        assert_eq!(fixed_info.name, "model-a");
        assert_eq!(fixed_info.provider_name, "Provider A");

        let routed_profile = RoutingProfile {
            id: RoutingProfileId::parse("router").unwrap(),
            default_target: selection_target("provider-a", "model-a"),
            fallback_targets: vec![selection_target("provider-b", "model-b")],
            callable_targets: BTreeMap::new(),
        };
        let routed_info = selection_info(&routed_profile, true, &provider_names).unwrap();
        assert_eq!(routed_info.presentation, SelectionPresentation::Router);
        assert_eq!(routed_info.name, "router");
        assert_eq!(routed_info.provider_name, "Provider A");
    }

    #[test]
    fn fixed_route_presentation_preserves_target_option_distinctions() {
        let mut target = selection_target("openai-codex", "gpt-5.6-terra");
        target.options.insert(
            "inference".to_owned(),
            PhenixValue::Map(BTreeMap::from([(
                "effort".to_owned(),
                PhenixValue::String("high".to_owned()),
            )])),
        );
        let profile = RoutingProfile {
            id: RoutingProfileId::parse("model.openai-codex.gpt-5.6-terra.fixture").unwrap(),
            default_target: target,
            fallback_targets: Vec::new(),
            callable_targets: BTreeMap::new(),
        };
        let info = selection_info(&profile, true, &BTreeMap::new()).unwrap();
        assert_eq!(info.provider_name, "openai-codex");
        assert_eq!(
            info.description.as_deref(),
            Some("openai-codex · effort high")
        );
    }

    #[test]
    fn authentication_discovery_projects_provider_owned_interactive_flows() {
        let mut worker = application_worker();
        let discovered = invoke_operation::<DiscoverAuthentication>(&mut worker, Empty {}).unwrap();
        let method = discovered
            .methods
            .iter()
            .find(|method| method.name == "OpenAI Codex (ChatGPT OAuth)")
            .expect("default suite exposes Codex OAuth");
        let (provider, local_method) =
            parse_authentication_method_id(&method.id).expect("application auth id round-trips");
        assert_eq!(provider.as_str(), "openai-codex");
        assert_eq!(method.provider_name, "OpenAI ChatGPT");
        assert!(!method.authenticated);
        assert_eq!(local_method, "oauth");
    }

    #[test]
    fn interaction_registration_admits_exact_schemas_before_replacing_slots() {
        let mut worker = application_worker();
        let permission = PermissionHandlerRef(client_callable(
            "phenix.application.permission@1",
            "permission-handler",
        ));
        let elicitation = ElicitationHandlerRef(client_callable(
            "phenix.application.elicitation@1",
            "elicitation-handler",
        ));
        let mut admitted = Vec::new();

        let result = worker.invoke_with_client_callables(
            &ContractId::parse(SetInteractionHandlers::ID).unwrap(),
            SetInteractionHandlersInput {
                handlers: InteractionHandlers {
                    permission: Some(permission.clone()),
                    elicitation: Some(elicitation.clone()),
                },
            }
            .to_value(),
            |callable, schema| {
                admitted.push((callable.clone(), schema));
                Ok(())
            },
        );

        assert!(result.is_ok());
        assert_eq!(admitted.len(), 2);
        assert_eq!(
            admitted[0].1,
            <PermissionHandlerRef as ValueCodec>::phenix_type()
        );
        assert_eq!(
            admitted[1].1,
            <ElicitationHandlerRef as ValueCodec>::phenix_type()
        );
        assert_eq!(worker.interaction_handlers().permission, Some(permission));
        assert_eq!(worker.interaction_handlers().elicitation, Some(elicitation));
    }

    #[test]
    fn failed_interaction_admission_keeps_previous_handler_slots() {
        let mut worker = application_worker();
        let previous = PermissionHandlerRef(client_callable(
            "phenix.application.permission@1",
            "previous-permission",
        ));
        worker.interaction_handlers = InteractionHandlers {
            permission: Some(previous.clone()),
            elicitation: None,
        };
        let replacement = PermissionHandlerRef(client_callable(
            "phenix.application.permission@1",
            "replacement-permission",
        ));

        let error = worker
            .invoke_with_client_callables(
                &ContractId::parse(SetInteractionHandlers::ID).unwrap(),
                SetInteractionHandlersInput {
                    handlers: InteractionHandlers {
                        permission: Some(replacement),
                        elicitation: None,
                    },
                }
                .to_value(),
                |_callable, _schema| {
                    Err(ApplicationError::SchemaMismatch {
                        message: "fixture rejection".to_owned(),
                    })
                },
            )
            .unwrap_err();

        assert!(matches!(error, ApplicationError::SchemaMismatch { .. }));
        assert_eq!(worker.interaction_handlers().permission, Some(previous));
        assert!(worker.interaction_handlers().elicitation.is_none());
    }

    #[test]
    fn worker_session_crud_updates_durable_truth_and_projection() {
        let mut worker = application_worker();
        let created = invoke_operation::<CreateSession>(
            &mut worker,
            SessionCreateInput {
                working_directory: "/workspace".into(),
                title: Some("initial".into()),
            },
        )
        .unwrap();
        assert_eq!(created.session_id.as_str(), "session-1");

        let listed =
            invoke_operation::<ListSessions>(&mut worker, PageInput { cursor: None }).unwrap();
        assert_eq!(listed.sessions, vec![created.clone()]);

        let renamed = invoke_operation::<RenameSession>(
            &mut worker,
            SessionRenameInput {
                session_id: created.session_id.clone(),
                title: "renamed".into(),
            },
        )
        .unwrap();
        assert_eq!(renamed.title.as_deref(), Some("renamed"));
        assert_eq!(
            worker.projection().state().sessions["session-1"].through_sequence,
            1
        );

        invoke_operation::<CloseSession>(
            &mut worker,
            ApplicationSessionInput {
                session_id: created.session_id.clone(),
            },
        )
        .unwrap();
        let projected = &worker.projection().state().sessions["session-1"];
        assert_eq!(projected.through_sequence, 2);
        assert!(matches!(projected.updates[1].update, SessionChange::Closed));

        let record = worker.session_record(&created.session_id).unwrap().unwrap();
        assert_eq!(record.lifecycle, SessionLifecycle::Closed);
        assert_eq!(record.title.as_deref(), Some("renamed"));
        assert_eq!(record.working_directory.as_deref(), Some("/workspace"));
    }

    #[test]
    fn retired_route_survives_session_resume_but_leaves_the_global_catalog() {
        let path = temp_db("retired-routing");
        let config_path = path.with_extension("json");
        let config = serde_json::json!({
            "agents": [], "orchestrations": [],
            "routing_profiles": [{"id":"default", "default_target": {
                "provider":"provider.fixture", "model":"model.fixture"
            }}]
        });
        std::fs::write(&config_path, serde_json::to_vec(&config).unwrap()).unwrap();
        let mut worker = persistent_application_worker(&path);
        super::super::runtime_config::apply_runtime_config(
            &mut worker.harness.lock(),
            &config_path,
        )
        .unwrap();
        let session = invoke_operation::<CreateSession>(
            &mut worker,
            SessionCreateInput {
                working_directory: "/workspace".into(),
                title: None,
            },
        )
        .unwrap();
        drop(worker);

        std::fs::write(
            &config_path,
            br#"{"agents":[],"orchestrations":[],"routing_profiles":[]}"#,
        )
        .unwrap();
        let mut worker = persistent_application_worker(&path);
        super::super::runtime_config::apply_runtime_config(
            &mut worker.harness.lock(),
            &config_path,
        )
        .unwrap();
        invoke_operation::<ResumeSession>(
            &mut worker,
            SessionResumeInput {
                session_id: session.session_id.clone(),
                after_sequence: None,
            },
        )
        .unwrap();
        let choices = invoke_operation::<ListSelections>(
            &mut worker,
            ApplicationSessionInput {
                session_id: session.session_id.clone(),
            },
        )
        .unwrap();
        assert_eq!(
            choices.selected.as_ref().map(RoutingProfileId::as_str),
            Some("default")
        );
        assert_eq!(choices.available.len(), 1);
        assert_eq!(choices.available[0].provider.as_str(), "provider.fixture");
        assert!(
            matches!(worker.invoke_model_command(ModelCommand::ListProfiles).unwrap(), ModelResponse::Profiles { profiles } if profiles.is_empty())
        );

        let retired = RoutingProfileId::parse("default").unwrap();
        let default_error = invoke_operation::<SelectDefaultSelection>(
            &mut worker,
            SelectionDefaultSelectInput {
                selection_id: retired.clone(),
            },
        )
        .unwrap_err();
        assert!(matches!(
            default_error,
            ApplicationError::InvalidInput { ref message }
                if message.contains("not available")
        ));
        let session_error = invoke_operation::<SelectSelection>(
            &mut worker,
            SelectionSelectInput {
                session_id: session.session_id,
                selection_id: retired,
            },
        )
        .unwrap_err();
        assert!(matches!(
            session_error,
            ApplicationError::InvalidInput { ref message }
                if message.contains("not available")
        ));
        drop(worker);
        std::fs::remove_file(config_path).unwrap();
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn worker_emits_journaled_session_updates() {
        let (sender, mut events) = mpsc::channel(1);
        let mut worker = application_worker().with_event_sender(sender);
        let created = invoke_operation::<CreateSession>(
            &mut worker,
            SessionCreateInput {
                working_directory: "/workspace".into(),
                title: None,
            },
        )
        .unwrap();

        invoke_operation::<RenameSession>(
            &mut worker,
            SessionRenameInput {
                session_id: created.session_id,
                title: "renamed".into(),
            },
        )
        .unwrap();

        let event = events.try_recv().expect("rename event");
        assert_eq!(event.event.as_str(), "phenix.application.session-update@1");
        let update = SessionUpdate::from_value(&event.payload).expect("session update payload");
        assert_eq!(update.sequence, 1);
        assert!(matches!(
            update.update,
            SessionChange::Renamed { title } if title == "renamed"
        ));
    }

    #[test]
    fn prompt_journal_failure_finishes_prepared_root_execution() {
        let (sender, _events) = mpsc::channel(1);
        let mut worker = application_worker().with_event_sender(sender.clone());
        let created = invoke_operation::<CreateSession>(
            &mut worker,
            SessionCreateInput {
                working_directory: "/workspace".into(),
                title: None,
            },
        )
        .unwrap();
        sender
            .try_send(ApplicationEvent {
                event: ContractId::parse("phenix.application.session-update@1").unwrap(),
                payload: Acknowledged {}.to_value(),
            })
            .expect("fill event queue");

        let error = invoke_operation::<Prompt>(
            &mut worker,
            PromptInput {
                session_id: created.session_id,
                content: vec![Content::Text {
                    text: "hello".into(),
                }],
            },
        )
        .unwrap_err();
        assert!(matches!(error, ApplicationError::Conflict { .. }));
        let response = worker
            .invoke_execution(ExecutionCommand::GetExecution {
                id: "execution-1".into(),
            })
            .unwrap();
        let ExecutionResponse::ExecutionLookup {
            execution: Some(execution),
        } = response
        else {
            panic!("prepared execution must remain queryable");
        };
        assert_eq!(execution.state, phenix_sdk::ExecutionState::Failed);
    }

    #[test]
    fn full_event_queue_rejects_mutation_before_journaling() {
        let (sender, _events) = mpsc::channel(1);
        let mut worker = application_worker().with_event_sender(sender.clone());
        let created = invoke_operation::<CreateSession>(
            &mut worker,
            SessionCreateInput {
                working_directory: "/workspace".into(),
                title: Some("initial".into()),
            },
        )
        .unwrap();
        sender
            .try_send(ApplicationEvent {
                event: ContractId::parse("phenix.application.session-update@1").unwrap(),
                payload: Acknowledged {}.to_value(),
            })
            .expect("fill the event queue");

        let error = invoke_operation::<RenameSession>(
            &mut worker,
            SessionRenameInput {
                session_id: created.session_id.clone(),
                title: "renamed".into(),
            },
        )
        .unwrap_err();
        assert!(matches!(error, ApplicationError::Conflict { .. }));
        let record = worker.session_record(&created.session_id).unwrap().unwrap();
        assert_eq!(record.title.as_deref(), Some("initial"));
        assert_eq!(
            worker.projection().state().sessions[created.session_id.as_str()].through_sequence,
            0
        );
    }

    #[test]
    fn worker_resume_reconstructs_durable_journal_after_restart() {
        let path = temp_db("application-resume");
        let session_id;
        {
            let mut worker = persistent_application_worker(&path);
            let created = invoke_operation::<CreateSession>(
                &mut worker,
                SessionCreateInput {
                    working_directory: "/workspace".into(),
                    title: Some("initial".into()),
                },
            )
            .unwrap();
            session_id = created.session_id.clone();
            invoke_operation::<RenameSession>(
                &mut worker,
                SessionRenameInput {
                    session_id: session_id.clone(),
                    title: "renamed".into(),
                },
            )
            .unwrap();
            invoke_operation::<CloseSession>(
                &mut worker,
                ApplicationSessionInput {
                    session_id: session_id.clone(),
                },
            )
            .unwrap();
        }

        let mut worker = persistent_application_worker(&path);
        let full = invoke_operation::<ResumeSession>(
            &mut worker,
            SessionResumeInput {
                session_id: session_id.clone(),
                after_sequence: None,
            },
        )
        .unwrap();
        assert_eq!(full.session.title.as_deref(), Some("renamed"));
        assert_eq!(full.through_sequence, 2);
        assert_eq!(full.updates.len(), 2);
        assert!(matches!(
            full.updates[0].update,
            SessionChange::Renamed { ref title } if title == "renamed"
        ));
        assert!(matches!(full.updates[1].update, SessionChange::Closed));
        assert_eq!(
            worker.projection().state().sessions[session_id.as_str()].through_sequence,
            2
        );

        let suffix = invoke_operation::<ResumeSession>(
            &mut worker,
            SessionResumeInput {
                session_id,
                after_sequence: Some(1),
            },
        )
        .unwrap();
        assert_eq!(suffix.through_sequence, 2);
        assert_eq!(suffix.updates.len(), 1);
        assert_eq!(suffix.updates[0].sequence, 2);
        assert!(matches!(suffix.updates[0].update, SessionChange::Closed));
        let _ = fs::remove_file(path);
    }

    #[test]
    fn worker_hides_non_application_sessions_from_session_list() {
        let mut worker = application_worker();
        worker
            .invoke_session(SessionCommand::Create {
                session: SessionRecord::new(SessionId::parse("internal").unwrap()),
            })
            .unwrap();
        let listed =
            invoke_operation::<ListSessions>(&mut worker, PageInput { cursor: None }).unwrap();
        assert!(listed.sessions.is_empty());
    }

    #[test]
    fn execution_ids_skip_durable_collisions_after_restart() {
        let path = temp_db("application-execution-id-restart");
        let session_id;
        {
            let mut worker = persistent_application_worker(&path);
            let created = invoke_operation::<CreateSession>(
                &mut worker,
                SessionCreateInput {
                    working_directory: "/workspace".into(),
                    title: None,
                },
            )
            .unwrap();
            session_id = created.session_id;
            let first = invoke_operation::<Prompt>(
                &mut worker,
                PromptInput {
                    session_id: session_id.clone(),
                    content: vec![Content::Text {
                        text: "first".into(),
                    }],
                },
            )
            .unwrap();
            assert_eq!(first.execution_id, "execution-1");
        }

        {
            let mut worker = persistent_application_worker(&path);
            let second = invoke_operation::<Prompt>(
                &mut worker,
                PromptInput {
                    session_id,
                    content: vec![Content::Text {
                        text: "second".into(),
                    }],
                },
            )
            .unwrap();
            assert_eq!(second.execution_id, "execution-2");
        }

        let _ = fs::remove_file(path);
    }

    #[test]
    fn legacy_execution_preflight_reproduces_duplicate_create_rejection() {
        let path = temp_db("application-execution-preflight-race");
        let mut first = persistent_application_worker(&path);
        let mut second = persistent_application_worker(&path);

        for worker in [&mut first, &mut second] {
            let lookup = worker
                .invoke_execution(ExecutionCommand::GetExecution {
                    id: "execution-1".into(),
                })
                .unwrap();
            assert!(matches!(
                lookup,
                ExecutionResponse::ExecutionLookup { execution: None }
            ));
        }

        first
            .invoke_execution(ExecutionCommand::CreateExecution {
                id: "execution-1".into(),
                requested_authority: ExecutionAuthority::new(Vec::<String>::new()),
            })
            .unwrap();
        let duplicate = second
            .invoke_execution(ExecutionCommand::CreateExecution {
                id: "execution-1".into(),
                requested_authority: ExecutionAuthority::new(Vec::<String>::new()),
            })
            .unwrap_err();
        assert!(
            duplicate
                .to_string()
                .contains("execution already exists: execution-1"),
            "unexpected duplicate-create error: {duplicate}"
        );

        let allocated = second.allocate_root_execution().unwrap();
        assert_eq!(allocated, "execution-2");
        let _ = fs::remove_file(path);
    }

    #[test]
    fn default_application_root_execution_keeps_orchestration_opt_in() {
        let mut worker = application_worker();
        let execution_id = worker.allocate_root_execution().unwrap();
        let response = worker
            .invoke_execution(ExecutionCommand::GetExecution {
                id: execution_id.clone(),
            })
            .unwrap();
        let ExecutionResponse::ExecutionLookup {
            execution: Some(execution),
        } = response
        else {
            panic!("allocated application execution must be queryable");
        };

        let expected = default_application_root_authority()
            .permissions()
            .map(|capability| capability.as_str().to_owned())
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(execution.authority.permissions, expected);
        for capability in [
            "workspace.read",
            "workspace.write",
            "workspace.shell",
            "workspace.git",
        ] {
            assert!(
                execution.authority.permissions.contains(capability),
                "default application execution is missing {capability}"
            );
        }
        for capability in [
            APPLICATION_SESSION_CONTROL_PERMISSION,
            RUNTIME_GENERATION_SELECT_PERMISSION,
            RUNTIME_PLUGIN_INSPECT_PERMISSION,
            RUNTIME_PLUGIN_BUILD_PERMISSION,
            RUNTIME_PLUGIN_TRIAL_PERMISSION,
            RUNTIME_PLUGIN_PROMOTE_PERMISSION,
            RUNTIME_PLUGIN_RETIRE_PERMISSION,
        ] {
            assert!(
                !execution.authority.permissions.contains(capability),
                "default application execution unexpectedly grants {capability}"
            );
        }

        let host_tools = host_model_tools(&default_application_root_authority());
        assert!(
            !host_tools
                .iter()
                .any(|tool| tool.id.as_str() == "phenix.session")
        );
        assert!(
            !host_tools
                .iter()
                .any(|tool| tool.id.as_str() == "phenix.plugin")
        );

        enable_runtime_orchestration(&worker);
        let enabled = worker
            .application_root_authority(&SessionId::parse("session-orchestration").unwrap())
            .unwrap();
        for capability in runtime_orchestration_authority().permissions() {
            assert!(
                enabled.permits(capability),
                "enabled application root is missing {capability}"
            );
        }
        let host_tools = host_model_tools(&enabled);
        assert!(
            host_tools
                .iter()
                .any(|tool| tool.id.as_str() == "phenix.session")
        );
        assert!(
            host_tools
                .iter()
                .any(|tool| tool.id.as_str() == "phenix.plugin")
        );
    }

    #[test]
    fn application_execution_allocation_survives_a_competing_durable_writer() {
        let path = temp_db("application-execution-allocation-race");
        let session_id;
        {
            let mut first = persistent_application_worker(&path);
            let created = invoke_operation::<CreateSession>(
                &mut first,
                SessionCreateInput {
                    working_directory: "/workspace".into(),
                    title: None,
                },
            )
            .unwrap();
            session_id = created.session_id;

            let response = first
                .invoke_execution(ExecutionCommand::CreateExecution {
                    id: "execution-1".into(),
                    requested_authority: ExecutionAuthority::new(Vec::<String>::new()),
                })
                .unwrap();
            assert!(matches!(response, ExecutionResponse::Execution { .. }));
        }

        let mut second = persistent_application_worker(&path);
        let prompt = invoke_operation::<Prompt>(
            &mut second,
            PromptInput {
                session_id,
                content: vec![Content::Text {
                    text: "second writer".into(),
                }],
            },
        )
        .unwrap();
        assert_eq!(prompt.execution_id, "execution-2");
        let _ = fs::remove_file(path);
    }

    #[test]
    fn prompt_preserves_multimodal_content_in_the_session_projection() {
        let mut worker = application_worker();
        let created = invoke_operation::<CreateSession>(
            &mut worker,
            SessionCreateInput {
                working_directory: "/workspace".into(),
                title: None,
            },
        )
        .unwrap();
        let content = vec![
            Content::Text {
                text: "inspect this".into(),
            },
            Content::Resource {
                uri: "file:///workspace/src/main.rs".into(),
                mime_type: Some("text/plain".into()),
                text: Some("fn main() {}".into()),
            },
            Content::Image {
                mime_type: "image/png".into(),
                data: Bytes::new(vec![137, 80, 78, 71]),
            },
        ];

        let prompt = invoke_operation::<Prompt>(
            &mut worker,
            PromptInput {
                session_id: created.session_id.clone(),
                content: content.clone(),
            },
        )
        .unwrap();

        assert_eq!(prompt.execution_id, "execution-1");
        assert_eq!(prompt.stop_reason, StopReason::EndTurn);
        let projection = &worker.projection().state().sessions[created.session_id.as_str()];
        assert_eq!(projection.through_sequence, 1);
        assert!(matches!(
            &projection.updates[0].update,
            SessionChange::Message {
                message: Message {
                    role: MessageRole::User,
                    content: projected,
                },
            } if projected == &content
        ));

        invoke_operation::<Cancel>(
            &mut worker,
            ApplicationSessionInput {
                session_id: created.session_id,
            },
        )
        .unwrap();
    }

    #[test]
    fn async_model_input_preserves_ordered_text_parts() {
        let input = model_input_from_content(&[
            Content::Text { text: "A".into() },
            Content::Text { text: "B".into() },
        ])
        .unwrap();
        assert_eq!(input.as_ref(), b"AB");
    }

    #[test]
    fn async_model_input_rejects_unrepresentable_content() {
        let error = model_input_from_content(&[Content::Image {
            mime_type: "image/png".into(),
            data: Bytes::new(vec![1, 2, 3]),
        }])
        .unwrap_err();
        assert!(matches!(error, ApplicationError::InvalidInput { .. }));
    }

    #[test]
    fn async_model_input_keeps_the_first_session_turn_unwrapped() {
        let mut reducer = SessionProjectionReducer::new();
        let session = session("session-1", None);
        let session_id = session.session_id.clone();
        reducer.insert_created(session);

        let input = model_input_from_session(
            reducer.state(),
            &session_id,
            &[Content::Text {
                text: "first question".into(),
            }],
        )
        .unwrap();

        assert_eq!(input.as_ref(), b"first question");
    }

    #[test]
    fn async_model_input_rehydrates_durable_history_after_restart() {
        let path = temp_db("application-session-memory");
        let session_id;
        {
            let mut worker = persistent_application_worker(&path);
            let created = invoke_operation::<CreateSession>(
                &mut worker,
                SessionCreateInput {
                    working_directory: "/workspace".into(),
                    title: None,
                },
            )
            .unwrap();
            session_id = created.session_id.clone();
            let record = worker.session_record(&session_id).unwrap().unwrap();
            for (role, text) in [
                (MessageRole::User, "remember alpha"),
                (MessageRole::Assistant, "alpha is remembered"),
            ] {
                worker
                    .append_session_change(
                        &record,
                        SessionChange::Message {
                            message: Message {
                                role,
                                content: vec![Content::Text { text: text.into() }],
                            },
                        },
                    )
                    .unwrap();
            }
        }

        let mut worker = persistent_application_worker(&path);
        assert!(
            !worker
                .projection()
                .state()
                .sessions
                .contains_key(session_id.as_str())
        );
        ensure_session_projection(&mut worker, &session_id).unwrap();
        let input = model_input_from_session(
            worker.projection().state(),
            &session_id,
            &[Content::Text {
                text: "what did I ask you to remember?".into(),
            }],
        )
        .unwrap();
        let input = String::from_utf8(input.as_ref().to_vec()).unwrap();

        assert!(input.contains("--- user ---\nremember alpha"));
        assert!(input.contains("--- assistant ---\nalpha is remembered"));
        assert!(input.ends_with("what did I ask you to remember?"));
        drop(worker);
        let _ = fs::remove_file(path);
    }

    #[test]
    fn async_model_input_replays_all_prior_session_messages() {
        let mut reducer = SessionProjectionReducer::new();
        let session = session("session-1", None);
        let session_id = session.session_id.clone();
        reducer.insert_created(session);

        for (sequence, role, text) in [
            (
                1,
                MessageRole::User,
                "whats the memory of this conversation you have?",
            ),
            (
                2,
                MessageRole::Assistant,
                "I can see the messages in this current conversation.",
            ),
            (3, MessageRole::User, "is that the entire chat?"),
            (
                4,
                MessageRole::Assistant,
                "I can only see the messages in this current conversation.",
            ),
        ] {
            reducer
                .apply_update(SessionUpdate {
                    session_id: session_id.clone(),
                    sequence,
                    update: SessionChange::Message {
                        message: Message {
                            role,
                            content: vec![Content::Text { text: text.into() }],
                        },
                    },
                })
                .unwrap();
        }

        let input = model_input_from_session(
            reducer.state(),
            &session_id,
            &[Content::Text {
                text: "what question?".into(),
            }],
        )
        .unwrap();
        let input = String::from_utf8(input.as_ref().to_vec()).unwrap();

        assert_eq!(
            input,
            "--- phenix session-history ---\n\
--- user ---\n\
whats the memory of this conversation you have?\n\
--- assistant ---\n\
I can see the messages in this current conversation.\n\
--- user ---\n\
is that the entire chat?\n\
--- assistant ---\n\
I can only see the messages in this current conversation.\n\
--- phenix current-user ---\n\
what question?"
        );
    }

    #[test]
    fn create_starts_projection_at_sequence_zero() {
        let mut reducer = SessionProjectionReducer::new();
        reducer.insert_created(session("session-1", None));

        let projection = &reducer.state().sessions["session-1"];
        assert_eq!(projection.through_sequence, 0);
        assert!(projection.updates.is_empty());
    }

    #[test]
    fn full_resume_replaces_projection() {
        let mut reducer = SessionProjectionReducer::new();
        reducer.insert_created(session("session-1", Some("old")));
        reducer.replace_snapshot(SessionSnapshot {
            session: session("session-1", Some("resumed")),
            through_sequence: 4,
            updates: Vec::new(),
        });

        let projection = &reducer.state().sessions["session-1"];
        assert_eq!(projection.session.title.as_deref(), Some("resumed"));
        assert_eq!(projection.through_sequence, 4);
    }

    #[test]
    fn ordered_rename_updates_history_and_projected_metadata() {
        let mut reducer = SessionProjectionReducer::new();
        reducer.insert_created(session("session-1", None));
        reducer
            .apply_update(rename("session-1", 1, "new title"))
            .unwrap();

        let projection = &reducer.state().sessions["session-1"];
        assert_eq!(projection.through_sequence, 1);
        assert_eq!(projection.session.title.as_deref(), Some("new title"));
        assert_eq!(projection.updates.len(), 1);
    }

    #[test]
    fn missing_projection_and_sequence_gap_require_repair() {
        let mut reducer = SessionProjectionReducer::new();
        let missing = reducer
            .apply_update(SessionUpdate {
                session_id: SessionId::parse("missing").unwrap(),
                sequence: 1,
                update: SessionChange::Closed,
            })
            .unwrap_err();
        assert!(matches!(missing, SessionProjectionError::Missing { .. }));

        reducer.insert_created(session("session-1", None));
        let gap = reducer
            .apply_update(SessionUpdate {
                session_id: SessionId::parse("session-1").unwrap(),
                sequence: 2,
                update: SessionChange::Closed,
            })
            .unwrap_err();
        assert_eq!(
            gap,
            SessionProjectionError::SequenceGap {
                session_id: SessionId::parse("session-1").unwrap(),
                expected: 1,
                actual: 2,
            }
        );
        assert_eq!(reducer.state().sessions["session-1"].through_sequence, 0);
    }

    #[test]
    fn observable_projection_commits_match_reducer_state() {
        let mut projection = SessionProjectionStore::new().unwrap();
        projection
            .insert_created(session("session-1", None))
            .unwrap();
        projection
            .apply_update(rename("session-1", 1, "observable"))
            .unwrap();

        let (_, value) = projection
            .store()
            .get(&ValueAddress {
                value: projection.value_id().clone(),
                path: ValuePath::root(),
            })
            .unwrap();
        let observed = SessionProjectionState::from_value(&value).unwrap();
        assert_eq!(&observed, projection.state());
        assert_eq!(
            observed.sessions["session-1"].session.title.as_deref(),
            Some("observable")
        );
    }

    #[test]
    fn projection_gap_does_not_advance_observable_version() {
        let mut projection = SessionProjectionStore::new().unwrap();
        projection
            .insert_created(session("session-1", None))
            .unwrap();
        let before = projection.store().metadata(projection.value_id()).unwrap();

        let error = projection
            .apply_update(SessionUpdate {
                session_id: SessionId::parse("session-1").unwrap(),
                sequence: 2,
                update: SessionChange::Closed,
            })
            .unwrap_err();
        assert!(matches!(
            error,
            SessionProjectionStoreError::Projection(SessionProjectionError::SequenceGap { .. })
        ));
        let after = projection.store().metadata(projection.value_id()).unwrap();
        assert_eq!(before.version, after.version);
        assert_eq!(projection.state().sessions["session-1"].through_sequence, 0);
    }

    #[test]
    fn repair_discards_snapshot_covered_updates_and_applies_contiguous_suffix() {
        let mut projection = SessionProjectionStore::new().unwrap();
        projection
            .insert_created(session("session-1", Some("stale")))
            .unwrap();
        projection
            .repair_with_snapshot(
                SessionSnapshot {
                    session: session("session-1", Some("snapshot")),
                    through_sequence: 2,
                    updates: vec![
                        rename("session-1", 1, "one"),
                        rename("session-1", 2, "snapshot"),
                    ],
                },
                [
                    rename("session-1", 2, "covered duplicate"),
                    rename("session-1", 3, "after repair"),
                ],
            )
            .unwrap();

        let repaired = &projection.state().sessions["session-1"];
        assert_eq!(repaired.through_sequence, 3);
        assert_eq!(repaired.updates.len(), 3);
        assert_eq!(repaired.session.title.as_deref(), Some("after repair"));
    }

    #[test]
    fn repair_keeps_authoritative_snapshot_when_suffix_still_has_gap() {
        let mut projection = SessionProjectionStore::new().unwrap();
        projection
            .insert_created(session("session-1", Some("stale")))
            .unwrap();

        let error = projection
            .repair_with_snapshot(
                SessionSnapshot {
                    session: session("session-1", Some("repaired")),
                    through_sequence: 2,
                    updates: vec![
                        rename("session-1", 1, "one"),
                        rename("session-1", 2, "repaired"),
                    ],
                },
                [rename("session-1", 4, "still gapped")],
            )
            .unwrap_err();

        assert!(matches!(
            error,
            SessionProjectionStoreError::Projection(SessionProjectionError::SequenceGap {
                expected: 3,
                actual: 4,
                ..
            })
        ));
        let repaired = &projection.state().sessions["session-1"];
        assert_eq!(repaired.through_sequence, 2);
        assert_eq!(repaired.session.title.as_deref(), Some("repaired"));
    }

    fn controller_lifecycle_model_manifest() -> PluginManifest {
        PluginManifest {
            id: PluginId::parse("fixture.controller-lifecycle-model").unwrap(),
            version: 1,
            execution: PluginExecution::Embedded,
            dependencies: Vec::new(),
            services: vec![ServiceContribution {
                role: ServiceRole::Terminal,
                service: model_inference_service(),
                priority: 100,
                required_authority: Authority::default(),
            }],
            resource_namespaces: Vec::new(),
            maximum_authority: Authority::default(),
        }
    }

    struct ControllerLifecycleModel;

    impl PluginInstance for ControllerLifecycleModel {
        fn start(&mut self, _host: &PluginHost<'_>) -> Result<(), String> {
            Ok(())
        }

        fn invoke(
            &mut self,
            service: &ServiceId,
            input: &[u8],
            _host: &PluginHost<'_>,
        ) -> Result<Vec<u8>, String> {
            if service != &model_inference_service() {
                return Err(format!(
                    "unsupported controller-lifecycle fixture service: {service}"
                ));
            }
            let value: PhenixValue =
                serde_json::from_slice(input).map_err(|error| error.to_string())?;
            let request = ModelInferenceRequest::try_from(Project(&value))
                .map_err(|error| error.to_string())?;
            let input = String::from_utf8_lossy(request.input.as_ref());

            let response = if input.trim_end().ends_with("LIFECYCLE_CHILD") {
                orchestration_response("child lifecycle complete", Vec::new())
            } else if input.trim_end().ends_with("LIFECYCLE_FAILED_CHILD") {
                match request.continuation.len() {
                    0 => orchestration_response(
                        "create failing child",
                        vec![orchestration_operation_call(
                            "lifecycle-create-failing-child",
                            "phenix.session",
                            "create",
                            BTreeMap::from([
                                (
                                    "working_directory".into(),
                                    PhenixValue::String("/workspace".into()),
                                ),
                                (
                                    "title".into(),
                                    PhenixValue::String("failing lifecycle child".into()),
                                ),
                            ]),
                        )],
                    ),
                    1 => {
                        let child =
                            SessionInfo::from_value(&orchestration_result(&request, 0)?.output)
                                .map_err(|error| error.to_string())?;
                        orchestration_response(
                            "send malformed child prompt",
                            vec![orchestration_operation_call(
                                "lifecycle-prompt-failing-child",
                                "phenix.session",
                                "prompt",
                                BTreeMap::from([
                                    (
                                        "session_id".into(),
                                        PhenixValue::String(child.session_id.to_string()),
                                    ),
                                    (
                                        "content".into(),
                                        PhenixValue::Map(BTreeMap::from([
                                            ("kind".into(), PhenixValue::String("text".into())),
                                            (
                                                "text".into(),
                                                PhenixValue::String("invalid shape".into()),
                                            ),
                                        ])),
                                    ),
                                ]),
                            )],
                        )
                    }
                    2 => {
                        let failed = request
                            .continuation
                            .get(1)
                            .and_then(|turn| turn.tool_results.first())
                            .ok_or_else(|| "missing failing child tool result".to_owned())?;
                        if !failed.is_error {
                            return Err("malformed child prompt unexpectedly succeeded".to_owned());
                        }
                        let child =
                            SessionInfo::from_value(&orchestration_result(&request, 0)?.output)
                                .map_err(|error| error.to_string())?;
                        orchestration_response(
                            "close failed child",
                            vec![orchestration_operation_call(
                                "lifecycle-close-failing-child",
                                "phenix.session",
                                "close",
                                BTreeMap::from([(
                                    "session_id".into(),
                                    PhenixValue::String(child.session_id.to_string()),
                                )]),
                            )],
                        )
                    }
                    3 => {
                        let _ = orchestration_result(&request, 2)?;
                        orchestration_response(
                            "controller failed child cleanup complete",
                            Vec::new(),
                        )
                    }
                    turns => {
                        return Err(format!(
                            "failed-child lifecycle fixture received {turns} continuation turns"
                        ));
                    }
                }
            } else if input.trim_end().ends_with("LIFECYCLE_SELF_CLOSE") {
                match request.continuation.as_slice() {
                    [] => {
                        let session_id = request.session_id.as_ref().ok_or_else(|| {
                            "controller lifecycle request has no session id".to_owned()
                        })?;
                        orchestration_response(
                            "attempt self close",
                            vec![orchestration_operation_call(
                                "lifecycle-self-close",
                                "phenix.session",
                                "close",
                                BTreeMap::from([(
                                    "session_id".into(),
                                    PhenixValue::String(session_id.to_string()),
                                )]),
                            )],
                        )
                    }
                    [turn] => {
                        if turn.tool_results.len() != 1 || !turn.tool_results[0].is_error {
                            return Err(
                                "controller self-close was not rejected by phenix.session".into()
                            );
                        }
                        orchestration_response("controller self close rejected", Vec::new())
                    }
                    turns => {
                        return Err(format!(
                            "controller self-close fixture received {} continuation turns",
                            turns.len()
                        ));
                    }
                }
            } else if input.trim_end().ends_with("LIFECYCLE_ENV_SWITCH") {
                match request.continuation.as_slice() {
                    [] => orchestration_response(
                        "attempt child environment switch",
                        vec![orchestration_operation_call(
                            "lifecycle-create-environment-switch",
                            "phenix.session",
                            "create",
                            BTreeMap::from([(
                                "working_directory".into(),
                                PhenixValue::String("/tmp/other".into()),
                            )]),
                        )],
                    ),
                    [turn] => {
                        if turn.tool_results.len() != 1 || !turn.tool_results[0].is_error {
                            return Err(
                                "child environment switch was not rejected by phenix.session"
                                    .into(),
                            );
                        }
                        orchestration_response("controller environment switch rejected", Vec::new())
                    }
                    turns => {
                        return Err(format!(
                            "environment-switch fixture received {} continuation turns",
                            turns.len()
                        ));
                    }
                }
            } else if input.trim_end().ends_with("LIFECYCLE_SECOND") {
                orchestration_response("controller second turn complete", Vec::new())
            } else if input.contains("LIFECYCLE_FIRST") {
                match request.continuation.len() {
                    0 => orchestration_response(
                        "create child",
                        vec![orchestration_operation_call(
                            "lifecycle-create-child",
                            "phenix.session",
                            "create",
                            BTreeMap::from([(
                                "title".into(),
                                PhenixValue::String("lifecycle child".into()),
                            )]),
                        )],
                    ),
                    1 => {
                        let child =
                            SessionInfo::from_value(&orchestration_result(&request, 0)?.output)
                                .map_err(|error| error.to_string())?;
                        orchestration_response(
                            "prompt child",
                            vec![orchestration_operation_call(
                                "lifecycle-prompt-child",
                                "phenix.session",
                                "prompt",
                                BTreeMap::from([
                                    (
                                        "session_id".into(),
                                        PhenixValue::String(child.session_id.to_string()),
                                    ),
                                    (
                                        "content".into(),
                                        PhenixValue::List(vec![
                                            Content::Text {
                                                text: "LIFECYCLE_CHILD".into(),
                                            }
                                            .to_value(),
                                        ]),
                                    ),
                                ]),
                            )],
                        )
                    }
                    2 => {
                        let child =
                            SessionInfo::from_value(&orchestration_result(&request, 0)?.output)
                                .map_err(|error| error.to_string())?;
                        let _ = orchestration_result(&request, 1)?;
                        orchestration_response(
                            "close child",
                            vec![orchestration_operation_call(
                                "lifecycle-close-child",
                                "phenix.session",
                                "close",
                                BTreeMap::from([(
                                    "session_id".into(),
                                    PhenixValue::String(child.session_id.to_string()),
                                )]),
                            )],
                        )
                    }
                    3 => {
                        let _ = orchestration_result(&request, 2)?;
                        orchestration_response("controller child close complete", Vec::new())
                    }
                    turns => {
                        return Err(format!(
                            "controller lifecycle fixture received {turns} continuation turns"
                        ));
                    }
                }
            } else {
                return Err(format!(
                    "unexpected controller lifecycle input: {}",
                    input.trim()
                ));
            };

            serde_json::to_vec(&PhenixValue::from(&response)).map_err(|error| error.to_string())
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn model_child_close_preserves_controller_with_admitted_client_tool() {
        let mut builder = crate::PhenixRuntimeBuilder::with_default_suite().unwrap();
        builder
            .add_embedded(controller_lifecycle_model_manifest(), || {
                Box::new(ControllerLifecycleModel)
            })
            .unwrap();
        let mut harness = builder.build().unwrap();
        harness.activate().unwrap();
        let mut worker = ApplicationWorker::new(harness).unwrap();
        enable_runtime_orchestration(&worker);
        configure_fixture_routing(
            &mut worker,
            "fixture.controller-lifecycle-model",
            "fixture-controller-lifecycle",
            "controller-lifecycle-regression",
        );

        let sdk = {
            let harness = worker.harness.lock();
            harness
                .resolved_generation()
                .resolve_sdk_contributions([sdk_contribution()])
                .unwrap()
        };
        let generation = {
            let harness = worker.harness.lock();
            ReferenceGenerationId::from(harness.generation())
        };
        let (callbacks, _callback_receiver) = ClientCallableCallbacks::bounded(1);
        let client_owner =
            ClientConnectionId::parse("fixture-controller-lifecycle-client").unwrap();
        let client_generation =
            ReferenceGenerationId::parse("fixture-controller-lifecycle-generation").unwrap();
        let service = SdkApplicationService::new(
            &sdk,
            worker.projection().store(),
            SharedCallableRegistry::default(),
            PluginRuntimeId::parse("fixture-controller-lifecycle-runtime").unwrap(),
            generation,
            callbacks,
            ClientReferenceIdentity::new(client_owner.clone(), client_generation.clone()),
        )
        .unwrap();
        let admission_service = service.clone();

        let (transport, receiver) = ChannelTransport::new(APPLICATION_INVOCATION_CAPACITY);
        let worker_task = tokio::spawn(serve_application_worker_with_execution_capacity(
            worker,
            service,
            transport.clone(),
            receiver,
            4,
        ));

        let controller = invoke_transport_operation::<CreateSession>(
            &transport,
            SessionCreateInput {
                working_directory: "/workspace".into(),
                title: Some("controller".into()),
            },
        )
        .await
        .unwrap();

        let client_tool = CallableRef::new(
            ContractId::parse("fixture.controller-lifecycle-client-tool@1").unwrap(),
            ReferenceOwnerId::Client(client_owner),
            client_generation,
            ReferenceId::parse("handler").unwrap(),
        );
        admission_service
            .invoke(
                &ContractId::parse(AddClientTool::ID).unwrap(),
                ClientToolAddInput {
                    session_id: controller.session_id.clone(),
                    tool: ClientToolDefinition {
                        id: CallableId::parse("fixture.client.context").unwrap(),
                        description: "Client context fixture".into(),
                        input: Type::Unit,
                        output: Type::Unit,
                        capabilities: Vec::new(),
                        requires_permission: false,
                        invoke: PhenixValue::Callable(client_tool),
                    },
                }
                .to_value(),
            )
            .expect("controller client tool admission must succeed");

        let first = invoke_transport_operation::<Prompt>(
            &transport,
            PromptInput {
                session_id: controller.session_id.clone(),
                content: vec![Content::Text {
                    text: "LIFECYCLE_FIRST".into(),
                }],
            },
        )
        .await
        .expect("controller must finish after closing its child");
        assert_eq!(first.stop_reason, StopReason::EndTurn);

        let second = invoke_transport_operation::<Prompt>(
            &transport,
            PromptInput {
                session_id: controller.session_id.clone(),
                content: vec![Content::Text {
                    text: "LIFECYCLE_SECOND".into(),
                }],
            },
        )
        .await
        .expect("child cleanup must not close or poison the controller session");
        assert_eq!(second.stop_reason, StopReason::EndTurn);
        assert_ne!(first.execution_id, second.execution_id);

        let self_close = invoke_transport_operation::<Prompt>(
            &transport,
            PromptInput {
                session_id: controller.session_id.clone(),
                content: vec![Content::Text {
                    text: "LIFECYCLE_SELF_CLOSE".into(),
                }],
            },
        )
        .await
        .expect("model-side self-close rejection must not abort the controller execution");
        assert_eq!(self_close.stop_reason, StopReason::EndTurn);

        let after_self_close = invoke_transport_operation::<Prompt>(
            &transport,
            PromptInput {
                session_id: controller.session_id.clone(),
                content: vec![Content::Text {
                    text: "LIFECYCLE_SECOND".into(),
                }],
            },
        )
        .await
        .expect("rejected self-close must leave the controller open");
        assert_eq!(after_self_close.stop_reason, StopReason::EndTurn);
        assert_ne!(self_close.execution_id, after_self_close.execution_id);

        let environment_switch = invoke_transport_operation::<Prompt>(
            &transport,
            PromptInput {
                session_id: controller.session_id.clone(),
                content: vec![Content::Text {
                    text: "LIFECYCLE_ENV_SWITCH".into(),
                }],
            },
        )
        .await
        .expect("rejected child Environment switch must not abort the controller");
        assert_eq!(environment_switch.stop_reason, StopReason::EndTurn);

        let after_environment_switch = invoke_transport_operation::<Prompt>(
            &transport,
            PromptInput {
                session_id: controller.session_id.clone(),
                content: vec![Content::Text {
                    text: "LIFECYCLE_SECOND".into(),
                }],
            },
        )
        .await
        .expect("rejected child Environment switch must leave the controller usable");
        assert_eq!(after_environment_switch.stop_reason, StopReason::EndTurn);
        assert_ne!(
            environment_switch.execution_id,
            after_environment_switch.execution_id
        );

        let failed_child_cleanup = invoke_transport_operation::<Prompt>(
            &transport,
            PromptInput {
                session_id: controller.session_id.clone(),
                content: vec![Content::Text {
                    text: "LIFECYCLE_FAILED_CHILD".into(),
                }],
            },
        )
        .await
        .expect("failed child prompt cleanup must not abort the controller");
        assert_eq!(failed_child_cleanup.stop_reason, StopReason::EndTurn);

        let after_failed_child = invoke_transport_operation::<Prompt>(
            &transport,
            PromptInput {
                session_id: controller.session_id.clone(),
                content: vec![Content::Text {
                    text: "LIFECYCLE_SECOND".into(),
                }],
            },
        )
        .await
        .expect("closing a failed child must leave the controller usable");
        assert_eq!(after_failed_child.stop_reason, StopReason::EndTurn);
        assert_ne!(
            failed_child_cleanup.execution_id,
            after_failed_child.execution_id
        );

        let snapshot = invoke_transport_operation::<ResumeSession>(
            &transport,
            SessionResumeInput {
                session_id: controller.session_id.clone(),
                after_sequence: None,
            },
        )
        .await
        .unwrap();
        assert!(snapshot.updates.iter().any(|entry| {
            matches!(
                &entry.update,
                SessionChange::TextDelta { text, .. }
                    if text == "controller child close complete"
            )
        }));
        assert!(snapshot.updates.iter().any(|entry| {
            matches!(
                &entry.update,
                SessionChange::TextDelta { text, .. }
                    if text == "controller second turn complete"
            )
        }));
        assert!(snapshot.updates.iter().any(|entry| {
            matches!(
                &entry.update,
                SessionChange::TextDelta { text, .. }
                    if text == "controller self close rejected"
            )
        }));
        assert!(snapshot.updates.iter().any(|entry| {
            matches!(
                &entry.update,
                SessionChange::TextDelta { text, .. }
                    if text == "controller environment switch rejected"
            )
        }));
        assert!(snapshot.updates.iter().any(|entry| {
            matches!(
                &entry.update,
                SessionChange::TextDelta { text, .. }
                    if text == "controller failed child cleanup complete"
            )
        }));

        let sessions =
            invoke_transport_operation::<ListSessions>(&transport, PageInput { cursor: None })
                .await
                .unwrap();
        assert_eq!(
            sessions
                .sessions
                .iter()
                .filter(|session| session.working_directory == "/workspace")
                .count(),
            3,
            "controller plus both closed children must remain independently addressable"
        );
        assert!(
            sessions
                .sessions
                .iter()
                .any(|session| session.session_id == controller.session_id)
        );

        drop(transport);
        worker_task.await.unwrap();
    }
}
