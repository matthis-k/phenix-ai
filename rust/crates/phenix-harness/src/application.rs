use phenix_application_interface::types::{
    SessionChange, SessionInfo, SessionProjection, SessionProjectionState, SessionSnapshot,
    SessionUpdate,
};
use phenix_core::SessionId;
use std::collections::BTreeMap;

pub const APPLICATION_INVOCATION_CAPACITY: usize = 64;
pub const CLIENT_CAPABILITY_CAPACITY: usize = 64;
pub const APPLICATION_EVENT_CAPACITY: usize = 256;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SessionProjectionError {
    Missing {
        session_id: SessionId,
    },
    SequenceGap {
        session_id: SessionId,
        expected: u64,
        actual: u64,
    },
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

#[cfg(test)]
mod tests {
    use super::*;
    use phenix_application_interface::types::SessionChange;

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
}
