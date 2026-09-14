use crate::{
    agent_loop_component_id, agent_loop_component_manifest, agent_loop_service,
    execution_component_manifest, execution_factory, execution_manifest, AgentLoopCommand,
    AgentLoopResponse, AgentLoopUsage,
};
use phenix_core::{
    Authority, Bytes, ComponentExport, ComponentId, ComponentInterface, ComponentManifest, Kernel,
    KernelError, ModelId, ModelInferenceResponse, ModelToolCall, ModelToolDescriptor, PhenixSchema,
    PhenixValue, PluginContext, PluginExecution, PluginHost, PluginId, PluginInstance,
    PluginManifest, Project, ResolvedHarness, ResolvedHarnessActivation, RoutingProfileId,
    ServiceContribution, ServiceId, ServiceRole,
};
use phenix_sdk::{
    ModelCommand, ModelResponse, ModelRoutingInterface, ModelTarget, MODEL_ROUTING_SERVICE,
};
use std::collections::BTreeMap;

const MODEL_PROVIDER: &str = "fixture.agent-loop-model";
const MODEL_PROVIDER_COMPONENT: &str = "fixture.agent-loop-model";

struct ModelProvider;

impl PluginInstance for ModelProvider {
    fn start(&mut self, _host: &PluginHost<'_>) -> Result<(), String> {
        Ok(())
    }

    fn invoke(
        &mut self,
        service: &ServiceId,
        input: &[u8],
        host: &PluginHost<'_>,
    ) -> Result<Vec<u8>, String> {
        if service.as_str() != MODEL_ROUTING_SERVICE {
            return Err(format!("unsupported model routing service: {service}"));
        }
        let context = PluginContext::new(host, (), (), ());
        let ModelCommand::Invoke { tools, .. } = context
            .kernel
            .decode_projected::<ModelCommand>(&ModelRoutingInterface::interface_id(), input)
            .map_err(|error| error.to_string())?
        else {
            return Err("fixture model provider received a non-inference command".into());
        };
        context
            .kernel
            .encode_value(&ModelResponse::Inference {
                target: ModelTarget {
                    provider_plugin: provider_id(),
                    model: ModelId::parse("fixture").unwrap(),
                    options: BTreeMap::new(),
                },
                response: ModelInferenceResponse {
                    output: Bytes::new(b"provider-output".to_vec()),
                    provider_metadata: BTreeMap::new(),
                    tool_calls: tools
                        .first()
                        .map(|tool| ModelToolCall {
                            call_id: "fixture-call".into(),
                            callable_id: tool.id.clone(),
                            input: PhenixValue::String("fixture-input".into()),
                        })
                        .into_iter()
                        .collect(),
                },
            })
            .map_err(|error| error.to_string())
    }
}

fn provider_id() -> PluginId {
    PluginId::parse(MODEL_PROVIDER).unwrap()
}

fn provider_component_id() -> ComponentId {
    ComponentId::parse(MODEL_PROVIDER_COMPONENT).unwrap()
}

fn provider_manifest() -> PluginManifest {
    PluginManifest {
        id: provider_id(),
        version: 1,
        execution: PluginExecution::Embedded,
        dependencies: Vec::new(),
        services: vec![ServiceContribution {
            role: ServiceRole::Terminal,
            service: ServiceId::parse(MODEL_ROUTING_SERVICE).unwrap(),
            priority: 200,
            required_authority: Authority::default(),
        }],
        resource_namespaces: Vec::new(),
        maximum_authority: Authority::default(),
    }
}

fn provider_component() -> ComponentManifest {
    ComponentManifest {
        listeners: Vec::new(),
        id: provider_component_id(),
        owner: provider_id(),
        imports: Vec::new(),
        exports: vec![ComponentExport {
            interface: ModelRoutingInterface::interface_id(),
            schema: ModelRoutingInterface::schema(),
            priority: 200,
            required_authority: Authority::default(),
        }],
        maximum_authority: Authority::default(),
    }
}

fn resolved_harness(with_provider: bool) -> ResolvedHarness {
    let execution = execution_manifest(Authority::default());
    let ceiling = execution.maximum_authority.clone();
    let mut plugins = vec![execution];
    let mut components = vec![
        execution_component_manifest(Authority::default()),
        agent_loop_component_manifest(Authority::default()),
    ];
    if with_provider {
        plugins.push(provider_manifest());
        components.push(provider_component());
    }
    ResolvedHarness::resolve(plugins, components, [], &ceiling).unwrap()
}

fn kernel(with_provider: bool) -> (Kernel, PluginId) {
    let resolved = resolved_harness(with_provider);
    let execution = execution_manifest(Authority::default()).id;
    let mut kernel = Kernel::new(resolved.kernel_config().clone());
    kernel.activate_resolved_harness(&resolved).unwrap();
    kernel
        .register_embedded_factory(execution.clone(), execution_factory)
        .unwrap();
    if with_provider {
        kernel
            .register_embedded_factory(provider_id(), || Box::new(ModelProvider))
            .unwrap();
    }
    kernel.activate_all().unwrap();
    (kernel, execution)
}

fn invoke_agent_loop(kernel: &mut Kernel, execution: &PluginId) -> Result<Vec<u8>, KernelError> {
    let command = AgentLoopCommand::Run {
        profile_id: RoutingProfileId::parse("default").unwrap(),
        callable_id: None,
        input: Bytes::new(b"prompt".to_vec()),
        tools: Vec::new(),
    };
    kernel.invoke_component(
        &agent_loop_component_id(),
        &agent_loop_service(),
        &serde_json::to_vec(&PhenixValue::from(&command)).unwrap(),
        &Authority::default(),
        execution,
    )
}

#[test]
fn resolved_agent_loop_returns_model_output_with_usage() {
    let (mut kernel, execution) = kernel(true);
    let output = invoke_agent_loop(&mut kernel, &execution).unwrap();
    let output: PhenixValue = serde_json::from_slice(&output).unwrap();
    let response = AgentLoopResponse::try_from(Project(&output)).unwrap();

    assert_eq!(
        response,
        AgentLoopResponse::Completed {
            output: Bytes::new(b"provider-output".to_vec()),
            tool_calls: Vec::new(),
            usage: AgentLoopUsage {
                model_calls: 1,
                tool_calls: 0,
            },
        }
    );
}

#[test]
fn agent_loop_preserves_typed_model_tool_calls() {
    let (mut kernel, execution) = kernel(true);
    let command = AgentLoopCommand::Run {
        profile_id: RoutingProfileId::parse("default").unwrap(),
        callable_id: None,
        input: Bytes::new(b"prompt".to_vec()),
        tools: vec![ModelToolDescriptor {
            id: phenix_core::CallableId::parse("fixture.client.echo").unwrap(),
            description: "Echo fixture input".into(),
            input_schema: PhenixSchema::Any,
            output_schema: PhenixSchema::Any,
        }],
    };
    let output = kernel
        .invoke_component(
            &agent_loop_component_id(),
            &agent_loop_service(),
            &serde_json::to_vec(&PhenixValue::from(&command)).unwrap(),
            &Authority::default(),
            &execution,
        )
        .unwrap();
    let output: PhenixValue = serde_json::from_slice(&output).unwrap();
    let response = AgentLoopResponse::try_from(Project(&output)).unwrap();

    assert_eq!(
        response,
        AgentLoopResponse::Completed {
            output: Bytes::new(b"provider-output".to_vec()),
            tool_calls: vec![ModelToolCall {
                call_id: "fixture-call".into(),
                callable_id: phenix_core::CallableId::parse("fixture.client.echo").unwrap(),
                input: PhenixValue::String("fixture-input".into()),
            }],
            usage: AgentLoopUsage {
                model_calls: 1,
                tool_calls: 1,
            },
        }
    );
}

#[test]
fn agent_loop_without_model_provider_fails_at_optional_import_boundary() {
    let (mut kernel, execution) = kernel(false);
    match invoke_agent_loop(&mut kernel, &execution).unwrap_err() {
        KernelError::ServiceInvoke {
            plugin,
            service,
            message,
        } => {
            assert_eq!(plugin, execution);
            assert_eq!(service, agent_loop_service());
            assert_eq!(
                message,
                format!(
                    "component {} has no bound provider for optional import {}",
                    agent_loop_component_id(),
                    ModelRoutingInterface::interface_id()
                )
            );
        }
        error => panic!("unexpected error: {error}"),
    }
}
