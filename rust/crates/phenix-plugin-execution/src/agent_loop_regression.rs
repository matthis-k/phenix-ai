use crate::{
    agent_loop_component_id, agent_loop_component_manifest, agent_loop_factory,
    agent_loop_manifest, agent_loop_service, execution_component_manifest, execution_factory,
    execution_manifest, AgentLoopCommand, AgentLoopResponse, AgentLoopUsage,
    ExecutionConfigurationCommand, ExecutionConfigurationInterface, ExecutionConfigurationResponse,
};
use phenix_core::{
    Authority, Bytes, ComponentExport, ComponentId, ComponentImport, ComponentInterface,
    ComponentManifest, Kernel, KernelError, ModelToolCall, ModelToolDescriptor, PhenixSchema,
    PhenixValue, PluginContext, PluginExecution, PluginHost, PluginId, PluginInstance,
    PluginManifest, Project, ResolvedHarness, ResolvedHarnessActivation, SdkClient,
    ServiceContribution, ServiceId, ServiceRole, SessionId,
};
use phenix_sdk::{
    default_invocation_service, AttemptOutcome, BudgetActual, ContextDemand,
    DefaultInvocationCommand, DefaultInvocationInterface, DelegationResourcePolicy, ExecutionState,
    RemainingBudget, StepAttemptRecord, StepRunnerResponse, StepSettlementBasis, TaskRequirements,
    UsageAttemptKind, UsageAttribution, UsagePlanningInput, UsagePolicy,
};
use std::collections::BTreeSet;

const INVOCATION_PROVIDER: &str = "fixture.agent-loop-invocation";
const INVOCATION_PROVIDER_COMPONENT: &str = "fixture.agent-loop-invocation";

struct InvocationProviderSdk<'host, 'runtime> {
    execution_configuration: SdkClient<'host, 'runtime, ExecutionConfigurationInterface>,
}

type InvocationProviderContext<'host, 'runtime> =
    PluginContext<'host, 'runtime, InvocationProviderSdk<'host, 'runtime>>;

fn invocation_provider_context<'host, 'runtime>(
    host: &'host PluginHost<'runtime>,
) -> InvocationProviderContext<'host, 'runtime> {
    PluginContext::new(
        host,
        InvocationProviderSdk {
            execution_configuration: SdkClient::new(host, provider_component_id()),
        },
        (),
        (),
    )
}

struct InvocationProvider;

impl PluginInstance for InvocationProvider {
    fn start(&mut self, _host: &PluginHost<'_>) -> Result<(), String> {
        Ok(())
    }

    fn invoke(
        &mut self,
        service: &ServiceId,
        input: &[u8],
        host: &PluginHost<'_>,
    ) -> Result<Vec<u8>, String> {
        if service != &default_invocation_service() {
            return Err(format!("unsupported default invocation service: {service}"));
        }
        let context = invocation_provider_context(host);
        let DefaultInvocationCommand::Invoke { request } = context
            .kernel
            .decode_projected::<DefaultInvocationCommand>(
                &DefaultInvocationInterface::interface_id(),
                input,
            )
            .map_err(|error| error.to_string())?;
        if request.execution_id != "execution-1" || request.parent_attempt_id.is_some() {
            return Err("agent loop changed invocation execution identity".into());
        }
        if request.session_id.as_ref().map(SessionId::as_str) != Some("session-1") {
            return Err("agent loop changed invocation session identity".into());
        }
        if request.input != Bytes::new(b"prompt".to_vec()) {
            return Err("agent loop changed invocation input".into());
        }
        let tool_calls = request
            .tools
            .first()
            .map(|tool| ModelToolCall {
                call_id: "fixture-call".into(),
                callable_id: tool.id.clone(),
                input: PhenixValue::String("fixture-input".into()),
            })
            .into_iter()
            .collect();

        let configuration: ExecutionConfigurationResponse = context
            .sdk
            .execution_configuration
            .invoke_projected(&ExecutionConfigurationCommand::ListAgents)
            .map_err(|error| format!("execution back-edge failed: {error}"))?;
        if !matches!(configuration, ExecutionConfigurationResponse::Agents { .. }) {
            return Err("execution back-edge returned a non-agent-list response".into());
        }

        context
            .kernel
            .encode_value(&StepRunnerResponse::Completed {
                attempt: fixture_attempt(),
                output: Bytes::new(b"provider-output".to_vec()),
                tool_calls,
                settled: BudgetActual {
                    fresh_input_tokens: 1,
                    output_tokens: 1,
                    cost_microunits: None,
                    attempts: 1,
                },
                settlement_basis: StepSettlementBasis::ReservedMaximum,
            })
            .map_err(|error| error.to_string())
    }
}

fn provider_id() -> PluginId {
    PluginId::parse(INVOCATION_PROVIDER).unwrap()
}

fn provider_component_id() -> ComponentId {
    ComponentId::parse(INVOCATION_PROVIDER_COMPONENT).unwrap()
}

fn provider_manifest() -> PluginManifest {
    PluginManifest {
        id: provider_id(),
        version: 1,
        execution: PluginExecution::Embedded,
        dependencies: Vec::new(),
        services: vec![ServiceContribution {
            role: ServiceRole::Terminal,
            service: default_invocation_service(),
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
        imports: vec![ComponentImport {
            interface: ExecutionConfigurationInterface::interface_id(),
            schema: ExecutionConfigurationInterface::schema(),
            required: true,
            authority: Authority::default(),
        }],
        exports: vec![ComponentExport {
            interface: DefaultInvocationInterface::interface_id(),
            schema: DefaultInvocationInterface::schema(),
            priority: 200,
            required_authority: Authority::default(),
        }],
        maximum_authority: Authority::default(),
    }
}

fn fixture_policy() -> UsagePolicy {
    UsagePolicy {
        revision: "policy-1".into(),
        max_fresh_input_tokens: 100,
        max_output_tokens: 100,
        max_cost_microunits: None,
        max_retries: 0,
        max_tool_result_bytes: 1024,
        max_tool_schemas: 8,
        max_skills: 0,
        require_known_capacity: false,
        delegation: DelegationResourcePolicy::default(),
    }
}

fn fixture_attempt() -> StepAttemptRecord {
    let policy = fixture_policy();
    let plan = policy
        .plan(&UsagePlanningInput {
            task: TaskRequirements {
                request_input_tokens: 1,
                context: ContextDemand {
                    mandatory_input_tokens: 0,
                    reducible_input_tokens: 0,
                    output_reserve_tokens: 1,
                    required_capabilities: BTreeSet::new(),
                },
                required_capabilities: BTreeSet::new(),
                required_tools: BTreeSet::new(),
                optional_tools: BTreeSet::new(),
                required_skills: BTreeSet::new(),
                optional_skills: BTreeSet::new(),
                requested_reasoning: None,
                deadline_at_ms: None,
            },
            execution_state: ExecutionState::Active,
            remaining: RemainingBudget {
                fresh_input_tokens: 100,
                output_tokens: 100,
                cost_microunits: None,
                attempts: 1,
            },
            now_ms: 0,
        })
        .unwrap();
    let mut attempt = StepAttemptRecord::new(
        UsageAttribution {
            root_execution_id: "execution-1".into(),
            execution_id: "execution-1".into(),
            attempt_id: "attempt-1".into(),
            parent_attempt_id: None,
            policy_revision: policy.revision,
            kind: UsageAttemptKind::Root,
            task_id: None,
        },
        plan,
    )
    .unwrap();
    attempt.reservation_id = Some("reservation-1".into());
    attempt.dispatch_id = Some("dispatch-1".into());
    attempt.phase = phenix_sdk::StepAttemptPhase::Settled;
    attempt.outcome = Some(AttemptOutcome::Succeeded);
    attempt
}

fn resolved_harness(with_provider: bool) -> ResolvedHarness {
    let execution = execution_manifest(Authority::default());
    let agent_loop = agent_loop_manifest(Authority::default());
    let ceiling = execution.maximum_authority.clone();
    let mut plugins = vec![execution, agent_loop];
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
    let agent_loop = agent_loop_manifest(Authority::default()).id;
    let mut kernel = Kernel::new(resolved.kernel_config().clone());
    kernel.activate_resolved_harness(&resolved).unwrap();
    kernel
        .register_embedded_factory(execution, execution_factory)
        .unwrap();
    kernel
        .register_embedded_factory(agent_loop.clone(), agent_loop_factory)
        .unwrap();
    if with_provider {
        kernel
            .register_embedded_factory(provider_id(), || Box::new(InvocationProvider))
            .unwrap();
    }
    kernel.activate_all().unwrap();
    (kernel, agent_loop)
}

fn command(tools: Vec<ModelToolDescriptor>) -> AgentLoopCommand {
    AgentLoopCommand::Run {
        execution_id: "execution-1".into(),
        session_id: Some(SessionId::parse("session-1").unwrap()),
        parent_attempt_id: None,
        callable_id: None,
        input: Bytes::new(b"prompt".to_vec()),
        tools,
        continuation: Vec::new(),
    }
}

fn invoke_agent_loop(kernel: &mut Kernel, agent_loop: &PluginId) -> Result<Vec<u8>, KernelError> {
    kernel.invoke_component(
        &agent_loop_component_id(),
        &agent_loop_service(),
        &serde_json::to_vec(&PhenixValue::from(&command(Vec::new()))).unwrap(),
        &Authority::default(),
        agent_loop,
    )
}

#[test]
fn agent_loop_plugin_is_distinct_from_execution_state_owner() {
    assert_ne!(
        agent_loop_manifest(Authority::default()).id,
        execution_manifest(Authority::default()).id
    );
}

#[test]
fn resolved_agent_loop_returns_central_invocation_output_with_usage() {
    let (mut kernel, agent_loop) = kernel(true);
    let output = invoke_agent_loop(&mut kernel, &agent_loop).unwrap();
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
fn agent_loop_preserves_typed_invocation_tool_calls() {
    let (mut kernel, agent_loop) = kernel(true);
    let command = command(vec![ModelToolDescriptor {
        id: phenix_core::CallableId::parse("fixture.client.echo").unwrap(),
        description: "Echo fixture input".into(),
        input_schema: PhenixSchema::Any,
        output_schema: PhenixSchema::Any,
    }]);
    let output = kernel
        .invoke_component(
            &agent_loop_component_id(),
            &agent_loop_service(),
            &serde_json::to_vec(&PhenixValue::from(&command)).unwrap(),
            &Authority::default(),
            &agent_loop,
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
fn agent_loop_without_default_invocation_fails_at_optional_import_boundary() {
    let (mut kernel, agent_loop) = kernel(false);
    match invoke_agent_loop(&mut kernel, &agent_loop).unwrap_err() {
        KernelError::ServiceInvoke {
            plugin,
            service,
            message,
        } => {
            assert_eq!(plugin, agent_loop);
            assert_eq!(service, agent_loop_service());
            assert_eq!(
                message,
                format!(
                    "component {} has no bound provider for optional import {}",
                    agent_loop_component_id(),
                    DefaultInvocationInterface::interface_id()
                )
            );
        }
        error => panic!("unexpected error: {error}"),
    }
}
