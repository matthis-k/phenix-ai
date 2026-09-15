use crate::{default_suite_authority, PhenixHarness};
use phenix_acp_stdio::{
    serve_stdio_with_events_and_callbacks, ApplicationEvent, ApplicationInvocation,
    ChannelTransport, ClientCapabilityCallbacks, ClientCapabilityIdentity, SdkApplicationService,
};
use phenix_application_interface::{
    types::{
        Acknowledged, ApplicationError, Content, ElicitationHandlerRef, ExecutionChange,
        ExecutionState, InteractionHandlers, Message, MessageRole, PageInput, PermissionHandlerRef,
        PromptInput, PromptResult, ReviewDecisionInput, ReviewRecord, SessionChange,
        SessionCreateInput, SessionInfo, SessionInput as ApplicationSessionInput, SessionList,
        SessionProjection, SessionProjectionState, SessionRenameInput, SessionResumeInput,
        SessionSnapshot, SessionUpdate, SetInteractionHandlersInput, StopReason,
    },
    AddClientTool, Cancel, CloseSession, CreateSession, DecideReview, GetSdk, InvokeCallable,
    InvokeCapability, ListCallables, ListSessions, Operation, Prompt, RemoveClientTool,
    RenameSession, ResumeSession, SetInteractionHandlers,
};
use phenix_core::{
    Authority, Bytes, CallableId, CapabilityGenerationId, ClientConnectionId, ContractId,
    HasPhenixSchema, LocalPersistence, ObservableError, ObservableRegistration, ObservableStore,
    PhenixContract, PhenixValue, PluginId, Project, RoutingProfileId, RuntimeId, SessionId,
    SharedCapabilityRegistry, SnapshotPolicy, ValueCodec, ValueId, ValuePath,
};
use phenix_plugin_catalog::{
    agent_loop_service, execution_review_service, sdk_contribution, session_service,
    AgentLoopCommand, AgentLoopResponse, ExecutionReviewCommand, ExecutionReviewResponse,
    OptionStartupPrecedence, SessionCommand, SessionJournalDraft, SessionJournalEntry,
    SessionLifecycle, SessionRecord, SessionResponse, SessionTransition, SDK_PLUGIN,
};
use std::{
    collections::BTreeMap,
    env, fs,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};
use tokio::sync::mpsc;

pub const APPLICATION_INVOCATION_CAPACITY: usize = 64;
pub const CLIENT_CAPABILITY_CAPACITY: usize = 64;
pub const APPLICATION_EVENT_CAPACITY: usize = 256;
const APPLICATION_EXECUTION_CAPACITY: usize = 64;
pub const SESSION_PROJECTION_VALUE: &str = "phenix.application.sessions@1";
const DEFAULT_ROUTING_PROFILE: &str = "router.mixed";
const DEFAULT_APPLICATION_AGENT: &str = "agent.coordinator";

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
    harness: Arc<PhenixHarness>,
    authority: Authority,
    projection: SessionProjectionStore,
    interaction_handlers: InteractionHandlers,
    event_sender: Option<mpsc::Sender<ApplicationEvent>>,
    next_session_ordinal: u64,
    next_execution_ordinal: u64,
}

impl ApplicationWorker {
    pub fn new(harness: PhenixHarness) -> Result<Self, ObservableError> {
        Ok(Self {
            harness: Arc::new(harness),
            authority: default_suite_authority(),
            projection: SessionProjectionStore::new()?,
            interaction_handlers: InteractionHandlers {
                permission: None,
                elicitation: None,
            },
            event_sender: None,
            next_session_ordinal: 1,
            next_execution_ordinal: 1,
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
            _ => Err(ApplicationError::UnsupportedCapability {
                capability: operation.clone(),
            }),
        }
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

    fn create_session(
        &mut self,
        request: SessionCreateInput,
    ) -> Result<SessionInfo, ApplicationError> {
        let id = self.allocate_session_id()?;
        let response = self.invoke_session(SessionCommand::Create {
            session: SessionRecord::application(id, request.working_directory, request.title),
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
        let session = self.require_open_application_session(&request.session_id)?;
        self.append_session_change(
            &session,
            SessionChange::Message {
                message: Message {
                    role: MessageRole::User,
                    content: request.content,
                },
            },
        )?;
        let execution_id = self.allocate_execution_id()?;
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

    fn append_execution_change(
        &mut self,
        session_id: &SessionId,
        execution_id: &str,
        update: ExecutionChange,
    ) -> Result<SessionUpdate, ApplicationError> {
        let session = self.require_open_application_session(session_id)?;
        self.append_session_change(
            &session,
            SessionChange::Execution {
                execution_id: execution_id.to_owned(),
                update,
            },
        )
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

    fn allocate_session_id(&mut self) -> Result<SessionId, ApplicationError> {
        loop {
            let ordinal = self.next_session_ordinal;
            self.next_session_ordinal =
                ordinal
                    .checked_add(1)
                    .ok_or_else(|| ApplicationError::Failed {
                        message: "application session id space exhausted".to_owned(),
                    })?;
            let id = SessionId::parse(format!("session-{ordinal}")).map_err(|message| {
                ApplicationError::Failed {
                    message: format!("generated invalid session id: {message}"),
                }
            })?;
            if self.session_record(&id)?.is_none() {
                return Ok(id);
            }
        }
    }

    fn allocate_execution_id(&mut self) -> Result<String, ApplicationError> {
        let ordinal = self.next_execution_ordinal;
        self.next_execution_ordinal =
            ordinal
                .checked_add(1)
                .ok_or_else(|| ApplicationError::Failed {
                    message: "application execution id space exhausted".to_owned(),
                })?;
        Ok(format!("execution-{ordinal}"))
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
    Harness(#[from] crate::HarnessBuildError),
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
    let mut harness = PhenixHarness::default_suite_with_persistence(persistence)?;
    harness.activate()?;
    apply_application_configuration(&mut harness)?;
    serve_configured_application(harness).await
}

fn configured_state_path() -> Result<PathBuf, ConfiguredApplicationError> {
    if let Some(path) = env::var_os("PHENIX_STATE_DB") {
        return Ok(PathBuf::from(path));
    }
    if let Some(state_home) = env::var_os("XDG_STATE_HOME") {
        return Ok(PathBuf::from(state_home).join("phenix/acp.sqlite"));
    }
    if let Some(home) = env::var_os("HOME") {
        return Ok(PathBuf::from(home).join(".local/state/phenix/acp.sqlite"));
    }
    Err(ConfiguredApplicationError::Configuration {
        message: "cannot determine durable state path; set PHENIX_STATE_DB or XDG_STATE_HOME"
            .to_owned(),
    })
}

fn apply_application_configuration(
    harness: &mut PhenixHarness,
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
    harness: PhenixHarness,
) -> Result<(), ConfiguredApplicationError> {
    let sdk = harness
        .resolved_harness()
        .resolve_sdk_contributions([sdk_contribution()])?;
    let (event_sender, event_receiver) =
        mpsc::channel::<ApplicationEvent>(APPLICATION_EVENT_CAPACITY);
    let worker = ApplicationWorker::new(harness)?.with_event_sender(event_sender);
    let runtime = RuntimeId::parse("phenix.application-runtime")
        .expect("static application runtime id is valid");
    let generation = CapabilityGenerationId::from(worker.harness.generation());
    let client = ClientCapabilityIdentity::new(
        ClientConnectionId::parse("stdio-client-1").expect("static ACP client id is valid"),
        CapabilityGenerationId::parse("stdio-connection-1")
            .expect("static ACP connection generation is valid"),
    );
    let (client_callbacks, callback_receiver) =
        ClientCapabilityCallbacks::bounded(CLIENT_CAPABILITY_CAPACITY);
    let service = SdkApplicationService::new(
        &sdk,
        worker.projection().store(),
        SharedCapabilityRegistry::default(),
        runtime,
        generation,
        client_callbacks,
        client,
    )?;
    let (transport, receiver) = ChannelTransport::new(APPLICATION_INVOCATION_CAPACITY);
    let worker = serve_application_worker(worker, service.clone(), receiver);
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
    cancellation: Arc<AtomicBool>,
    prompt: ApplicationInvocation,
}

struct ExecutionCompletion {
    session_id: SessionId,
    execution_id: String,
    result: Result<String, ApplicationError>,
}

async fn serve_application_worker(
    mut worker: ApplicationWorker,
    service: SdkApplicationService,
    mut receiver: mpsc::Receiver<ApplicationInvocation>,
) {
    let (completion_sender, mut completions) =
        mpsc::channel::<ExecutionCompletion>(APPLICATION_EXECUTION_CAPACITY);
    let mut active = BTreeMap::<String, ActiveExecution>::new();
    let mut input_closed = false;

    loop {
        if input_closed && active.is_empty() {
            break;
        }
        tokio::select! {
            invocation = receiver.recv(), if !input_closed => {
                let Some(invocation) = invocation else {
                    input_closed = true;
                    continue;
                };
                if invocation.operation.as_str() == Prompt::ID {
                    start_prompt(&mut worker, &service, &completion_sender, &mut active, invocation);
                    continue;
                }
                if invocation.operation.as_str() == Cancel::ID {
                    cancel_prompt(&mut worker, &mut active, invocation);
                    continue;
                }
                let operation = invocation.operation.clone();
                let input = invocation.input.clone();
                let result = if is_sdk_operation(&operation) {
                    service.invoke(&operation, input)
                } else {
                    worker.invoke_with_client_callables(&operation, input, |callable, schema| {
                        service.admit_current_client_callable(callable, schema)
                    })
                };
                invocation.respond(result);
            }
            completion = completions.recv(), if !active.is_empty() => {
                if let Some(completion) = completion {
                    finish_prompt(&mut worker, &mut active, completion);
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

fn start_prompt(
    worker: &mut ApplicationWorker,
    _service: &SdkApplicationService,
    completion_sender: &mpsc::Sender<ExecutionCompletion>,
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
    let model_input = match model_input_from_content(&request.content) {
        Ok(input) => input,
        Err(error) => {
            invocation.respond(Err(error));
            return;
        }
    };
    let prompt = match worker.prompt(request.clone()) {
        Ok(prompt) => prompt,
        Err(error) => {
            invocation.respond(Err(error));
            return;
        }
    };
    if let Err(error) = worker.append_execution_change(
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
            cancellation: Arc::clone(&cancellation),
            prompt: invocation,
        },
    );

    let harness = Arc::clone(&worker.harness);
    let authority = worker.authority.clone();
    let session_id = request.session_id;
    let execution_id = prompt.execution_id;
    let sender = completion_sender.clone();
    tokio::spawn(async move {
        let blocking_cancellation = Arc::clone(&cancellation);
        let result = tokio::task::spawn_blocking(move || {
            run_agent_execution(harness, authority, model_input, blocking_cancellation)
        })
        .await
        .map_err(|error| ApplicationError::Failed {
            message: format!("application execution worker failed: {error}"),
        })
        .and_then(|result| result);
        let _ = sender
            .send(ExecutionCompletion {
                session_id,
                execution_id,
                result,
            })
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
    let acknowledgement = worker.cancel(request.clone());
    if acknowledgement.is_ok() {
        if let Some(execution) = active.remove(request.session_id.as_str()) {
            execution.cancellation.store(true, Ordering::Release);
            let _ = worker.append_execution_change(
                &request.session_id,
                &execution.execution_id,
                ExecutionChange::State {
                    state: ExecutionState::Cancelled,
                },
            );
            execution.prompt.respond(Ok(PromptResult {
                execution_id: execution.execution_id,
                stop_reason: StopReason::Cancelled,
            }
            .to_value()));
        }
    }
    invocation.respond(acknowledgement.map(|value| value.to_value()));
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
    if execution.cancellation.load(Ordering::Acquire) {
        execution.prompt.respond(Ok(PromptResult {
            execution_id: completion.execution_id,
            stop_reason: StopReason::Cancelled,
        }
        .to_value()));
        return;
    }

    let result = match completion.result {
        Ok(text) => complete_prompt_output(
            worker,
            &completion.session_id,
            &completion.execution_id,
            text,
        ),
        Err(error) => {
            let _ = worker.append_execution_change(
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

fn complete_prompt_output(
    worker: &mut ApplicationWorker,
    session_id: &SessionId,
    execution_id: &str,
    text: String,
) -> Result<PromptResult, ApplicationError> {
    let session = worker.require_open_application_session(session_id)?;
    worker.append_session_change(
        &session,
        SessionChange::TextDelta {
            execution_id: execution_id.to_owned(),
            text: text.clone(),
        },
    )?;
    worker.append_session_change(
        &session,
        SessionChange::Message {
            message: Message {
                role: MessageRole::Assistant,
                content: vec![Content::Text { text }],
            },
        },
    )?;
    worker.append_execution_change(
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

fn model_input_from_content(content: &[Content]) -> Result<Bytes, ApplicationError> {
    let mut text = String::new();
    for part in content {
        let Content::Text { text: part } = part else {
            return Err(ApplicationError::InvalidInput {
                message: "the current model-turn ABI accepts text prompt content only; resource and image turns require the typed model-turn contract".to_owned(),
            });
        };
        text.push_str(part);
    }
    if text.trim().is_empty() {
        return Err(ApplicationError::InvalidInput {
            message: "prompt text must not be empty".to_owned(),
        });
    }
    Ok(Bytes::new(text.into_bytes()))
}

fn run_agent_execution(
    harness: Arc<PhenixHarness>,
    authority: Authority,
    input: Bytes,
    cancellation: Arc<AtomicBool>,
) -> Result<String, ApplicationError> {
    if cancellation.load(Ordering::Acquire) {
        return Err(ApplicationError::Cancelled);
    }
    let profile =
        env::var("PHENIX_ROUTING_PROFILE").unwrap_or_else(|_| DEFAULT_ROUTING_PROFILE.to_owned());
    let profile_id =
        RoutingProfileId::parse(profile).map_err(|error| ApplicationError::InvalidInput {
            message: format!("invalid PHENIX_ROUTING_PROFILE: {error}"),
        })?;
    let callable_id =
        CallableId::parse(DEFAULT_APPLICATION_AGENT).map_err(|error| ApplicationError::Failed {
            message: format!("invalid application agent id: {error}"),
        })?;
    let command = AgentLoopCommand::Run {
        profile_id,
        callable_id: Some(callable_id),
        input,
        tools: Vec::new(),
    };
    let input = serde_json::to_vec(&PhenixValue::from(&command)).map_err(|error| {
        ApplicationError::InvalidInput {
            message: error.to_string(),
        }
    })?;
    let output = harness
        .invoke(&agent_loop_service(), &input, &authority, None)
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
    let AgentLoopResponse::Completed {
        output, tool_calls, ..
    } = response;
    if !tool_calls.is_empty() {
        return Err(ApplicationError::Failed {
            message:
                "agent loop returned tool calls before typed tool-result continuation is available"
                    .to_owned(),
        });
    }
    String::from_utf8(output.as_ref().to_vec()).map_err(|error| ApplicationError::InvalidResponse {
        message: format!("agent output is not UTF-8: {error}"),
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
            | InvokeCapability::ID
    )
}

fn configured_capabilities() -> Vec<ContractId> {
    [
        "discovery",
        "sessions",
        "session-list",
        "session-resume",
        "session-rename",
        "prompt",
        "sdk",
        "capabilities",
        "callables",
        "client-tools",
        "interaction",
        "review",
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
        types::Content, Cancel, CloseSession, CreateSession, ListSessions, Prompt, RenameSession,
        ResumeSession,
    };
    use phenix_core::{Bytes, LocalPersistence, SessionId, ValueAddress};
    use std::{
        fs,
        path::PathBuf,
        time::{SystemTime, UNIX_EPOCH},
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

    fn application_worker() -> ApplicationWorker {
        let mut harness = PhenixHarness::default_suite().unwrap();
        harness.activate().unwrap();
        ApplicationWorker::new(harness).unwrap()
    }

    fn persistent_application_worker(path: &PathBuf) -> ApplicationWorker {
        let persistence = LocalPersistence::open(path).unwrap();
        let mut harness = PhenixHarness::default_suite_with_persistence(persistence).unwrap();
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
            phenix_core::CapabilityOwnerId::Client(
                phenix_core::ClientConnectionId::parse("client-1").unwrap(),
            ),
            phenix_core::CapabilityGenerationId::parse("generation-1").unwrap(),
            phenix_core::ReferenceId::parse(reference).unwrap(),
        )
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
}
