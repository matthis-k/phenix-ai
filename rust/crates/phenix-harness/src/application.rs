use phenix_application_interface::types::{
    SessionChange, SessionInfo, SessionProjection, SessionProjectionState, SessionSnapshot,
    SessionUpdate,
};
use phenix_core::{
    ObservableError, ObservableRegistration, ObservableStore, PhenixContract, PluginId,
    SnapshotPolicy, ValueAddress, ValueCodec, ValueId, ValuePath,
};
use phenix_plugin_catalog::SDK_PLUGIN;
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
    #[error(
        "session projection sequence gap for {session_id}: expected {expected}, got {actual}"
    )]
    SequenceGap {
        session_id: phenix_core::SessionId,
        expected: u64,
        actual: u64,
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
        let projection = self.state.sessions.get_mut(&key).ok_or_else(|| {
            SessionProjectionError::Missing {
                session_id: update.session_id.clone(),
            }
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

#[cfg(test)]
mod tests {
    use super::*;
    use phenix_core::SessionId;

    fn session(id: &str, title: Option<&str>) -> SessionInfo {
        SessionInfo {
            session_id: SessionId::parse(id).unwrap(),
            title: title.map(str::to_owned),
            working_directory: "/workspace".into(),
        }
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
            .apply_update(SessionUpdate {
                session_id: SessionId::parse("session-1").unwrap(),
                sequence: 1,
                update: SessionChange::Renamed {
                    title: "new title".into(),
                },
            })
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
            .apply_update(SessionUpdate {
                session_id: SessionId::parse("session-1").unwrap(),
                sequence: 1,
                update: SessionChange::Renamed {
                    title: "observable".into(),
                },
            })
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
}
