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
        FrontendCommand::ReleaseRoot { execution_id } => {
            context.plugin.state.root_routes.remove(&execution_id);
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
                .remove(&correlation_id)
                .ok_or_else(|| format!("unknown frontend correlation id: {correlation_id}"))?;
            if pending.connection_id != connection_id {
                context.plugin.state.pending.insert(correlation_id, pending);
                return Err("frontend response came from the wrong connection".into());
            }
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
        match execution.parent_execution {
            Some(parent) => current = parent,
            None => return Ok(current),
        }
    }
}

fn validate_provider(provider: &FrontendProviderDescriptor) -> Result<(), String> {
    validate_id("frontend provider id", &provider.id)?;
    if provider
        .capabilities
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
