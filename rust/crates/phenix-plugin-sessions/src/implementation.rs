use crate::{
    session_service, SessionCommand, SessionInput, SessionInputKind, SessionInterface,
    SessionJournalDraft, SessionJournalEntry, SessionRecord, SessionResponse, SessionTransition,
};
use phenix_core::{
    Authority, Bytes, CapabilityId, ComponentExport, ComponentId, ComponentInterface,
    ComponentManifest, ContractId, DurableSchema, PluginContext, PluginExecution, PluginHost,
    PluginId, PluginInstance, PluginManifest, ResourceNamespace, ServiceContribution, ServiceId,
    SessionId, TransactionOp,
};
use phenix_sdk::{
    session_mutation_service, SessionHistoryDraft, SessionHistoryEntry, SessionLifecycle,
    SessionMutationCommand, SessionMutationInterface, SessionMutationResponse,
};

const SESSION_PLUGIN: &str = "phenix.sessions";
const SESSION_COMPONENT: &str = "phenix.sessions";
const SESSION_NAMESPACE: &str = "phenix.sessions.state";
const PERSISTENCE_SCHEMA: &str = "kernel.persistence.schema";
const PERSISTENCE_READ: &str = "kernel.persistence.read";
const PERSISTENCE_WRITE: &str = "kernel.persistence.write";
const ALL_SESSIONS_KEY: &str = "sessions/@all";

type SessionContext<'host, 'runtime> = PluginContext<'host, 'runtime, ()>;

fn context<'host, 'runtime>(host: &'host PluginHost<'runtime>) -> SessionContext<'host, 'runtime> {
    PluginContext::new(host, (), (), ())
}

#[must_use]
pub fn session_manifest() -> PluginManifest {
    PluginManifest {
        id: PluginId::parse(SESSION_PLUGIN).expect("static plugin id is valid"),
        version: 1,
        execution: PluginExecution::Embedded,
        dependencies: Vec::new(),
        services: vec![
            ServiceContribution {
                role: phenix_core::ServiceRole::Terminal,
                service: session_service(),
                priority: 100,
                required_authority: Authority::default(),
            },
            ServiceContribution {
                role: phenix_core::ServiceRole::Terminal,
                service: session_mutation_service(),
                priority: 100,
                required_authority: Authority::default(),
            },
        ],
        resource_namespaces: vec![session_namespace()],
        maximum_authority: Authority::new([
            capability(PERSISTENCE_SCHEMA),
            capability(PERSISTENCE_READ),
            capability(PERSISTENCE_WRITE),
        ]),
    }
}

#[must_use]
pub fn session_component_manifest() -> ComponentManifest {
    ComponentManifest {
        listeners: Vec::new(),
        id: ComponentId::parse(SESSION_COMPONENT).expect("static component id is valid"),
        owner: PluginId::parse(SESSION_PLUGIN).expect("static plugin id is valid"),
        imports: Vec::new(),
        exports: vec![
            ComponentExport {
                interface: SessionInterface::interface_id(),
                schema: SessionInterface::schema(),
                priority: 100,
                required_authority: Authority::default(),
            },
            ComponentExport {
                interface: SessionMutationInterface::interface_id(),
                schema: SessionMutationInterface::schema(),
                priority: 100,
                required_authority: Authority::default(),
            },
        ],
        maximum_authority: session_manifest().maximum_authority,
    }
}

#[must_use]
pub fn session_factory() -> Box<dyn PluginInstance> {
    Box::new(SessionPlugin)
}

fn session_namespace() -> ResourceNamespace {
    ResourceNamespace::parse(SESSION_NAMESPACE).expect("static namespace is valid")
}

fn capability(value: &str) -> CapabilityId {
    CapabilityId::parse(value).expect("static capability is valid")
}

struct SessionPlugin;

impl PluginInstance for SessionPlugin {
    fn start(&mut self, host: &PluginHost<'_>) -> Result<(), String> {
        context(host)
            .kernel
            .register_durable_schema(&DurableSchema::new(session_namespace(), 1))
            .map_err(|error| error.to_string())
    }

    fn invoke(
        &mut self,
        service: &ServiceId,
        input: &[u8],
        host: &PluginHost<'_>,
    ) -> Result<Vec<u8>, String> {
        let context = context(host);
        if service == &session_service() {
            let interface = SessionInterface::interface_id();
            let command = context
                .kernel
                .decode_projected::<SessionCommand>(&interface, input)
                .map_err(|error| error.to_string())?;
            let response = handle_session(&context, command)?;
            return context
                .kernel
                .encode_value(&response)
                .map_err(|error| error.to_string());
        }
        if service == &session_mutation_service() {
            let interface = SessionMutationInterface::interface_id();
            let command = context
                .kernel
                .decode_projected::<SessionMutationCommand>(&interface, input)
                .map_err(|error| error.to_string())?;
            let response = handle_mutation(&context, command)?;
            return context
                .kernel
                .encode_value(&response)
                .map_err(|error| error.to_string());
        }
        Err(format!("unsupported session service: {service}"))
    }
}

fn handle_session(
    context: &SessionContext<'_, '_>,
    command: SessionCommand,
) -> Result<SessionResponse, String> {
    match command {
        SessionCommand::Create { session } => create_session(context, session),
        SessionCommand::Get { id } => Ok(SessionResponse::Session {
            session: read_session(context, &id)?,
        }),
        SessionCommand::List => Ok(SessionResponse::Sessions {
            sessions: read_sessions(context)?,
        }),
        SessionCommand::Rename { id, title } => rename_session(context, &id, title),
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
        SessionCommand::Continue { id, kind, content } => {
            continue_session(context, &id, kind, content)
        }
        SessionCommand::Inputs { id } => {
            if read_session(context, &id)?.is_none() {
                return Err(format!("unknown session: {id}"));
            }
            Ok(SessionResponse::Inputs {
                inputs: read_inputs(context, &id)?,
            })
        }
        SessionCommand::ResolveInput { resource } => Ok(SessionResponse::Input {
            input: resolve_input_resource(context, &resource)?,
        }),
        SessionCommand::AppendHistory { id, entry } => append_history(context, &id, entry),
        SessionCommand::History { id } => {
            if read_session(context, &id)?.is_none() {
                return Err(format!("unknown session: {id}"));
            }
            Ok(SessionResponse::History {
                entries: read_history(context, &id)?,
            })
        }
        SessionCommand::ResolveHistory { resource } => Ok(SessionResponse::HistoryEntry {
            entry: resolve_history_resource(context, &resource)?,
        }),
    }
}

fn handle_mutation(
    context: &SessionContext<'_, '_>,
    command: SessionMutationCommand,
) -> Result<SessionMutationResponse, String> {
    let SessionMutationCommand::PrepareCreate { session } = command;
    let (session, operations) = prepare_create(context, session)?;
    let mutation = context
        .kernel
        .prepare_durable_transaction(&session_namespace(), &operations)
        .map_err(|error| error.to_string())?;
    Ok(SessionMutationResponse::PreparedCreate { session, mutation })
}

fn prepare_create(
    context: &SessionContext<'_, '_>,
    session: SessionRecord,
) -> Result<(SessionRecord, Vec<TransactionOp>), String> {
    if !session.is_open() {
        return Err("new sessions must start open".into());
    }
    if read_session(context, &session.id)?.is_some() {
        return Err(format!("session already exists: {}", session.id));
    }

    let session_key = session_key(&session.id);
    let old_sessions = read_raw(context, ALL_SESSIONS_KEY)?;
    let mut sessions = decode_ids(old_sessions.as_deref())?;
    sessions.push(session.id.clone());
    sessions.sort();
    sessions.dedup();
    let operations = vec![
        TransactionOp::AssertValue {
            key: session_key.clone(),
            expected: None,
        },
        TransactionOp::AssertValue {
            key: ALL_SESSIONS_KEY.into(),
            expected: old_sessions,
        },
        TransactionOp::Put {
            key: session_key,
            value: serde_json::to_vec(&session).map_err(|error| error.to_string())?,
        },
        TransactionOp::Put {
            key: ALL_SESSIONS_KEY.into(),
            value: serde_json::to_vec(&sessions).map_err(|error| error.to_string())?,
        },
    ];
    Ok((session, operations))
}

fn create_session(
    context: &SessionContext<'_, '_>,
    session: SessionRecord,
) -> Result<SessionResponse, String> {
    let (session, operations) = prepare_create(context, session)?;
    context
        .kernel
        .transact_durable(&session_namespace(), &operations)
        .map_err(|error| error.to_string())?;
    Ok(SessionResponse::Created { session })
}

fn rename_session(
    context: &SessionContext<'_, '_>,
    id: &SessionId,
    title: String,
) -> Result<SessionResponse, String> {
    update_session(context, id, move |session| {
        require_open(session)?;
        session.title = Some(title);
        Ok(())
    })
}

fn close_session(
    context: &SessionContext<'_, '_>,
    id: &SessionId,
) -> Result<SessionResponse, String> {
    update_session(context, id, |session| {
        require_open(session)?;
        session.lifecycle = SessionLifecycle::Closed;
        Ok(())
    })
}

fn update_session(
    context: &SessionContext<'_, '_>,
    id: &SessionId,
    mutate: impl FnOnce(&mut SessionRecord) -> Result<(), String>,
) -> Result<SessionResponse, String> {
    let key = session_key(id);
    let old = read_raw(context, &key)?.ok_or_else(|| format!("unknown session: {id}"))?;
    let mut session: SessionRecord =
        serde_json::from_slice(&old).map_err(|error| error.to_string())?;
    mutate(&mut session)?;
    let value = serde_json::to_vec(&session).map_err(|error| error.to_string())?;
    context
        .kernel
        .transact_durable(
            &session_namespace(),
            &[
                TransactionOp::AssertValue {
                    key: key.clone(),
                    expected: Some(old),
                },
                TransactionOp::Put { key, value },
            ],
        )
        .map_err(|error| error.to_string())?;
    Ok(SessionResponse::Updated { session })
}

fn transition_session(
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
        return Err(format!(
            "session journal stream mismatch for {id}: {}",
            draft.stream
        ));
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
        return Err(format!(
            "session journal stream mismatch for {id}: {stream}"
        ));
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

fn require_open(session: &SessionRecord) -> Result<(), String> {
    if session.is_open() {
        Ok(())
    } else {
        Err(format!("session is closed: {}", session.id))
    }
}

fn require_open_session(
    context: &SessionContext<'_, '_>,
    id: &SessionId,
) -> Result<SessionRecord, String> {
    let session = read_session(context, id)?.ok_or_else(|| format!("unknown session: {id}"))?;
    require_open(&session)?;
    Ok(session)
}

fn continue_session(
    context: &SessionContext<'_, '_>,
    id: &SessionId,
    kind: SessionInputKind,
    content: Bytes,
) -> Result<SessionResponse, String> {
    let session = require_open_session(context, id)?;
    let key = inputs_key(id);
    let old_inputs = read_raw(context, &key)?;
    let mut inputs = decode_inputs(old_inputs.as_deref())?;
    let input = SessionInput {
        sequence: u64::try_from(inputs.len())
            .map_err(|_| "session input sequence overflow".to_owned())?
            + 1,
        kind,
        content,
    };
    inputs.push(input.clone());
    context
        .kernel
        .transact_durable(
            &session_namespace(),
            &[
                TransactionOp::AssertValue {
                    key: key.clone(),
                    expected: old_inputs,
                },
                TransactionOp::Put {
                    key,
                    value: serde_json::to_vec(&inputs).map_err(|error| error.to_string())?,
                },
            ],
        )
        .map_err(|error| error.to_string())?;
    Ok(SessionResponse::Continued { session, input })
}

fn append_history(
    context: &SessionContext<'_, '_>,
    id: &SessionId,
    draft: SessionHistoryDraft,
) -> Result<SessionResponse, String> {
    require_open_session(context, id)?;
    let key = history_key(id);
    let old_history = read_raw(context, &key)?;
    let mut entries = decode_history(old_history.as_deref())?;
    let entry = SessionHistoryEntry {
        sequence: u64::try_from(entries.len())
            .map_err(|_| "session history sequence overflow".to_owned())?
            + 1,
        role: draft.role,
        content: draft.content,
        tool_calls: draft.tool_calls,
        tool_results: draft.tool_results,
        finish_reason: draft.finish_reason,
        usage: draft.usage,
        context_revision: draft.context_revision,
        instruction_revision: draft.instruction_revision,
    };
    entries.push(entry.clone());
    context
        .kernel
        .transact_durable(
            &session_namespace(),
            &[
                TransactionOp::AssertValue {
                    key: key.clone(),
                    expected: old_history,
                },
                TransactionOp::Put {
                    key,
                    value: serde_json::to_vec(&entries).map_err(|error| error.to_string())?,
                },
            ],
        )
        .map_err(|error| error.to_string())?;
    Ok(SessionResponse::HistoryAppended { entry })
}

fn read_session(
    context: &SessionContext<'_, '_>,
    id: &SessionId,
) -> Result<Option<SessionRecord>, String> {
    read_raw(context, &session_key(id))?
        .map(|value| serde_json::from_slice(&value).map_err(|error| error.to_string()))
        .transpose()
}

fn read_sessions(context: &SessionContext<'_, '_>) -> Result<Vec<SessionRecord>, String> {
    let ids = decode_ids(read_raw(context, ALL_SESSIONS_KEY)?.as_deref())?;
    ids.into_iter()
        .map(|id| {
            read_session(context, &id)?.ok_or_else(|| format!("missing durable session: {id}"))
        })
        .collect()
}

fn read_inputs(
    context: &SessionContext<'_, '_>,
    id: &SessionId,
) -> Result<Vec<SessionInput>, String> {
    decode_inputs(read_raw(context, &inputs_key(id))?.as_deref())
}

fn read_history(
    context: &SessionContext<'_, '_>,
    id: &SessionId,
) -> Result<Vec<SessionHistoryEntry>, String> {
    decode_history(read_raw(context, &history_key(id))?.as_deref())
}

fn resolve_input_resource(
    context: &SessionContext<'_, '_>,
    resource: &str,
) -> Result<Option<SessionInput>, String> {
    let value = resource
        .strip_prefix("input/")
        .ok_or_else(|| format!("invalid session input resource: {resource}"))?;
    let (session, sequence) = value
        .rsplit_once('/')
        .ok_or_else(|| format!("invalid session input resource: {resource}"))?;
    let session = SessionId::parse(session).map_err(|error| error.to_string())?;
    let sequence = sequence
        .parse::<u64>()
        .map_err(|_| format!("invalid session input resource: {resource}"))?;
    if read_session(context, &session)?.is_none() {
        return Ok(None);
    }
    Ok(read_inputs(context, &session)?
        .into_iter()
        .find(|input| input.sequence == sequence))
}

fn resolve_history_resource(
    context: &SessionContext<'_, '_>,
    resource: &str,
) -> Result<Option<SessionHistoryEntry>, String> {
    let value = resource
        .strip_prefix("history/")
        .ok_or_else(|| format!("invalid session history resource: {resource}"))?;
    let (session, sequence) = value
        .rsplit_once('/')
        .ok_or_else(|| format!("invalid session history resource: {resource}"))?;
    let session = SessionId::parse(session).map_err(|error| error.to_string())?;
    let sequence = sequence
        .parse::<u64>()
        .map_err(|_| format!("invalid session history resource: {resource}"))?;
    if read_session(context, &session)?.is_none() {
        return Ok(None);
    }
    Ok(read_history(context, &session)?
        .into_iter()
        .find(|entry| entry.sequence == sequence))
}

fn read_raw(context: &SessionContext<'_, '_>, key: &str) -> Result<Option<Vec<u8>>, String> {
    context
        .kernel
        .read_durable(&session_namespace(), key)
        .map_err(|error| error.to_string())
}

fn decode_ids(value: Option<&[u8]>) -> Result<Vec<SessionId>, String> {
    value
        .map(|value| serde_json::from_slice(value).map_err(|error| error.to_string()))
        .unwrap_or_else(|| Ok(Vec::new()))
}

fn decode_inputs(value: Option<&[u8]>) -> Result<Vec<SessionInput>, String> {
    value
        .map(|value| serde_json::from_slice(value).map_err(|error| error.to_string()))
        .unwrap_or_else(|| Ok(Vec::new()))
}

fn decode_history(value: Option<&[u8]>) -> Result<Vec<SessionHistoryEntry>, String> {
    value
        .map(|value| serde_json::from_slice(value).map_err(|error| error.to_string()))
        .unwrap_or_else(|| Ok(Vec::new()))
}

fn decode_journal(value: Option<&[u8]>) -> Result<Vec<SessionJournalEntry>, String> {
    value
        .map(|value| serde_json::from_slice(value).map_err(|error| error.to_string()))
        .unwrap_or_else(|| Ok(Vec::new()))
}

fn session_key(id: &SessionId) -> String {
    format!("session/{id}")
}

fn inputs_key(id: &SessionId) -> String {
    format!("inputs/{id}")
}

fn history_key(id: &SessionId) -> String {
    format!("history/{id}")
}

fn journal_key(id: &SessionId, stream: &ContractId) -> String {
    format!("journal/{id}/{}", stream.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;
    use phenix_core::{ContractId, Kernel, KernelConfig, LocalPersistence, PhenixValue, Project};
    use std::{
        fs,
        path::PathBuf,
        time::{SystemTime, UNIX_EPOCH},
    };

    fn authority() -> Authority {
        session_manifest().maximum_authority
    }

    fn invoke(kernel: &mut Kernel, command: &SessionCommand) -> Result<SessionResponse, String> {
        let input = serde_json::to_vec(&PhenixValue::from(command)).unwrap();
        let output = kernel
            .invoke(&session_service(), &input, &authority(), None)
            .map_err(|error| error.to_string())?;
        let output: PhenixValue =
            serde_json::from_slice(&output).map_err(|error| error.to_string())?;
        SessionResponse::try_from(Project(&output)).map_err(|error| error.to_string())
    }

    fn kernel_with(path: &PathBuf) -> Kernel {
        let manifest = session_manifest();
        let plugin = manifest.id.clone();
        let persistence = LocalPersistence::open(path).unwrap();
        let mut kernel =
            Kernel::with_persistence(KernelConfig::new([manifest]).unwrap(), persistence);
        kernel
            .register_embedded_factory(plugin, session_factory)
            .unwrap();
        kernel.activate_all().unwrap();
        kernel
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

    #[test]
    fn flat_sessions_and_ordered_inputs_are_durable_across_plugin_restart() {
        let path = temp_db("sessions");
        let root = SessionId::parse("root").unwrap();
        {
            let mut kernel = kernel_with(&path);
            invoke(
                &mut kernel,
                &SessionCommand::Create {
                    session: SessionRecord::new(root.clone()),
                },
            )
            .unwrap();
            for (kind, content) in [
                (SessionInputKind::Root, b"system".to_vec()),
                (SessionInputKind::User, b"hello".to_vec()),
            ] {
                invoke(
                    &mut kernel,
                    &SessionCommand::Continue {
                        id: root.clone(),
                        kind,
                        content: content.into(),
                    },
                )
                .unwrap();
            }
        }

        let mut restored = kernel_with(&path);
        assert_eq!(
            invoke(&mut restored, &SessionCommand::List).unwrap(),
            SessionResponse::Sessions {
                sessions: vec![SessionRecord::new(root.clone())],
            }
        );
        assert_eq!(
            invoke(&mut restored, &SessionCommand::Inputs { id: root },).unwrap(),
            SessionResponse::Inputs {
                inputs: vec![
                    SessionInput {
                        sequence: 1,
                        kind: SessionInputKind::Root,
                        content: b"system".to_vec().into(),
                    },
                    SessionInput {
                        sequence: 2,
                        kind: SessionInputKind::User,
                        content: b"hello".to_vec().into(),
                    },
                ],
            }
        );
        let _ = fs::remove_file(path);
    }

    #[test]
    fn durable_input_resource_resolves_after_restart() {
        let path = temp_db("input-resource");
        let root = SessionId::parse("root").unwrap();
        {
            let mut kernel = kernel_with(&path);
            invoke(
                &mut kernel,
                &SessionCommand::Create {
                    session: SessionRecord::new(root.clone()),
                },
            )
            .unwrap();
            invoke(
                &mut kernel,
                &SessionCommand::Continue {
                    id: root.clone(),
                    kind: SessionInputKind::User,
                    content: b"hello".to_vec().into(),
                },
            )
            .unwrap();
        }

        let mut restored = kernel_with(&path);
        assert_eq!(
            invoke(
                &mut restored,
                &SessionCommand::ResolveInput {
                    resource: phenix_sdk::session_input_resource(&root, 1),
                },
            )
            .unwrap(),
            SessionResponse::Input {
                input: Some(SessionInput {
                    sequence: 1,
                    kind: SessionInputKind::User,
                    content: b"hello".to_vec().into(),
                }),
            }
        );
        let _ = fs::remove_file(path);
    }

    #[test]
    fn portable_history_is_durable_and_resolvable_after_restart() {
        let path = temp_db("history-resource");
        let root = SessionId::parse("root").unwrap();
        let draft = SessionHistoryDraft {
            role: phenix_sdk::SessionHistoryRole::Assistant,
            content: vec![phenix_sdk::SessionHistoryContentPart::Text {
                text: "done".into(),
            }],
            tool_calls: Vec::new(),
            tool_results: Vec::new(),
            finish_reason: Some(phenix_sdk::SessionHistoryFinishReason::Complete),
            usage: None,
            context_revision: "ctx-1".into(),
            instruction_revision: "instructions-1".into(),
        };
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
                &SessionCommand::AppendHistory {
                    id: root.clone(),
                    entry: draft.clone(),
                },
            )
            .unwrap();
            assert!(matches!(
                response,
                SessionResponse::HistoryAppended { ref entry }
                    if entry.sequence == 1 && entry.context_revision == "ctx-1"
            ));
        }

        let mut restored = kernel_with(&path);
        let history = invoke(&mut restored, &SessionCommand::History { id: root.clone() }).unwrap();
        assert!(matches!(
            history,
            SessionResponse::History { ref entries }
                if entries.len() == 1 && entries[0].content == draft.content
        ));
        let resolved = invoke(
            &mut restored,
            &SessionCommand::ResolveHistory {
                resource: phenix_sdk::session_history_resource(&root, 1),
            },
        )
        .unwrap();
        assert!(matches!(
            resolved,
            SessionResponse::HistoryEntry { entry: Some(ref entry) }
                if entry.sequence == 1 && entry.instruction_revision == "instructions-1"
        ));
        let _ = fs::remove_file(path);
    }

    #[test]
    fn application_metadata_and_closed_lifecycle_are_durable() {
        let path = temp_db("session-metadata");
        let root = SessionId::parse("root").unwrap();
        {
            let mut kernel = kernel_with(&path);
            let created = SessionRecord::application(
                root.clone(),
                "/workspace".into(),
                Some("initial".into()),
            );
            assert!(matches!(
                invoke(
                    &mut kernel,
                    &SessionCommand::Create {
                        session: created.clone(),
                    },
                )
                .unwrap(),
                SessionResponse::Created { session } if session == created
            ));
            assert!(matches!(
                invoke(
                    &mut kernel,
                    &SessionCommand::Rename {
                        id: root.clone(),
                        title: "renamed".into(),
                    },
                )
                .unwrap(),
                SessionResponse::Updated { ref session }
                    if session.title.as_deref() == Some("renamed")
                        && session.working_directory.as_deref() == Some("/workspace")
            ));
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
                    && session.working_directory.as_deref() == Some("/workspace")
                    && session.lifecycle == SessionLifecycle::Open
        ));
        assert!(matches!(
            invoke(
                &mut restored,
                &SessionCommand::Close { id: root.clone() },
            )
            .unwrap(),
            SessionResponse::Updated { ref session }
                if session.lifecycle == SessionLifecycle::Closed
        ));
        let error = invoke(
            &mut restored,
            &SessionCommand::Continue {
                id: root.clone(),
                kind: SessionInputKind::User,
                content: b"closed".to_vec().into(),
            },
        )
        .unwrap_err();
        assert!(error.contains("session is closed"));
        drop(restored);

        let mut restored = kernel_with(&path);
        assert!(matches!(
            invoke(&mut restored, &SessionCommand::Get { id: root }).unwrap(),
            SessionResponse::Session { session: Some(ref session) }
                if session.lifecycle == SessionLifecycle::Closed
        ));
        let _ = fs::remove_file(path);
    }

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
}
