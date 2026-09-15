use crate::context_component_id;
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
    Box::new(ContextPlugin)
}

fn context_namespace() -> ResourceNamespace {
    ResourceNamespace::parse(CONTEXT_NAMESPACE).expect("static namespace is valid")
}

fn capability(value: &str) -> CapabilityId {
    CapabilityId::parse(value).expect("static capability is valid")
}

struct ContextPlugin;

impl PluginInstance for ContextPlugin {
    fn start(&mut self, host: &PluginHost<'_>) -> Result<(), String> {
        context(host)
            .kernel
            .register_durable_schema(&DurableSchema::new(context_namespace(), 1))
            .map_err(|error| error.to_string())
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
        let interface = ContextInterface::interface_id();
        let command = context
            .kernel
            .decode_projected::<ContextCommand>(&interface, input)
            .map_err(|error| error.to_string())?;
        let response = handle(&context, command)?;
        context
            .kernel
            .encode_value(&response)
            .map_err(|error| error.to_string())
    }
}

fn handle(
    context: &ContextPluginContext<'_, '_>,
    command: ContextCommand,
) -> Result<ContextResponse, String> {
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
    }
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
    let old = read_raw(context, &key)?;
    let mut injections = decode_injections(old.as_deref())?;
    let sequence = u64::try_from(injections.len())
        .map_err(|_| "context injection sequence overflow".to_owned())?
        + 1;
    let injection = ContextInjection {
        sequence,
        execution_id,
        source: ExactContextReference {
            resource_id,
            revision,
        },
        requester,
        lifetime,
        reason,
    };
    injections.push(injection.clone());
    context
        .kernel
        .transact_durable(
            &context_namespace(),
            &[
                TransactionOp::AssertValue {
                    key: key.clone(),
                    expected: old,
                },
                TransactionOp::Put {
                    key,
                    value: serde_json::to_vec(&injections).map_err(|error| error.to_string())?,
                },
            ],
        )
        .map_err(|error| error.to_string())?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use phenix_core::{
        Kernel, KernelConfig, LocalPersistence, PluginState, ResolvedHarness,
        ResolvedHarnessActivation,
    };
    use phenix_plugin_execution::{
        execution_component_manifest, execution_factory, execution_manifest,
    };
    use phenix_sdk::{execution_service, ExecutionAuthority};
    use std::{
        fs,
        path::PathBuf,
        time::{SystemTime, UNIX_EPOCH},
    };

    fn authority() -> Authority {
        context_manifest().maximum_authority
    }

    fn invoke(kernel: &mut Kernel, command: &ContextCommand) -> Result<ContextResponse, String> {
        let input = serde_json::to_vec(&phenix_core::PhenixValue::from(command)).unwrap();
        let output = kernel
            .invoke(&context_service(), &input, &authority(), None)
            .map_err(|error| error.to_string())?;
        let output: phenix_core::PhenixValue =
            serde_json::from_slice(&output).map_err(|error| error.to_string())?;
        output.project().map_err(|error| error.to_string())
    }

    fn kernel_with(path: &PathBuf) -> Kernel {
        let context_manifest = context_manifest();
        let context_plugin = context_manifest.id.clone();
        let execution_manifest = execution_manifest(authority());
        let execution_plugin = execution_manifest.id.clone();
        let resolved = ResolvedHarness::resolve(
            [execution_manifest.clone(), context_manifest.clone()],
            [
                execution_component_manifest(authority()),
                crate::context_component_manifest(),
            ],
            [],
            &authority(),
        )
        .unwrap();
        let persistence = LocalPersistence::open(path).unwrap();
        let mut kernel = Kernel::with_persistence(
            KernelConfig::new([execution_manifest, context_manifest]).unwrap(),
            persistence,
        );
        kernel.activate_resolved_harness(&resolved).unwrap();
        kernel
            .register_embedded_factory(execution_plugin.clone(), execution_factory)
            .unwrap();
        kernel
            .register_embedded_factory(context_plugin.clone(), context_factory)
            .unwrap();
        kernel.activate_all().unwrap();
        assert_eq!(kernel.state(&execution_plugin), Some(PluginState::Active));
        assert_eq!(kernel.state(&context_plugin), Some(PluginState::Active));
        kernel
    }

    fn create_execution(kernel: &mut Kernel, execution_id: &str) {
        let command = ExecutionCommand::CreateExecution {
            id: execution_id.to_owned(),
            requested_authority: ExecutionAuthority::new(Vec::<String>::new()),
        };
        kernel
            .invoke(
                &execution_service(),
                &serde_json::to_vec(&phenix_core::PhenixValue::from(&command)).unwrap(),
                &authority(),
                None,
            )
            .unwrap();
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

    fn discovered(response: ContextResponse) -> Vec<ContextDescriptor> {
        match response {
            ContextResponse::Discovered { descriptors } => descriptors,
            other => panic!("unexpected response: {other:?}"),
        }
    }

    #[test]
    fn exact_context_registration_load_projection_and_restore_are_durable() {
        let path = temp_db("context-restore");
        let descriptor;
        {
            let mut kernel = kernel_with(&path);
            let response = invoke(
                &mut kernel,
                &ContextCommand::Register {
                    resource_id: ContextResourceId::parse("skill:review").unwrap(),
                    kind: ContextResourceKind::Skill,
                    source: "skills/review/SKILL.md".into(),
                    scope: ContextScope::Workspace,
                    content: b"review exactly".to_vec().into(),
                },
            )
            .unwrap();
            descriptor = match response {
                ContextResponse::Registered { resource } => resource.descriptor,
                other => panic!("unexpected response: {other:?}"),
            };
            create_execution(&mut kernel, "exec-1");
            invoke(
                &mut kernel,
                &ContextCommand::Load {
                    execution_id: "exec-1".into(),
                    resource_id: descriptor.resource_id.clone(),
                    revision: descriptor.revision.clone(),
                    requester: ContextInjectionRequester::User,
                    lifetime: ContextInjectionLifetime::Execution,
                    reason: "explicit activation".into(),
                },
            )
            .unwrap();
        }

        let mut restored = kernel_with(&path);
        let response = invoke(
            &mut restored,
            &ContextCommand::Project {
                execution_id: "exec-1".into(),
            },
        )
        .unwrap();
        match response {
            ContextResponse::Projection { projection } => {
                assert_eq!(projection.entries.len(), 1);
                assert_eq!(projection.entries[0].resource.descriptor, descriptor);
                assert_eq!(
                    projection.entries[0].resource.content.as_ref(),
                    b"review exactly"
                );
            }
            other => panic!("unexpected response: {other:?}"),
        }
        let _ = fs::remove_file(path);
    }

    #[test]
    fn repository_discovery_preserves_scoped_project_file_behavior_and_exact_revisions() {
        let path = temp_db("context-discovery");
        let mut kernel = kernel_with(&path);
        let first = discovered(
            invoke(
                &mut kernel,
                &ContextCommand::DiscoverRepository {
                    workspace_id: "ws".into(),
                    sources: vec![
                        RepositoryContextSource {
                            path: "AGENTS.md".into(),
                            content: b"root rules".to_vec().into(),
                        },
                        RepositoryContextSource {
                            path: "crates/ui/AGENTS.override.md".into(),
                            content: b"ui rules".to_vec().into(),
                        },
                        RepositoryContextSource {
                            path: "CONTRIBUTING.md".into(),
                            content: b"contribute".to_vec().into(),
                        },
                        RepositoryContextSource {
                            path: "skills/review/SKILL.md".into(),
                            content: b"review skill".to_vec().into(),
                        },
                        RepositoryContextSource {
                            path: "README.md".into(),
                            content: b"not automatic context".to_vec().into(),
                        },
                    ],
                },
            )
            .unwrap(),
        );
        assert_eq!(first.len(), 4);
        assert!(first.iter().any(|descriptor| {
            descriptor.kind == ContextResourceKind::ProjectInstruction
                && descriptor.source == "AGENTS.md"
                && descriptor.scope == ContextScope::Workspace
        }));
        assert!(first.iter().any(|descriptor| {
            descriptor.kind == ContextResourceKind::ProjectInstruction
                && descriptor.source == "crates/ui/AGENTS.override.md"
                && descriptor.scope == ContextScope::PathPrefix("crates/ui".into())
        }));
        assert!(first.iter().any(|descriptor| {
            descriptor.kind == ContextResourceKind::Skill
                && descriptor.source == "skills/review/SKILL.md"
                && descriptor.scope == ContextScope::Workspace
        }));

        let contributing = first
            .iter()
            .find(|descriptor| descriptor.source == "CONTRIBUTING.md")
            .unwrap()
            .clone();
        let changed = discovered(
            invoke(
                &mut kernel,
                &ContextCommand::DiscoverRepository {
                    workspace_id: "ws".into(),
                    sources: vec![RepositoryContextSource {
                        path: "CONTRIBUTING.md".into(),
                        content: b"changed".to_vec().into(),
                    }],
                },
            )
            .unwrap(),
        );
        assert_ne!(changed[0].revision, contributing.revision);
        assert_eq!(changed[0].resource_id, contributing.resource_id);

        let agents = first
            .iter()
            .find(|descriptor| descriptor.source == "AGENTS.md")
            .unwrap();
        assert!(matches!(
            invoke(
                &mut kernel,
                &ContextCommand::Get {
                    resource_id: agents.resource_id.clone(),
                    revision: agents.revision.clone(),
                },
            )
            .unwrap(),
            ContextResponse::Resource { resource: Some(_) }
        ));
        let _ = fs::remove_file(path);
    }

    #[test]
    fn context_load_rejects_unknown_or_finished_execution() {
        let path = temp_db("context-execution-provenance");
        let mut kernel = kernel_with(&path);
        let registered = invoke(
            &mut kernel,
            &ContextCommand::Register {
                resource_id: ContextResourceId::parse("skill:bounded").unwrap(),
                kind: ContextResourceKind::Skill,
                source: "skills/bounded/SKILL.md".into(),
                scope: ContextScope::Workspace,
                content: b"bounded".to_vec().into(),
            },
        )
        .unwrap();
        let descriptor = match registered {
            ContextResponse::Registered { resource } => resource.descriptor,
            other => panic!("unexpected response: {other:?}"),
        };

        let unknown = invoke(
            &mut kernel,
            &ContextCommand::Load {
                execution_id: "missing".into(),
                resource_id: descriptor.resource_id.clone(),
                revision: descriptor.revision.clone(),
                requester: ContextInjectionRequester::User,
                lifetime: ContextInjectionLifetime::Execution,
                reason: "must be execution-bound".into(),
            },
        )
        .unwrap_err();
        assert!(unknown.contains("unknown context target execution"));

        create_execution(&mut kernel, "finished");
        kernel
            .invoke(
                &execution_service(),
                &serde_json::to_vec(&phenix_core::PhenixValue::from(
                    &ExecutionCommand::FinishExecution {
                        id: "finished".into(),
                        success: true,
                    },
                ))
                .unwrap(),
                &authority(),
                None,
            )
            .unwrap();
        let finished = invoke(
            &mut kernel,
            &ContextCommand::Load {
                execution_id: "finished".into(),
                resource_id: descriptor.resource_id,
                revision: descriptor.revision,
                requester: ContextInjectionRequester::User,
                lifetime: ContextInjectionLifetime::Execution,
                reason: "must still be active".into(),
            },
        )
        .unwrap_err();
        assert!(finished.contains("context target execution is not active"));
        let _ = fs::remove_file(path);
    }

    #[test]
    fn context_and_skill_activation_require_no_unrelated_caller_authority() {
        let path = temp_db("context-authority");
        let manifest = context_manifest();
        assert_eq!(
            manifest.services[0].required_authority,
            Authority::default()
        );
        assert!(!manifest
            .maximum_authority
            .permits(&capability("workspace.write")));
        assert!(!manifest
            .maximum_authority
            .permits(&capability("network.outbound")));

        let mut kernel = kernel_with(&path);
        let registered = invoke(
            &mut kernel,
            &ContextCommand::Register {
                resource_id: ContextResourceId::parse("skill:manual").unwrap(),
                kind: ContextResourceKind::Skill,
                source: "skills/manual/SKILL.md".into(),
                scope: ContextScope::Workspace,
                content: b"manual".to_vec().into(),
            },
        )
        .unwrap();
        assert!(matches!(registered, ContextResponse::Registered { .. }));
        let _ = fs::remove_file(path);
    }
}
