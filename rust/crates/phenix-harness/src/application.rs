use crate::{default_suite_authority, PhenixHarness};
use phenix_application_interface::{
    types::{
        Acknowledged, ApplicationError, PageInput, SessionChange, SessionCreateInput, SessionInfo,
        SessionInput as ApplicationSessionInput, SessionList, SessionProjection,
        SessionProjectionState, SessionRenameInput, SessionSnapshot, SessionUpdate,
    },
    CloseSession, CreateSession, ListSessions, Operation, RenameSession,
};
use phenix_core::{
    Authority, ContractId, HasPhenixSchema, ObservableError, ObservableRegistration,
    ObservableStore, PhenixValue, PluginId, Project, SessionId, SnapshotPolicy, ValueCodec,
    ValueId, ValuePath,
};
use phenix_plugin_catalog::{
    session_service, SessionCommand, SessionLifecycle, SessionRecord, SessionResponse, SDK_PLUGIN,
};
use std::collections::BTreeMap;

pub const APPLICATION_INVOCATION_CAPACITY: usize = 64;
pub const CLIENT_CAPABILITY_CAPACITY: usize = 64;
pub const APPLICATION_EVENT_CAPACITY: usize = 256;
pub const SESSION_PROJECTION_VALUE: &str = "phenix.application.sessions@1";

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
    ///
    /// A remaining gap is returned after the snapshot (and any preceding
    /// contiguous suffix) has committed. The caller must fetch another full
    /// snapshot rather than guessing the missing transition.
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
    harness: PhenixHarness,
    authority: Authority,
    projection: SessionProjectionStore,
    next_session_ordinal: u64,
}

impl ApplicationWorker {
    pub fn new(harness: PhenixHarness) -> Result<Self, ObservableError> {
        Ok(Self {
            harness,
            authority: default_suite_authority(),
            projection: SessionProjectionStore::new()?,
            next_session_ordinal: 1,
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
            RenameSession::ID => self
                .rename_session(decode(input)?)
                .map(|value| value.to_value()),
            CloseSession::ID => self
                .close_session(decode(input)?)
                .map(|value| value.to_value()),
            _ => Err(ApplicationError::UnsupportedCapability {
                capability: operation.clone(),
            }),
        }
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
        let response = self.invoke_session(SessionCommand::Rename {
            id: request.session_id.clone(),
            title: request.title.clone(),
        })?;
        let SessionResponse::Updated { session } = response else {
            return Err(unexpected_session_response("rename", response));
        };
        let info = application_session_info(&session)?;
        self.project_session_change(
            info.clone(),
            SessionChange::Renamed {
                title: request.title,
            },
        )?;
        Ok(info)
    }

    fn close_session(
        &mut self,
        request: ApplicationSessionInput,
    ) -> Result<Acknowledged, ApplicationError> {
        self.require_open_application_session(&request.session_id)?;
        let response = self.invoke_session(SessionCommand::Close {
            id: request.session_id.clone(),
        })?;
        let SessionResponse::Updated { session } = response else {
            return Err(unexpected_session_response("close", response));
        };
        let info = application_session_info(&session)?;
        self.project_session_change(info, SessionChange::Closed)?;
        Ok(Acknowledged {})
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

    fn project_session_change(
        &mut self,
        session: SessionInfo,
        update: SessionChange,
    ) -> Result<(), ApplicationError> {
        let key = session.session_id.as_str();
        if !self.projection.state().sessions.contains_key(key) {
            self.projection
                .insert_created(session.clone())
                .map_err(application_projection_error)?;
        }
        let sequence = self.projection.state().sessions[key]
            .through_sequence
            .checked_add(1)
            .ok_or_else(|| ApplicationError::Failed {
                message: format!(
                    "session projection sequence overflow for {}",
                    session.session_id
                ),
            })?;
        self.projection
            .apply_update(SessionUpdate {
                session_id: session.session_id,
                sequence,
                update,
            })
            .map_err(application_projection_error)
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

#[cfg(test)]
mod tests {
    use super::*;
    use phenix_application_interface::{CloseSession, CreateSession, ListSessions, RenameSession};
    use phenix_core::{SessionId, ValueAddress};

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

    fn rename(id: &str, sequence: u64, title: &str) -> SessionUpdate {
        SessionUpdate {
            session_id: SessionId::parse(id).unwrap(),
            sequence,
            update: SessionChange::Renamed {
                title: title.to_owned(),
            },
        }
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
