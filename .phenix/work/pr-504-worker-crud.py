from pathlib import Path

application = Path("rust/crates/phenix-harness/src/application.rs")
source = application.read_text()
source = source.replace(
'''use phenix_application_interface::types::{
    SessionChange, SessionInfo, SessionProjection, SessionProjectionState, SessionSnapshot,
    SessionUpdate,
};
use phenix_core::{
    HasPhenixSchema, ObservableError, ObservableRegistration, ObservableStore, PluginId,
    SnapshotPolicy, ValueAddress, ValueCodec, ValueId, ValuePath,
};
use phenix_plugin_catalog::SDK_PLUGIN;
use std::collections::BTreeMap;
''',
'''use crate::{default_suite_authority, PhenixHarness};
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
''',
1,
)

marker = "#[cfg(test)]\nmod tests {"
if marker not in source:
    raise SystemExit("application test module marker missing")

worker = r'''
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
            CreateSession::ID => self.create_session(decode(input)?).map(|value| value.to_value()),
            ListSessions::ID => self.list_sessions(decode(input)?).map(|value| value.to_value()),
            RenameSession::ID => self.rename_session(decode(input)?).map(|value| value.to_value()),
            CloseSession::ID => self.close_session(decode(input)?).map(|value| value.to_value()),
            _ => Err(ApplicationError::UnsupportedCapability {
                capability: operation.clone(),
            }),
        }
    }

    fn create_session(&mut self, request: SessionCreateInput) -> Result<SessionInfo, ApplicationError> {
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
            self.next_session_ordinal = ordinal.checked_add(1).ok_or_else(|| ApplicationError::Failed {
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
        let session = self.session_record(id)?.ok_or_else(|| ApplicationError::NotFound {
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

    fn session_record(&mut self, id: &SessionId) -> Result<Option<SessionRecord>, ApplicationError> {
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
                message: format!("session projection sequence overflow for {}", session.session_id),
            })?;
        self.projection
            .apply_update(SessionUpdate {
                session_id: session.session_id,
                sequence,
                update,
            })
            .map_err(application_projection_error)
    }

    fn invoke_session(&mut self, command: SessionCommand) -> Result<SessionResponse, ApplicationError> {
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
        let output: PhenixValue = serde_json::from_slice(&output).map_err(|error| {
            ApplicationError::InvalidResponse {
                message: error.to_string(),
            }
        })?;
        SessionResponse::try_from(Project(&output)).map_err(|error| ApplicationError::InvalidResponse {
            message: error.to_string(),
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

'''
source = source.replace(marker, worker + marker, 1)
source = source.replace(
"    use phenix_core::SessionId;",
"    use phenix_core::{SessionId, ValueAddress};\n    use phenix_application_interface::{CloseSession, CreateSession, ListSessions, RenameSession};",
1,
)

unit_marker = "    fn rename(id: &str, sequence: u64, title: &str) -> SessionUpdate {"
if unit_marker not in source:
    raise SystemExit("application test helper marker missing")
helper = r'''
    fn invoke_operation<O: Operation>(
        worker: &mut ApplicationWorker,
        input: O::Input,
    ) -> Result<O::Output, ApplicationError> {
        let value = worker.invoke(
            &ContractId::parse(O::ID).unwrap(),
            input.to_value(),
        )?;
        O::Output::from_value(&value).map_err(|error| ApplicationError::InvalidResponse {
            message: error.to_string(),
        })
    }

    fn application_worker() -> ApplicationWorker {
        let mut harness = PhenixHarness::default_suite().unwrap();
        harness.activate().unwrap();
        ApplicationWorker::new(harness).unwrap()
    }

'''
source = source.replace(unit_marker, helper + unit_marker, 1)

test_marker = "    #[test]\n    fn create_starts_projection_at_sequence_zero() {"
if test_marker not in source:
    raise SystemExit("application first test marker missing")
crud_test = r'''
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

        let listed = invoke_operation::<ListSessions>(
            &mut worker,
            PageInput { cursor: None },
        )
        .unwrap();
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
        let listed = invoke_operation::<ListSessions>(
            &mut worker,
            PageInput { cursor: None },
        )
        .unwrap();
        assert!(listed.sessions.is_empty());
    }

'''
source = source.replace(test_marker, crud_test + test_marker, 1)
application.write_text(source)

lib = Path("rust/crates/phenix-harness/src/lib.rs")
source = lib.read_text()
if "pub mod application;" not in source:
    source = source.replace("mod basic_suite;", "pub mod application;\nmod basic_suite;", 1)
lib.write_text(source)

# Restore the normal maintenance workflow and remove this one-shot patcher in the same bot commit.
workflow = Path(".github/workflows/sync-maintenance.yml")
source = workflow.read_text()
source = source.replace("          python3 .phenix/work/pr-504-worker-crud.py\n", "")
source = source.replace(
    '          git commit -m "feat(application): add session worker CRUD"\n',
    '          git commit -m "chore: apply CI autofixes"\n',
)
workflow.write_text(source)
Path(__file__).unlink()
