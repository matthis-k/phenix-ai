#![forbid(unsafe_code)]

//! Basic implementations of the replaceable declarative agent workflow nodes.
//!
//! Core schedules the selected topology. This plugin implements turn and tool
//! nodes using shared SDK validation and observation contracts.

use phenix_core::{
    Authority, ComponentExport, ComponentId, ComponentImport, ComponentInterface,
    ComponentManifest, ModelToolTurn, PermissionId, PluginContext, PluginExecution, PluginHost,
    PluginId, PluginInstance, PluginManifest, SdkClient, ServiceContribution, ServiceId,
    ServiceRole, SessionId, SharedPluginInvocation, ValueCodec,
};
use phenix_sdk::{
    AGENT_DIAGNOSTIC_EVENT_VERSION, AgentDiagnosticEvent, AgentLoopControlInterface,
    AgentLoopControlRequest, AgentLoopControlResponse, AgentLoopFailure, AgentLoopPolicy,
    AgentLoopProgress, AgentLoopProgressInterface, AgentLoopProgressRecord,
    AgentLoopProgressResponse, AgentLoopUsage, AgentToolBatchInterface, AgentToolBatchRequest,
    AgentToolBatchResponse, AgentToolExecutionInterface, AgentToolExecutionRequest,
    AgentToolExecutionResponse, AgentTurnState, AgentTurnStepInterface, AgentTurnStepRequest,
    AgentTurnStepResponse, DefaultInvocationCommand, DefaultInvocationInterface, InvocationRequest,
    StepRunnerResponse, activate_tools, agent_diagnostic_event_type, agent_tool_batch_service,
    agent_turn_step_service, validate_initial_tools, validate_model_tool_calls,
};
use std::{collections::BTreeSet, sync::Arc};

pub const BASIC_AGENT_NODES_PLUGIN: &str = "phenix.basic-agent-nodes";

struct NodeSdk<'host, 'runtime> {
    invocation: SdkClient<'host, 'runtime, DefaultInvocationInterface>,
    control: SdkClient<'host, 'runtime, AgentLoopControlInterface>,
    tools: SdkClient<'host, 'runtime, AgentToolExecutionInterface>,
    progress: SdkClient<'host, 'runtime, AgentLoopProgressInterface>,
}

type NodeContext<'host, 'runtime> = PluginContext<'host, 'runtime, NodeSdk<'host, 'runtime>>;

fn node_context<'host, 'runtime>(
    host: &'host PluginHost<'runtime>,
) -> NodeContext<'host, 'runtime> {
    let component = basic_agent_nodes_component_id();
    PluginContext::new(
        host,
        NodeSdk {
            invocation: SdkClient::new(host, component.clone()),
            control: SdkClient::new(host, component.clone()),
            tools: SdkClient::new(host, component.clone()),
            progress: SdkClient::new(host, component),
        },
        (),
        (),
    )
}

fn node_progress_authority() -> Authority {
    Authority::new([
        PermissionId::parse("kernel.persistence.read").expect("static permission"),
        PermissionId::parse("kernel.persistence.write").expect("static permission"),
    ])
}

pub fn basic_agent_nodes_component_id() -> ComponentId {
    ComponentId::parse(BASIC_AGENT_NODES_PLUGIN).expect("static basic agent component")
}

pub fn basic_agent_nodes_manifest(authority: Authority) -> PluginManifest {
    PluginManifest {
        id: PluginId::parse(BASIC_AGENT_NODES_PLUGIN).expect("static basic agent plugin"),
        version: 1,
        execution: PluginExecution::Embedded,
        dependencies: Vec::new(),
        services: [agent_turn_step_service(), agent_tool_batch_service()]
            .into_iter()
            .map(|service| ServiceContribution {
                service,
                role: ServiceRole::Terminal,
                priority: 100,
                required_authority: Authority::default(),
            })
            .collect(),
        resource_namespaces: Vec::new(),
        maximum_authority: authority,
    }
}

pub fn basic_agent_nodes_component_manifest(authority: Authority) -> ComponentManifest {
    ComponentManifest {
        id: basic_agent_nodes_component_id(),
        owner: PluginId::parse(BASIC_AGENT_NODES_PLUGIN).expect("static basic agent plugin"),
        imports: vec![
            ComponentImport {
                interface: DefaultInvocationInterface::interface_id(),
                schema: DefaultInvocationInterface::schema(),
                required: true,
                authority: authority.clone(),
            },
            ComponentImport {
                interface: AgentLoopControlInterface::interface_id(),
                schema: AgentLoopControlInterface::schema(),
                required: true,
                authority: Authority::default(),
            },
            ComponentImport {
                interface: AgentToolExecutionInterface::interface_id(),
                schema: AgentToolExecutionInterface::schema(),
                required: true,
                authority: authority.clone(),
            },
            ComponentImport {
                interface: AgentLoopProgressInterface::interface_id(),
                schema: AgentLoopProgressInterface::schema(),
                required: true,
                authority: node_progress_authority(),
            },
        ],
        exports: vec![
            ComponentExport {
                interface: AgentTurnStepInterface::interface_id(),
                schema: AgentTurnStepInterface::schema(),
                priority: 100,
                required_authority: Authority::default(),
            },
            ComponentExport {
                interface: AgentToolBatchInterface::interface_id(),
                schema: AgentToolBatchInterface::schema(),
                priority: 100,
                required_authority: Authority::default(),
            },
        ],
        listeners: Vec::new(),
        maximum_authority: authority,
    }
}

pub fn basic_agent_nodes_factory() -> Box<dyn PluginInstance> {
    basic_agent_nodes_factory_with_policy(AgentLoopPolicy::default())
}

pub fn basic_agent_nodes_factory_with_policy(policy: AgentLoopPolicy) -> Box<dyn PluginInstance> {
    Box::new(BasicAgentNodes { policy })
}

struct BasicAgentNodes {
    policy: AgentLoopPolicy,
}

impl PluginInstance for BasicAgentNodes {
    fn start(&mut self, _host: &PluginHost<'_>) -> Result<(), String> {
        Ok(())
    }

    fn shared_invocation(&self) -> Option<Arc<dyn SharedPluginInvocation>> {
        Some(Arc::new(BasicAgentNodes {
            policy: self.policy,
        }))
    }

    fn invoke(
        &mut self,
        service: &ServiceId,
        input: &[u8],
        host: &PluginHost<'_>,
    ) -> Result<Vec<u8>, String> {
        dispatch(self.policy, service, input, host)
    }
}

impl SharedPluginInvocation for BasicAgentNodes {
    fn invoke(
        &self,
        service: &ServiceId,
        input: &[u8],
        host: &PluginHost<'_>,
    ) -> Result<Vec<u8>, String> {
        dispatch(self.policy, service, input, host)
    }
}

fn dispatch(
    policy: AgentLoopPolicy,
    service: &ServiceId,
    input: &[u8],
    host: &PluginHost<'_>,
) -> Result<Vec<u8>, String> {
    let context = node_context(host);
    if service == &agent_turn_step_service() {
        let request = context
            .kernel
            .decode_projected::<AgentTurnStepRequest>(
                &AgentTurnStepInterface::interface_id(),
                input,
            )
            .map_err(|error| error.to_string())?;
        let response = run_turn(&context, policy, request.state)?;
        return context
            .kernel
            .encode_value(&response)
            .map_err(|error| error.to_string());
    }
    if service == &agent_tool_batch_service() {
        let request = context
            .kernel
            .decode_projected::<AgentToolBatchRequest>(
                &AgentToolBatchInterface::interface_id(),
                input,
            )
            .map_err(|error| error.to_string())?;
        let response = run_tool_batch(&context, policy, request)?;
        return context
            .kernel
            .encode_value(&response)
            .map_err(|error| error.to_string());
    }
    Err(format!("unsupported basic agent node service: {service}"))
}

fn run_turn(
    context: &NodeContext<'_, '_>,
    policy: AgentLoopPolicy,
    mut state: AgentTurnState,
) -> Result<AgentTurnStepResponse, String> {
    if state.usage.model_calls == 0 {
        validate_initial_tools(&state.tools)?;
        emit_agent_diagnostic(
            context,
            AgentDiagnosticEvent::RunStarted {
                execution_id: state.execution_id.clone(),
                session_id: state.session_id.clone(),
                callable_id: state.callable_id.clone(),
            },
        );
    }
    if let Some(limit) = policy.max_model_turns()
        && state.usage.model_calls >= limit.get()
    {
        let failure = AgentLoopFailure::ModelTurnLimitExceeded { limit: limit.get() };
        emit_run_failed(
            context,
            &state.execution_id,
            &state.session_id,
            &state.usage,
            &format!("{failure:?}"),
        );
        return Ok(AgentTurnStepResponse::Failed { state, failure });
    }
    if context
        .kernel
        .cancellation_token()
        .is_some_and(|token| token.is_cancelled())
    {
        emit_cancelled(context, &state);
        return Ok(AgentTurnStepResponse::Cancelled { state });
    }
    let control: AgentLoopControlResponse = context
        .sdk
        .control
        .invoke_projected(&AgentLoopControlRequest {
            execution_id: state.execution_id.clone(),
            session_id: state.session_id.clone(),
        })
        .map_err(|error| error.to_string())?;
    if matches!(control, AgentLoopControlResponse::Cancelled) {
        emit_cancelled(context, &state);
        return Ok(AgentTurnStepResponse::Cancelled { state });
    }
    let turn = state
        .usage
        .model_calls
        .checked_add(1)
        .ok_or_else(|| "agent loop model-call usage overflowed".to_owned())?;
    emit_agent_diagnostic(
        context,
        AgentDiagnosticEvent::ModelTurnStarted {
            execution_id: state.execution_id.clone(),
            session_id: state.session_id.clone(),
            turn,
        },
    );
    let response = context
        .sdk
        .invocation
        .invoke_projected(&DefaultInvocationCommand::Invoke {
            request: InvocationRequest {
                execution_id: state.execution_id.clone(),
                session_id: state.session_id.clone(),
                parent_attempt_id: state.parent_attempt_id.clone(),
                callable_id: state.callable_id.clone(),
                input: state.input.clone(),
                tools: state.tools.clone(),
                continuation: state.continuation.clone(),
            },
        });
    let response: StepRunnerResponse = match response {
        Ok(response) => response,
        Err(error) => {
            let reason = error.to_string();
            emit_agent_diagnostic(
                context,
                AgentDiagnosticEvent::ModelTurnFailed {
                    execution_id: state.execution_id.clone(),
                    session_id: state.session_id.clone(),
                    turn,
                    reason: reason.clone(),
                },
            );
            emit_run_failed(
                context,
                &state.execution_id,
                &state.session_id,
                &state.usage,
                &reason,
            );
            return Err(reason);
        }
    };
    state.usage.model_calls = turn;
    let StepRunnerResponse::Completed {
        output, tool_calls, ..
    } = response;
    let actual = u32::try_from(tool_calls.len())
        .map_err(|_| "model returned too many tool calls to represent".to_owned())?;
    let mut seen: BTreeSet<String> = state.seen_tool_call_ids.iter().cloned().collect();
    if let Err(reason) = validate_model_tool_calls(&tool_calls, &mut seen) {
        emit_agent_diagnostic(
            context,
            AgentDiagnosticEvent::ModelTurnFailed {
                execution_id: state.execution_id.clone(),
                session_id: state.session_id.clone(),
                turn,
                reason: reason.clone(),
            },
        );
        emit_run_failed(
            context,
            &state.execution_id,
            &state.session_id,
            &state.usage,
            &reason,
        );
        return Err(reason);
    }
    state.seen_tool_call_ids = seen.into_iter().collect();
    emit_agent_diagnostic(
        context,
        AgentDiagnosticEvent::ModelTurnCompleted {
            execution_id: state.execution_id.clone(),
            session_id: state.session_id.clone(),
            turn,
            tool_calls: actual,
        },
    );
    if tool_calls.is_empty() {
        emit_agent_diagnostic(
            context,
            AgentDiagnosticEvent::RunCompleted {
                execution_id: state.execution_id.clone(),
                session_id: state.session_id.clone(),
                model_calls: state.usage.model_calls,
                tool_calls: state.usage.tool_calls,
            },
        );
        return Ok(AgentTurnStepResponse::Final { state, output });
    }
    if let Some(limit) = policy.max_tool_calls_per_turn()
        && actual > limit.get()
    {
        let failure = AgentLoopFailure::ToolCallLimitExceeded {
            limit: limit.get(),
            actual,
        };
        emit_run_failed(
            context,
            &state.execution_id,
            &state.session_id,
            &state.usage,
            &format!("{failure:?}"),
        );
        return Ok(AgentTurnStepResponse::Failed { state, failure });
    }
    Ok(AgentTurnStepResponse::ToolCalls {
        state,
        assistant_output: output,
        tool_calls,
    })
}

fn run_tool_batch(
    context: &NodeContext<'_, '_>,
    policy: AgentLoopPolicy,
    request: AgentToolBatchRequest,
) -> Result<AgentToolBatchResponse, String> {
    let AgentToolBatchRequest {
        mut state,
        assistant_output,
        tool_calls,
    } = request;
    let mut tool_results = Vec::with_capacity(tool_calls.len());
    for call in &tool_calls {
        emit_agent_diagnostic(
            context,
            AgentDiagnosticEvent::ToolInvocationStarted {
                execution_id: state.execution_id.clone(),
                session_id: state.session_id.clone(),
                call_id: call.call_id.clone(),
                callable_id: call.callable_id.clone(),
            },
        );
        emit_progress(
            context,
            AgentLoopProgressRecord {
                execution_id: state.execution_id.clone(),
                session_id: state.session_id.clone(),
                progress: AgentLoopProgress::ToolCall { call: call.clone() },
            },
        )?;
        let response: AgentToolExecutionResponse =
            match context
                .sdk
                .tools
                .invoke_projected(&AgentToolExecutionRequest {
                    execution_id: state.execution_id.clone(),
                    session_id: state.session_id.clone(),
                    call: call.clone(),
                }) {
                Ok(response) => response,
                Err(error) => {
                    let reason = error.to_string();
                    emit_agent_diagnostic(
                        context,
                        AgentDiagnosticEvent::ToolInvocationFailed {
                            execution_id: state.execution_id.clone(),
                            session_id: state.session_id.clone(),
                            call_id: call.call_id.clone(),
                            callable_id: call.callable_id.clone(),
                            reason: reason.clone(),
                        },
                    );
                    emit_run_failed(
                        context,
                        &state.execution_id,
                        &state.session_id,
                        &state.usage,
                        &reason,
                    );
                    return Err(reason);
                }
            };
        let (mut result, activated_tools, observation) = match response {
            AgentToolExecutionResponse::Completed {
                result,
                activated_tools,
                observation,
            } => (result, activated_tools, observation),
            AgentToolExecutionResponse::Cancelled => {
                emit_agent_diagnostic(
                    context,
                    AgentDiagnosticEvent::ToolInvocationFailed {
                        execution_id: state.execution_id.clone(),
                        session_id: state.session_id.clone(),
                        call_id: call.call_id.clone(),
                        callable_id: call.callable_id.clone(),
                        reason: "cancelled".into(),
                    },
                );
                emit_cancelled(context, &state);
                return Ok(AgentToolBatchResponse::Cancelled { state });
            }
        };
        if result.call_id != call.call_id || result.callable_id != call.callable_id {
            return Err(format!(
                "tool executor changed call identity for {}",
                call.call_id
            ));
        }
        if let Some(observation) = observation {
            if observation.occurrence_id != call.call_id {
                return Err(format!(
                    "tool executor changed observation occurrence identity for {}",
                    call.call_id
                ));
            }
            let projection = observation
                .project(
                    state.observations.get(&call.callable_id),
                    policy
                        .max_tool_observation_model_bytes()
                        .map_or(u64::MAX, std::num::NonZeroU64::get),
                    policy.result_reduction(),
                )
                .map_err(|error| format!("tool observation projection failed: {error:?}"))?;
            result.output = projection.to_value();
            state
                .observations
                .insert(call.callable_id.clone(), *observation);
        }
        state.usage.tool_calls = state
            .usage
            .tool_calls
            .checked_add(1)
            .ok_or_else(|| "agent loop tool-call usage overflowed".to_owned())?;
        emit_agent_diagnostic(
            context,
            AgentDiagnosticEvent::ToolInvocationCompleted {
                execution_id: state.execution_id.clone(),
                session_id: state.session_id.clone(),
                call_id: result.call_id.clone(),
                callable_id: result.callable_id.clone(),
            },
        );
        emit_progress(
            context,
            AgentLoopProgressRecord {
                execution_id: state.execution_id.clone(),
                session_id: state.session_id.clone(),
                progress: AgentLoopProgress::ToolResult {
                    result: result.clone(),
                },
            },
        )?;
        tool_results.push(result);
        activate_tools(&mut state.tools, activated_tools)?;
    }
    state.continuation.push(ModelToolTurn {
        assistant_output,
        tool_calls,
        tool_results,
    });
    Ok(AgentToolBatchResponse::Continue { state })
}

fn emit_run_failed(
    context: &NodeContext<'_, '_>,
    execution_id: &str,
    session_id: &Option<SessionId>,
    usage: &AgentLoopUsage,
    reason: &str,
) {
    emit_agent_diagnostic(
        context,
        AgentDiagnosticEvent::RunFailed {
            execution_id: execution_id.to_owned(),
            session_id: session_id.clone(),
            reason: reason.to_owned(),
            model_calls: usage.model_calls,
            tool_calls: usage.tool_calls,
        },
    );
}

fn emit_agent_diagnostic(context: &NodeContext<'_, '_>, diagnostic: AgentDiagnosticEvent) {
    let Ok(payload) = serde_json::to_vec(&diagnostic) else {
        return;
    };
    let _ = context.kernel.dispatch_event(
        agent_diagnostic_event_type(),
        AGENT_DIAGNOSTIC_EVENT_VERSION,
        0,
        0,
        payload,
    );
}

fn emit_progress(
    context: &NodeContext<'_, '_>,
    record: AgentLoopProgressRecord,
) -> Result<(), String> {
    let response: AgentLoopProgressResponse = context
        .sdk
        .progress
        .invoke_projected(&record)
        .map_err(|error| error.to_string())?;
    match response {
        AgentLoopProgressResponse::Recorded => Ok(()),
    }
}

fn emit_cancelled(context: &NodeContext<'_, '_>, state: &AgentTurnState) {
    emit_agent_diagnostic(
        context,
        AgentDiagnosticEvent::RunCancelled {
            execution_id: state.execution_id.clone(),
            session_id: state.session_id.clone(),
            model_calls: state.usage.model_calls,
            tool_calls: state.usage.tool_calls,
        },
    );
}
