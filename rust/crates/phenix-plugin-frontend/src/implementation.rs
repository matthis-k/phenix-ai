use crate::{
    ExecutionCommand, ExecutionInterface, ExecutionResponse, ExecutionState, FrontendCommand,
    FrontendProviderDescriptor, FrontendResponse, FrontendServiceRequest, FrontendServiceResult,
    LiveFrontendProvider, frontend_component_id,
};
use phenix_core::{
    Authority, ComponentInterface, PhenixValue, PluginContext, PluginExecution, PluginHost,
    PluginId, PluginInstance, PluginManifest, SdkClient, ServiceContribution, ServiceId,
};
use std::collections::{BTreeMap, BTreeSet};

pub use phenix_sdk::FRONTEND_SERVICE;
const FRONTEND_PLUGIN: &str = "phenix.frontend-services";

#[must_use]
pub fn frontend_manifest(maximum_authority: Authority) -> PluginManifest {
    PluginManifest {
        id: PluginId::parse(FRONTEND_PLUGIN).expect("static plugin id is valid"),
        version: 1,
        execution: PluginExecution::Embedded,
        dependencies: Vec::new(),
        services: vec![ServiceContribution {
            role: phenix_core::ServiceRole::Terminal,
            service: frontend_service(),
            priority: 100,
            required_authority: Authority::default(),
        }],
        resource_namespaces: Vec::new(),
        maximum_authority,
    }
}

#[must_use]
pub fn frontend_factory() -> Box<dyn PluginInstance> {
    Box::new(FrontendPlugin::default())
}

#[must_use]
pub fn frontend_service() -> ServiceId {
    ServiceId::parse(FRONTEND_SERVICE).expect("static service id is valid")
}

#[derive(Clone, Debug)]
struct PendingCall {
    connection_id: String,
    descriptor: FrontendProviderDescriptor,
    execution_root: Option<String>,
    execution_id: Option<String>,
}

#[derive(Default)]
struct FrontendState {
    providers: BTreeMap<String, BTreeMap<String, FrontendProviderDescriptor>>,
    root_routes: BTreeMap<String, String>,
    pending: BTreeMap<u64, PendingCall>,
    next_correlation_id: u64,
}

struct FrontendSdk<'host, 'runtime> {
    execution: SdkClient<'host, 'runtime, ExecutionInterface>,
}

type FrontendContext<'host, 'runtime, 'state> =
    PluginContext<'host, 'runtime, FrontendSdk<'host, 'runtime>, (), &'state mut FrontendState>;

fn context<'host, 'runtime, 'state>(
    host: &'host PluginHost<'runtime>,
    state: &'state mut FrontendState,
) -> FrontendContext<'host, 'runtime, 'state> {
    PluginContext::new(
        host,
        FrontendSdk {
            execution: SdkClient::new(host, frontend_component_id()),
        },
        (),
        state,
    )
}

#[derive(Default)]
struct FrontendPlugin {
    state: FrontendState,
}

impl PluginInstance for FrontendPlugin {
    fn start(&mut self, host: &PluginHost<'_>) -> Result<(), String> {
        context(host, &mut self.state)
            .plugin
            .state
            .next_correlation_id = 1;
        Ok(())
    }

    fn invoke(
        &mut self,
        service: &ServiceId,
        input: &[u8],
        host: &PluginHost<'_>,
    ) -> Result<Vec<u8>, String> {
        if service != &frontend_service() {
            return Err(format!("unsupported frontend service: {service}"));
        }
        let mut context = context(host, &mut self.state);
        let interface = crate::FrontendInterface::interface_id();
        let command = context
            .kernel
            .decode_projected::<FrontendCommand>(&interface, input)
            .map_err(|error| error.to_string())?;
        let response = handle(&mut context, command)?;
        context
            .kernel
            .encode_value(&response)
            .map_err(|error| error.to_string())
    }
}

fn handle(
    context: &mut FrontendContext<'_, '_, '_>,
    command: FrontendCommand,
) -> Result<FrontendResponse, String> {
    match command {
        FrontendCommand::SetProviders {
            connection_id,
            providers,
        } => {
            validate_id("frontend connection id", &connection_id)?;
            let mut indexed = BTreeMap::new();
            for provider in providers {
                validate_provider(&provider)?;
                if indexed.insert(provider.id.clone(), provider).is_some() {
                    return Err("duplicate frontend provider id in advertisement".into());
                }
            }
            let state = &mut context.plugin.state;
            // An in-flight reply is only valid while its advertised contract
            // remains unchanged. A renderer withdrawal cannot complete a call
            // from its previous capability epoch.
            state.pending.retain(|_, pending| {
                pending.connection_id != connection_id
                    || indexed.get(&pending.descriptor.id) == Some(&pending.descriptor)
            });
            state.providers.insert(connection_id, indexed);
            Ok(FrontendResponse::Updated)
        }
        FrontendCommand::Disconnect { connection_id } => {
            let state = &mut context.plugin.state;
            state.providers.remove(&connection_id);
            state.root_routes.retain(|_, owner| owner != &connection_id);
            state
                .pending
                .retain(|_, call| call.connection_id != connection_id);
            Ok(FrontendResponse::Updated)
        }
        FrontendCommand::Catalog => Ok(FrontendResponse::Providers {
            providers: catalog(context.plugin.state),
        }),
        FrontendCommand::BindRoot {
            execution_id,
            connection_id,
        } => {
            validate_id("execution id", &execution_id)?;
            if !context.plugin.state.providers.contains_key(&connection_id) {
                return Err(format!("unknown frontend connection: {connection_id}"));
            }
            let execution = execution_lookup(context, &execution_id)?
                .ok_or_else(|| format!("unknown execution: {execution_id}"))?;
            if execution.parent_execution.is_some() {
                return Err("only root executions may be bound to a frontend connection".into());
            }
            if !matches!(execution.state, ExecutionState::Active) {
                return Err("only active root executions may be bound".into());
            }
            if context.plugin.state.root_routes.contains_key(&execution_id) {
                return Err(format!(
                    "root execution already has a frontend route: {execution_id}"
                ));
            }
            context
                .plugin
                .state
                .root_routes
                .insert(execution_id, connection_id);
            Ok(FrontendResponse::Updated)
        }
        FrontendCommand::ReleaseRoot {
            execution_id,
            connection_id,
        } => {
            let state = &mut context.plugin.state;
            if let Some(owner) = state.root_routes.get(&execution_id) {
                if owner != &connection_id {
                    return Err("frontend root release came from the wrong connection".into());
                }
            }
            state.root_routes.remove(&execution_id);
            state
                .pending
                .retain(|_, call| call.execution_root.as_deref() != Some(&execution_id));
            Ok(FrontendResponse::Updated)
        }
        FrontendCommand::BeginExecutionCall {
            execution_id,
            provider,
            method,
            params,
        } => {
            let root = execution_root(context, &execution_id)?;
            let connection_id = context
                .plugin
                .state
                .root_routes
                .get(&root)
                .cloned()
                .ok_or_else(|| {
                    format!("execution has no live frontend root route: {execution_id}")
                })?;
            begin_call(
                context.plugin.state,
                connection_id,
                provider,
                method,
                params,
                &BTreeSet::new(),
                Some(root),
                Some(execution_id),
            )
        }
        FrontendCommand::CheckExecutionCapabilities {
            execution_id,
            provider,
            required_capabilities,
        } => {
            validate_id("frontend provider id", &provider)?;
            validate_capabilities(&required_capabilities)?;
            let root = execution_root(context, &execution_id)?;
            let supported = context
                .plugin
                .state
                .root_routes
                .get(&root)
                .and_then(|connection| context.plugin.state.providers.get(connection))
                .and_then(|providers| providers.get(&provider))
                .is_some_and(|descriptor| {
                    required_capabilities.is_subset(&descriptor.capabilities)
                });
            Ok(FrontendResponse::CapabilityCheck { supported })
        }
        FrontendCommand::BeginExecutionCallWithRequirements {
            execution_id,
            provider,
            method,
            params,
            required_capabilities,
        } => {
            validate_capabilities(&required_capabilities)?;
            let root = execution_root(context, &execution_id)?;
            let connection_id = context
                .plugin
                .state
                .root_routes
                .get(&root)
                .cloned()
                .ok_or_else(|| {
                    format!("execution has no live frontend root route: {execution_id}")
                })?;
            begin_call(
                context.plugin.state,
                connection_id,
                provider,
                method,
                params,
                &required_capabilities,
                Some(root),
                Some(execution_id),
            )
        }
        FrontendCommand::BeginDirectCall {
            connection_id,
            provider,
            method,
            params,
        } => begin_call(
            context.plugin.state,
            connection_id,
            provider,
            method,
            params,
            &BTreeSet::new(),
            None,
            None,
        ),
        FrontendCommand::CompleteCall {
            connection_id,
            correlation_id,
            result,
        } => {
            let pending = context
                .plugin
                .state
                .pending
                .get(&correlation_id)
                .cloned()
                .ok_or_else(|| format!("unknown frontend correlation id: {correlation_id}"))?;
            if pending.connection_id != connection_id {
                return Err("frontend response came from the wrong connection".into());
            }
            if let Some(execution_id) = pending.execution_id.as_deref() {
                let active_owner = execution_root(context, execution_id).is_ok_and(|root| {
                    context.plugin.state.root_routes.get(&root) == Some(&connection_id)
                });
                if !active_owner {
                    context.plugin.state.pending.remove(&correlation_id);
                    return Err(
                        "execution-scoped frontend call is no longer active or owned".into(),
                    );
                }
            }
            context.plugin.state.pending.remove(&correlation_id);
            Ok(FrontendResponse::Result {
                result: FrontendServiceResult {
                    correlation_id,
                    result,
                },
            })
        }
    }
}

fn catalog(state: &FrontendState) -> Vec<LiveFrontendProvider> {
    let mut result = Vec::new();
    for (connection_id, providers) in &state.providers {
        for descriptor in providers.values() {
            result.push(LiveFrontendProvider {
                connection_id: connection_id.clone(),
                descriptor: descriptor.clone(),
            });
        }
    }
    result
}

fn begin_call(
    state: &mut FrontendState,
    connection_id: String,
    provider: String,
    method: String,
    params: PhenixValue,
    required_capabilities: &BTreeSet<String>,
    execution_root: Option<String>,
    execution_id: Option<String>,
) -> Result<FrontendResponse, String> {
    validate_id("frontend connection id", &connection_id)?;
    validate_id("frontend provider id", &provider)?;
    validate_id("frontend method", &method)?;
    let descriptor = state
        .providers
        .get(&connection_id)
        .and_then(|providers| providers.get(&provider))
        .cloned()
        .ok_or_else(|| {
            format!("frontend connection {connection_id} does not advertise provider {provider}")
        })?;
    if !required_capabilities.is_subset(&descriptor.capabilities) {
        return Err(format!(
            "frontend provider {provider} lacks required capabilities on connection {connection_id}"
        ));
    }
    let correlation_id = state.next_correlation_id;
    state.next_correlation_id = state
        .next_correlation_id
        .checked_add(1)
        .ok_or_else(|| "frontend correlation id exhausted".to_owned())?;
    state.pending.insert(
        correlation_id,
        PendingCall {
            connection_id: connection_id.clone(),
            descriptor,
            execution_root,
            execution_id,
        },
    );
    Ok(FrontendResponse::Request {
        request: FrontendServiceRequest {
            correlation_id,
            connection_id,
            provider,
            method,
            params,
        },
    })
}

fn execution_lookup(
    context: &FrontendContext<'_, '_, '_>,
    execution_id: &str,
) -> Result<Option<crate::ExecutionRecord>, String> {
    let response = context
        .sdk
        .execution
        .invoke_projected::<ExecutionCommand, ExecutionResponse>(&ExecutionCommand::GetExecution {
            id: execution_id.to_owned(),
        })
        .map_err(|error| error.to_string())?;
    match response {
        ExecutionResponse::ExecutionLookup { execution } => Ok(execution),
        other => Err(format!("unexpected execution lookup response: {other:?}")),
    }
}

fn execution_root(
    context: &FrontendContext<'_, '_, '_>,
    execution_id: &str,
) -> Result<String, String> {
    let mut current = execution_id.to_owned();
    let mut visited = BTreeSet::new();
    loop {
        if !visited.insert(current.clone()) {
            return Err("execution parent cycle while resolving frontend route".into());
        }
        let execution = execution_lookup(context, &current)?
            .ok_or_else(|| format!("unknown execution: {current}"))?;
        // Routing is scoped to a live execution, not just a retained root
        // binding. Reject completed descendants and completed ancestors.
        if !matches!(execution.state, ExecutionState::Active) {
            return Err(format!(
                "frontend call requires an active execution: {current}"
            ));
        }
        match execution.parent_execution {
            Some(parent) => current = parent,
            None => return Ok(current),
        }
    }
}

fn validate_provider(provider: &FrontendProviderDescriptor) -> Result<(), String> {
    validate_id("frontend provider id", &provider.id)?;
    validate_capabilities(&provider.capabilities)
}

fn validate_capabilities(capabilities: &BTreeSet<String>) -> Result<(), String> {
    if capabilities
        .iter()
        .any(|capability| capability.trim().is_empty())
    {
        return Err("frontend provider capabilities must not be empty strings".into());
    }
    Ok(())
}

fn validate_id(label: &str, value: &str) -> Result<(), String> {
    if value.trim().is_empty() {
        Err(format!("{label} must not be empty"))
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use phenix_core::{
        Kernel, KernelConfig, PhenixValue, Project, ResolvedGeneration,
        ResolvedGenerationActivation,
    };
    use phenix_plugin_execution::{
        execution_component_manifest, execution_factory, execution_manifest,
    };
    use phenix_sdk::{ExecutionAuthority, execution_service};

    fn kernel() -> Kernel {
        let execution_manifest = execution_manifest(Authority::default());
        let execution_id = execution_manifest.id.clone();
        let authority = execution_manifest.maximum_authority.clone();
        let frontend_manifest = frontend_manifest(authority.clone());
        let frontend_id = frontend_manifest.id.clone();
        let resolved = ResolvedGeneration::resolve(
            [execution_manifest.clone(), frontend_manifest.clone()],
            [
                execution_component_manifest(authority.clone()),
                crate::frontend_component_manifest(authority.clone()),
            ],
            [],
            &authority,
        )
        .unwrap();
        let mut kernel =
            Kernel::new(KernelConfig::new([execution_manifest, frontend_manifest]).unwrap());
        kernel.activate_resolved_generation(&resolved).unwrap();
        kernel
            .register_embedded_factory(execution_id, execution_factory)
            .unwrap();
        kernel
            .register_embedded_factory(frontend_id, frontend_factory)
            .unwrap();
        kernel.activate_all().unwrap();
        kernel
    }

    fn suite_authority() -> Authority {
        execution_manifest(Authority::default()).maximum_authority
    }

    fn invoke(kernel: &mut Kernel, command: FrontendCommand) -> Result<FrontendResponse, String> {
        let output = kernel
            .invoke(
                &frontend_service(),
                &serde_json::to_vec(&PhenixValue::from(&command)).unwrap(),
                &suite_authority(),
                None,
            )
            .map_err(|error| error.to_string())?;
        let output: PhenixValue =
            serde_json::from_slice(&output).map_err(|error| error.to_string())?;
        FrontendResponse::try_from(Project(&output)).map_err(|error| error.to_string())
    }

    fn execution(kernel: &mut Kernel, id: &str, parent: Option<&str>) {
        let command = match parent {
            None => ExecutionCommand::CreateExecution {
                id: id.into(),
                requested_authority: ExecutionAuthority::new(Vec::<String>::new()),
            },
            Some(parent) => ExecutionCommand::DelegateExecution {
                parent_execution: parent.into(),
                id: id.into(),
                requested_authority: ExecutionAuthority::new(Vec::<String>::new()),
            },
        };
        kernel
            .invoke(
                &execution_service(),
                &serde_json::to_vec(&phenix_core::PhenixValue::from(&command)).unwrap(),
                &suite_authority(),
                None,
            )
            .unwrap();
    }

    #[test]
    fn descendant_calls_route_to_root_owner_and_wrong_connection_response_is_rejected() {
        let mut kernel = kernel();
        execution(&mut kernel, "root", None);
        execution(&mut kernel, "child", Some("root"));
        invoke(
            &mut kernel,
            FrontendCommand::SetProviders {
                connection_id: "frontend-a".into(),
                providers: vec![FrontendProviderDescriptor {
                    id: "web".into(),
                    capabilities: BTreeSet::from(["search".into()]),
                }],
            },
        )
        .unwrap();
        invoke(
            &mut kernel,
            FrontendCommand::BindRoot {
                execution_id: "root".into(),
                connection_id: "frontend-a".into(),
            },
        )
        .unwrap();
        let response = invoke(
            &mut kernel,
            FrontendCommand::BeginExecutionCall {
                execution_id: "child".into(),
                provider: "web".into(),
                method: "search".into(),
                params: serde_json::json!({"q":"nix"}).into(),
            },
        )
        .unwrap();
        let correlation_id = match response {
            FrontendResponse::Request { request } => {
                assert_eq!(request.connection_id, "frontend-a");
                request.correlation_id
            }
            other => panic!("unexpected response: {other:?}"),
        };
        assert!(
            invoke(
                &mut kernel,
                FrontendCommand::CompleteCall {
                    connection_id: "frontend-b".into(),
                    correlation_id,
                    result: serde_json::json!({}).into(),
                }
            )
            .unwrap_err()
            .contains("wrong connection")
        );
        assert!(matches!(
            invoke(
                &mut kernel,
                FrontendCommand::CompleteCall {
                    connection_id: "frontend-a".into(),
                    correlation_id,
                    result: serde_json::json!({"ok":true}).into(),
                }
            )
            .unwrap(),
            FrontendResponse::Result { .. }
        ));
    }

    #[test]
    fn terminal_executions_cannot_use_retained_frontend_routes() {
        let mut kernel = kernel();
        execution(&mut kernel, "root", None);
        execution(&mut kernel, "child", Some("root"));
        invoke(
            &mut kernel,
            FrontendCommand::SetProviders {
                connection_id: "frontend-a".into(),
                providers: vec![FrontendProviderDescriptor {
                    id: "document".into(),
                    capabilities: BTreeSet::from(["document.v1".into()]),
                }],
            },
        )
        .unwrap();
        invoke(
            &mut kernel,
            FrontendCommand::BindRoot {
                execution_id: "root".into(),
                connection_id: "frontend-a".into(),
            },
        )
        .unwrap();

        let call = |kernel: &mut Kernel, execution_id: &str| {
            invoke(
                kernel,
                FrontendCommand::BeginExecutionCall {
                    execution_id: execution_id.into(),
                    provider: "document".into(),
                    method: "present".into(),
                    params: serde_json::json!({}).into(),
                },
            )
        };
        let child_correlation = match call(&mut kernel, "child").unwrap() {
            FrontendResponse::Request { request } => request.correlation_id,
            other => panic!("expected frontend request, got {other:?}"),
        };

        let finish = |kernel: &mut Kernel, execution_id: &str| {
            kernel
                .invoke(
                    &execution_service(),
                    &serde_json::to_vec(&PhenixValue::from(&ExecutionCommand::FinishExecution {
                        id: execution_id.into(),
                        success: true,
                    }))
                    .unwrap(),
                    &suite_authority(),
                    None,
                )
                .unwrap();
        };
        finish(&mut kernel, "child");
        assert!(
            invoke(
                &mut kernel,
                FrontendCommand::CompleteCall {
                    connection_id: "frontend-a".into(),
                    correlation_id: child_correlation,
                    result: serde_json::json!({}).into(),
                },
            )
            .unwrap_err()
            .contains("no longer active or owned"),
            "late reply to a finished child must be rejected"
        );
        assert!(
            call(&mut kernel, "child")
                .unwrap_err()
                .contains("active execution: child"),
            "a finished child must not issue frontend requests"
        );

        finish(&mut kernel, "root");
        assert!(
            call(&mut kernel, "root")
                .unwrap_err()
                .contains("active execution: root"),
            "a retained root binding must not authorize a finished execution"
        );

        execution(&mut kernel, "second-root", None);
        execution(&mut kernel, "active-child", Some("second-root"));
        invoke(
            &mut kernel,
            FrontendCommand::BindRoot {
                execution_id: "second-root".into(),
                connection_id: "frontend-a".into(),
            },
        )
        .unwrap();
        finish(&mut kernel, "second-root");
        assert!(
            call(&mut kernel, "active-child")
                .unwrap_err()
                .contains("active execution: second-root"),
            "an active child cannot outlive the root frontend binding"
        );
    }

    #[test]
    fn duplicate_root_binding_must_not_steal_existing_frontend_owner() {
        let mut kernel = kernel();
        execution(&mut kernel, "root", None);
        for connection in ["frontend-a", "frontend-b"] {
            invoke(
                &mut kernel,
                FrontendCommand::SetProviders {
                    connection_id: connection.into(),
                    providers: vec![FrontendProviderDescriptor {
                        id: "document".into(),
                        capabilities: BTreeSet::from(["render.v1".into()]),
                    }],
                },
            )
            .unwrap();
        }

        invoke(
            &mut kernel,
            FrontendCommand::BindRoot {
                execution_id: "root".into(),
                connection_id: "frontend-a".into(),
            },
        )
        .unwrap();
        let error = invoke(
            &mut kernel,
            FrontendCommand::BindRoot {
                execution_id: "root".into(),
                connection_id: "frontend-b".into(),
            },
        )
        .unwrap_err();
        assert!(error.contains("already has a frontend route"), "{error}");

        let response = invoke(
            &mut kernel,
            FrontendCommand::BeginExecutionCall {
                execution_id: "root".into(),
                provider: "document".into(),
                method: "render".into(),
                params: serde_json::json!({}).into(),
            },
        )
        .unwrap();
        assert!(matches!(
            response,
            FrontendResponse::Request { request } if request.connection_id == "frontend-a"
        ));

        invoke(
            &mut kernel,
            FrontendCommand::Disconnect {
                connection_id: "frontend-b".into(),
            },
        )
        .unwrap();
        assert!(matches!(
            invoke(
                &mut kernel,
                FrontendCommand::BeginExecutionCall {
                    execution_id: "root".into(),
                    provider: "document".into(),
                    method: "render".into(),
                    params: serde_json::json!({}).into(),
                },
            )
            .unwrap(),
            FrontendResponse::Request { request } if request.connection_id == "frontend-a"
        ));
    }

    #[test]
    fn capability_checks_and_guarded_calls_use_only_the_active_root_owner() {
        let mut kernel = kernel();
        execution(&mut kernel, "root", None);
        execution(&mut kernel, "child", Some("root"));
        let required = BTreeSet::from(["document.v1".to_owned()]);
        for (connection, capabilities) in [
            ("frontend-a", BTreeSet::new()),
            ("frontend-b", required.clone()),
        ] {
            invoke(
                &mut kernel,
                FrontendCommand::SetProviders {
                    connection_id: connection.into(),
                    providers: vec![FrontendProviderDescriptor {
                        id: "document".into(),
                        capabilities,
                    }],
                },
            )
            .unwrap();
        }
        invoke(
            &mut kernel,
            FrontendCommand::BindRoot {
                execution_id: "root".into(),
                connection_id: "frontend-a".into(),
            },
        )
        .unwrap();

        let check = |kernel: &mut Kernel| {
            invoke(
                kernel,
                FrontendCommand::CheckExecutionCapabilities {
                    execution_id: "child".into(),
                    provider: "document".into(),
                    required_capabilities: required.clone(),
                },
            )
        };
        let begin = |kernel: &mut Kernel| {
            invoke(
                kernel,
                FrontendCommand::BeginExecutionCallWithRequirements {
                    execution_id: "child".into(),
                    provider: "document".into(),
                    method: "present".into(),
                    params: serde_json::json!({}).into(),
                    required_capabilities: required.clone(),
                },
            )
        };

        assert_eq!(
            check(&mut kernel).unwrap(),
            FrontendResponse::CapabilityCheck { supported: false },
            "another frontend cannot advertise capabilities for this execution"
        );
        assert!(
            begin(&mut kernel)
                .unwrap_err()
                .contains("lacks required capabilities")
        );

        invoke(
            &mut kernel,
            FrontendCommand::SetProviders {
                connection_id: "frontend-a".into(),
                providers: vec![FrontendProviderDescriptor {
                    id: "document".into(),
                    capabilities: required.clone(),
                }],
            },
        )
        .unwrap();
        assert_eq!(
            check(&mut kernel).unwrap(),
            FrontendResponse::CapabilityCheck { supported: true }
        );
        let correlation_id = match begin(&mut kernel).unwrap() {
            FrontendResponse::Request { request } => {
                assert_eq!(request.connection_id, "frontend-a");
                request.correlation_id
            }
            other => panic!("expected a request, got {other:?}"),
        };

        invoke(
            &mut kernel,
            FrontendCommand::SetProviders {
                connection_id: "frontend-a".into(),
                providers: vec![FrontendProviderDescriptor {
                    id: "document".into(),
                    capabilities: BTreeSet::new(),
                }],
            },
        )
        .unwrap();
        assert_eq!(
            check(&mut kernel).unwrap(),
            FrontendResponse::CapabilityCheck { supported: false }
        );
        assert!(
            invoke(
                &mut kernel,
                FrontendCommand::CompleteCall {
                    connection_id: "frontend-a".into(),
                    correlation_id,
                    result: serde_json::json!({}).into(),
                },
            )
            .unwrap_err()
            .contains("unknown frontend correlation"),
            "capability withdrawal must revoke pending UI results"
        );

        invoke(
            &mut kernel,
            FrontendCommand::Disconnect {
                connection_id: "frontend-a".into(),
            },
        )
        .unwrap();
        assert_eq!(
            check(&mut kernel).unwrap(),
            FrontendResponse::CapabilityCheck { supported: false }
        );
        assert!(
            begin(&mut kernel)
                .unwrap_err()
                .contains("no live frontend root route")
        );
    }

    #[test]
    fn provider_replacement_revokes_stale_pending_calls_without_affecting_identical_contracts() {
        let mut kernel = kernel();
        execution(&mut kernel, "root", None);
        let advertise = |kernel: &mut Kernel, capabilities: BTreeSet<String>| {
            invoke(
                kernel,
                FrontendCommand::SetProviders {
                    connection_id: "frontend-a".into(),
                    providers: vec![FrontendProviderDescriptor {
                        id: "document".into(),
                        capabilities,
                    }],
                },
            )
            .unwrap();
        };
        advertise(&mut kernel, BTreeSet::from(["document.v1".into()]));
        invoke(
            &mut kernel,
            FrontendCommand::BindRoot {
                execution_id: "root".into(),
                connection_id: "frontend-a".into(),
            },
        )
        .unwrap();

        let begin = |kernel: &mut Kernel| {
            let response = invoke(
                kernel,
                FrontendCommand::BeginExecutionCall {
                    execution_id: "root".into(),
                    provider: "document".into(),
                    method: "present".into(),
                    params: serde_json::json!({}).into(),
                },
            )
            .unwrap();
            let FrontendResponse::Request { request } = response else {
                panic!("expected pending frontend call");
            };
            request.correlation_id
        };
        let first = begin(&mut kernel);
        advertise(&mut kernel, BTreeSet::from(["document.v1".into()]));
        assert!(matches!(
            invoke(
                &mut kernel,
                FrontendCommand::CompleteCall {
                    connection_id: "frontend-a".into(),
                    correlation_id: first,
                    result: serde_json::json!({"ok": true}).into(),
                },
            )
            .unwrap(),
            FrontendResponse::Result { .. }
        ));

        let second = begin(&mut kernel);
        advertise(&mut kernel, BTreeSet::from(["document.v2".into()]));
        assert!(
            invoke(
                &mut kernel,
                FrontendCommand::CompleteCall {
                    connection_id: "frontend-a".into(),
                    correlation_id: second,
                    result: serde_json::json!({"ok": true}).into(),
                },
            )
            .unwrap_err()
            .contains("unknown frontend correlation")
        );

        let third = begin(&mut kernel);
        invoke(
            &mut kernel,
            FrontendCommand::SetProviders {
                connection_id: "frontend-a".into(),
                providers: Vec::new(),
            },
        )
        .unwrap();
        assert!(
            invoke(
                &mut kernel,
                FrontendCommand::CompleteCall {
                    connection_id: "frontend-a".into(),
                    correlation_id: third,
                    result: serde_json::json!({"ok": true}).into(),
                },
            )
            .unwrap_err()
            .contains("unknown frontend correlation")
        );
        assert!(
            invoke(
                &mut kernel,
                FrontendCommand::BeginExecutionCall {
                    execution_id: "root".into(),
                    provider: "document".into(),
                    method: "present".into(),
                    params: serde_json::json!({}).into(),
                },
            )
            .unwrap_err()
            .contains("does not advertise")
        );
    }

    #[test]
    fn releasing_root_revokes_only_its_pending_execution_calls() {
        let mut kernel = kernel();
        execution(&mut kernel, "root", None);
        invoke(
            &mut kernel,
            FrontendCommand::SetProviders {
                connection_id: "frontend-a".into(),
                providers: vec![FrontendProviderDescriptor {
                    id: "document".into(),
                    capabilities: BTreeSet::from(["document.v1".into()]),
                }],
            },
        )
        .unwrap();
        invoke(
            &mut kernel,
            FrontendCommand::BindRoot {
                execution_id: "root".into(),
                connection_id: "frontend-a".into(),
            },
        )
        .unwrap();

        let pending_execution = invoke(
            &mut kernel,
            FrontendCommand::BeginExecutionCallWithRequirements {
                execution_id: "root".into(),
                provider: "document".into(),
                method: "present".into(),
                params: serde_json::json!({}).into(),
                required_capabilities: BTreeSet::from(["document.v1".into()]),
            },
        )
        .unwrap();
        let pending_direct = invoke(
            &mut kernel,
            FrontendCommand::BeginDirectCall {
                connection_id: "frontend-a".into(),
                provider: "document".into(),
                method: "inspect".into(),
                params: serde_json::json!({}).into(),
            },
        )
        .unwrap();
        let correlation = |response| match response {
            FrontendResponse::Request { request } => request.correlation_id,
            other => panic!("expected request, got {other:?}"),
        };
        let execution_id = correlation(pending_execution);
        let direct_id = correlation(pending_direct);

        let wrong_owner = invoke(
            &mut kernel,
            FrontendCommand::ReleaseRoot {
                execution_id: "root".into(),
                connection_id: "frontend-b".into(),
            },
        )
        .unwrap_err();
        assert!(wrong_owner.contains("wrong connection"), "{wrong_owner}");
        invoke(
            &mut kernel,
            FrontendCommand::ReleaseRoot {
                execution_id: "root".into(),
                connection_id: "frontend-a".into(),
            },
        )
        .unwrap();
        assert!(
            invoke(
                &mut kernel,
                FrontendCommand::CompleteCall {
                    connection_id: "frontend-a".into(),
                    correlation_id: execution_id,
                    result: serde_json::json!({}).into(),
                },
            )
            .unwrap_err()
            .contains("unknown frontend correlation")
        );
        assert!(matches!(
            invoke(
                &mut kernel,
                FrontendCommand::CompleteCall {
                    connection_id: "frontend-a".into(),
                    correlation_id: direct_id,
                    result: serde_json::json!({}).into(),
                },
            )
            .unwrap(),
            FrontendResponse::Result { .. }
        ));
        assert_eq!(
            invoke(
                &mut kernel,
                FrontendCommand::CheckExecutionCapabilities {
                    execution_id: "root".into(),
                    provider: "document".into(),
                    required_capabilities: BTreeSet::from(["document.v1".into()]),
                },
            )
            .unwrap(),
            FrontendResponse::CapabilityCheck { supported: false }
        );
    }

    #[test]
    fn disconnect_removes_provider_catalog_routes_and_pending_calls_without_durable_restore() {
        let mut kernel = kernel();
        execution(&mut kernel, "root", None);
        invoke(
            &mut kernel,
            FrontendCommand::SetProviders {
                connection_id: "frontend-a".into(),
                providers: vec![FrontendProviderDescriptor {
                    id: "web".into(),
                    capabilities: BTreeSet::new(),
                }],
            },
        )
        .unwrap();
        invoke(
            &mut kernel,
            FrontendCommand::BindRoot {
                execution_id: "root".into(),
                connection_id: "frontend-a".into(),
            },
        )
        .unwrap();
        invoke(
            &mut kernel,
            FrontendCommand::Disconnect {
                connection_id: "frontend-a".into(),
            },
        )
        .unwrap();
        assert_eq!(
            invoke(&mut kernel, FrontendCommand::Catalog).unwrap(),
            FrontendResponse::Providers {
                providers: Vec::new()
            }
        );
        assert!(
            invoke(
                &mut kernel,
                FrontendCommand::BeginExecutionCall {
                    execution_id: "root".into(),
                    provider: "web".into(),
                    method: "search".into(),
                    params: serde_json::json!({}).into(),
                }
            )
            .unwrap_err()
            .contains("no live frontend root route")
        );
    }
}
