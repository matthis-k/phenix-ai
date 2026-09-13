use crate::{
    context_component_id,
    state_service::{ContextStateService, CONTEXT_PROJECTION_STATE_KEY},
};
use phenix_core::{
    Authority, Bytes, CapabilityId, ComponentInterface, ContextResourceId, ContextRevisionId,
    DurableSchema, PluginContext, PluginExecution, PluginHost, PluginId, PluginInstance,
    PluginManifest, ResourceNamespace, SdkClient, ServiceContribution, ServiceId, TransactionOp,
};
use phenix_sdk::{
    context_service, ContextCommand, ContextDescriptor, ContextInjection, ContextInjectionLifetime,
    ContextInjectionRequester, ContextInterface, ContextResourceKind, ContextResourceRevision,
    ContextResponse, ContextScope, ExactContextReference, ExecutionCommand,
    ExecutionContextProjection, ExecutionInterface, ExecutionResponse, ExecutionState,
    ProjectedContextEntry, RepositoryContextSource,
};
use sha2::{Digest, Sha256};

const CONTEXT_PLUGIN: &str = "phenix.context";
const CONTEXT_NAMESPACE: &str = "phenix.context.state";
const PERSISTENCE_SCHEMA: &str = "kernel.persistence.schema";
const PERSISTENCE_READ: &str = "kernel.persistence.read";
const PERSISTENCE_WRITE: &str = "kernel.persistence.write";
const ALL_RESOURCES_KEY: &str = "resources/@all";

struct ContextSdk<'host, 'runtime> {
    execution: SdkClient<'host, 'runtime, ExecutionInterface>,
}

type ContextPluginContext<'host, 'runtime> =
    PluginContext<'host, 'runtime, ContextSdk<'host, 'runtime>>;

fn context<'host, 'runtime>(
    host: &'host PluginHost<'runtime>,
) -> ContextPluginContext<'host, 'runtime> {
    PluginContext::new(
        host,
        ContextSdk {
            execution: SdkClient::new(host, context_component_id()),
        },
        (),
        (),
    )
}

#[must_use]
pub fn context_manifest() -> PluginManifest {
    PluginManifest {
        id: PluginId::parse(CONTEXT_PLUGIN).expect("static plugin id is valid"),
        version: 1,
        execution: PluginExecution::Embedded,
        dependencies: Vec::new(),
        services: vec![ServiceContribution {
            role: phenix_core::ServiceRole::Terminal,
            service: context_service(),
            priority: 100,
            required_authority: Authority::default(),
        }],
        resource_namespaces: vec![context_namespace()],
        maximum_authority: Authority::new([
            capability(PERSISTENCE_SCHEMA),
            capability(PERSISTENCE_READ),
            capability(PERSISTENCE_WRITE),
        ]),
    }
}

#[must_use]
pub fn context_factory() -> Box<dyn PluginInstance> {
    Box::new(ContextPlugin {
        state: ContextStateService::default(),
    })
}

pub(crate) fn context_namespace() -> ResourceNamespace {
    ResourceNamespace::parse(CONTEXT_NAMESPACE).expect("static namespace is valid")
}

fn capability(value: &str) -> CapabilityId {
    CapabilityId::parse(value).expect("static capability is valid")
}

struct ContextPlugin {
    state: ContextStateService,
}

impl PluginInstance for ContextPlugin {
    fn start(&mut self, host: &PluginHost<'_>) -> Result<(), String> {
        let context = context(host);
        context
            .kernel
            .register_durable_schema(&DurableSchema::new(context_namespace(), 1))
            .map_err(|error| error.to_string())?;
        let snapshot = context
            .kernel
            .read_durable(&context_namespace(), CONTEXT_PROJECTION_STATE_KEY)
            .map_err(|error| error.to_string())?;
        self.state = ContextStateService::restore(snapshot.as_deref())
            .map_err(|error| format!("invalid durable context projection state: {error:?}"))?;
        Ok(())
    }

    fn invoke(
        &mut self,
        service: &ServiceId,
        input: &[u8],
        host: &PluginHost<'_>,
    ) -> Result<Vec<u8>, String> {
        if service != &context_service() {
            return Err(format!("unsupported context service: {service}"));
        }
        let context = context(host);
        let command = context
            .kernel
            .decode_projected::<ContextCommand>(&ContextInterface::interface_id(), input)
            .map_err(|error| error.to_string())?;
        let response = handle(&context, &mut self.state, command)?;
        context
            .kernel
            .encode_value(&response)
            .map_err(|error| error.to_string())
    }
}

fn handle(
    context: &ContextPluginContext<'_, '_>,
    state: &mut ContextStateService,
    command: ContextCommand,
) -> Result<ContextResponse, String> {
    if let Some(execution_id) = state_command_execution(&command) {
        require_active_execution(context, execution_id)?;
        return handle_state_command(context, state, command);
    }

    match command {
        ContextCommand::Register {
            resource_id,
            kind,
            source,
            scope,
            content,
        } => Ok(ContextResponse::Registered {
            resource: register_resource(context, resource_id, kind, source, scope, content)?,
        }),
        ContextCommand::Get {
            resource_id,
            revision,
        } => Ok(ContextResponse::Resource {
            resource: read_resource(context, &resource_id, &revision)?,
        }),
        ContextCommand::List => Ok(ContextResponse::Resources {
            descriptors: list_descriptors(context)?,
        }),
        ContextCommand::DiscoverRepository {
            workspace_id,
            sources,
        } => Ok(ContextResponse::Discovered {
            descriptors: discover_repository(context, &workspace_id, sources)?,
        }),
        ContextCommand::Load {
            execution_id,
            resource_id,
            revision,
            requester,
            lifetime,
            reason,
        } => {
            let (injection, resource) = load_context(
                context,
                state,
                execution_id,
                resource_id,
                revision,
                requester,
                lifetime,
                reason,
            )?;
            Ok(ContextResponse::Loaded {
                injection,
                resource,
            })
        }
        ContextCommand::Project { execution_id } => Ok(ContextResponse::Projection {
            projection: project_context(context, execution_id)?,
        }),
        ContextCommand::Admit { .. }
        | ContextCommand::PrepareCompaction { .. }
        | ContextCommand::CommitCompaction { .. }
        | ContextCommand::InvalidateProjection { .. } => {
            Err("context projection command leaked past state dispatcher".into())
        }
    }
}

fn state_command_execution(command: &ContextCommand) -> Option<&str> {
    match command {
        ContextCommand::Admit { request } => Some(&request.execution_id),
        ContextCommand::PrepareCompaction { proposal } => Some(&proposal.execution_id),
        ContextCommand::CommitCompaction { execution_id, .. }
        | ContextCommand::InvalidateProjection { execution_id } => Some(execution_id),
        _ => None,
    }
}

fn handle_state_command(
    context: &ContextPluginContext<'_, '_>,
    state: &mut ContextStateService,
    command: ContextCommand,
) -> Result<ContextResponse, String> {
    let previous = read_raw(context, CONTEXT_PROJECTION_STATE_KEY)?;
    let response = match state.handle_state_command(command) {
        Some(Ok(response)) => response,
        Some(Err(error)) => {
            *state = ContextStateService::restore(previous.as_deref())
                .map_err(|restore| format!("context state rollback failed: {restore:?}"))?;
            return Err(error);
        }
        None => return Err("non-state command reached context state dispatcher".into()),
    };
    persist_state(context, state, previous)?;
    Ok(response)
}

fn persist_state(
    context: &ContextPluginContext<'_, '_>,
    state: &mut ContextStateService,
    previous: Option<Vec<u8>>,
) -> Result<(), String> {
    let next = match state.snapshot() {
        Ok(snapshot) => snapshot,
        Err(error) => {
            *state = ContextStateService::restore(previous.as_deref())
                .map_err(|restore| format!("context state rollback failed: {restore:?}"))?;
            return Err(format!("context state snapshot failed: {error:?}"));
        }
    };
    let result = context.kernel.transact_durable(
        &context_namespace(),
        &[
            TransactionOp::AssertValue {
                key: CONTEXT_PROJECTION_STATE_KEY.into(),
                expected: previous.clone(),
            },
            TransactionOp::Put {
                key: CONTEXT_PROJECTION_STATE_KEY.into(),
                value: next,
            },
        ],
    );
    if let Err(error) = result {
        *state = ContextStateService::restore(previous.as_deref())
            .map_err(|restore| format!("context state rollback failed: {restore:?}"))?;
        return Err(error.to_string());
    }
    Ok(())
}

fn register_resource(
    context: &ContextPluginContext<'_, '_>,
    resource_id: ContextResourceId,
    kind: ContextResourceKind,
    source: String,
    scope: ContextScope,
    content: Bytes,
) -> Result<ContextResourceRevision, String> {
    validate_identity("context source", &source)?;
    if let ContextScope::PathPrefix(prefix) = &scope {
        validate_identity("context path scope", prefix)?;
    }
    let revision = content_hash(content.as_ref());
    let estimated_bytes = u64::try_from(content.as_ref().len())
        .map_err(|_| "context resource byte length exceeds u64".to_owned())?;
    let resource = ContextResourceRevision {
        descriptor: ContextDescriptor {
            resource_id: resource_id.clone(),
            revision: revision.clone(),
            kind,
            source,
            scope,
            content_identity: revision.as_str().to_owned(),
            estimated_bytes,
        },
        content,
    };
    let key = resource_key(&resource_id, &revision);
    if let Some(existing) = read_raw(context, &key)? {
        let existing: ContextResourceRevision =
            serde_json::from_slice(&existing).map_err(|error| error.to_string())?;
        if existing != resource {
            return Err(format!(
                "immutable context revision collision: {resource_id}@{revision}"
            ));
        }
        return Ok(existing);
    }
    let old_refs = read_raw(context, ALL_RESOURCES_KEY)?;
    let mut refs = decode_refs(old_refs.as_deref())?;
    refs.push(ExactContextReference {
        resource_id,
        revision,
    });
    refs.sort();
    refs.dedup();
    context
        .kernel
        .transact_durable(
            &context_namespace(),
            &[
                TransactionOp::AssertValue {
                    key: key.clone(),
                    expected: None,
                },
                TransactionOp::AssertValue {
                    key: ALL_RESOURCES_KEY.into(),
                    expected: old_refs,
                },
                TransactionOp::Put {
                    key,
                    value: serde_json::to_vec(&resource).map_err(|error| error.to_string())?,
                },
                TransactionOp::Put {
                    key: ALL_RESOURCES_KEY.into(),
                    value: serde_json::to_vec(&refs).map_err(|error| error.to_string())?,
                },
            ],
        )
        .map_err(|error| error.to_string())?;
    Ok(resource)
}

fn discover_repository(
    context: &ContextPluginContext<'_, '_>,
    workspace_id: &str,
    mut sources: Vec<RepositoryContextSource>,
) -> Result<Vec<ContextDescriptor>, String> {
    validate_identity("workspace id", workspace_id)?;
    sources.sort_by(|left, right| left.path.cmp(&right.path));
    let mut descriptors = Vec::new();
    for source in sources {
        let Some(kind) = project_file_kind(&source.path) else {
            continue;
        };
        let scope = match kind {
            ContextResourceKind::Skill => ContextScope::Workspace,
            _ => match parent_path(&source.path) {
                Some(parent) if !parent.is_empty() => ContextScope::PathPrefix(parent.to_owned()),
                _ => ContextScope::Workspace,
            },
        };
        let prefix = match kind {
            ContextResourceKind::ProjectInstruction => "project-instruction",
            ContextResourceKind::ProjectDocument => "project-document",
            ContextResourceKind::Skill => "skill",
            ContextResourceKind::External => unreachable!(),
        };
        let resource_id =
            ContextResourceId::parse(format!("{prefix}:{workspace_id}:{}", source.path))
                .map_err(str::to_owned)?;
        let resource = register_resource(
            context,
            resource_id,
            kind,
            source.path,
            scope,
            source.content,
        )?;
        descriptors.push(resource.descriptor);
    }
    descriptors.sort_by(|left, right| {
        left.resource_id
            .cmp(&right.resource_id)
            .then_with(|| left.revision.cmp(&right.revision))
    });
    Ok(descriptors)
}

fn project_file_kind(path: &str) -> Option<ContextResourceKind> {
    match file_name(path) {
        "AGENTS.md" | "AGENTS.override.md" => Some(ContextResourceKind::ProjectInstruction),
        "CONTRIBUTING.md" | "DEVELOPMENT.md" => Some(ContextResourceKind::ProjectDocument),
        "SKILL.md" => Some(ContextResourceKind::Skill),
        _ => None,
    }
}

fn load_context(
    context: &ContextPluginContext<'_, '_>,
    state: &mut ContextStateService,
    execution_id: String,
    resource_id: ContextResourceId,
    revision: ContextRevisionId,
    requester: ContextInjectionRequester,
    lifetime: ContextInjectionLifetime,
    reason: String,
) -> Result<(ContextInjection, ContextResourceRevision), String> {
    validate_identity("execution id", &execution_id)?;
    validate_identity("context load reason", &reason)?;
    require_active_execution(context, &execution_id)?;
    let resource = read_resource(context, &resource_id, &revision)?
        .ok_or_else(|| format!("unknown context revision: {resource_id}@{revision}"))?;
    let key = injections_key(&execution_id);
    let old_injections = read_raw(context, &key)?;
    let mut injections = decode_injections(old_injections.as_deref())?;
    let sequence = u64::try_from(injections.len())
        .map_err(|_| "context injection sequence overflow".to_owned())?
        + 1;
    let injection = ContextInjection {
        sequence,
        execution_id: execution_id.clone(),
        source: ExactContextReference {
            resource_id,
            revision,
        },
        requester,
        lifetime,
        reason,
    };
    injections.push(injection.clone());

    let old_state = read_raw(context, CONTEXT_PROJECTION_STATE_KEY)?;
    let mut next_state = state.clone();
    let state_changed = next_state.invalidate_if_present(&execution_id).is_some();
    let next_state_bytes = state_changed
        .then(|| {
            next_state
                .snapshot()
                .map_err(|error| format!("context state snapshot failed: {error:?}"))
        })
        .transpose()?;

    let mut operations = vec![
        TransactionOp::AssertValue {
            key: key.clone(),
            expected: old_injections,
        },
        TransactionOp::Put {
            key,
            value: serde_json::to_vec(&injections).map_err(|error| error.to_string())?,
        },
    ];
    if let Some(next_state_bytes) = next_state_bytes {
        operations.push(TransactionOp::AssertValue {
            key: CONTEXT_PROJECTION_STATE_KEY.into(),
            expected: old_state,
        });
        operations.push(TransactionOp::Put {
            key: CONTEXT_PROJECTION_STATE_KEY.into(),
            value: next_state_bytes,
        });
    }
    context
        .kernel
        .transact_durable(&context_namespace(), &operations)
        .map_err(|error| error.to_string())?;
    if state_changed {
        *state = next_state;
    }
    Ok((injection, resource))
}

fn require_active_execution(
    context: &ContextPluginContext<'_, '_>,
    execution_id: &str,
) -> Result<(), String> {
    let response = context
        .sdk
        .execution
        .invoke_projected::<ExecutionCommand, ExecutionResponse>(&ExecutionCommand::GetExecution {
            id: execution_id.to_owned(),
        })
        .map_err(|error| error.to_string())?;
    match response {
        ExecutionResponse::ExecutionLookup {
            execution: Some(execution),
        } if execution.state == ExecutionState::Active => Ok(()),
        ExecutionResponse::ExecutionLookup {
            execution: Some(execution),
        } => Err(format!(
            "context target execution is not active: {execution_id} ({:?})",
            execution.state
        )),
        ExecutionResponse::ExecutionLookup { execution: None } => {
            Err(format!("unknown context target execution: {execution_id}"))
        }
        other => Err(format!("unexpected execution lookup response: {other:?}")),
    }
}

fn project_context(
    context: &ContextPluginContext<'_, '_>,
    execution_id: String,
) -> Result<ExecutionContextProjection, String> {
    validate_identity("execution id", &execution_id)?;
    let injections =
        decode_injections(read_raw(context, &injections_key(&execution_id))?.as_deref())?;
    let entries = injections
        .into_iter()
        .map(|injection| {
            let resource = read_resource(
                context,
                &injection.source.resource_id,
                &injection.source.revision,
            )?
            .ok_or_else(|| {
                format!(
                    "missing durable context revision: {}@{}",
                    injection.source.resource_id, injection.source.revision
                )
            })?;
            Ok(ProjectedContextEntry {
                injection,
                resource,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    Ok(ExecutionContextProjection {
        execution_id,
        entries,
    })
}

fn read_resource(
    context: &ContextPluginContext<'_, '_>,
    resource_id: &ContextResourceId,
    revision: &ContextRevisionId,
) -> Result<Option<ContextResourceRevision>, String> {
    read_raw(context, &resource_key(resource_id, revision))?
        .map(|value| serde_json::from_slice(&value).map_err(|error| error.to_string()))
        .transpose()
}

fn list_descriptors(
    context: &ContextPluginContext<'_, '_>,
) -> Result<Vec<ContextDescriptor>, String> {
    let refs = decode_refs(read_raw(context, ALL_RESOURCES_KEY)?.as_deref())?;
    refs.into_iter()
        .map(|reference| {
            read_resource(context, &reference.resource_id, &reference.revision)?
                .map(|resource| resource.descriptor)
                .ok_or_else(|| {
                    format!(
                        "missing durable context revision: {}@{}",
                        reference.resource_id, reference.revision
                    )
                })
        })
        .collect()
}

fn read_raw(context: &ContextPluginContext<'_, '_>, key: &str) -> Result<Option<Vec<u8>>, String> {
    context
        .kernel
        .read_durable(&context_namespace(), key)
        .map_err(|error| error.to_string())
}

fn decode_refs(value: Option<&[u8]>) -> Result<Vec<ExactContextReference>, String> {
    value
        .map(|value| serde_json::from_slice(value).map_err(|error| error.to_string()))
        .unwrap_or_else(|| Ok(Vec::new()))
}

fn decode_injections(value: Option<&[u8]>) -> Result<Vec<ContextInjection>, String> {
    value
        .map(|value| serde_json::from_slice(value).map_err(|error| error.to_string()))
        .unwrap_or_else(|| Ok(Vec::new()))
}

fn resource_key(resource_id: &ContextResourceId, revision: &ContextRevisionId) -> String {
    format!("resource/{resource_id}/{revision}")
}

fn injections_key(execution_id: &str) -> String {
    format!("injections/{execution_id}")
}

fn validate_identity(label: &str, value: &str) -> Result<(), String> {
    if value.trim().is_empty() {
        Err(format!("{label} must not be empty"))
    } else {
        Ok(())
    }
}

fn content_hash(content: &[u8]) -> ContextRevisionId {
    let digest = Sha256::digest(content);
    let value: String = digest.iter().map(|byte| format!("{byte:02x}")).collect();
    ContextRevisionId::parse(value).expect("sha256 hex is a valid context revision")
}

fn file_name(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

fn parent_path(path: &str) -> Option<&str> {
    path.rsplit_once('/').map(|(parent, _)| parent)
}
