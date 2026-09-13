from pathlib import Path


def replace_once(path: Path, old: str, new: str) -> None:
    source = path.read_text()
    if old not in source:
        raise SystemExit(f"expected source fragment missing in {path}: {old[:100]!r}")
    path.write_text(source.replace(old, new, 1))

# Runtime-neutral durable session journal contract.
sdk = Path("rust/crates/phenix-sdk/src/contracts/sessions.rs")
source = sdk.read_text()
source = source.replace(
    "    Bytes, CallableId, ComponentInterface, InterfaceId, PhenixValue, PreparedMutationHandle,\n",
    "    Bytes, CallableId, ComponentInterface, ContractId, InterfaceId, PhenixValue, PreparedMutationHandle,\n",
    1,
)
marker = '''#[derive(\n    Clone,\n    Copy,\n    Debug,\n    Default,'''
journal_types = '''#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
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

'''
if marker not in source:
    raise SystemExit("session lifecycle insertion marker missing")
source = source.replace(marker, journal_types + marker, 1)
source = source.replace(
'''    Close {
        id: SessionId,
    },
    Continue {''',
'''    Close {
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
    Continue {''',
    1,
)
source = source.replace(
'''    Updated {
        session: SessionRecord,
    },
    Continued {''',
'''    Updated {
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
    Continued {''',
    1,
)
sdk.write_text(source)

# Re-export the journal through the session plugin/catalog boundary already used by Harness.
for path in [
    Path("rust/crates/phenix-plugin-sessions/src/lib.rs"),
    Path("rust/crates/phenix-plugin-catalog/src/lib.rs"),
]:
    source = path.read_text()
    source = source.replace(
        "SessionInput, SessionInputKind, SessionInterface, SessionLifecycle, SessionRecord,\n    SessionResponse",
        "SessionInput, SessionInputKind, SessionInterface, SessionJournalDraft, SessionJournalEntry,\n    SessionLifecycle, SessionRecord, SessionResponse, SessionTransition",
        1,
    )
    path.write_text(source)

# Session owner: per-session/per-stream CAS journal, including atomic metadata+journal transitions.
implementation = Path("rust/crates/phenix-plugin-sessions/src/implementation.rs")
source = implementation.read_text()
source = source.replace(
'''use crate::{
    session_service, SessionCommand, SessionInput, SessionInputKind, SessionInterface,
    SessionRecord, SessionResponse,
};''',
'''use crate::{
    session_service, SessionCommand, SessionInput, SessionInputKind, SessionInterface,
    SessionJournalDraft, SessionJournalEntry, SessionRecord, SessionResponse, SessionTransition,
};''',
    1,
)
source = source.replace(
'''    Authority, Bytes, CapabilityId, ComponentExport, ComponentId, ComponentInterface,
    ComponentManifest, DurableSchema, PluginContext, PluginExecution, PluginHost, PluginId,
''',
'''    Authority, Bytes, CapabilityId, ComponentExport, ComponentId, ComponentInterface, ContractId,
    ComponentManifest, DurableSchema, PluginContext, PluginExecution, PluginHost, PluginId,
''',
    1,
)
source = source.replace(
'''        SessionCommand::Rename { id, title } => rename_session(context, &id, title),
        SessionCommand::Close { id } => close_session(context, &id),
        SessionCommand::Continue { id, kind, content } => {''',
'''        SessionCommand::Rename { id, title } => rename_session(context, &id, title),
        SessionCommand::Close { id } => close_session(context, &id),
        SessionCommand::Transition {
            id,
            transition,
            journal,
        } => transition_session(context, &id, transition, journal),
        SessionCommand::AppendJournal { id, entry } => append_journal(context, &id, entry),
        SessionCommand::Journal {
            id,
            stream,
            after_sequence,
        } => read_journal_response(context, &id, &stream, after_sequence),
        SessionCommand::Continue { id, kind, content } => {''',
    1,
)
marker = '''fn require_open(session: &SessionRecord) -> Result<(), String> {'''
journal_impl = '''fn transition_session(
    context: &SessionContext<'_, '_>,
    id: &SessionId,
    transition: SessionTransition,
    journal: SessionJournalDraft,
) -> Result<SessionResponse, String> {
    let key = session_key(id);
    let old = read_raw(context, &key)?.ok_or_else(|| format!("unknown session: {id}"))?;
    let mut session: SessionRecord =
        serde_json::from_slice(&old).map_err(|error| error.to_string())?;
    require_open(&session)?;
    match transition {
        SessionTransition::Rename { title } => session.title = Some(title),
        SessionTransition::Close => session.lifecycle = SessionLifecycle::Closed,
    }
    let value = serde_json::to_vec(&session).map_err(|error| error.to_string())?;
    let (journal, mut journal_operations) = prepare_journal_append(context, id, journal)?;
    let mut operations = vec![
        TransactionOp::AssertValue {
            key: key.clone(),
            expected: Some(old),
        },
        TransactionOp::Put { key, value },
    ];
    operations.append(&mut journal_operations);
    context
        .kernel
        .transact_durable(&session_namespace(), &operations)
        .map_err(|error| error.to_string())?;
    Ok(SessionResponse::Transitioned { session, journal })
}

fn append_journal(
    context: &SessionContext<'_, '_>,
    id: &SessionId,
    draft: SessionJournalDraft,
) -> Result<SessionResponse, String> {
    if read_session(context, id)?.is_none() {
        return Err(format!("unknown session: {id}"));
    }
    let (entry, operations) = prepare_journal_append(context, id, draft)?;
    context
        .kernel
        .transact_durable(&session_namespace(), &operations)
        .map_err(|error| error.to_string())?;
    Ok(SessionResponse::JournalAppended { entry })
}

fn prepare_journal_append(
    context: &SessionContext<'_, '_>,
    id: &SessionId,
    draft: SessionJournalDraft,
) -> Result<(SessionJournalEntry, Vec<TransactionOp>), String> {
    let key = journal_key(id, &draft.stream);
    let old = read_raw(context, &key)?;
    let mut entries = decode_journal(old.as_deref())?;
    if entries.iter().any(|entry| entry.stream != draft.stream) {
        return Err(format!("session journal stream mismatch for {id}: {}", draft.stream));
    }
    let sequence = entries
        .last()
        .map(|entry| {
            entry
                .sequence
                .checked_add(1)
                .ok_or_else(|| "session journal sequence overflow".to_owned())
        })
        .transpose()?
        .unwrap_or(1);
    let entry = SessionJournalEntry {
        sequence,
        stream: draft.stream,
        payload: draft.payload,
    };
    entries.push(entry.clone());
    Ok((
        entry,
        vec![
            TransactionOp::AssertValue {
                key: key.clone(),
                expected: old,
            },
            TransactionOp::Put {
                key,
                value: serde_json::to_vec(&entries).map_err(|error| error.to_string())?,
            },
        ],
    ))
}

fn read_journal_response(
    context: &SessionContext<'_, '_>,
    id: &SessionId,
    stream: &ContractId,
    after_sequence: Option<u64>,
) -> Result<SessionResponse, String> {
    if read_session(context, id)?.is_none() {
        return Err(format!("unknown session: {id}"));
    }
    let entries = decode_journal(read_raw(context, &journal_key(id, stream))?.as_deref())?;
    if entries.iter().any(|entry| &entry.stream != stream) {
        return Err(format!("session journal stream mismatch for {id}: {stream}"));
    }
    let through_sequence = entries.last().map_or(0, |entry| entry.sequence);
    let entries = match after_sequence {
        Some(sequence) => entries
            .into_iter()
            .filter(|entry| entry.sequence > sequence)
            .collect(),
        None => entries,
    };
    Ok(SessionResponse::Journal {
        through_sequence,
        entries,
    })
}

'''
if marker not in source:
    raise SystemExit("session journal implementation marker missing")
source = source.replace(marker, journal_impl + marker, 1)
source = source.replace(
'''fn decode_history(value: Option<&[u8]>) -> Result<Vec<SessionHistoryEntry>, String> {
    value
        .map(|value| serde_json::from_slice(value).map_err(|error| error.to_string()))
        .unwrap_or_else(|| Ok(Vec::new()))
}

fn session_key''',
'''fn decode_history(value: Option<&[u8]>) -> Result<Vec<SessionHistoryEntry>, String> {
    value
        .map(|value| serde_json::from_slice(value).map_err(|error| error.to_string()))
        .unwrap_or_else(|| Ok(Vec::new()))
}

fn decode_journal(value: Option<&[u8]>) -> Result<Vec<SessionJournalEntry>, String> {
    value
        .map(|value| serde_json::from_slice(value).map_err(|error| error.to_string()))
        .unwrap_or_else(|| Ok(Vec::new()))
}

fn session_key''',
    1,
)
source = source.replace(
'''fn history_key(id: &SessionId) -> String {
    format!("history/{id}")
}
''',
'''fn history_key(id: &SessionId) -> String {
    format!("history/{id}")
}

fn journal_key(id: &SessionId, stream: &ContractId) -> String {
    format!("journal/{id}/{}", stream.as_str())
}
''',
    1,
)
# Pin journal durability, stream-local sequences, and atomic metadata+journal transition.
test_import = "    use phenix_core::{Kernel, KernelConfig, LocalPersistence, PhenixValue, Project};"
if test_import not in source:
    raise SystemExit("session tests import marker missing")
source = source.replace(
    test_import,
    "    use phenix_core::{ContractId, Kernel, KernelConfig, LocalPersistence, PhenixValue, Project};",
    1,
)
regression = r'''

    #[test]
    fn journal_streams_and_transition_are_durable_across_restart() {
        let path = temp_db("session-journal");
        let root = SessionId::parse("root").unwrap();
        let application = ContractId::parse("test.session.application@1").unwrap();
        let other = ContractId::parse("test.session.other@1").unwrap();
        {
            let mut kernel = kernel_with(&path);
            invoke(
                &mut kernel,
                &SessionCommand::Create {
                    session: SessionRecord::new(root.clone()),
                },
            )
            .unwrap();
            let response = invoke(
                &mut kernel,
                &SessionCommand::AppendJournal {
                    id: root.clone(),
                    entry: SessionJournalDraft {
                        stream: other.clone(),
                        payload: PhenixValue::String("other-1".into()),
                    },
                },
            )
            .unwrap();
            assert!(matches!(
                response,
                SessionResponse::JournalAppended { ref entry }
                    if entry.sequence == 1 && entry.stream == other
            ));
            let response = invoke(
                &mut kernel,
                &SessionCommand::Transition {
                    id: root.clone(),
                    transition: SessionTransition::Rename {
                        title: "renamed".into(),
                    },
                    journal: SessionJournalDraft {
                        stream: application.clone(),
                        payload: PhenixValue::String("rename".into()),
                    },
                },
            )
            .unwrap();
            assert!(matches!(
                response,
                SessionResponse::Transitioned {
                    ref session,
                    ref journal,
                } if session.title.as_deref() == Some("renamed")
                    && journal.sequence == 1
                    && journal.stream == application
            ));
            invoke(
                &mut kernel,
                &SessionCommand::AppendJournal {
                    id: root.clone(),
                    entry: SessionJournalDraft {
                        stream: application.clone(),
                        payload: PhenixValue::String("second".into()),
                    },
                },
            )
            .unwrap();
        }

        let mut restored = kernel_with(&path);
        assert!(matches!(
            invoke(
                &mut restored,
                &SessionCommand::Get { id: root.clone() },
            )
            .unwrap(),
            SessionResponse::Session { session: Some(ref session) }
                if session.title.as_deref() == Some("renamed")
        ));
        let response = invoke(
            &mut restored,
            &SessionCommand::Journal {
                id: root.clone(),
                stream: application.clone(),
                after_sequence: Some(1),
            },
        )
        .unwrap();
        assert!(matches!(
            response,
            SessionResponse::Journal {
                through_sequence: 2,
                ref entries,
            } if entries.len() == 1 && entries[0].sequence == 2
        ));
        let response = invoke(
            &mut restored,
            &SessionCommand::Journal {
                id: root,
                stream: other,
                after_sequence: None,
            },
        )
        .unwrap();
        assert!(matches!(
            response,
            SessionResponse::Journal {
                through_sequence: 1,
                ref entries,
            } if entries.len() == 1 && entries[0].sequence == 1
        ));
        let _ = fs::remove_file(path);
    }
'''
source = source.rstrip()
if not source.endswith("}"):
    raise SystemExit("session test module terminator missing")
source = source[:-1] + regression + "}\n"
implementation.write_text(source)

# Application worker: descriptor-backed ResumeSession and journal-derived sequence truth.
application = Path("rust/crates/phenix-harness/src/application.rs")
source = application.read_text()
source = source.replace(
'''        SessionInput as ApplicationSessionInput, SessionList, SessionProjection,
        SessionProjectionState, SessionRenameInput, SessionSnapshot, SessionUpdate,
    },
    CloseSession, CreateSession, ListSessions, Operation, RenameSession,
};''',
'''        SessionInput as ApplicationSessionInput, SessionList, SessionProjection,
        SessionProjectionState, SessionRenameInput, SessionResumeInput, SessionSnapshot,
        SessionUpdate,
    },
    CloseSession, CreateSession, ListSessions, Operation, RenameSession, ResumeSession,
};''',
    1,
)
source = source.replace(
'''    Authority, ContractId, HasPhenixSchema, ObservableError, ObservableRegistration,
    ObservableStore, PhenixValue, PluginId, Project, SessionId, SnapshotPolicy, ValueCodec,
''',
'''    Authority, ContractId, HasPhenixSchema, ObservableError, ObservableRegistration,
    ObservableStore, PhenixContract, PhenixValue, PluginId, Project, SessionId, SnapshotPolicy, ValueCodec,
''',
    1,
)
source = source.replace(
'''    session_service, SessionCommand, SessionLifecycle, SessionRecord, SessionResponse, SDK_PLUGIN,
};''',
'''    session_service, SessionCommand, SessionJournalDraft, SessionJournalEntry, SessionLifecycle,
    SessionRecord, SessionResponse, SessionTransition, SDK_PLUGIN,
};''',
    1,
)
source = source.replace(
'''            ListSessions::ID => self
                .list_sessions(decode(input)?)
                .map(|value| value.to_value()),
            RenameSession::ID => self''',
'''            ListSessions::ID => self
                .list_sessions(decode(input)?)
                .map(|value| value.to_value()),
            ResumeSession::ID => self
                .resume_session(decode(input)?)
                .map(|value| value.to_value()),
            RenameSession::ID => self''',
    1,
)
old_rename = '''    fn rename_session(
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
'''
new_rename = '''    fn rename_session(
        &mut self,
        request: SessionRenameInput,
    ) -> Result<SessionInfo, ApplicationError> {
        self.require_open_application_session(&request.session_id)?;
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
'''
if old_rename not in source:
    raise SystemExit("application rename implementation marker missing")
source = source.replace(old_rename, new_rename, 1)
old_close = '''    fn close_session(
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
'''
new_close = '''    fn close_session(
        &mut self,
        request: ApplicationSessionInput,
    ) -> Result<Acknowledged, ApplicationError> {
        self.require_open_application_session(&request.session_id)?;
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
'''
if old_close not in source:
    raise SystemExit("application close implementation marker missing")
source = source.replace(old_close, new_close, 1)
old_project = '''    fn project_session_change(
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
'''
new_project = '''    fn resume_session(
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
        let session = self
            .session_record(&request.session_id)?
            .ok_or_else(|| ApplicationError::NotFound {
                resource: request.session_id.to_string(),
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
            expected = expected.checked_add(1).ok_or_else(|| ApplicationError::Failed {
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
    ) -> Result<(), ApplicationError> {
        let update = session_update_from_journal(&session.session_id, journal)?;
        let contiguous = self
            .projection
            .state()
            .sessions
            .get(session.session_id.as_str())
            .and_then(|projection| projection.through_sequence.checked_add(1))
            == Some(update.sequence);
        if contiguous {
            return self
                .projection
                .apply_update(update)
                .map_err(application_projection_error);
        }
        let snapshot = self.load_session_snapshot(SessionResumeInput {
            session_id: session.session_id,
            after_sequence: None,
        })?;
        self.projection
            .repair_with_snapshot(snapshot, [update])
            .map_err(application_projection_error)
    }
'''
if old_project not in source:
    raise SystemExit("application projection writer marker missing")
source = source.replace(old_project, new_project, 1)
helper_marker = '''fn decode<T: ValueCodec>(value: PhenixValue) -> Result<T, ApplicationError> {'''
helpers = '''fn session_change_stream() -> ContractId {
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

'''
if helper_marker not in source:
    raise SystemExit("application helper insertion marker missing")
source = source.replace(helper_marker, helpers + helper_marker, 1)
# Test imports and persistent worker helpers.
source = source.replace(
'''    use phenix_application_interface::{CloseSession, CreateSession, ListSessions, RenameSession};
    use phenix_core::{SessionId, ValueAddress};
''',
'''    use phenix_application_interface::{
        CloseSession, CreateSession, ListSessions, RenameSession, ResumeSession,
    };
    use phenix_core::{LocalPersistence, SessionId, ValueAddress};
    use std::{
        fs,
        path::PathBuf,
        time::{SystemTime, UNIX_EPOCH},
    };
''',
    1,
)
worker_helper = '''    fn application_worker() -> ApplicationWorker {
        let mut harness = PhenixHarness::default_suite().unwrap();
        harness.activate().unwrap();
        ApplicationWorker::new(harness).unwrap()
    }
'''
persistent_helpers = worker_helper + r'''

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
'''
if worker_helper not in source:
    raise SystemExit("application test worker helper missing")
source = source.replace(worker_helper, persistent_helpers, 1)
crud_marker = '''    #[test]
    fn worker_hides_non_application_sessions_from_session_list() {'''
resume_test = r'''    #[test]
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

'''
if crud_marker not in source:
    raise SystemExit("application resume test insertion marker missing")
source = source.replace(crud_marker, resume_test + crud_marker, 1)
application.write_text(source)

# Self-clean and restore ordinary maintenance behavior.
workflow = Path(".github/workflows/sync-maintenance.yml")
source = workflow.read_text()
source = source.replace("          python3 .phenix/work/pr-504-session-journal.py\n", "")
source = source.replace(
    '          git commit -m "feat(application): persist session update journal"\n',
    '          git commit -m "chore: apply CI autofixes"\n',
)
workflow.write_text(source)
Path(__file__).unlink()
