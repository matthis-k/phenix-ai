use phenix_core::{
    Authority, Bytes, CallableId, ComponentExport, ComponentId, ComponentImport,
    ComponentInterface, ComponentManifest, ModelToolCall, ModelToolDescriptor, ModelToolTurn,
    PermissionId, PluginContext, PluginExecution, PluginHost, PluginId, PluginInstance,
    PluginManifest, SdkClient, ServiceContribution, ServiceId, ServiceRole,
    SessionId, SharedPluginInvocation, ValueCodec,
};
use phenix_sdk::{
    AGENT_DIAGNOSTIC_EVENT_VERSION, AgentDiagnosticEvent, DefaultInvocationCommand,
    DefaultInvocationInterface, InvocationRequest, StepRunnerResponse, ToolObservation,
    agent_diagnostic_event_type,
};
use std::{
    collections::BTreeMap,
    num::{NonZeroU32, NonZeroU64},
    sync::Arc,
};

pub const AGENT_LOOP_PLUGIN: &str = "phenix.agent-loop";
const AGENT_LOOP_COMPONENT: &str = "phenix.agent-loop";

// Backward-compatible exports; contract ownership belongs to phenix-sdk.
pub use phenix_sdk::{
    AGENT_LOOP_CONTROL_SERVICE, AGENT_LOOP_PROGRESS_SERVICE, AGENT_LOOP_SERVICE,
    AGENT_TOOL_EXECUTION_SERVICE, AgentLoopCommand, AgentLoopControlInterface,
    AgentLoopControlRequest, AgentLoopControlResponse, AgentLoopFailure, AgentLoopInterface,
    AgentLoopProgress, AgentLoopProgressInterface, AgentLoopProgressRecord,
    AgentLoopProgressResponse, AgentLoopResponse, AgentLoopUsage, AgentToolExecutionInterface,
    AgentToolExecutionRequest, AgentToolExecutionResponse, agent_loop_control_service,
    agent_loop_progress_service, agent_loop_service, agent_tool_execution_service,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AgentLoopPolicy {
    max_model_turns: Option<NonZeroU32>,
    max_tool_calls_per_turn: Option<NonZeroU32>,
    max_tool_observation_model_bytes: Option<NonZeroU64>,
    result_reduction: bool,
}

impl AgentLoopPolicy {
    #[must_use]
    pub const fn new(max_model_turns: NonZeroU32, max_tool_calls_per_turn: NonZeroU32) -> Self {
        Self {
            max_model_turns: Some(max_model_turns),
            max_tool_calls_per_turn: Some(max_tool_calls_per_turn),
            max_tool_observation_model_bytes: None,
            result_reduction: true,
        }
    }

    #[must_use]
    pub const fn max_model_turns(self) -> Option<NonZeroU32> {
        self.max_model_turns
    }

    #[must_use]
    pub const fn max_tool_calls_per_turn(self) -> Option<NonZeroU32> {
        self.max_tool_calls_per_turn
    }

    #[must_use]
    pub const fn max_tool_observation_model_bytes(self) -> Option<NonZeroU64> {
        self.max_tool_observation_model_bytes
    }

    #[must_use]
    pub const fn with_tool_observation_model_bytes(mut self, limit: NonZeroU64) -> Self {
        self.max_tool_observation_model_bytes = Some(limit);
        self
    }

    #[must_use]
    pub const fn with_result_reduction(mut self, enabled: bool) -> Self {
        self.result_reduction = enabled;
        self
    }

    #[must_use]
    pub const fn result_reduction(self) -> bool {
        self.result_reduction
    }
}

impl Default for AgentLoopPolicy {
    fn default() -> Self {
        Self {
            max_model_turns: None,
            max_tool_calls_per_turn: None,
            max_tool_observation_model_bytes: None,
            result_reduction: true,
        }
    }
}

#[must_use]
pub fn agent_loop_component_id() -> ComponentId {
    ComponentId::parse(AGENT_LOOP_COMPONENT).expect("static agent loop component id is valid")
}

#[must_use]
pub fn agent_loop_progress_authority() -> Authority {
    Authority::new([
        PermissionId::parse("kernel.persistence.read")
            .expect("static persistence read capability is valid"),
        PermissionId::parse("kernel.persistence.write")
            .expect("static persistence write capability is valid"),
    ])
}

#[must_use]
pub fn agent_loop_manifest(maximum_authority: Authority) -> PluginManifest {
    PluginManifest {
        id: PluginId::parse(AGENT_LOOP_PLUGIN).expect("static agent loop plugin id is valid"),
        version: 1,
        execution: PluginExecution::Embedded,
        dependencies: Vec::new(),
        services: vec![ServiceContribution {
            role: ServiceRole::Terminal,
            service: agent_loop_service(),
            priority: 100,
            required_authority: Authority::default(),
        }],
        resource_namespaces: Vec::new(),
        maximum_authority,
    }
}

#[must_use]
pub fn agent_loop_component_manifest(maximum_authority: Authority) -> ComponentManifest {
    ComponentManifest {
        listeners: Vec::new(),
        id: agent_loop_component_id(),
        owner: PluginId::parse(AGENT_LOOP_PLUGIN).expect("static agent loop plugin id is valid"),
        imports: vec![
            ComponentImport {
                interface: DefaultInvocationInterface::interface_id(),
                schema: DefaultInvocationInterface::schema(),
                required: false,
                authority: maximum_authority.clone(),
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
                authority: maximum_authority.clone(),
            },
            ComponentImport {
                interface: AgentLoopProgressInterface::interface_id(),
                schema: AgentLoopProgressInterface::schema(),
                required: true,
                authority: agent_loop_progress_authority(),
            },
        ],
        exports: vec![ComponentExport {
            interface: AgentLoopInterface::interface_id(),
            schema: AgentLoopInterface::schema(),
            priority: 100,
            required_authority: Authority::default(),
        }],
        maximum_authority,
    }
}

#[must_use]
pub fn agent_loop_factory() -> Box<dyn PluginInstance> {
    agent_loop_factory_with_policy(AgentLoopPolicy::default())
}

#[must_use]
pub fn agent_loop_factory_with_policy(policy: AgentLoopPolicy) -> Box<dyn PluginInstance> {
    Box::new(AgentLoopPlugin { policy })
}

struct AgentLoopSdk<'host, 'runtime> {
    invocation: SdkClient<'host, 'runtime, DefaultInvocationInterface>,
    control: SdkClient<'host, 'runtime, AgentLoopControlInterface>,
    tools: SdkClient<'host, 'runtime, AgentToolExecutionInterface>,
    progress: SdkClient<'host, 'runtime, AgentLoopProgressInterface>,
}

type AgentLoopContext<'host, 'runtime> =
    PluginContext<'host, 'runtime, AgentLoopSdk<'host, 'runtime>>;

fn context<'host, 'runtime>(
    host: &'host PluginHost<'runtime>,
) -> AgentLoopContext<'host, 'runtime> {
    PluginContext::new(
        host,
        AgentLoopSdk {
            invocation: SdkClient::new(host, agent_loop_component_id()),
            control: SdkClient::new(host, agent_loop_component_id()),
            tools: SdkClient::new(host, agent_loop_component_id()),
            progress: SdkClient::new(host, agent_loop_component_id()),
        },
        (),
        (),
    )
}

struct AgentLoopPlugin {
    policy: AgentLoopPolicy,
}

#[derive(Clone, Copy)]
struct AgentLoopInvocation {
    policy: AgentLoopPolicy,
}

impl SharedPluginInvocation for AgentLoopInvocation {
    fn invoke(
        &self,
        service: &ServiceId,
        input: &[u8],
        host: &PluginHost<'_>,
    ) -> Result<Vec<u8>, String> {
        invoke_agent_loop(self.policy, service, input, host)
    }
}

impl PluginInstance for AgentLoopPlugin {
    fn start(&mut self, _host: &PluginHost<'_>) -> Result<(), String> {
        Ok(())
    }

    fn shared_invocation(&self) -> Option<Arc<dyn SharedPluginInvocation>> {
        Some(Arc::new(AgentLoopInvocation {
            policy: self.policy,
        }))
    }

    fn invoke(
        &mut self,
        service: &ServiceId,
        input: &[u8],
        host: &PluginHost<'_>,
    ) -> Result<Vec<u8>, String> {
        invoke_agent_loop(self.policy, service, input, host)
    }
}

fn invoke_agent_loop(
    policy: AgentLoopPolicy,
    service: &ServiceId,
    input: &[u8],
    host: &PluginHost<'_>,
) -> Result<Vec<u8>, String> {
    if service != &agent_loop_service() {
        return Err(format!("unsupported agent loop service: {service}"));
    }
    let context = context(host);
    let interface = AgentLoopInterface::interface_id();
    let command = context
        .kernel
        .decode_projected::<AgentLoopCommand>(&interface, input)
        .map_err(|error| error.to_string())?;
    let response = handle(&context, policy, command)?;
    context
        .kernel
        .encode_value(&response)
        .map_err(|error| error.to_string())
}

fn handle(
    context: &AgentLoopContext<'_, '_>,
    policy: AgentLoopPolicy,
    command: AgentLoopCommand,
) -> Result<AgentLoopResponse, String> {
    match command {
        AgentLoopCommand::Run {
            execution_id,
            session_id,
            parent_attempt_id,
            callable_id,
            input,
            tools,
        } => run(
            context,
            policy,
            execution_id,
            session_id,
            parent_attempt_id,
            callable_id,
            input,
            tools,
        ),
    }
}

#[allow(clippy::too_many_arguments)]
fn run(
    context: &AgentLoopContext<'_, '_>,
    policy: AgentLoopPolicy,
    execution_id: String,
    session_id: Option<SessionId>,
    parent_attempt_id: Option<String>,
    callable_id: Option<CallableId>,
    input: Bytes,
    mut tools: Vec<ModelToolDescriptor>,
) -> Result<AgentLoopResponse, String> {
    validate_initial_tools(&tools)?;
    let mut continuation = Vec::<ModelToolTurn>::new();
    let mut observations = BTreeMap::<CallableId, ToolObservation>::new();
    let mut seen_tool_call_ids = std::collections::BTreeSet::<String>::new();
    let mut usage = AgentLoopUsage {
        model_calls: 0,
        tool_calls: 0,
    };
    emit_agent_diagnostic(
        context,
        AgentDiagnosticEvent::RunStarted {
            execution_id: execution_id.clone(),
            session_id: session_id.clone(),
            callable_id: callable_id.clone(),
        },
    );

    loop {
        if let Some(limit) = policy.max_model_turns()
            && usage.model_calls >= limit.get()
        {
            let failure = AgentLoopFailure::ModelTurnLimitExceeded { limit: limit.get() };
            emit_agent_diagnostic(
                context,
                AgentDiagnosticEvent::RunFailed {
                    execution_id: execution_id.clone(),
                    session_id: session_id.clone(),
                    reason: format!("{failure:?}"),
                    model_calls: usage.model_calls,
                    tool_calls: usage.tool_calls,
                },
            );
            return Ok(AgentLoopResponse::Failed { failure, usage });
        }
        if context
            .kernel
            .cancellation_token()
            .is_some_and(|token| token.is_cancelled())
        {
            emit_agent_diagnostic(
                context,
                AgentDiagnosticEvent::RunCancelled {
                    execution_id: execution_id.clone(),
                    session_id: session_id.clone(),
                    model_calls: usage.model_calls,
                    tool_calls: usage.tool_calls,
                },
            );
            return Ok(AgentLoopResponse::Cancelled { usage });
        }
        let control: AgentLoopControlResponse =
            match context
                .sdk
                .control
                .invoke_projected(&AgentLoopControlRequest {
                    execution_id: execution_id.clone(),
                    session_id: session_id.clone(),
                }) {
                Ok(control) => control,
                Err(error) => {
                    let reason = error.to_string();
                    emit_run_failed(context, &execution_id, &session_id, &usage, &reason);
                    return Err(reason);
                }
            };
        if matches!(control, AgentLoopControlResponse::Cancelled) {
            emit_agent_diagnostic(
                context,
                AgentDiagnosticEvent::RunCancelled {
                    execution_id: execution_id.clone(),
                    session_id: session_id.clone(),
                    model_calls: usage.model_calls,
                    tool_calls: usage.tool_calls,
                },
            );
            return Ok(AgentLoopResponse::Cancelled { usage });
        }

        let turn = usage
            .model_calls
            .checked_add(1)
            .ok_or_else(|| "agent loop model-call usage overflowed".to_owned())?;
        emit_agent_diagnostic(
            context,
            AgentDiagnosticEvent::ModelTurnStarted {
                execution_id: execution_id.clone(),
                session_id: session_id.clone(),
                turn,
            },
        );

        let response =
            match context
                .sdk
                .invocation
                .invoke_projected(&DefaultInvocationCommand::Invoke {
                    request: InvocationRequest {
                        execution_id: execution_id.clone(),
                        session_id: session_id.clone(),
                        parent_attempt_id: parent_attempt_id.clone(),
                        callable_id: callable_id.clone(),
                        input: input.clone(),
                        tools: tools.clone(),
                        continuation: continuation.clone(),
                    },
                }) {
                Ok(response) => response,
                Err(error) => {
                    let reason = error.to_string();
                    emit_agent_diagnostic(
                        context,
                        AgentDiagnosticEvent::ModelTurnFailed {
                            execution_id: execution_id.clone(),
                            session_id: session_id.clone(),
                            turn,
                            reason: reason.clone(),
                        },
                    );
                    emit_run_failed(context, &execution_id, &session_id, &usage, &reason);
                    return Err(reason);
                }
            };
        usage.model_calls = usage
            .model_calls
            .checked_add(1)
            .ok_or_else(|| "agent loop model-call usage overflowed".to_owned())?;

        let StepRunnerResponse::Completed {
            output, tool_calls, ..
        } = response;
        let actual = u32::try_from(tool_calls.len())
            .map_err(|_| "model returned too many tool calls to represent".to_owned())?;
        if let Err(reason) = validate_model_tool_calls(&tool_calls, &mut seen_tool_call_ids) {
            emit_agent_diagnostic(
                context,
                AgentDiagnosticEvent::ModelTurnFailed {
                    execution_id: execution_id.clone(),
                    session_id: session_id.clone(),
                    turn: usage.model_calls,
                    reason: reason.clone(),
                },
            );
            emit_run_failed(context, &execution_id, &session_id, &usage, &reason);
            return Err(reason);
        }
        emit_agent_diagnostic(
            context,
            AgentDiagnosticEvent::ModelTurnCompleted {
                execution_id: execution_id.clone(),
                session_id: session_id.clone(),
                turn: usage.model_calls,
                tool_calls: actual,
            },
        );
        if tool_calls.is_empty() {
            emit_agent_diagnostic(
                context,
                AgentDiagnosticEvent::RunCompleted {
                    execution_id: execution_id.clone(),
                    session_id: session_id.clone(),
                    model_calls: usage.model_calls,
                    tool_calls: usage.tool_calls,
                },
            );
            return Ok(AgentLoopResponse::Completed { output, usage });
        }

        if let Some(limit) = policy.max_tool_calls_per_turn()
            && actual > limit.get()
        {
            let failure = AgentLoopFailure::ToolCallLimitExceeded {
                limit: limit.get(),
                actual,
            };
            emit_agent_diagnostic(
                context,
                AgentDiagnosticEvent::RunFailed {
                    execution_id: execution_id.clone(),
                    session_id: session_id.clone(),
                    reason: format!("{failure:?}"),
                    model_calls: usage.model_calls,
                    tool_calls: usage.tool_calls,
                },
            );
            return Ok(AgentLoopResponse::Failed { failure, usage });
        }

        let mut tool_results = Vec::with_capacity(tool_calls.len());
        for call in &tool_calls {
            emit_agent_diagnostic(
                context,
                AgentDiagnosticEvent::ToolInvocationStarted {
                    execution_id: execution_id.clone(),
                    session_id: session_id.clone(),
                    call_id: call.call_id.clone(),
                    callable_id: call.callable_id.clone(),
                },
            );
            emit_progress(
                context,
                AgentLoopProgressRecord {
                    execution_id: execution_id.clone(),
                    session_id: session_id.clone(),
                    progress: AgentLoopProgress::ToolCall { call: call.clone() },
                },
            )?;

            let response: AgentToolExecutionResponse =
                match context
                    .sdk
                    .tools
                    .invoke_projected(&AgentToolExecutionRequest {
                        execution_id: execution_id.clone(),
                        session_id: session_id.clone(),
                        call: call.clone(),
                    }) {
                    Ok(response) => response,
                    Err(error) => {
                        let reason = error.to_string();
                        emit_agent_diagnostic(
                            context,
                            AgentDiagnosticEvent::ToolInvocationFailed {
                                execution_id: execution_id.clone(),
                                session_id: session_id.clone(),
                                call_id: call.call_id.clone(),
                                callable_id: call.callable_id.clone(),
                                reason: reason.clone(),
                            },
                        );
                        emit_run_failed(context, &execution_id, &session_id, &usage, &reason);
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
                            execution_id: execution_id.clone(),
                            session_id: session_id.clone(),
                            call_id: call.call_id.clone(),
                            callable_id: call.callable_id.clone(),
                            reason: "cancelled".into(),
                        },
                    );
                    emit_agent_diagnostic(
                        context,
                        AgentDiagnosticEvent::RunCancelled {
                            execution_id: execution_id.clone(),
                            session_id: session_id.clone(),
                            model_calls: usage.model_calls,
                            tool_calls: usage.tool_calls,
                        },
                    );
                    return Ok(AgentLoopResponse::Cancelled { usage });
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
                        observations.get(&call.callable_id),
                        policy
                            .max_tool_observation_model_bytes()
                            .map_or(u64::MAX, NonZeroU64::get),
                        policy.result_reduction(),
                    )
                    .map_err(|error| format!("tool observation projection failed: {error:?}"))?;
                result.output = projection.to_value();
                observations.insert(call.callable_id.clone(), *observation);
            }
            usage.tool_calls = usage
                .tool_calls
                .checked_add(1)
                .ok_or_else(|| "agent loop tool-call usage overflowed".to_owned())?;
            emit_agent_diagnostic(
                context,
                AgentDiagnosticEvent::ToolInvocationCompleted {
                    execution_id: execution_id.clone(),
                    session_id: session_id.clone(),
                    call_id: result.call_id.clone(),
                    callable_id: result.callable_id.clone(),
                },
            );
            emit_progress(
                context,
                AgentLoopProgressRecord {
                    execution_id: execution_id.clone(),
                    session_id: session_id.clone(),
                    progress: AgentLoopProgress::ToolResult {
                        result: result.clone(),
                    },
                },
            )?;
            tool_results.push(result);
            activate_tools(&mut tools, activated_tools)?;
        }

        continuation.push(ModelToolTurn {
            assistant_output: output,
            tool_calls,
            tool_results,
        });
    }
}

fn validate_model_tool_calls(
    calls: &[ModelToolCall],
    seen: &mut std::collections::BTreeSet<String>,
) -> Result<(), String> {
    let mut current = std::collections::BTreeSet::new();
    for call in calls {
        if call.call_id.trim().is_empty() {
            return Err("model returned a tool call with an empty call id".to_owned());
        }
        if seen.contains(&call.call_id) || !current.insert(call.call_id.clone()) {
            return Err(format!(
                "model returned duplicate tool call id {}",
                call.call_id
            ));
        }
    }
    seen.extend(current);
    Ok(())
}

fn validate_initial_tools(tools: &[ModelToolDescriptor]) -> Result<(), String> {
    let mut ids = std::collections::BTreeSet::new();
    for tool in tools {
        if !ids.insert(tool.id.clone()) {
            return Err(format!(
                "agent loop received duplicate tool descriptor {}",
                tool.id
            ));
        }
    }
    Ok(())
}

fn activate_tools(
    active: &mut Vec<ModelToolDescriptor>,
    activated: Vec<ModelToolDescriptor>,
) -> Result<(), String> {
    let mut additions = Vec::new();
    for tool in activated {
        if let Some(existing) = active.iter().find(|existing| existing.id == tool.id) {
            if existing != &tool {
                return Err(format!(
                    "tool executor attempted to change active descriptor {}",
                    tool.id
                ));
            }
            continue;
        }
        if let Some(existing) = additions
            .iter()
            .find(|existing: &&ModelToolDescriptor| existing.id == tool.id)
        {
            if *existing != tool {
                return Err(format!(
                    "tool executor returned conflicting activated descriptors {}",
                    tool.id
                ));
            }
            continue;
        }
        additions.push(tool);
    }
    active.extend(additions);
    Ok(())
}

fn emit_run_failed(
    context: &AgentLoopContext<'_, '_>,
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

fn emit_agent_diagnostic(context: &AgentLoopContext<'_, '_>, diagnostic: AgentDiagnosticEvent) {
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
    context: &AgentLoopContext<'_, '_>,
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

#[cfg(test)]
mod tests {
    use super::*;
    use phenix_core::PhenixValue;

    #[test]
    fn model_tool_call_ids_must_be_nonempty_and_unique_for_the_execution() {
        let mut seen = std::collections::BTreeSet::new();
        assert!(validate_model_tool_calls(&[], &mut seen).is_ok());
        assert!(
            validate_model_tool_calls(
                &[ModelToolCall {
                    call_id: String::new(),
                    callable_id: CallableId::parse("fixture.tool").unwrap(),
                    input: PhenixValue::Unit,
                }],
                &mut seen,
            )
            .unwrap_err()
            .contains("empty call id")
        );

        let duplicate = vec![
            ModelToolCall {
                call_id: "call-1".into(),
                callable_id: CallableId::parse("fixture.one").unwrap(),
                input: PhenixValue::Unit,
            },
            ModelToolCall {
                call_id: "call-1".into(),
                callable_id: CallableId::parse("fixture.two").unwrap(),
                input: PhenixValue::Unit,
            },
        ];
        assert!(
            validate_model_tool_calls(&duplicate, &mut seen)
                .unwrap_err()
                .contains("duplicate tool call id call-1")
        );
        assert!(seen.is_empty());

        validate_model_tool_calls(
            &[ModelToolCall {
                call_id: "call-across-turns".into(),
                callable_id: CallableId::parse("fixture.one").unwrap(),
                input: PhenixValue::Unit,
            }],
            &mut seen,
        )
        .unwrap();
        let reused = validate_model_tool_calls(
            &[ModelToolCall {
                call_id: "call-across-turns".into(),
                callable_id: CallableId::parse("fixture.two").unwrap(),
                input: PhenixValue::Unit,
            }],
            &mut seen,
        )
        .unwrap_err();
        assert!(reused.contains("duplicate tool call id call-across-turns"));
    }

    #[test]
    fn tool_execution_import_carries_agent_loop_authority() {
        let shell = phenix_core::PermissionId::parse("workspace.shell").unwrap();
        let component = agent_loop_component_manifest(Authority::new([shell.clone()]));

        assert!(
            component.imports[2].authority.permits(&shell),
            "agent-loop tool calls must carry the authority granted to the agent loop"
        );
    }

    #[test]
    fn progress_import_carries_only_durable_journal_authority() {
        let shell = PermissionId::parse("workspace.shell").unwrap();
        let read = PermissionId::parse("kernel.persistence.read").unwrap();
        let write = PermissionId::parse("kernel.persistence.write").unwrap();
        let component = agent_loop_component_manifest(Authority::new([shell.clone()]));

        assert!(component.imports[3].authority.permits(&read));
        assert!(component.imports[3].authority.permits(&write));
        assert!(!component.imports[3].authority.permits(&shell));
    }

    #[test]
    fn default_progression_policy_has_no_model_turn_limit() {
        let policy = AgentLoopPolicy::default();
        assert_eq!(policy.max_model_turns(), None);
        assert_eq!(policy.max_tool_calls_per_turn(), None);
        assert_eq!(policy.max_tool_observation_model_bytes(), None);
    }

    #[test]
    fn explicit_model_turn_limit_remains_available() {
        let limit = NonZeroU32::new(16).unwrap();
        let per_turn = NonZeroU32::new(10).unwrap();
        let policy = AgentLoopPolicy::new(limit, per_turn);
        assert_eq!(policy.max_model_turns(), Some(limit));
    }
}
