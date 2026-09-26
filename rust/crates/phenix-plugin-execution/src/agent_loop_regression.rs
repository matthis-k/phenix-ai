use crate::configuration::ExecutionConfigurationInterface;
use crate::{
    agent_loop_component_id, agent_loop_component_manifest, agent_loop_control_service,
    agent_loop_factory, agent_loop_manifest, agent_loop_progress_service, agent_loop_service,
    agent_tool_execution_service, execution_component_manifest, execution_factory,
    execution_manifest, AgentLoopCommand, AgentLoopControlInterface, AgentLoopControlRequest,
    AgentLoopControlResponse, AgentLoopFailure, AgentLoopProgress, AgentLoopProgressInterface,
    AgentLoopProgressRecord, AgentLoopProgressResponse, AgentLoopResponse, AgentLoopUsage,
    AgentToolExecutionInterface, AgentToolExecutionRequest, AgentToolExecutionResponse,
    ExecutionConfigurationCommand, ExecutionConfigurationResponse, DEFAULT_MAX_MODEL_TURNS,
    DEFAULT_MAX_TOOL_CALLS_PER_TURN,
};
use phenix_core::{
    Authority, Bytes, CallableId, CapabilityId, ComponentExport, ComponentId, ComponentImport,
    ComponentInterface, ComponentManifest, Kernel, KernelError, ModelToolCall, ModelToolDescriptor,
    ModelToolResult, PhenixSchema, PhenixValue, PluginContext, PluginExecution, PluginHost,
    PluginId, PluginInstance, PluginManifest, Project, ResolvedHarness, ResolvedHarnessActivation,
    SdkClient, ServiceContribution, ServiceId, ServiceRole, SessionId,
};
use phenix_sdk::{
    default_invocation_service, AttemptOutcome, BudgetActual, ContextDemand,
    DefaultInvocationCommand, DefaultInvocationInterface, DelegationResourcePolicy, ExecutionState,
    RemainingBudget, StepAttemptRecord, StepRunnerResponse, StepSettlementBasis, TaskRequirements,
    UsageAttemptKind, UsageAttribution, UsagePlanningInput, UsagePolicy,
};
use std::{
    collections::BTreeSet,
    sync::{
        atomic::{AtomicBool, AtomicU32, Ordering},
        Arc, Mutex,
    },
};

const INVOCATION_PROVIDER: &str = "fixture.agent-loop-invocation";
const INVOCATION_PROVIDER_COMPONENT: &str = "fixture.agent-loop-invocation";
const TOOL_ADAPTER: &str = "fixture.agent-loop-tools";
const TOOL_ADAPTER_COMPONENT: &str = "fixture.agent-loop-tools";
const PERSISTENCE_SCHEMA: &str = "kernel.persistence.schema";
const PERSISTENCE_READ: &str = "kernel.persistence.read";
const PERSISTENCE_WRITE: &str = "kernel.persistence.write";

fn regression_authority() -> Authority {
    Authority::new([
        CapabilityId::parse(PERSISTENCE_SCHEMA).unwrap(),
        CapabilityId::parse(PERSISTENCE_READ).unwrap(),
        CapabilityId::parse(PERSISTENCE_WRITE).unwrap(),
    ])
}

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

struct InvocationProvider {
    calls: u32,
}

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
        self.calls += 1;
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

        let configuration: ExecutionConfigurationResponse = context
            .sdk
            .execution_configuration
            .invoke_projected(&ExecutionConfigurationCommand::ListAgents)
            .map_err(|error| format!("execution back-edge failed: {error}"))?;
        if !matches!(configuration, ExecutionConfigurationResponse::Agents { .. }) {
            return Err("execution back-edge returned a non-agent-list response".into());
        }

        let tool_calls = if let Some(tool) = request.tools.first() {
            match tool.id.as_str() {
                "fixture.many" if request.continuation.is_empty() => (0..11)
                    .map(|index| ModelToolCall {
                        call_id: format!("fixture-call-{index}"),
                        callable_id: tool.id.clone(),
                        input: PhenixValue::String("fixture-input".into()),
                    })
                    .collect(),
                "fixture.loop" => vec![ModelToolCall {
                    call_id: format!("fixture-call-{}", self.calls),
                    callable_id: tool.id.clone(),
                    input: PhenixValue::String("fixture-input".into()),
                }],
                _ if request.continuation.is_empty() => vec![ModelToolCall {
                    call_id: "fixture-call-1".into(),
                    callable_id: tool.id.clone(),
                    input: PhenixValue::String("fixture-input".into()),
                }],
                _ => {
                    let turn = request
                        .continuation
                        .last()
                        .ok_or_else(|| "agent loop lost continuation".to_owned())?;
                    let expected_result = if tool.id.as_str() == "fixture.error" {
                        ModelToolResult {
                            call_id: "fixture-call-1".into(),
                            callable_id: tool.id.clone(),
                            output: PhenixValue::String("fixture-error".into()),
                            is_error: true,
                        }
                    } else {
                        ModelToolResult {
                            call_id: "fixture-call-1".into(),
                            callable_id: tool.id.clone(),
                            output: PhenixValue::String("fixture-result".into()),
                            is_error: false,
                        }
                    };
                    if turn.assistant_output != Bytes::new(b"provider-output".to_vec())
                        || turn.tool_calls.len() != 1
                        || turn.tool_results != vec![expected_result]
                    {
                        return Err("agent loop changed typed continuation".into());
                    }
                    Vec::new()
                }
            }
        } else {
            Vec::new()
        };

        context
            .kernel
            .encode_value(&StepRunnerResponse::Completed {
                attempt: fixture_attempt(),
                output: Bytes::new(if request.continuation.is_empty() {
                    b"provider-output".to_vec()
                } else {
                    b"provider-output-2".to_vec()
                }),
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
        maximum_authority: regression_authority(),
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
            authority: regression_authority(),
        }],
        exports: vec![ComponentExport {
            interface: DefaultInvocationInterface::interface_id(),
            schema: DefaultInvocationInterface::schema(),
            priority: 200,
            required_authority: Authority::default(),
        }],
        maximum_authority: regression_authority(),
    }
}

struct ToolAdapter {
    executions: Arc<AtomicU32>,
    progress: Arc<Mutex<Vec<String>>>,
    cancel_on_next_control: Arc<AtomicBool>,
}

impl PluginInstance for ToolAdapter {
    fn start(&mut self, _host: &PluginHost<'_>) -> Result<(), String> {
        Ok(())
    }

    fn invoke(
        &mut self,
        service: &ServiceId,
        input: &[u8],
        _host: &PluginHost<'_>,
    ) -> Result<Vec<u8>, String> {
        if service == &agent_loop_control_service() {
            let value: PhenixValue = serde_json::from_slice(input).map_err(|e| e.to_string())?;
            let request =
                AgentLoopControlRequest::try_from(Project(&value)).map_err(|e| e.to_string())?;
            if request.execution_id != "execution-1"
                || request.session_id.as_ref().map(SessionId::as_str) != Some("session-1")
            {
                return Err("agent loop changed control identity".into());
            }
            let response = if self.cancel_on_next_control.load(Ordering::SeqCst) {
                AgentLoopControlResponse::Cancelled
            } else {
                AgentLoopControlResponse::Continue
            };
            return serde_json::to_vec(&PhenixValue::from(&response))
                .map_err(|error| error.to_string());
        }
        if service == &agent_tool_execution_service() {
            let value: PhenixValue = serde_json::from_slice(input).map_err(|e| e.to_string())?;
            let request = AgentToolExecutionRequest::try_from(Project(&value))
                .map_err(|error| error.to_string())?;
            self.executions.fetch_add(1, Ordering::SeqCst);
            if request.call.callable_id.as_str() == "fixture.cancel-during" {
                return serde_json::to_vec(&PhenixValue::from(
                    &AgentToolExecutionResponse::Cancelled,
                ))
                .map_err(|error| error.to_string());
            }
            if request.call.callable_id.as_str() == "fixture.cancel-between" {
                self.cancel_on_next_control.store(true, Ordering::SeqCst);
            }
            let is_error = request.call.callable_id.as_str() == "fixture.error";
            return serde_json::to_vec(&PhenixValue::from(
                &AgentToolExecutionResponse::Completed {
                    result: ModelToolResult {
                        call_id: request.call.call_id,
                        callable_id: request.call.callable_id,
                        output: PhenixValue::String(
                            if is_error {
                                "fixture-error"
                            } else {
                                "fixture-result"
                            }
                            .into(),
                        ),
                        is_error,
                    },
                },
            ))
            .map_err(|error| error.to_string());
        }
        if service == &agent_loop_progress_service() {
            let value: PhenixValue = serde_json::from_slice(input).map_err(|e| e.to_string())?;
            let record =
                AgentLoopProgressRecord::try_from(Project(&value)).map_err(|e| e.to_string())?;
            let entry = match record.progress {
                AgentLoopProgress::ToolCall { call } => format!("call:{}", call.call_id),
                AgentLoopProgress::ToolResult { result } => {
                    format!("result:{}", result.call_id)
                }
            };
            self.progress.lock().unwrap().push(entry);
            return serde_json::to_vec(&PhenixValue::from(&AgentLoopProgressResponse::Recorded))
                .map_err(|error| error.to_string());
        }
        Err(format!(
            "unsupported fixture tool adapter service: {service}"
        ))
    }
}

fn tool_adapter_id() -> PluginId {
    PluginId::parse(TOOL_ADAPTER).unwrap()
}

fn tool_adapter_component_id() -> ComponentId {
    ComponentId::parse(TOOL_ADAPTER_COMPONENT).unwrap()
}

fn tool_adapter_manifest() -> PluginManifest {
    PluginManifest {
        id: tool_adapter_id(),
        version: 1,
        execution: PluginExecution::Embedded,
        dependencies: Vec::new(),
        services: vec![
            ServiceContribution {
                role: ServiceRole::Terminal,
                service: agent_loop_control_service(),
                priority: 200,
                required_authority: Authority::default(),
            },
            ServiceContribution {
                role: ServiceRole::Terminal,
                service: agent_tool_execution_service(),
                priority: 200,
                required_authority: Authority::default(),
            },
            ServiceContribution {
                role: ServiceRole::Terminal,
                service: agent_loop_progress_service(),
                priority: 200,
                required_authority: Authority::default(),
            },
        ],
        resource_namespaces: Vec::new(),
        maximum_authority: regression_authority(),
    }
}

fn tool_adapter_component() -> ComponentManifest {
    ComponentManifest {
        listeners: Vec::new(),
        id: tool_adapter_component_id(),
        owner: tool_adapter_id(),
        imports: Vec::new(),
        exports: vec![
            ComponentExport {
                interface: AgentLoopControlInterface::interface_id(),
                schema: AgentLoopControlInterface::schema(),
                priority: 200,
                required_authority: Authority::default(),
            },
            ComponentExport {
                interface: AgentToolExecutionInterface::interface_id(),
                schema: AgentToolExecutionInterface::schema(),
                priority: 200,
                required_authority: Authority::default(),
            },
            ComponentExport {
                interface: AgentLoopProgressInterface::interface_id(),
                schema: AgentLoopProgressInterface::schema(),
                priority: 200,
                required_authority: Authority::default(),
            },
        ],
        maximum_authority: regression_authority(),
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
            historical_estimates: Vec::new(),
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
    let authority = regression_authority();
    let execution = execution_manifest(authority.clone());
    let agent_loop = agent_loop_manifest(authority.clone());
    let tool_adapter = tool_adapter_manifest();
    let ceiling = execution.maximum_authority.clone();
    let mut plugins = vec![execution, agent_loop, tool_adapter];
    let mut components = vec![
        execution_component_manifest(authority.clone()),
        agent_loop_component_manifest(authority),
        tool_adapter_component(),
    ];
    if with_provider {
        plugins.push(provider_manifest());
        components.push(provider_component());
    }
    ResolvedHarness::resolve(plugins, components, [], &ceiling).unwrap()
}

fn kernel(with_provider: bool) -> (Kernel, PluginId, Arc<AtomicU32>, Arc<Mutex<Vec<String>>>) {
    let resolved = resolved_harness(with_provider);
    let execution = execution_manifest(Authority::default()).id;
    let agent_loop = agent_loop_manifest(Authority::default()).id;
    let executions = Arc::new(AtomicU32::new(0));
    let progress = Arc::new(Mutex::new(Vec::new()));
    let cancel_on_next_control = Arc::new(AtomicBool::new(false));
    let mut kernel = Kernel::new(resolved.kernel_config().clone());
    kernel.activate_resolved_harness(&resolved).unwrap();
    kernel
        .register_embedded_factory(execution, execution_factory)
        .unwrap();
    kernel
        .register_embedded_factory(agent_loop.clone(), agent_loop_factory)
        .unwrap();
    let executions_for_factory = Arc::clone(&executions);
    let progress_for_factory = Arc::clone(&progress);
    let cancellation_for_factory = Arc::clone(&cancel_on_next_control);
    kernel
        .register_embedded_factory(tool_adapter_id(), move || {
            Box::new(ToolAdapter {
                executions: Arc::clone(&executions_for_factory),
                progress: Arc::clone(&progress_for_factory),
                cancel_on_next_control: Arc::clone(&cancellation_for_factory),
            })
        })
        .unwrap();
    if with_provider {
        kernel
            .register_embedded_factory(provider_id(), || Box::new(InvocationProvider { calls: 0 }))
            .unwrap();
    }
    kernel.activate_all().unwrap();
    (kernel, agent_loop, executions, progress)
}

fn command(tools: Vec<ModelToolDescriptor>) -> AgentLoopCommand {
    AgentLoopCommand::Run {
        execution_id: "execution-1".into(),
        session_id: Some(SessionId::parse("session-1").unwrap()),
        parent_attempt_id: None,
        callable_id: None,
        input: Bytes::new(b"prompt".to_vec()),
        tools,
    }
}

fn descriptor(id: &str) -> ModelToolDescriptor {
    ModelToolDescriptor {
        id: CallableId::parse(id).unwrap(),
        description: "fixture tool".into(),
        input_schema: PhenixSchema::Any,
        output_schema: PhenixSchema::Any,
    }
}

fn invoke_agent_loop(
    kernel: &mut Kernel,
    agent_loop: &PluginId,
    command: AgentLoopCommand,
) -> Result<Vec<u8>, KernelError> {
    kernel.invoke_component(
        &agent_loop_component_id(),
        &agent_loop_service(),
        &serde_json::to_vec(&PhenixValue::from(&command)).unwrap(),
        &regression_authority(),
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
    let (mut kernel, agent_loop, _, _) = kernel(true);
    let output = invoke_agent_loop(&mut kernel, &agent_loop, command(Vec::new())).unwrap();
    let output: PhenixValue = serde_json::from_slice(&output).unwrap();
    let response = AgentLoopResponse::try_from(Project(&output)).unwrap();

    assert_eq!(
        response,
        AgentLoopResponse::Completed {
            output: Bytes::new(b"provider-output".to_vec()),
            usage: AgentLoopUsage {
                model_calls: 1,
                tool_calls: 0,
            },
        }
    );
}

#[test]
fn one_agent_call_owns_two_model_turns_and_typed_continuation() {
    let (mut kernel, agent_loop, executions, progress) = kernel(true);
    let output = invoke_agent_loop(
        &mut kernel,
        &agent_loop,
        command(vec![descriptor("fixture.client.echo")]),
    )
    .unwrap();
    let output: PhenixValue = serde_json::from_slice(&output).unwrap();
    let response = AgentLoopResponse::try_from(Project(&output)).unwrap();

    assert_eq!(
        response,
        AgentLoopResponse::Completed {
            output: Bytes::new(b"provider-output-2".to_vec()),
            usage: AgentLoopUsage {
                model_calls: 2,
                tool_calls: 1,
            },
        }
    );
    assert_eq!(executions.load(Ordering::SeqCst), 1);
    assert_eq!(
        progress.lock().unwrap().as_slice(),
        ["call:fixture-call-1", "result:fixture-call-1"]
    );
}

#[test]
fn eleven_calls_fail_before_any_tool_executes() {
    let (mut kernel, agent_loop, executions, _) = kernel(true);
    let output = invoke_agent_loop(
        &mut kernel,
        &agent_loop,
        command(vec![descriptor("fixture.many")]),
    )
    .unwrap();
    let output: PhenixValue = serde_json::from_slice(&output).unwrap();
    let response = AgentLoopResponse::try_from(Project(&output)).unwrap();

    assert_eq!(
        response,
        AgentLoopResponse::Failed {
            failure: AgentLoopFailure::ToolCallLimitExceeded {
                limit: DEFAULT_MAX_TOOL_CALLS_PER_TURN,
                actual: 11,
            },
            usage: AgentLoopUsage {
                model_calls: 1,
                tool_calls: 0,
            },
        }
    );
    assert_eq!(executions.load(Ordering::SeqCst), 0);
}

#[test]
fn seventeenth_model_turn_fails_at_loop_boundary() {
    let (mut kernel, agent_loop, executions, _) = kernel(true);
    let output = invoke_agent_loop(
        &mut kernel,
        &agent_loop,
        command(vec![descriptor("fixture.loop")]),
    )
    .unwrap();
    let output: PhenixValue = serde_json::from_slice(&output).unwrap();
    let response = AgentLoopResponse::try_from(Project(&output)).unwrap();

    assert_eq!(
        response,
        AgentLoopResponse::Failed {
            failure: AgentLoopFailure::ModelTurnLimitExceeded {
                limit: DEFAULT_MAX_MODEL_TURNS,
            },
            usage: AgentLoopUsage {
                model_calls: DEFAULT_MAX_MODEL_TURNS,
                tool_calls: DEFAULT_MAX_MODEL_TURNS,
            },
        }
    );
    assert_eq!(executions.load(Ordering::SeqCst), DEFAULT_MAX_MODEL_TURNS);
}

#[test]
fn ordinary_tool_failure_is_continuation_not_run_failure() {
    let (mut kernel, agent_loop, executions, progress) = kernel(true);
    let output = invoke_agent_loop(
        &mut kernel,
        &agent_loop,
        command(vec![descriptor("fixture.error")]),
    )
    .unwrap();
    let output: PhenixValue = serde_json::from_slice(&output).unwrap();
    let response = AgentLoopResponse::try_from(Project(&output)).unwrap();

    assert_eq!(
        response,
        AgentLoopResponse::Completed {
            output: Bytes::new(b"provider-output-2".to_vec()),
            usage: AgentLoopUsage {
                model_calls: 2,
                tool_calls: 1,
            },
        }
    );
    assert_eq!(executions.load(Ordering::SeqCst), 1);
    assert_eq!(
        progress.lock().unwrap().as_slice(),
        ["call:fixture-call-1", "result:fixture-call-1"]
    );
}

#[test]
fn cancellation_between_turns_stops_before_the_next_model_invocation() {
    let (mut kernel, agent_loop, executions, _) = kernel(true);
    let output = invoke_agent_loop(
        &mut kernel,
        &agent_loop,
        command(vec![descriptor("fixture.cancel-between")]),
    )
    .unwrap();
    let output: PhenixValue = serde_json::from_slice(&output).unwrap();
    let response = AgentLoopResponse::try_from(Project(&output)).unwrap();

    assert_eq!(
        response,
        AgentLoopResponse::Cancelled {
            usage: AgentLoopUsage {
                model_calls: 1,
                tool_calls: 1,
            },
        }
    );
    assert_eq!(executions.load(Ordering::SeqCst), 1);
}

#[test]
fn cancellation_during_a_tool_terminates_the_run() {
    let (mut kernel, agent_loop, executions, _) = kernel(true);
    let output = invoke_agent_loop(
        &mut kernel,
        &agent_loop,
        command(vec![descriptor("fixture.cancel-during")]),
    )
    .unwrap();
    let output: PhenixValue = serde_json::from_slice(&output).unwrap();
    let response = AgentLoopResponse::try_from(Project(&output)).unwrap();

    assert_eq!(
        response,
        AgentLoopResponse::Cancelled {
            usage: AgentLoopUsage {
                model_calls: 1,
                tool_calls: 0,
            },
        }
    );
    assert_eq!(executions.load(Ordering::SeqCst), 1);
}

#[test]
fn agent_loop_without_default_invocation_fails_at_optional_import_boundary() {
    let (mut kernel, agent_loop, _, _) = kernel(false);
    match invoke_agent_loop(&mut kernel, &agent_loop, command(Vec::new())).unwrap_err() {
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
