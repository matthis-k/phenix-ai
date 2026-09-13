use crate::session_tree_component_id;
use phenix_core::{
    Authority, CapabilityId, ComponentInterface, DurableSchema, LayerResult, PluginContext,
    PluginExecution, PluginHost, PluginId, PluginInstance, PluginManifest, PreparedMutationHandle,
    ResourceNamespace, SdkClient, ServiceContribution, ServiceId, SessionId, TransactionOp,
};
use phenix_sdk::{
    session_service, SessionCommand, SessionInterface, SessionMutationInterface, SessionRecord,
    SessionResponse,
};
use serde::{Deserialize, Serialize};

pub const SESSION_TREE_SERVICE: &str = "phenix.session-tree@1";
const SESSION_TREE_PLUGIN: &str = "phenix.session-tree";
const SESSION_TREE_NAMESPACE: &str = "phenix.session-tree.state";
const SESSION_PLUGIN: &str = "phenix.sessions";
const PERSISTENCE_SCHEMA: &str = "kernel.persistence.schema";
const PERSISTENCE_READ: &str = "kernel.persistence.read";
const PERSISTENCE_WRITE: &str = "kernel.persistence.write";

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
pub struct SessionLineage {
    pub session_id: SessionId,
    pub parent_session_id: Option<SessionId>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub enum SessionTreeCommand {
    CreateChild {
        session_id: SessionId,
        parent_session_id: SessionId,
    },
    Link {
        session_id: SessionId,
        parent_session_id: Option<SessionId>,
    },
    Parent {
        session_id: SessionId,
    },
    Children {
        parent_session_id: Option<SessionId>,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(tag = "result", rename_all = "snake_case", deny_unknown_fields)]
pub enum SessionTreeResponse {
    ChildCreated {
        session: SessionRecord,
        lineage: SessionLineage,
    },
    Lineage {
        lineage: SessionLineage,
    },
    Parent {
        parent_session_id: Option<SessionId>,
    },
    Children {
        session_ids: Vec<SessionId>,
    },
}

#[must_use]
pub fn session_tree_manifest() -> PluginManifest {
    PluginManifest {
        id: PluginId::parse(SESSION_TREE_PLUGIN).expect("static plugin id is valid"),
        version: 1,
        execution: PluginExecution::Embedded,
        dependencies: vec![PluginId::parse(SESSION_PLUGIN).expect("static plugin id")],
        services: vec![
            ServiceContribution {
                role: phenix_core::ServiceRole::Terminal,
                service: session_tree_service(),
                priority: 100,
                required_authority: Authority::default(),
            },
            ServiceContribution {
                role: phenix_core::ServiceRole::Layer,
                service: session_service(),
                priority: 100,
                required_authority: Authority::default(),
            },
        ],
        resource_namespaces: vec![session_tree_namespace()],
        maximum_authority: Authority::new([
            capability(PERSISTENCE_SCHEMA),
            capability(PERSISTENCE_READ),
            capability(PERSISTENCE_WRITE),
        ]),
    }
}

#[must_use]
pub fn session_tree_factory() -> Box<dyn PluginInstance> {
    Box::new(SessionTreePlugin)
}

#[must_use]
pub fn session_tree_service() -> ServiceId {
    ServiceId::parse(SESSION_TREE_SERVICE).expect("static service id is valid")
}

fn session_tree_namespace() -> ResourceNamespace {
    ResourceNamespace::parse(SESSION_TREE_NAMESPACE).expect("static namespace is valid")
}

fn capability(value: &str) -> CapabilityId {
    CapabilityId::parse(value).expect("static capability is valid")
}

#[derive(Clone, Debug, Eq, PartialEq, phenix_sdk_macros::PhenixValue)]
enum SessionMutationRequest {
    PrepareCreate { session: SessionRecord },
}

#[derive(Clone, Debug, Eq, PartialEq, phenix_sdk_macros::PhenixValue)]
enum SessionMutationResult {
    PreparedCreate {
        session: SessionRecord,
        mutation: PreparedMutationHandle,
    },
}

struct SessionTreeSdk<'host, 'runtime> {
    sessions: SdkClient<'host, 'runtime, SessionInterface>,
    mutations: SdkClient<'host, 'runtime, SessionMutationInterface>,
}

type SessionTreeContext<'host, 'runtime> =
    PluginContext<'host, 'runtime, SessionTreeSdk<'host, 'runtime>>;

fn context<'host, 'runtime>(
    host: &'host PluginHost<'runtime>,
) -> SessionTreeContext<'host, 'runtime> {
    let component = session_tree_component_id();
    PluginContext::new(
        host,
        SessionTreeSdk {
            sessions: SdkClient::new(host, component.clone()),
            mutations: SdkClient::new(host, component),
        },
        (),
        (),
    )
}

struct SessionTreePlugin;

impl PluginInstance for SessionTreePlugin {
    fn start(&mut self, host: &PluginHost<'_>) -> Result<(), String> {
        context(host)
            .kernel
            .register_durable_schema(&DurableSchema::new(session_tree_namespace(), 1))
            .map_err(|error| error.to_string())
    }

    fn invoke_layer(
        &mut self,
        service: &ServiceId,
        input: &[u8],
        host: &PluginHost<'_>,
    ) -> Result<LayerResult, String> {
        if service != &session_service() {
            return Err(format!("unsupported session-tree layer service: {service}"));
        }
        let context = context(host);
        context
            .kernel
            .continue_service(input, context.call.authority)
            .map(LayerResult::Handled)
            .map_err(|error| error.to_string())
    }

    fn invoke(
        &mut self,
        service: &ServiceId,
        input: &[u8],
        host: &PluginHost<'_>,
    ) -> Result<Vec<u8>, String> {
        if service != &session_tree_service() {
            return Err(format!("unsupported session-tree service: {service}"));
        }
        let context = context(host);
        let interface = crate::SessionTreeInterface::interface_id();
        let command = context
            .kernel
            .decode_projected::<SessionTreeCommand>(&interface, input)
            .map_err(|error| error.to_string())?;
        let response = handle(&context, command)?;
        context
            .kernel
            .encode_value(&response)
            .map_err(|error| error.to_string())
    }
}

fn handle(
    context: &SessionTreeContext<'_, '_>,
    command: SessionTreeCommand,
) -> Result<SessionTreeResponse, String> {
    match command {
        SessionTreeCommand::CreateChild {
            session_id,
            parent_session_id,
        } => create_child(context, session_id, parent_session_id),
        SessionTreeCommand::Link {
            session_id,
            parent_session_id,
        } => link(context, session_id, parent_session_id),
        SessionTreeCommand::Parent { session_id } => Ok(SessionTreeResponse::Parent {
            parent_session_id: read_lineage(context, &session_id)?
                .and_then(|lineage| lineage.parent_session_id),
        }),
        SessionTreeCommand::Children { parent_session_id } => Ok(SessionTreeResponse::Children {
            session_ids: read_children(context, parent_session_id.as_ref())?,
        }),
    }
}

fn create_child(
    context: &SessionTreeContext<'_, '_>,
    session_id: SessionId,
    parent_session_id: SessionId,
) -> Result<SessionTreeResponse, String> {
    if session_id == parent_session_id {
        return Err("session cannot be its own parent".into());
    }
    require_session(context, &parent_session_id)?;
    if read_lineage(context, &session_id)?.is_some() {
        return Err(format!("session lineage already exists: {session_id}"));
    }

    let prepared = context
        .sdk
        .mutations
        .invoke_projected::<SessionMutationRequest, SessionMutationResult>(
            &SessionMutationRequest::PrepareCreate {
                session: SessionRecord::new(session_id.clone()),
            },
        )
        .map_err(|error| error.to_string())?;
    let SessionMutationResult::PreparedCreate {
        session,
        mutation: session_mutation,
    } = prepared;

    let lineage = SessionLineage {
        session_id: session_id.clone(),
        parent_session_id: Some(parent_session_id.clone()),
    };
    let children_key = children_key(Some(&parent_session_id));
    let old_children = read_raw(context, &children_key)?;
    let mut children = decode_ids(old_children.as_deref())?;
    children.push(session_id.clone());
    children.sort();
    children.dedup();
    let tree_operations = vec![
        TransactionOp::AssertValue {
            key: lineage_key(&session_id),
            expected: None,
        },
        TransactionOp::AssertValue {
            key: children_key.clone(),
            expected: old_children,
        },
        TransactionOp::Put {
            key: lineage_key(&session_id),
            value: serde_json::to_vec(&lineage).map_err(|error| error.to_string())?,
        },
        TransactionOp::Put {
            key: children_key,
            value: serde_json::to_vec(&children).map_err(|error| error.to_string())?,
        },
    ];
    let tree_mutation = context
        .kernel
        .prepare_durable_transaction(&session_tree_namespace(), &tree_operations)
        .map_err(|error| error.to_string())?;

    context
        .kernel
        .transact_prepared(&[session_mutation, tree_mutation])
        .map_err(|error| error.to_string())?;
    Ok(SessionTreeResponse::ChildCreated { session, lineage })
}

fn link(
    context: &SessionTreeContext<'_, '_>,
    session_id: SessionId,
    parent_session_id: Option<SessionId>,
) -> Result<SessionTreeResponse, String> {
    require_session(context, &session_id)?;
    if let Some(parent) = parent_session_id.as_ref() {
        if parent == &session_id {
            return Err("session cannot be its own parent".into());
        }
        require_session(context, parent)?;
        if would_cycle(context, &session_id, parent)? {
            return Err("session lineage would contain a cycle".into());
        }
    }
    if read_lineage(context, &session_id)?.is_some() {
        return Err(format!("session lineage already exists: {session_id}"));
    }

    let lineage = SessionLineage {
        session_id: session_id.clone(),
        parent_session_id: parent_session_id.clone(),
    };
    let children_key = children_key(parent_session_id.as_ref());
    let old_children = read_raw(context, &children_key)?;
    let mut children = decode_ids(old_children.as_deref())?;
    children.push(session_id.clone());
    children.sort();
    children.dedup();

    context
        .kernel
        .transact_durable(
            &session_tree_namespace(),
            &[
                TransactionOp::AssertValue {
                    key: lineage_key(&session_id),
                    expected: None,
                },
                TransactionOp::AssertValue {
                    key: children_key.clone(),
                    expected: old_children,
                },
                TransactionOp::Put {
                    key: lineage_key(&session_id),
                    value: serde_json::to_vec(&lineage).map_err(|error| error.to_string())?,
                },
                TransactionOp::Put {
                    key: children_key,
                    value: serde_json::to_vec(&children).map_err(|error| error.to_string())?,
                },
            ],
        )
        .map_err(|error| error.to_string())?;

    Ok(SessionTreeResponse::Lineage { lineage })
}

fn require_session(
    context: &SessionTreeContext<'_, '_>,
    id: &SessionId,
) -> Result<SessionRecord, String> {
    let response = context
        .sdk
        .sessions
        .invoke_projected(&SessionCommand::Get { id: id.clone() })
        .map_err(|error| error.to_string())?;
    match response {
        SessionResponse::Session {
            session: Some(session),
        } => Ok(session),
        SessionResponse::Session { session: None } => Err(format!("unknown session: {id}")),
        other => Err(format!(
            "unexpected session response while linking lineage: {other:?}"
        )),
    }
}

fn would_cycle(
    context: &SessionTreeContext<'_, '_>,
    child: &SessionId,
    parent: &SessionId,
) -> Result<bool, String> {
    let mut cursor = Some(parent.clone());
    while let Some(id) = cursor {
        if &id == child {
            return Ok(true);
        }
        cursor = read_lineage(context, &id)?.and_then(|lineage| lineage.parent_session_id);
    }
    Ok(false)
}

fn read_lineage(
    context: &SessionTreeContext<'_, '_>,
    id: &SessionId,
) -> Result<Option<SessionLineage>, String> {
    read_raw(context, &lineage_key(id))?
        .map(|value| serde_json::from_slice(&value).map_err(|error| error.to_string()))
        .transpose()
}

fn read_children(
    context: &SessionTreeContext<'_, '_>,
    parent: Option<&SessionId>,
) -> Result<Vec<SessionId>, String> {
    decode_ids(read_raw(context, &children_key(parent))?.as_deref())
}

fn read_raw(context: &SessionTreeContext<'_, '_>, key: &str) -> Result<Option<Vec<u8>>, String> {
    context
        .kernel
        .read_durable(&session_tree_namespace(), key)
        .map_err(|error| error.to_string())
}

fn decode_ids(value: Option<&[u8]>) -> Result<Vec<SessionId>, String> {
    value
        .map(|value| serde_json::from_slice(value).map_err(|error| error.to_string()))
        .unwrap_or_else(|| Ok(Vec::new()))
}

fn lineage_key(id: &SessionId) -> String {
    format!("lineage/{id}")
}

fn children_key(parent: Option<&SessionId>) -> String {
    match parent {
        Some(parent) => format!("children/{parent}"),
        None => "children/@root".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session_tree_component_manifest;
    use phenix_core::{
        Kernel, KernelConfig, LocalPersistence, PhenixValue, Project, ResolvedHarness,
        ResolvedHarnessActivation,
    };
    use phenix_plugin_sessions::{session_component_manifest, session_factory, session_manifest};
    use std::{
        fs,
        path::PathBuf,
        time::{SystemTime, UNIX_EPOCH},
    };

    fn session(value: &str) -> SessionId {
        SessionId::parse(value).unwrap()
    }

    fn authority() -> Authority {
        Authority::new(
            session_manifest()
                .maximum_authority
                .capabilities()
                .cloned()
                .chain(
                    session_tree_manifest()
                        .maximum_authority
                        .capabilities()
                        .cloned(),
                ),
        )
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

    fn kernel_with(path: &PathBuf) -> Kernel {
        let sessions = session_manifest();
        let tree = session_tree_manifest();
        let session_plugin = sessions.id.clone();
        let tree_plugin = tree.id.clone();
        let resolved = ResolvedHarness::resolve(
            [sessions.clone(), tree.clone()],
            [
                session_component_manifest(),
                session_tree_component_manifest(),
            ],
            [],
            &authority(),
        )
        .unwrap();
        let mut kernel = Kernel::with_persistence(
            KernelConfig::new([sessions, tree]).unwrap(),
            LocalPersistence::open(path).unwrap(),
        );
        kernel.activate_resolved_harness(&resolved).unwrap();
        kernel
            .register_embedded_factory(session_plugin, session_factory)
            .unwrap();
        kernel
            .register_embedded_factory(tree_plugin, session_tree_factory)
            .unwrap();
        kernel.activate_all().unwrap();
        kernel
    }

    fn invoke_session(kernel: &mut Kernel, command: SessionCommand) -> SessionResponse {
        let output = kernel
            .invoke(
                &session_service(),
                &serde_json::to_vec(&PhenixValue::from(&command)).unwrap(),
                &authority(),
                None,
            )
            .unwrap();
        let output: PhenixValue = serde_json::from_slice(&output).unwrap();
        SessionResponse::try_from(Project(&output)).unwrap()
    }

    fn invoke_tree(
        kernel: &mut Kernel,
        command: SessionTreeCommand,
    ) -> Result<SessionTreeResponse, String> {
        let output = kernel
            .invoke(
                &session_tree_service(),
                &serde_json::to_vec(&PhenixValue::from(&command)).unwrap(),
                &authority(),
                None,
            )
            .map_err(|error| error.to_string())?;
        let output: PhenixValue =
            serde_json::from_slice(&output).map_err(|error| error.to_string())?;
        output.project().map_err(|error| error.to_string())
    }

    #[test]
    fn lineage_is_optional_and_durable_without_changing_flat_session_identity() {
        let path = temp_db("session-tree");
        let root = session("root");
        let child = session("child");
        {
            let mut kernel = kernel_with(&path);
            invoke_session(
                &mut kernel,
                SessionCommand::Create {
                    session: phenix_sdk::SessionRecord::new(root.clone()),
                },
            );
            invoke_session(
                &mut kernel,
                SessionCommand::Create {
                    session: phenix_sdk::SessionRecord::new(child.clone()),
                },
            );
            assert_eq!(
                invoke_tree(
                    &mut kernel,
                    SessionTreeCommand::Link {
                        session_id: root.clone(),
                        parent_session_id: None,
                    },
                )
                .unwrap(),
                SessionTreeResponse::Lineage {
                    lineage: SessionLineage {
                        session_id: root.clone(),
                        parent_session_id: None,
                    },
                }
            );
            invoke_tree(
                &mut kernel,
                SessionTreeCommand::Link {
                    session_id: child.clone(),
                    parent_session_id: Some(root.clone()),
                },
            )
            .unwrap();
            assert_eq!(
                invoke_session(&mut kernel, SessionCommand::Get { id: child.clone() }),
                SessionResponse::Session {
                    session: Some(SessionRecord::new(child.clone())),
                }
            );
        }

        let mut restored = kernel_with(&path);
        assert_eq!(
            invoke_tree(
                &mut restored,
                SessionTreeCommand::Children {
                    parent_session_id: Some(root.clone()),
                },
            )
            .unwrap(),
            SessionTreeResponse::Children {
                session_ids: vec![child.clone()],
            }
        );
        assert_eq!(
            invoke_tree(
                &mut restored,
                SessionTreeCommand::Parent { session_id: child },
            )
            .unwrap(),
            SessionTreeResponse::Parent {
                parent_session_id: Some(root),
            }
        );
        let _ = fs::remove_file(path);
    }

    #[test]
    fn lineage_rejects_missing_sessions() {
        let path = temp_db("session-tree-missing");
        let mut kernel = kernel_with(&path);
        invoke_session(
            &mut kernel,
            SessionCommand::Create {
                session: phenix_sdk::SessionRecord::new(session("root")),
            },
        );
        assert!(invoke_tree(
            &mut kernel,
            SessionTreeCommand::Link {
                session_id: session("missing"),
                parent_session_id: Some(session("root")),
            },
        )
        .unwrap_err()
        .contains("unknown session"));
        let _ = fs::remove_file(path);
    }
}
